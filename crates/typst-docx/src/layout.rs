//! Compiler-native layout export: the geometry oracle.
//!
//! Serializes a finished [`PagedDocument`] — its page frames and the shaped
//! text runs within them — to JSON. Unlike the semantic HTML export, this
//! reflects where content *actually* ended up after line breaking, spacing and
//! placement, which downstream tools can use to reason about page overflow,
//! widows/orphans and overlaps.

use std::fmt::Write as _;

use typst_library::{World, WorldExt};
use typst_library::layout::{Frame, FrameItem, Point};
use typst_library::text::{FontStyle, TextItem};
use typst_layout::{Page, PagedDocument};

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
    walk(world, &page.frame, Point::zero(), out, &mut first);
    out.push_str("]}");
}

/// Recursively collect text runs, accumulating placement offsets.
///
/// Only translations are applied from group transforms (rotation, scaling and
/// skew are ignored), which is sufficient for the common `place`/`move` cases.
fn walk(
    world: &dyn World,
    frame: &Frame,
    origin: Point,
    out: &mut String,
    first: &mut bool,
) {
    for (pos, item) in frame.items() {
        let at = Point::new(origin.x + pos.x, origin.y + pos.y);
        match item {
            FrameItem::Text(text) => write_run(world, text, at, out, first),
            FrameItem::Group(group) => {
                let inner =
                    Point::new(at.x + group.transform.tx, at.y + group.transform.ty);
                walk(world, &group.frame, inner, out, first);
            }
            _ => {}
        }
    }
}

/// Write a single shaped text run.
fn write_run(
    world: &dyn World,
    text: &TextItem,
    at: Point,
    out: &mut String,
    first: &mut bool,
) {
    if !*first {
        out.push(',');
    }
    *first = false;

    let info = text.font.font().info();
    let variant = info.variant;
    let italic = matches!(variant.style, FontStyle::Italic | FontStyle::Oblique);
    let bold = variant.weight.to_number() >= 700;

    let _ = write!(
        out,
        "{{\"text\":\"{}\",\"xPt\":{:.3},\"yPt\":{:.3},\"sizePt\":{:.3},\
         \"font\":\"{}\",\"bold\":{bold},\"italic\":{italic},\"span\":",
        escape(&text.text),
        at.x.to_pt(),
        at.y.to_pt(),
        text.size.to_pt(),
        escape(&info.family),
    );

    match text
        .glyphs
        .first()
        .and_then(|glyph| world.range(glyph.span.0))
    {
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
