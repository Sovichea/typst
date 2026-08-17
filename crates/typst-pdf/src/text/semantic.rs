//! Logical text preparation for externally shaped glyphs.

use std::collections::HashMap;
use std::ops::Range;

use krilla::surface::Location;
use krilla::text::GlyphId;
use typst_library::text::FontInstance;
use unicode_pdf::{
    CidAllocator, ExternalGlyph, FontId, logical_units_from_external_glyphs,
    plan_text_run, synthesize_truetype_composites,
};

use super::PdfGlyph;

pub(super) struct SemanticFragment<'a> {
    pub(super) start: krilla::geom::Point,
    pub(super) glyphs: &'a [PdfGlyph],
    pub(super) text: &'a str,
    pub(super) text_offset: usize,
}

pub(super) struct SemanticGlyphRun {
    pub(super) start: krilla::geom::Point,
    pub(super) glyphs: Vec<SemanticPdfGlyph>,
    pub(super) font: krilla::text::Font,
}

pub(super) fn prepare_semantic_run(
    fragments: &[SemanticFragment<'_>],
    source_font: &FontInstance,
    text: &str,
    font_size: f32,
) -> Option<SemanticGlyphRun> {
    if text.is_empty()
        || fragments.is_empty()
        || !font_size.is_finite()
        || font_size <= 0.0
        || source_font.index() != 0
        || !source_font.variations().0.is_empty()
        || source_font.ttf().tables().glyf.is_none()
    {
        return None;
    }

    let units_per_em = f32::from(source_font.ttf().units_per_em());
    let anchor = fragments.first()?.start;
    let scale = units_per_em / font_size;
    let glyph_count = fragments.iter().map(|fragment| fragment.glyphs.len()).sum();
    let mut external = Vec::with_capacity(glyph_count);
    let mut locations = HashMap::with_capacity(glyph_count);

    for fragment in fragments {
        let fragment_end = fragment.text_offset.checked_add(fragment.text.len())?;
        if fragment_end > text.len()
            || text.get(fragment.text_offset..fragment_end)? != fragment.text
        {
            return None;
        }

        let mut run_x = fragment.start.x - anchor.x;
        let run_y = -(fragment.start.y - anchor.y);
        for glyph in fragment.glyphs {
            let local_range = krilla::text::Glyph::text_range(glyph);
            if local_range.end > fragment.text.len() {
                return None;
            }
            let text_range = fragment.text_offset.checked_add(local_range.start)?
                ..fragment.text_offset.checked_add(local_range.end)?;
            external.push(ExternalGlyph {
                glyph_id: krilla::text::Glyph::glyph_id(glyph).to_u32(),
                text_range: text_range.clone(),
                run_x: rounded_i32(run_x * scale)?,
                run_y: rounded_i32(run_y * scale)?,
                x_offset: rounded_i32(
                    krilla::text::Glyph::x_offset(glyph, font_size) * scale,
                )?,
                y_offset: rounded_i32(
                    krilla::text::Glyph::y_offset(glyph, font_size) * scale,
                )?,
                x_advance: rounded_i32(
                    krilla::text::Glyph::x_advance(glyph, font_size) * scale,
                )?,
                y_advance: rounded_i32(
                    krilla::text::Glyph::y_advance(glyph, font_size) * scale,
                )?,
            });
            locations
                .entry((text_range.start, text_range.end))
                .or_insert_with(|| krilla::text::Glyph::location(glyph));
            run_x += krilla::text::Glyph::x_advance(glyph, font_size);
        }
    }

    let logical = logical_units_from_external_glyphs(text, FontId(0), &external).ok()?;
    logical.validate_round_trip().ok()?;
    let mut allocator = CidAllocator::new();
    let plan = plan_text_run(&logical.units, &mut allocator).ok()?;
    let synthesized =
        synthesize_truetype_composites(source_font.data().as_ref(), allocator.entries())
            .ok()?;
    let synthetic_font = krilla::text::Font::new(synthesized.bytes.into(), 0)?;
    let gids: HashMap<_, _> = synthesized
        .synthetic_glyphs
        .iter()
        .map(|record| (record.cid, record.glyph_id))
        .collect();

    let mut logical_cursor = 0_i32;
    let mut semantic_glyphs = Vec::with_capacity(logical.units.len());
    for (unit, planned) in logical.units.iter().zip(plan.units.iter()) {
        let glyph_id = *gids.get(&planned.cid)?;
        let width = planned.visual_end_x.checked_sub(planned.visual_x)?;
        let x_offset = planned.visual_x.checked_sub(logical_cursor)?;
        let source_range = unit.source_range.as_ref()?.0.clone();
        let location = locations
            .get(&(source_range.start, source_range.end))
            .copied()
            .flatten();
        semantic_glyphs.push(SemanticPdfGlyph {
            glyph_id: GlyphId::new(u32::from(glyph_id)),
            x_advance: width as f32 / units_per_em,
            x_offset: x_offset as f32 / units_per_em,
            text_range: source_range,
            location,
        });
        logical_cursor = logical_cursor.checked_add(width)?;
    }

    Some(SemanticGlyphRun {
        start: anchor,
        glyphs: semantic_glyphs,
        font: synthetic_font,
    })
}

fn rounded_i32(value: f32) -> Option<i32> {
    if !value.is_finite() || value < i32::MIN as f32 || value > i32::MAX as f32 {
        None
    } else {
        Some(value.round() as i32)
    }
}

pub(super) struct SemanticPdfGlyph {
    glyph_id: GlyphId,
    x_advance: f32,
    x_offset: f32,
    text_range: Range<usize>,
    location: Option<Location>,
}

impl krilla::text::Glyph for SemanticPdfGlyph {
    fn glyph_id(&self) -> GlyphId {
        self.glyph_id
    }

    fn text_range(&self) -> Range<usize> {
        self.text_range.clone()
    }

    fn x_advance(&self, size: f32) -> f32 {
        self.x_advance * size
    }

    fn x_offset(&self, size: f32) -> f32 {
        self.x_offset * size
    }

    fn y_offset(&self, _size: f32) -> f32 {
        0.0
    }

    fn y_advance(&self, _size: f32) -> f32 {
        0.0
    }

    fn location(&self) -> Option<Location> {
        self.location
    }
}
