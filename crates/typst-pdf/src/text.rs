use std::ops::Range;
use std::sync::Arc;

use bytemuck::TransparentWrapper;
use krilla::surface::{Location, Surface};
use krilla::text::GlyphId;
use typst_library::diag::{SourceResult, bail};
use typst_library::layout::{FrameItem, Point};
use typst_library::text::{FontInstance, Glyph, TextItem};
use typst_library::visualize::{FillRule, Paint};
use typst_syntax::Span;
use typst_utils::defer;

mod logical;

use self::logical::{LogicalFragment, prepare_logical_run};

use crate::convert::{FrameContext, GlobalContext};
use crate::util::{AbsExt, TransformExt, display_font};
use crate::{paint, tags};

pub(crate) struct PreparedTextBatch<'a> {
    pub(crate) consumed: usize,
    fragments: Vec<PreparedTextFragment<'a>>,
    logical_text: String,
}

struct PreparedTextFragment<'a> {
    point: Point,
    item: &'a TextItem,
    text_offset: usize,
}

/// Merge consecutive Typst text items that originate from one shaped source
/// run but were separated for visual baseline positioning.
pub(crate) fn prepare_text_batch<'a>(
    items: &'a [(Point, FrameItem)],
) -> Option<PreparedTextBatch<'a>> {
    let (first_point, FrameItem::Text(first)) = items.first()? else {
        return None;
    };
    if !matches!(first.fill, Paint::Solid(_))
        || first
            .stroke
            .as_ref()
            .is_some_and(|stroke| !matches!(stroke.paint, Paint::Solid(_)))
    {
        return None;
    }
    let (source_span, first_base, source_shift) =
        if let Some((span, base)) = text_source_base(first) {
            (span, base, 0)
        } else {
            // A style boundary can leave an unrelated separator at the start of
            // the first item. Find the source coordinates shared with the first
            // visual overlay and shift them past that authoritative prefix.
            let (_, FrameItem::Text(next)) = items.get(1)? else {
                return None;
            };
            if !compatible_text(first, next) {
                return None;
            }
            let (span, _) = text_source_base(next)?;
            let shift = mixed_source_prefix_shift(&first.text, &first.glyphs, span)?;
            (span, 0, shift)
        };
    let mut candidates = vec![(*first_point, first, first_base)];
    let mut minimum_baseline = first_point.y.to_f32();
    let mut maximum_baseline = minimum_baseline;
    let maximum_baseline_span = first.size.to_f32() * 0.75;

    for (point, item) in &items[1..] {
        let FrameItem::Text(text) = item else {
            break;
        };
        if !compatible_text(first, text) {
            break;
        }
        let Some((span, base)) = text_source_base(text) else {
            break;
        };
        let Some(base) = base.checked_add(source_shift) else {
            break;
        };
        if span != source_span {
            break;
        }
        let baseline = point.y.to_f32();
        let Some((next_minimum, next_maximum)) = extend_baseline_span(
            minimum_baseline,
            maximum_baseline,
            baseline,
            maximum_baseline_span,
        ) else {
            break;
        };
        minimum_baseline = next_minimum;
        maximum_baseline = next_maximum;
        candidates.push((*point, text, base));
    }

    if candidates.len() < 2 || !has_source_overlap(&candidates) {
        return None;
    }

    let segments: Vec<_> = candidates
        .iter()
        .map(|(_, text, base)| (*base, text.text.as_str()))
        .collect();
    let (minimum, logical_text) = merge_text_segments(&segments)?;

    let fragments = candidates
        .into_iter()
        .map(|(point, item, base)| {
            Some(PreparedTextFragment {
                point,
                item,
                text_offset: base.checked_sub(minimum)?,
            })
        })
        .collect::<Option<Vec<_>>>()?;

    Some(PreparedTextBatch { consumed: fragments.len(), fragments, logical_text })
}

fn extend_baseline_span(
    minimum: f32,
    maximum: f32,
    baseline: f32,
    limit: f32,
) -> Option<(f32, f32)> {
    let next_minimum = minimum.min(baseline);
    let next_maximum = maximum.max(baseline);
    (next_maximum - next_minimum <= limit).then_some((next_minimum, next_maximum))
}

fn merge_text_segments(segments: &[(usize, &str)]) -> Option<(usize, String)> {
    let minimum = segments.iter().map(|(base, _)| *base).min()?;
    let mut ordered = segments.to_vec();
    ordered.sort_unstable_by_key(|(base, _)| *base);
    let mut logical_text = String::new();
    for (base, text) in ordered {
        let offset = base.checked_sub(minimum)?;
        if offset > logical_text.len() {
            return None;
        }
        let overlap = logical_text.len() - offset;
        let shared = overlap.min(text.len());
        if !text.is_char_boundary(shared)
            || logical_text.as_bytes().get(offset..offset + shared)?
                != text.as_bytes().get(..shared)?
        {
            return None;
        }
        if shared < text.len() {
            logical_text.push_str(text.get(shared..)?);
        }
    }
    Some((minimum, logical_text))
}

fn compatible_text(first: &TextItem, text: &TextItem) -> bool {
    text.font == first.font
        && text.size == first.size
        && text.fill == first.fill
        && text.stroke == first.stroke
        && text.lang == first.lang
        && text.region == first.region
}

/// Return the length of a foreign prefix before glyphs from `source_span`.
///
/// Source positions are relative to their own span and can therefore precede
/// local item positions. Keep that affine delta signed until it has been
/// validated as an exact prefix shift.
fn mixed_source_prefix_shift(
    text: &str,
    glyphs: &[Glyph],
    source_span: Span,
) -> Option<usize> {
    let mut shared = glyphs.iter().filter(|glyph| glyph.span.0 == source_span);
    let first = shared.next()?;
    let delta = i32::from(first.span.1) - i32::from(first.range.start);
    let shift = usize::try_from(delta.checked_neg()?).ok()?;
    if shift == 0 || shift >= text.len() || !text.is_char_boundary(shift) {
        return None;
    }

    let mut shared_start = usize::MAX;
    for glyph in glyphs {
        let range = glyph.range();
        if range.end > text.len()
            || !text.is_char_boundary(range.start)
            || !text.is_char_boundary(range.end)
        {
            return None;
        }
        if glyph.span.0 == source_span {
            if i32::from(glyph.span.1) - i32::from(glyph.range.start) != delta
                || range.start < shift
            {
                return None;
            }
            shared_start = shared_start.min(range.start);
        } else if range.end > shift {
            // Only a contiguous foreign prefix is safe to preserve as part of
            // the authoritative first fragment.
            return None;
        }
    }

    (shared_start == shift).then_some(shift)
}

fn text_source_base(text: &TextItem) -> Option<(Span, usize)> {
    let first = text.glyphs.first()?;
    let span = first.span.0;
    let base = usize::from(first.span.1).checked_sub(usize::from(first.range.start))?;
    for glyph in &text.glyphs {
        if glyph.span.0 != span
            || usize::from(glyph.span.1).checked_sub(usize::from(glyph.range.start))?
                != base
        {
            return None;
        }
    }
    Some((span, base))
}

fn has_source_overlap(candidates: &[(Point, &TextItem, usize)]) -> bool {
    for (index, (_, text, base)) in candidates.iter().enumerate() {
        let Some(end) = base.checked_add(text.text.len()) else {
            return false;
        };
        for (_, other, other_base) in &candidates[index + 1..] {
            let Some(other_end) = other_base.checked_add(other.text.len()) else {
                return false;
            };
            if *base < other_end && *other_base < end {
                return true;
            }
        }
    }
    false
}

#[typst_macros::time(name = "handle text")]
pub(crate) fn handle_text(
    fc: &mut FrameContext,
    t: &TextItem,
    surface: &mut Surface,
    gc: &mut GlobalContext,
) -> SourceResult<()> {
    let mut handle = tags::text(gc, fc, surface, t);
    let surface = handle.surface();

    let font = convert_font(gc, t.font.clone())?;
    let fill = paint::convert_fill(
        gc,
        &t.fill,
        FillRule::NonZero,
        true,
        surface,
        fc.state(),
        None,
    )?;
    let stroke = if let Some(stroke) = t.stroke.as_ref() {
        Some(paint::convert_stroke(gc, stroke, true, surface, fc.state(), None)?)
    } else {
        None
    };
    let size = t.size;
    let glyphs: &[PdfGlyph] = TransparentWrapper::wrap_slice(t.glyphs.as_slice());
    let fragment = LogicalFragment {
        start: krilla::geom::Point::from_xy(0.0, 0.0),
        glyphs,
        text: t.text.as_str(),
        text_offset: 0,
    };
    let logical =
        prepare_logical_run(&[fragment], &t.font, t.text.as_str(), size.to_f32());

    surface.push_transform(&fc.state().transform().to_krilla());
    let mut surface = defer(surface, |s| s.pop());
    surface.set_fill(Some(fill));
    surface.set_stroke(stroke);
    if let Some(logical) = logical {
        let units = logical.units(t.text.as_str());
        surface.draw_pdf_logical_units(logical.start, &units, font, size.to_f32(), false);
    } else {
        surface.draw_glyphs(
            krilla::geom::Point::from_xy(0.0, 0.0),
            glyphs,
            font,
            t.text.as_str(),
            size.to_f32(),
            false,
        );
    }

    Ok(())
}

pub(crate) fn handle_text_batch(
    fc: &mut FrameContext,
    batch: &PreparedTextBatch<'_>,
    surface: &mut Surface,
    gc: &mut GlobalContext,
) -> SourceResult<()> {
    let first = batch.fragments.first().unwrap().item;
    let tag_fragments: Vec<_> = batch
        .fragments
        .iter()
        .map(|fragment| (fragment.point, fragment.item))
        .collect();
    let mut handle = tags::text_batch(gc, fc, surface, &tag_fragments);
    let surface = handle.surface();
    let font = convert_font(gc, first.font.clone())?;
    let fill = paint::convert_fill(
        gc,
        &first.fill,
        FillRule::NonZero,
        true,
        surface,
        fc.state(),
        None,
    )?;
    let stroke = if let Some(stroke) = first.stroke.as_ref() {
        Some(paint::convert_stroke(gc, stroke, true, surface, fc.state(), None)?)
    } else {
        None
    };

    let fragments: Vec<LogicalFragment<'_>> = batch
        .fragments
        .iter()
        .map(|fragment| LogicalFragment {
            start: krilla::geom::Point::from_xy(
                fragment.point.x.to_f32(),
                fragment.point.y.to_f32(),
            ),
            glyphs: <PdfGlyph as TransparentWrapper<Glyph>>::wrap_slice(
                fragment.item.glyphs.as_slice(),
            ),
            text: fragment.item.text.as_str(),
            text_offset: fragment.text_offset,
        })
        .collect();
    let logical = prepare_logical_run(
        &fragments,
        &first.font,
        &batch.logical_text,
        first.size.to_f32(),
    );

    surface.push_transform(&fc.state().transform().to_krilla());
    let mut surface = defer(surface, |s| s.pop());
    surface.set_fill(Some(fill));
    surface.set_stroke(stroke);
    if let Some(logical) = logical {
        let units = logical.units(&batch.logical_text);
        surface.draw_pdf_logical_units(
            logical.start,
            &units,
            font,
            first.size.to_f32(),
            false,
        );
    } else {
        for fragment in fragments {
            surface.draw_glyphs(
                fragment.start,
                fragment.glyphs,
                font.clone(),
                fragment.text,
                first.size.to_f32(),
                false,
            );
        }
    }
    Ok(())
}

fn convert_font(
    gc: &mut GlobalContext,
    typst_font: FontInstance,
) -> SourceResult<krilla::text::Font> {
    if let Some(font) = gc.fonts_forward.get(&typst_font) {
        Ok(font.clone())
    } else {
        let font = build_font(typst_font.clone())?;

        gc.fonts_forward.insert(typst_font.clone(), font.clone());
        gc.fonts_backward.insert(font.clone(), typst_font.clone());

        Ok(font)
    }
}

#[comemo::memoize]
fn build_font(typst_font: FontInstance) -> SourceResult<krilla::text::Font> {
    let font_data: Arc<dyn AsRef<[u8]> + Send + Sync> =
        Arc::new(typst_font.data().clone());

    let variations = typst_font
        .variations()
        .0
        .iter()
        .map(|(tag, value)| (krilla::text::Tag::new(&tag.to_bytes()), value.0))
        .collect::<Vec<_>>();

    match krilla::text::Font::new_variable(
        font_data.into(),
        typst_font.index(),
        &variations,
    ) {
        Some(f) => Ok(f),
        None => {
            bail!(
                Span::detached(),
                "failed to process {}",
                display_font(Some(&typst_font)),
            )
        }
    }
}

#[derive(Debug, TransparentWrapper)]
#[repr(transparent)]
struct PdfGlyph(Glyph);

impl krilla::text::Glyph for PdfGlyph {
    #[inline(always)]
    fn glyph_id(&self) -> GlyphId {
        GlyphId::new(self.0.id as u32)
    }

    #[inline(always)]
    fn text_range(&self) -> Range<usize> {
        self.0.range.start as usize..self.0.range.end as usize
    }

    #[inline(always)]
    fn x_advance(&self, size: f32) -> f32 {
        // Don't use `Em::at`, because it contains an expensive check whether the result is finite.
        self.0.x_advance.get() as f32 * size
    }

    #[inline(always)]
    fn x_offset(&self, size: f32) -> f32 {
        // Don't use `Em::at`, because it contains an expensive check whether the result is finite.
        self.0.x_offset.get() as f32 * size
    }

    #[inline(always)]
    fn y_offset(&self, size: f32) -> f32 {
        // Don't use `Em::at`, because it contains an expensive check whether the result is finite.
        self.0.y_offset.get() as f32 * size
    }

    #[inline(always)]
    fn y_advance(&self, size: f32) -> f32 {
        // Don't use `Em::at`, because it contains an expensive check whether the result is finite.
        self.0.y_advance.get() as f32 * size
    }

    fn location(&self) -> Option<Location> {
        Some(self.0.span.0.into_raw())
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU64;

    use typst_library::layout::Em;
    use typst_library::text::Glyph;
    use typst_syntax::Span;

    use super::{extend_baseline_span, merge_text_segments, mixed_source_prefix_shift};

    fn glyph(span: Span, source: u16, range: std::ops::Range<u16>) -> Glyph {
        Glyph {
            id: 0,
            x_advance: Em::zero(),
            x_offset: Em::zero(),
            y_advance: Em::zero(),
            y_offset: Em::zero(),
            range,
            span: (span, source),
        }
    }

    #[test]
    fn merges_overlapping_visual_fragments_once() {
        let cluster = "ខ្ញុំ";
        let paragraph = "ខ្ញុំស្រឡាញ់ភាសាខ្មែរ។";
        let (_, merged) =
            merge_text_segments(&[(100, cluster), (100, cluster), (100, paragraph)])
                .unwrap();
        assert_eq!(merged, paragraph);
    }

    #[test]
    fn keeps_legitimate_repeated_source_text() {
        let cluster = "ខ្ញុំ";
        let (_, merged) =
            merge_text_segments(&[(100, cluster), (100 + cluster.len(), cluster)])
                .unwrap();
        assert_eq!(merged, "ខ្ញុំខ្ញុំ");
    }

    #[test]
    fn rejects_gaps_or_conflicting_overlaps() {
        assert!(merge_text_segments(&[(0, "ab"), (3, "c")]).is_none());
        assert!(merge_text_segments(&[(0, "ab"), (1, "x")]).is_none());
    }

    #[test]
    fn normalizes_a_foreign_prefix_before_shared_source_text() {
        let foreign = Span::from_raw(NonZeroU64::new(1).unwrap());
        let shared = Span::from_raw(NonZeroU64::new(2).unwrap());
        let text = " ក្រសួ";
        let glyphs = vec![
            glyph(foreign, 0, 0..1),
            glyph(shared, 0, 1..10),
            glyph(shared, 9, 10..16),
        ];

        let shift = mixed_source_prefix_shift(text, &glyphs, shared).unwrap();
        let (_, merged) = merge_text_segments(&[(0, text), (9 + shift, "សួង")]).unwrap();

        assert_eq!(shift, 1);
        assert_eq!(merged, " ក្រសួង");
    }

    #[test]
    fn rejects_foreign_text_after_the_prefix() {
        let foreign = Span::from_raw(NonZeroU64::new(1).unwrap());
        let shared = Span::from_raw(NonZeroU64::new(2).unwrap());
        let text = " ក្រ x";
        let glyphs = vec![
            glyph(foreign, 0, 0..1),
            glyph(shared, 0, 1..10),
            glyph(foreign, 1, 10..12),
        ];

        assert_eq!(mixed_source_prefix_shift(text, &glyphs, shared), None);
    }

    #[test]
    fn allows_mark_offsets_but_rejects_the_next_visual_line() {
        assert_eq!(extend_baseline_span(70.0, 70.0, 75.5, 8.0), Some((70.0, 75.5)));
        assert_eq!(extend_baseline_span(70.0, 75.5, 88.0, 8.0), None);
    }
}
