//! Compiler-native layout export: the geometry oracle.
//!
//! Serializes a finished [`PagedDocument`] — its page frames and the shaped
//! text runs within them — to JSON, and exposes the same runs to the DOCX
//! exporter so it can bake Typst's *resolved* typography into Word styles.
//! Unlike the semantic HTML export, this reflects where content actually ended
//! up after line breaking, spacing and placement.

use std::fmt::Write as _;
use std::ops::Range;

use typst_library::layout::{Frame, FrameItem, Point};
use typst_library::text::{FontStyle, TextItem};
use typst_library::visualize::Paint;
use typst_library::{World, WorldExt};
use typst_layout::{Page, PagedDocument};

/// A shaped text run measured from the paged layout.
#[derive(Clone, Debug)]
pub struct Run {
    /// The run's plain text.
    pub text: String,
    /// The resolved font family.
    pub family: String,
    /// The resolved font size in points.
    pub size_pt: f64,
    /// Whether the run is bold.
    pub bold: bool,
    /// Whether the run is italic.
    pub italic: bool,
    /// The resolved text color as `RRGGBB`.
    pub color: String,
    /// The source byte range, if resolvable.
    pub span: Option<Range<usize>>,
    /// The horizontal position of the run's origin, in points.
    pub x_pt: f64,
    /// The vertical position of the run's baseline, in points.
    pub y_pt: f64,
    /// The 1-based page number the run appears on.
    pub page: u64,
    /// The distance from the baseline to the top of the glyph box, in points.
    pub ascent_pt: f64,
    /// The distance from the baseline to the bottom of the glyph box, in points.
    pub descent_pt: f64,
}

/// Serialize a paged document's layout to JSON.
pub fn layout_json(world: &dyn World, document: &PagedDocument) -> String {
    let mut out = String::from("{\"pages\":[");
    for (index, page) in document.pages().iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        write_page(&mut out, world, page, index as u64 + 1);
    }
    out.push_str("]}");
    out
}

/// Collect every text run in the document, in reading order.
pub fn collect_runs(document: &PagedDocument) -> Vec<Run> {
    let mut runs = Vec::new();
    for (index, page) in document.pages().iter().enumerate() {
        walk(&page.frame, Point::zero(), &mut |text, at| {
            let mut run = run_of(text, None);
            run.x_pt = at.x.to_pt();
            run.y_pt = at.y.to_pt();
            run.page = index as u64 + 1;
            runs.push(run);
        });
    }
    runs
}

/// Write a single page and its text runs.
fn write_page(out: &mut String, world: &dyn World, page: &Page, number: u64) {
    let size = page.frame.size();
    let _ = write!(
        out,
        "{{\"number\":{number},\"widthPt\":{:.3},\"heightPt\":{:.3},\"runs\":[",
        size.x.to_pt(),
        size.y.to_pt(),
    );

    let mut first = true;
    walk(&page.frame, Point::zero(), &mut |text, at| {
        write_run(out, &run_of(text, Some(world)), at, &mut first);
    });
    out.push_str("]}");
}

/// Recursively visit text runs, accumulating placement offsets.
///
/// Only translations are applied from group transforms (rotation, scaling and
/// skew are ignored), which is sufficient for the common `place`/`move` cases.
fn walk(frame: &Frame, origin: Point, f: &mut dyn FnMut(&TextItem, Point)) {
    for (pos, item) in frame.items() {
        let at = Point::new(origin.x + pos.x, origin.y + pos.y);
        match item {
            FrameItem::Text(text) => f(text, at),
            FrameItem::Group(group) => {
                let inner =
                    Point::new(at.x + group.transform.tx, at.y + group.transform.ty);
                walk(&group.frame, inner, f);
            }
            _ => {}
        }
    }
}

/// Measure a shaped text run's resolved typography.
fn run_of(text: &TextItem, world: Option<&dyn World>) -> Run {
    let info = text.font.font().info();
    let variant = info.variant;
    // Use the font's typographic ascent/descent (the line box), not the glyph
    // ink box, so block gaps can be measured against Typst's own boxes.
    let metrics = text.font.metrics();
    let ascent = metrics.ascender.at(text.size).to_pt().abs();
    let descent = metrics.descender.at(text.size).to_pt().abs();
    Run {
        text: text.text.to_string(),
        family: info.family.clone(),
        size_pt: text.size.to_pt(),
        bold: variant.weight.to_number() >= 700,
        italic: matches!(variant.style, FontStyle::Italic | FontStyle::Oblique),
        color: match &text.fill {
            Paint::Solid(color) => {
                color.to_hex().trim_start_matches('#').to_ascii_uppercase()
            }
            _ => "000000".to_string(),
        },
        span: world.and_then(|w| text.glyphs.first().and_then(|g| w.range(g.span.0))),
        x_pt: 0.0,
        y_pt: 0.0,
        page: 0,
        ascent_pt: ascent,
        descent_pt: descent,
    }
}

/// Write a single shaped text run as JSON.
fn write_run(out: &mut String, run: &Run, at: Point, first: &mut bool) {
    if !*first {
        out.push(',');
    }
    *first = false;

    let _ = write!(
        out,
        "{{\"text\":\"{}\",\"xPt\":{:.3},\"yPt\":{:.3},\"sizePt\":{:.3},\
         \"ascPt\":{:.3},\"descPt\":{:.3},\
         \"font\":\"{}\",\"bold\":{},\"italic\":{},\"color\":\"{}\",\"span\":",
        escape(&run.text),
        at.x.to_pt(),
        at.y.to_pt(),
        run.size_pt,
        run.ascent_pt,
        run.descent_pt,
        escape(&run.family),
        run.bold,
        run.italic,
        run.color,
    );

    match &run.span {
        Some(range) => {
            let _ = write!(out, "[{},{}]", range.start, range.end);
        }
        None => out.push_str("null"),
    }

    out.push('}');
}

/// Escape a string for inclusion in JSON.
fn escape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for c in value.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out
}
