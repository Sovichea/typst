//! Construction of Krilla logical PDF units from Typst-shaped glyphs.

use std::collections::BTreeMap;
use std::ops::Range;

use krilla::geom::Point;
use krilla::surface::Location;
use krilla::text::{Glyph, GlyphId, KrillaGlyph, PdfLogicalUnit};
use typst_library::text::FontInstance;

use super::PdfGlyph;

/// One positioned Typst fragment that belongs to a complete logical text run.
pub(super) struct LogicalFragment<'a> {
    pub(super) start: Point,
    pub(super) glyphs: &'a [PdfGlyph],
    pub(super) text: &'a str,
    pub(super) text_offset: usize,
}

/// Owned logical units that can be borrowed as Krilla's public unit type.
pub(super) struct PreparedLogicalRun {
    pub(super) start: Point,
    units: Vec<OwnedLogicalUnit>,
}

impl PreparedLogicalRun {
    /// Borrow the prepared units together with their authoritative Unicode.
    pub(super) fn units<'a>(
        &'a self,
        text: &'a str,
    ) -> Vec<PdfLogicalUnit<'a, KrillaGlyph>> {
        self.units
            .iter()
            .map(|unit| {
                let logical = PdfLogicalUnit::new(
                    &text[unit.range.clone()],
                    &unit.glyphs,
                    unit.visual_x,
                    unit.visual_y,
                );
                if let Some(location) = unit.location {
                    logical.with_location(location)
                } else {
                    logical
                }
            })
            .collect()
    }
}

struct OwnedLogicalUnit {
    range: Range<usize>,
    glyphs: Vec<KrillaGlyph>,
    visual_x: f32,
    visual_y: f32,
    location: Option<Location>,
}

struct PositionedGlyph {
    glyph_id: GlyphId,
    range: Range<usize>,
    run_x: f32,
    run_y: f32,
    x_offset: f32,
    y_offset: f32,
    x_advance: f32,
    location: Option<Location>,
}

/// Build logical units in source order while retaining shaped visual positions.
pub(super) fn prepare_logical_run(
    fragments: &[LogicalFragment<'_>],
    source_font: &FontInstance,
    text: &str,
    font_size: f32,
) -> Option<PreparedLogicalRun> {
    if text.is_empty()
        || fragments.is_empty()
        || !font_size.is_finite()
        || font_size <= 0.0
        || source_font.index() != 0
        || !source_font.variations().0.is_empty()
        || source_font.ttf().tables().glyf.is_none()
        || source_font.data().as_ref().starts_with(b"ttcf")
    {
        return None;
    }

    let anchor = fragments.first()?.start;
    // TrueType composite glyph offsets are stored as i16. A logical unit whose
    // visual width would push a component offset past that range cannot be
    // synthesized, so fall back to ordinary glyph drawing for such runs (e.g.
    // repeated-dot outline leaders). Leave a two-em margin so the synthesized
    // composite bbox and component offsets stay within the i16 range.
    let max_logical_width = i16::MAX as f32 / source_font.units_per_em() as f32 - 2.0;
    let mut grouped = BTreeMap::<(usize, usize), Vec<PositionedGlyph>>::new();

    for fragment in fragments {
        let fragment_end = fragment.text_offset.checked_add(fragment.text.len())?;
        if fragment_end > text.len()
            || text.get(fragment.text_offset..fragment_end)? != fragment.text
        {
            return None;
        }

        let mut run_x = (fragment.start.x - anchor.x) / font_size;
        let mut run_y = -(fragment.start.y - anchor.y) / font_size;
        for glyph in fragment.glyphs {
            let local_range = glyph.text_range();
            if local_range.start >= local_range.end
                || local_range.end > fragment.text.len()
            {
                return None;
            }
            let range = fragment.text_offset.checked_add(local_range.start)?
                ..fragment.text_offset.checked_add(local_range.end)?;
            if !text.is_char_boundary(range.start) || !text.is_char_boundary(range.end) {
                return None;
            }

            let x_advance = glyph.x_advance(1.0);
            let y_advance = glyph.y_advance(1.0);
            grouped
                .entry((range.start, range.end))
                .or_default()
                .push(PositionedGlyph {
                    glyph_id: glyph.glyph_id(),
                    range,
                    run_x,
                    run_y,
                    x_offset: glyph.x_offset(1.0),
                    y_offset: glyph.y_offset(1.0),
                    x_advance,
                    location: glyph.location(),
                });
            run_x += x_advance;
            run_y += y_advance;
        }
    }

    let mut units = Vec::with_capacity(grouped.len().saturating_add(1));
    let mut cursor = 0;
    for ((start, end), glyphs) in grouped {
        if start < cursor || end > text.len() {
            return None;
        }
        if cursor < start {
            units.push(empty_unit(cursor..start));
        }
        units.push(positioned_unit(start..end, glyphs, max_logical_width)?);
        cursor = end;
    }
    if cursor < text.len() {
        units.push(empty_unit(cursor..text.len()));
    }

    Some(PreparedLogicalRun { start: anchor, units })
}

fn empty_unit(range: Range<usize>) -> OwnedLogicalUnit {
    OwnedLogicalUnit {
        range,
        glyphs: vec![],
        visual_x: 0.0,
        visual_y: 0.0,
        location: None,
    }
}

fn positioned_unit(
    range: Range<usize>,
    positioned: Vec<PositionedGlyph>,
    max_width: f32,
) -> Option<OwnedLogicalUnit> {
    let origin_x = positioned
        .iter()
        .flat_map(|glyph| [glyph.run_x, glyph.run_x + glyph.x_advance])
        .reduce(f32::min)?;
    let end_x = positioned
        .iter()
        .flat_map(|glyph| [glyph.run_x, glyph.run_x + glyph.x_advance])
        .reduce(f32::max)?;
    let width = end_x - origin_x;
    if !origin_x.is_finite() || !width.is_finite() || width < 0.0 || width > max_width {
        return None;
    }

    let location = positioned.iter().find_map(|glyph| glyph.location);
    let mut pen_x = 0.0;
    let glyphs = positioned
        .into_iter()
        .enumerate()
        .map(|(index, glyph)| {
            let x_advance = if index == 0 { width } else { 0.0 };
            let x_offset = glyph.run_x + glyph.x_offset - origin_x - pen_x;
            let y_offset = glyph.run_y + glyph.y_offset;
            pen_x += x_advance;
            KrillaGlyph::new(
                glyph.glyph_id,
                x_advance,
                x_offset,
                y_offset,
                0.0,
                glyph.range,
                glyph.location,
            )
        })
        .collect();

    Some(OwnedLogicalUnit {
        range,
        glyphs,
        visual_x: origin_x,
        visual_y: 0.0,
        location,
    })
}
