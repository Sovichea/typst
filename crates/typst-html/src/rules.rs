use std::num::NonZeroUsize;
use std::sync::Arc;

use az::SaturatingAs;
use comemo::Track;
use ecow::{EcoVec, eco_format};
use typst_library::diag::{At, warning};
use typst_library::foundations::{
    Content, Context, NativeElement, NativeRuleMap, Selector, ShowFn, Smart, StyleChain,
    Target,
};use typst_library::introspection::{
    Counter, DocumentIntrospection, Locator, QueryIntrospection,
};
use typst_library::layout::resolve::{Cell, CellGrid, Entry, Header};
use typst_library::layout::{
    AlignElem, Alignment, BlockElem, Celled, ColbreakElem, ColumnsElem, GridCell, GridElem,
    HAlignment, HElem, Length, OuterVAlignment, PagebreakElem, Rel, Sides, Sizing,
};
use typst_library::math::EquationElem;
use typst_library::math::ir::resolve_equation;
use typst_library::model::{
    Attribution, BibliographyElem, CiteElem, CiteGroup, CslIndentElem, CslLightElem,
    Destination, DirectLinkElem, DividerElem, EarlyLinkResolver, EmphElem, EnumElem,
    FigureCaption, FigureElem, FootnoteContainer, FootnoteElem, FootnoteEntry,
    FootnoteMarker, HeadingElem, LinkElem, LinkTarget, ListElem, OutlineElem,
    OutlineEntry, OutlineNode, ParElem, ParbreakElem, QuoteElem, RefElem, StrongElem,
    TableCell, TableElem, TermsElem, TitleElem, Works,
};
use typst_library::routines::Arenas;
use typst_library::text::{
    HighlightElem, LinebreakElem, OverlineElem, RawElem, RawLine, SmallcapsElem,
    SpaceElem, StrikeElem, SubElem, SuperElem, TextElem, UnderlineElem,
};
use typst_library::visualize::{Color, ImageElem, LineElem, Paint};
use typst_syntax::Span;

use crate::mathml::convert_math_to_nodes;
use crate::{FrameElem, HtmlAttr, HtmlAttrs, HtmlElem, HtmlTag, attr, css, tag};

/// Registers show rules for the [HTML target](Target::Html).
pub fn register(rules: &mut NativeRuleMap) {
    use Target::{Html, Paged};

    // Model.
    rules.register(Html, PAR_RULE);
    rules.register(Html, STRONG_RULE);
    rules.register(Html, EMPH_RULE);
    rules.register(Html, LIST_RULE);
    rules.register(Html, ENUM_RULE);
    rules.register(Html, TERMS_RULE);
    rules.register(Html, LINK_RULE);
    rules.register(Html, DIRECT_LINK_RULE);
    rules.register(Html, DIVIDER_RULE);
    rules.register(Html, TITLE_RULE);
    rules.register(Html, HEADING_RULE);
    rules.register(Html, FIGURE_RULE);
    rules.register(Html, FIGURE_CAPTION_RULE);
    rules.register(Html, QUOTE_RULE);
    rules.register(Html, FOOTNOTE_RULE);
    rules.register(Html, FOOTNOTE_MARKER_RULE);
    rules.register(Html, FOOTNOTE_CONTAINER_RULE);
    rules.register(Html, FOOTNOTE_ENTRY_RULE);
    rules.register(Html, OUTLINE_RULE);
    rules.register(Html, OUTLINE_ENTRY_RULE);
    rules.register(Html, REF_RULE);
    rules.register(Html, CITE_GROUP_RULE);
    rules.register(Html, BIBLIOGRAPHY_RULE);
    rules.register(Html, CSL_LIGHT_RULE);
    rules.register(Html, CSL_INDENT_RULE);
    rules.register(Html, TABLE_RULE);
    rules.register(Html, TABLE_CELL_RULE);
    rules.register(Html, GRID_RULE);
    rules.register(Html, COLUMNS_RULE);
    rules.register(Html, COLBREAK_RULE);
    rules.register(Html, LINE_RULE);
    rules.register(Html, PAGEBREAK_RULE);
    rules.register(Html, ALIGN_RULE);

    // Text.
    rules.register(Html, SUB_RULE);
    rules.register(Html, SUPER_RULE);
    rules.register(Html, UNDERLINE_RULE);
    rules.register(Html, OVERLINE_RULE);
    rules.register(Html, STRIKE_RULE);
    rules.register(Html, HIGHLIGHT_RULE);
    rules.register(Html, SMALLCAPS_RULE);
    rules.register(Html, RAW_RULE);
    rules.register(Html, RAW_LINE_RULE);

    // Visualize.
    rules.register(Html, IMAGE_RULE);

    // Math.
    rules.register(Html, EQUATION_RULE);

    // For the HTML target, `html.frame` is a primitive. In the laid-out target,
    // it should be a no-op so that nested frames don't break (things like `show
    // math.equation: html.frame` can result in nested ones).
    rules.register::<FrameElem>(Paged, |elem, _, _| Ok(elem.body.clone()));
}

const PAR_RULE: ShowFn<ParElem> = |elem, _, styles| {
    let mut paragraph = HtmlElem::new(tag::p).with_body(Some(elem.body.clone()));
    if elem.justify.get(styles) {
        paragraph = paragraph.with_attr(attr::style, "text-align: justify");
    }
    Ok(paragraph.pack())
};

const STRONG_RULE: ShowFn<StrongElem> =
    |elem, _, _| Ok(HtmlElem::new(tag::strong).with_body(Some(elem.body.clone())).pack());

const EMPH_RULE: ShowFn<EmphElem> =
    |elem, _, _| Ok(HtmlElem::new(tag::em).with_body(Some(elem.body.clone())).pack());

const LIST_RULE: ShowFn<ListElem> = |elem, _, styles| {
    Ok(BlockElem::packed(
        HtmlElem::new(tag::ul)
            .with_body(Some(Content::sequence(elem.children.iter().map(|item| {
                // Text in wide lists shall always turn into paragraphs.
                let mut body = item.body.clone();
                if !elem.tight.get(styles) {
                    body += ParbreakElem::shared();
                }
                HtmlElem::new(tag::li)
                    .with_body(Some(body))
                    .pack()
                    .spanned(item.span())
            }))))
            .pack()
            .spanned(elem.span()),
    ))
};

const ENUM_RULE: ShowFn<EnumElem> = |elem, _, styles| {
    let mut ol = HtmlElem::new(tag::ol);

    if elem.reversed.get(styles) {
        ol = ol.with_attr(attr::reversed, "reversed");
    }

    if let Some(n) = elem.start.get(styles).custom() {
        ol = ol.with_attr(attr::start, eco_format!("{n}"));
    }

    let body = Content::sequence(elem.children.iter().map(|item| {
        let mut li = HtmlElem::new(tag::li);
        if let Smart::Custom(nr) = item.number.get(styles) {
            li = li.with_attr(attr::value, eco_format!("{nr}"));
        }
        // Text in wide enums shall always turn into paragraphs.
        let mut body = item.body.clone();
        if !elem.tight.get(styles) {
            body += ParbreakElem::shared();
        }
        li.with_body(Some(body)).pack().spanned(item.span())
    }));

    Ok(BlockElem::packed(ol.with_body(Some(body)).pack().spanned(elem.span())))
};

const TERMS_RULE: ShowFn<TermsElem> = |elem, _, styles| {
    Ok(BlockElem::packed(
        HtmlElem::new(tag::dl)
            .with_body(Some(Content::sequence(elem.children.iter().flat_map(|item| {
                // Text in wide term lists shall always turn into paragraphs.
                let mut description = item.description.clone();
                if !elem.tight.get(styles) {
                    description += ParbreakElem::shared();
                }

                [
                    HtmlElem::new(tag::dt)
                        .with_body(Some(item.term.clone()))
                        .pack()
                        .spanned(item.term.span()),
                    HtmlElem::new(tag::dd)
                        .with_body(Some(description))
                        .pack()
                        .spanned(item.description.span()),
                ]
            }))))
            .pack()
            .spanned(elem.span()),
    ))
};

// Also check `PATCHED_LINK_RULE` in `docs/src/main.rs` when editing this.
const LINK_RULE: ShowFn<LinkElem> = |elem, engine, _| {
    let span = elem.span();
    let dest = elem.dest.resolve_early(engine, span)?;

    let href = match dest {
        Destination::Url(url) => Some(url.clone().into_inner()),
        Destination::Position(_) => {
            engine
                .sink
                .warn(warning!(span, "positional link was ignored during HTML export"));
            None
        }
        Destination::Location(location) => Some(
            EarlyLinkResolver::new(elem.location().unwrap(), span)
                .resolve(engine, location)
                .and_then(|link| link.into_relative_uri())
                .at(span)?,
        ),
    };

    Ok(HtmlElem::new(tag::a)
        .with_optional_attr(attr::href, href)
        .with_body(Some(elem.body.clone()))
        .pack())
};

const DIRECT_LINK_RULE: ShowFn<DirectLinkElem> = |elem, _, _| {
    Ok(LinkElem::new(
        LinkTarget::Dest(Destination::Location(elem.loc)),
        elem.body.clone(),
    )
    .pack())
};

const DIVIDER_RULE: ShowFn<DividerElem> = |elem, _, _| {
    Ok(BlockElem::packed(HtmlElem::new(tag::hr).pack().spanned(elem.span())))
};

/// A `#line` becomes a `<hr>` carrying its thickness and color, so a downstream
/// consumer can reproduce it (previously it was dropped).
const LINE_RULE: ShowFn<LineElem> = |elem, _, styles| {
    let stroke = elem.stroke.get_cloned(styles);
    let thickness = stroke.thickness.unwrap_or(Length::zero()).abs.to_pt().max(0.5);
    let color = match stroke.paint.unwrap_or(Paint::Solid(Color::BLACK)) {
        Paint::Solid(color) => color.to_hex().trim_start_matches('#').to_string(),
        _ => "000000".to_string(),
    };
    let style = eco_format!("border-top-width: {thickness}pt; border-top-color: #{color}");
    Ok(BlockElem::packed(
        HtmlElem::new(tag::hr)
            .with_attr(attr::style, style)
            .pack()
            .spanned(elem.span()),
    ))
};

/// A `#pagebreak()` becomes a `<div class="pagebreak">` so a downstream
/// consumer can emit a page break.
const PAGEBREAK_RULE: ShowFn<PagebreakElem> = |elem, _, _| {
    Ok(BlockElem::packed(
        HtmlElem::new(tag::div)
            .with_attr(attr::class, "pagebreak")
            .pack()
            .spanned(elem.span()),
    ))
};

/// Keep column content and explicit breaks in the semantic tree. The DOCX
/// exporter can then place each column in a borderless layout grid.
const COLUMNS_RULE: ShowFn<ColumnsElem> = |elem, _, styles| {
    let gutter = elem.gutter.get(styles);
    let style = eco_format!(
        "column-count: {}; column-gap: calc({}pt + {}%)",
        elem.count.get(styles).get(),
        gutter.abs.abs.to_pt() + gutter.abs.em.get() * styles.resolve(TextElem::size).to_pt(),
        gutter.rel.get() * 100.0,
    );
    Ok(BlockElem::packed(
        HtmlElem::new(tag::div)
            .with_attr(attr::class, "typst-columns")
            .with_attr(attr::style, style)
            .with_body(Some(elem.body.clone()))
            .pack()
            .spanned(elem.span()),
    ))
};

const COLBREAK_RULE: ShowFn<ColbreakElem> = |elem, _, _| {
    Ok(BlockElem::packed(
        HtmlElem::new(tag::div)
            .with_attr(attr::class, "typst-colbreak")
            .with_attr(attr::style, "break-before: column")
            .pack()
            .spanned(elem.span()),
    ))
};

/// `#align` is not yet a first-class HTML concept, but its content must not be
/// dropped. Emit it as a block-level `<div>`, carrying a horizontal `text-align`
/// when one is requested.
const ALIGN_RULE: ShowFn<AlignElem> = |elem, _, styles| {
    let mut div = HtmlElem::new(tag::div).with_body(Some(elem.body.clone()));
    if let Some(horizontal) = elem.alignment.get(styles).x() {
        let value = match horizontal {
            HAlignment::Center => "center",
            HAlignment::Right | HAlignment::End => "right",
            _ => "left",
        };
        div = div.with_attr(attr::style, eco_format!("text-align: {value}"));
    }
    Ok(BlockElem::packed(div.pack().spanned(elem.span())))
};

const TITLE_RULE: ShowFn<TitleElem> = |elem, _, styles| {
    Ok(BlockElem::packed(
        HtmlElem::new(tag::h1)
            .with_body(Some(elem.resolve_body(styles).at(elem.span())?))
            .pack()
            .spanned(elem.span()),
    ))
};

const HEADING_RULE: ShowFn<HeadingElem> = |elem, engine, styles| {
    let span = elem.span();

    let mut realized = elem.body.clone();
    if let Some(numbering) = elem.numbering.get_ref(styles).as_ref() {
        let location = elem.location().unwrap();
        let numbering = Counter::of(HeadingElem::ELEM)
            .display_at(engine, location, styles, numbering, span)?
            .spanned(span);
        realized = numbering + SpaceElem::shared().clone() + realized;
    }

    // HTML's h1 is closer to a title element. There should only be one.
    // Meanwhile, a level 1 Typst heading is a section heading. For this
    // reason, levels are offset by one: A Typst level 1 heading becomes
    // a `<h2>`.
    let level = elem.resolve_level(styles).get();
    Ok(BlockElem::packed(if level >= 6 {
        engine.sink.warn(warning!(
            span,
            "heading of level {} was transformed to \
             <div role=\"heading\" aria-level=\"{}\">, which is not \
             supported by all assistive technology",
            level, level + 1;
            hint: "HTML only supports <h1> to <h6>, not <h{}>", level + 1;
            hint: "you may want to restructure your document so that \
                   it doesn't contain deep headings";
        ));
        HtmlElem::new(tag::div)
            .with_body(Some(realized))
            .with_attr(attr::role, "heading")
            .with_attr(attr::aria_level, eco_format!("{}", level + 1))
            .pack()
            .spanned(elem.span())
    } else {
        let t = [tag::h2, tag::h3, tag::h4, tag::h5, tag::h6][level - 1];
        HtmlElem::new(t).with_body(Some(realized)).pack().spanned(elem.span())
    }))
};

const FIGURE_RULE: ShowFn<FigureElem> = |elem, _, styles| {
    let span = elem.span();
    let gap = elem.gap.get(styles);
    let gap_pt = gap.abs.to_pt() + gap.em.get() * 10.5;
    let mut realized = elem.body.clone();

    // Build the caption, if any.
    if let Some(caption) = elem.caption.get_cloned(styles) {
        realized = match caption.position.get(styles) {
            OuterVAlignment::Top => caption.pack() + realized,
            OuterVAlignment::Bottom => realized + caption.pack(),
        };
    }

    // Ensure that the body is considered a paragraph.
    realized += ParbreakElem::shared().clone().spanned(span);

    Ok(BlockElem::packed(
        HtmlElem::new(tag::figure)
            .with_attr(attr::style, eco_format!("text-align: center; figure-gap: {gap_pt}pt"))
            .with_body(Some(realized))
            .pack()
            .spanned(elem.span()),
    ))
};

const FIGURE_CAPTION_RULE: ShowFn<FigureCaption> = |elem, engine, styles| {
    let numbered = elem.numbering.as_ref().is_some_and(Option::is_some)
        && elem.counter.as_ref().is_some_and(Option::is_some)
        && elem.supplement.as_ref().is_some_and(Option::is_some);
    let mut caption = HtmlElem::new(tag::figcaption);
    if numbered {
        caption = caption.with_attr(attr::class, "typst-numbered-caption");
    }
    Ok(BlockElem::packed(
        caption
            .with_body(Some(elem.realize(engine, styles)?))
            .pack()
            .spanned(elem.span()),
    ))
};

const QUOTE_RULE: ShowFn<QuoteElem> = |elem, _, styles| {
    let span = elem.span();
    let block = elem.block.get(styles);

    let mut realized = elem.body.clone();

    if elem.quotes.get(styles).unwrap_or(!block) {
        realized = QuoteElem::quoted(realized, styles);
    }

    let attribution = elem.attribution.get_ref(styles);

    if block {
        let mut blockquote = HtmlElem::new(tag::blockquote).with_body(Some(realized));
        if let Some(Attribution::Content(attribution)) = attribution
            && let Some(link) = attribution.to_packed::<LinkElem>()
            && let LinkTarget::Dest(Destination::Url(url)) = &link.dest
        {
            blockquote = blockquote.with_attr(attr::cite, url.clone().into_inner());
        }

        realized = BlockElem::packed(blockquote.pack().spanned(span));

        if let Some(attribution) = attribution.as_ref() {
            realized += attribution.realize(span);
            realized += ParbreakElem::shared();
        }
    } else if let Some(Attribution::Label(label)) = attribution {
        realized += SpaceElem::shared().clone();
        realized += CiteElem::new(*label).pack().spanned(span);
    }

    Ok(realized)
};

const FOOTNOTE_RULE: ShowFn<FootnoteElem> = |elem, engine, styles| {
    let span = elem.span();

    // The footnote number that links to the footnote entry.
    let link = elem.realize(engine, styles)?;
    let sup = SuperElem::new(link)
        .pack()
        .styled(HtmlElem::role.set(Some("doc-noteref".into())))
        .spanned(span);

    // Indicates the presence of a default footnote rule to emit an error when
    // no footnote container is available.
    let marker = FootnoteMarker::new().pack().spanned(span);

    Ok(HElem::hole().clone() + sup + marker)
};

const FOOTNOTE_MARKER_RULE: ShowFn<FootnoteMarker> = |_, _, _| Ok(Content::empty());

const FOOTNOTE_CONTAINER_RULE: ShowFn<FootnoteContainer> = |elem, engine, _| {
    let mut selector = FootnoteElem::ELEM.select();

    // In bundle export, we only want the footnotes in the current document.
    if let Some(doc_location) =
        engine.introspect(DocumentIntrospection(elem.location().unwrap(), elem.span()))
    {
        selector = Selector::Within {
            selector: Arc::new(FootnoteElem::ELEM.select()),
            ancestor: Arc::new(doc_location.into()),
        };
    }

    // Create entries for all footnotes in the document.
    let notes = engine.introspect(QueryIntrospection(selector, elem.span()));
    let items = notes.into_iter().filter_map(|note| {
        let note = note.into_packed::<FootnoteElem>().unwrap();
        if note.is_ref() {
            return None;
        }

        let loc = note.location().unwrap();
        let span = note.span();
        Some(
            HtmlElem::new(tag::li)
                .with_body(Some(FootnoteEntry::new(note).pack().spanned(span)))
                .with_parent(loc)
                .pack()
                .located(loc.variant(1))
                .spanned(span),
        )
    });

    // Don't create a container if we filtered out all notes.
    let mut items = items.peekable();
    if items.peek().is_none() {
        return Ok(Content::empty());
    }

    // There can be multiple footnotes in a container, so they semantically
    // represent an ordered list. However, the list is already numbered with the
    // footnote superscripts in the DOM, so we turn off CSS' list enumeration.
    let list = HtmlElem::new(tag::ol)
        .with_css(css::Properties::new().with("list-style-type", "none"))
        .with_body(Some(Content::sequence(items)))
        .pack();

    // The user may want to style the whole footnote element so we wrap it in an
    // additional selectable container. This is also how it's done in the ARIA
    // spec (although there, the section also contains an additional heading).
    Ok(BlockElem::packed(
        HtmlElem::new(tag::section)
            .with_attr(attr::role, "doc-endnotes")
            .with_body(Some(list))
            .pack()
            .spanned(elem.span()),
    ))
};

const FOOTNOTE_ENTRY_RULE: ShowFn<FootnoteEntry> = |elem, engine, styles| {
    let (sup, body) = elem.realize(engine, styles)?;

    // The prefix is a link back to the first footnote reference, so
    // `doc-backlink` is the appropriate ARIA role.
    let prefix = sup
        .styled(HtmlElem::role.set(Some("doc-backlink".into())))
        .spanned(elem.span());

    // We do not use the ARIA role `doc-footnote` because it "is only for
    // representing individual notes that occur within the body of a work" (see
    // <https://www.w3.org/TR/dpub-aria-1.1/#doc-footnote>). Our footnotes more
    // appropriately modelled as ARIA endnotes. This is also in line with how
    // Pandoc handles footnotes.
    Ok(prefix + body)
};

const OUTLINE_RULE: ShowFn<OutlineElem> = |elem, engine, styles| {
    fn convert_list(list: Vec<OutlineNode>) -> Content {
        // The Digital Publishing ARIA spec also proposed to add
        // `role="directory"` to the `<ol>` element, but this role is
        // deprecated, so we don't do that. The elements are already easily
        // selectable via `nav[role="doc-toc"] ol`.
        HtmlElem::new(tag::ol)
            .with_css(css::Properties::new().with("list-style-type", "none"))
            .with_body(Some(Content::sequence(list.into_iter().map(convert_node))))
            .pack()
    }

    fn convert_node(node: OutlineNode) -> Content {
        let body = if !node.children.is_empty() {
            // The `<div>` is not technically necessary, but otherwise it
            // auto-wraps in a `<p>`, which results in bad spacing. Perhaps, we
            // can remove this in the future. See also:
            // <https://github.com/typst/typst/issues/5907>
            HtmlElem::new(tag::div).with_body(Some(node.entry.pack())).pack()
                + convert_list(node.children)
        } else {
            node.entry.pack()
        };
        HtmlElem::new(tag::li).with_body(Some(body)).pack()
    }

    let title = elem.realize_title(styles);
    let tree = elem.realize_tree(engine, styles)?;
    let list = convert_list(tree);

    Ok(BlockElem::packed(
        HtmlElem::new(tag::nav)
            .with_attr(attr::role, "doc-toc")
            .with_body(Some(title.unwrap_or_default() + list))
            .pack()
            .spanned(elem.span()),
    ))
};

const OUTLINE_ENTRY_RULE: ShowFn<OutlineEntry> = |elem, engine, styles| {
    let span = elem.span();
    let context = Context::new(None, Some(styles));

    let mut realized = elem.body().at(span)?;

    if let Some(prefix) = elem.prefix(engine, context.track(), span)? {
        let wrapped = HtmlElem::new(tag::span)
            .with_attr(attr::class, "prefix")
            .with_body(Some(prefix))
            .pack()
            .spanned(span);

        let separator = match elem.element.to_packed::<FigureElem>() {
            Some(elem) => elem.resolve_separator(styles),
            None => SpaceElem::shared().clone(),
        };

        realized = Content::sequence([wrapped, separator, realized]);
    }

    let loc = elem.element_location().at(span)?;
    let dest = Destination::Location(loc);

    Ok(LinkElem::new(dest.into(), realized).pack())
};

const REF_RULE: ShowFn<RefElem> = |elem, engine, styles| elem.realize(engine, styles);

const CITE_GROUP_RULE: ShowFn<CiteGroup> = |elem, engine, _| {
    Ok(elem
        .realize(engine)?
        .styled(HtmlElem::role.set(Some("doc-biblioref".into()))))
};

// For the bibliography, we have a few elements that should be styled (e.g.
// indent), but inline styles are not apprioriate because they couldn't be
// properly overridden. For those, we currently emit classes so that a user can
// style them with CSS, but do not emit any styles ourselves.
const BIBLIOGRAPHY_RULE: ShowFn<BibliographyElem> = |elem, engine, styles| {
    let loc = elem.location().unwrap();
    let span = elem.span();
    let works = Works::generate(engine, elem.span())?;
    let bibliography = works.bibliography(loc, span)?;

    let items = bibliography.entries.iter().map(|entry| {
        let mut realized = entry.body.clone();

        if let Some(mut prefix) = entry.prefix.clone() {
            // If we have a link back to the first citation referencing this
            // entry, attach the appropriate role.
            if prefix.is::<DirectLinkElem>() {
                prefix = prefix.set(HtmlElem::role, Some("doc-backlink".into()));
            }

            let wrapped = HtmlElem::new(tag::span)
                .with_attr(attr::class, "prefix")
                .with_body(Some(prefix))
                .pack()
                .spanned(span);

            let separator = SpaceElem::shared().clone();
            realized = Content::sequence([wrapped, separator, realized]);
        }

        HtmlElem::new(tag::li)
            .with_body(Some(realized))
            .pack()
            .located(entry.backlink)
            .spanned(span)
    });

    let title = elem.realize_title(styles);
    let list = HtmlElem::new(tag::ul)
        .with_css(css::Properties::new().with("list-style-type", "none"))
        .with_body(Some(Content::sequence(items)))
        .pack()
        .spanned(span);

    Ok(BlockElem::packed(
        HtmlElem::new(tag::section)
            .with_attr(attr::role, "doc-bibliography")
            .with_optional_attr(
                attr::class,
                bibliography.hanging_indent.then_some("hanging-indent"),
            )
            .with_body(Some(title.unwrap_or_default() + list))
            .pack()
            .spanned(elem.span()),
    ))
};

const CSL_LIGHT_RULE: ShowFn<CslLightElem> = |elem, _, _| {
    Ok(HtmlElem::new(tag::span)
        .with_attr(attr::class, "light")
        .with_body(Some(elem.body.clone()))
        .pack())
};

const CSL_INDENT_RULE: ShowFn<CslIndentElem> = |elem, _, _| {
    Ok(BlockElem::packed(
        HtmlElem::new(tag::div)
            .with_attr(attr::class, "indent")
            .with_body(Some(elem.body.clone()))
            .pack()
            .spanned(elem.span()),
    ))
};

const TABLE_RULE: ShowFn<TableElem> = |elem, _, styles| {
    let grid = elem.grid.as_ref().unwrap();
    Ok(show_cellgrid(grid, styles, elem.span(), false, &elem.align.get_cloned(styles), &elem.inset.get_cloned(styles)))
};

/// A `grid` has the same resolved cell structure as a `table`, so it exports as
/// a `<table>` too (previously it was dropped entirely). It is marked so a
/// downstream consumer knows it has no borders (unlike a `table`).
const GRID_RULE: ShowFn<GridElem> = |elem, _, styles| {
    let grid = elem.grid.as_ref().unwrap();
    Ok(show_cellgrid(grid, styles, elem.span(), true, &elem.align.get_cloned(styles), &elem.inset.get_cloned(styles)))
};

/// A cell column's inset as a CSS `padding` value (`T R B L`).
fn column_padding(
    inset: &Celled<Sides<Option<Rel<Length>>>>,
    column: usize,
) -> String {
    let value = match inset {
        Celled::Value(value) => value.clone(),
        Celled::Array(array) if !array.is_empty() => array[column % array.len()].clone(),
        _ => Sides::splat(None),
    };
    let side = |side: Option<Rel<Length>>| {
        side.map(|rel| rel.relative_to(Length::zero()).abs.to_pt())
            .unwrap_or(0.0)
    };
    format!(
        "{}pt {}pt {}pt {}pt",
        side(value.top),
        side(value.right),
        side(value.bottom),
        side(value.left)
    )
}

/// A cell column's horizontal alignment as a CSS `text-align` value.
fn column_align(align: &Celled<Smart<Alignment>>, column: usize) -> &'static str {
    let value = match align {
        Celled::Value(value) => value.clone(),
        Celled::Array(array) if !array.is_empty() => array[column % array.len()].clone(),
        _ => Smart::Auto,
    };
    alignment_css(&value)
}

fn alignment_css(value: &Smart<Alignment>) -> &'static str {
    match value {
        Smart::Custom(alignment) => match alignment.x() {
            Some(HAlignment::Center) => "center",
            Some(HAlignment::Right) | Some(HAlignment::End) => "right",
            Some(HAlignment::Left) | Some(HAlignment::Start) => "left",
            None => "",
        },
        Smart::Auto => "",
    }
}

fn show_cellgrid(
    grid: &CellGrid,
    styles: StyleChain,
    span: Span,
    borderless: bool,
    align: &Celled<Smart<Alignment>>,
    inset: &Celled<Sides<Option<Rel<Length>>>>,
) -> Content {
    let elem = |tag, body| HtmlElem::new(tag).with_body(Some(body)).pack().spanned(span);
    let mut rows: Vec<_> = grid.entries.chunks(grid.non_gutter_column_count()).collect();

    let tr = |tag, row: &[Entry]| {
        let mut column = 0;
        let mut cells = Vec::new();
        for entry in row {
            let Some(cell) = entry.as_cell() else { continue };
            cells.push(show_cell(
                tag,
                cell,
                styles,
                column_align(align, column),
                &column_padding(inset, column),
            ));
            column += cell.colspan.get();
        }
        elem(tag::tr, Content::sequence(cells))
    };

    // TODO(subfooters): similarly to headers, take consecutive footers from
    // the end for 'tfoot'.
    let footer = grid.footer.as_ref().map(|ft| {
        // Convert from gutter to non-gutter coordinates. Use ceil as it might
        // include the previous gutter row
        // (cf. typst-library/layout/grid/resolve.rs).
        let footer_start = if grid.has_gutter { ft.start.div_ceil(2) } else { ft.start };
        let rows = rows.drain(footer_start..);
        elem(tag::tfoot, Content::sequence(rows.map(|row| tr(tag::td, row))))
    });

    // Header range converting from gutter (doubled) to non-gutter coordinates.
    let header_range = |hd: &Header| {
        if grid.has_gutter {
            // Use ceil as it might be `2 * row_amount - 1` if the header is at
            // the end (cf. typst-library/layout/grid/resolve.rs).
            hd.range.start / 2..hd.range.end.div_ceil(2)
        } else {
            hd.range.clone()
        }
    };

    // Store all consecutive headers at the start in 'thead'. All remaining
    // headers are just 'th' rows across the table body.
    let mut consecutive_header_end = 0;
    let first_mid_table_header = grid
        .headers
        .iter()
        .take_while(|hd| {
            let range = header_range(hd);
            let is_consecutive = range.start == consecutive_header_end;
            consecutive_header_end = range.end;
            is_consecutive
        })
        .count();

    let (y_offset, header) = if first_mid_table_header > 0 {
        let removed_header_rows =
            header_range(grid.headers.get(first_mid_table_header - 1).unwrap()).end;
        let rows = rows.drain(..removed_header_rows);

        (
            removed_header_rows,
            Some(elem(tag::thead, Content::sequence(rows.map(|row| tr(tag::th, row))))),
        )
    } else {
        (0, None)
    };

    // TODO: Consider improving accessibility properties of multi-level headers
    // inside tables in the future, e.g. indicating which columns they are
    // relative to and so on. See also:
    // https://www.w3.org/WAI/tutorials/tables/multi-level/
    let mut next_header = first_mid_table_header;
    let mut body =
        Content::sequence(rows.into_iter().enumerate().map(|(relative_y, row)| {
            let y = relative_y + y_offset;
            if let Some(current_header_range) =
                grid.headers.get(next_header).map(|h| header_range(h))
                && current_header_range.contains(&y)
            {
                if y + 1 == current_header_range.end {
                    next_header += 1;
                }

                tr(tag::th, row)
            } else {
                tr(tag::td, row)
            }
        }));

    if header.is_some() || footer.is_some() {
        body = elem(tag::tbody, body);
    }

    let content = header.into_iter().chain(core::iter::once(body)).chain(footer);

    // Carry the column tracks (and gutter) so a downstream consumer can
    // reproduce the grid's columns exactly instead of guessing equal widths.
    let tracks: Vec<String> = if grid.has_gutter {
        grid.cols.iter().step_by(2).copied().map(sizing_css).collect()
    } else {
        grid.cols.iter().copied().map(sizing_css).collect()
    };
    let gutter = if grid.has_gutter {
        grid.cols.get(1).copied().map(sizing_css)
    } else {
        None
    };

    let mut attrs = HtmlAttrs::new();
    attrs.push(attr::class, if borderless { "grid" } else { "table" });
    let mut style = eco_format!("grid-template-columns: {}", tracks.join(" "));
    if let Some(gutter) = gutter {
        style.push_str(&eco_format!("; column-gap: {gutter}"));
    }
    attrs.push(attr::style, style);

    BlockElem::packed(
        HtmlElem::new(tag::table)
            .with_body(Some(Content::sequence(content)))
            .with_attrs(attrs)
            .pack()
            .spanned(span),
    )
}

/// Serialize a track sizing to a CSS value.
fn sizing_css(sizing: Sizing) -> String {
    match sizing {
        Sizing::Auto => "auto".to_string(),
        Sizing::Fr(fr) => format!("{}fr", fr.get()),
        Sizing::Rel(rel) => {
            let length = rel.relative_to(Length::zero());
            if length.em.get() != 0.0 {
                format!("{}em", length.em.get())
            } else {
                format!("{}pt", length.abs.to_pt())
            }
        }
    }
}

fn show_cell(
    tag: HtmlTag,
    cell: &Cell,
    styles: StyleChain,
    align: &str,
    padding: &str,
) -> Content {
    let body = cell.body.clone();
    let cell_align = body
        .to_packed::<TableCell>()
        .map(|cell| alignment_css(&cell.align.get(styles)))
        .or_else(|| {
            body.to_packed::<GridCell>()
                .map(|cell| alignment_css(&cell.align.get(styles)))
        })
        .filter(|value| !value.is_empty())
        .unwrap_or(align);
    let span = |n: NonZeroUsize| (n != NonZeroUsize::MIN).then(|| n.to_string());
    let mut attrs = HtmlAttrs::new();
    let mut style = String::new();
    if !cell_align.is_empty() {
        style.push_str(&eco_format!("text-align: {cell_align}; "));
    }
    if !padding.is_empty() {
        style.push_str(&eco_format!("padding: {padding}"));
    }
    if !style.is_empty() {
        attrs.push(attr::style, style);
    }

    let (content, source) = if let Some(table) = body.to_packed::<TableCell>() {
        if let Some(colspan) = span(table.colspan.get(styles)) {
            attrs.push(attr::colspan, colspan);
        }
        if let Some(rowspan) = span(table.rowspan.get(styles)) {
            attrs.push(attr::rowspan, rowspan);
        }
        (table.body.clone(), table.span())
    } else if let Some(grid) = body.to_packed::<GridCell>() {
        if let Some(colspan) = span(grid.colspan.get(styles)) {
            attrs.push(attr::colspan, colspan);
        }
        if let Some(rowspan) = span(grid.rowspan.get(styles)) {
            attrs.push(attr::rowspan, rowspan);
        }
        (grid.body.clone(), grid.span())
    } else {
        (body, Span::detached())
    };

    HtmlElem::new(tag)
        .with_body(Some(content))
        .with_attrs(attrs)
        .pack()
        .spanned(source)
}

const TABLE_CELL_RULE: ShowFn<TableCell> = |elem, _, _| Ok(elem.body.clone());

const SUB_RULE: ShowFn<SubElem> =
    |elem, _, _| Ok(HtmlElem::new(tag::sub).with_body(Some(elem.body.clone())).pack());

const SUPER_RULE: ShowFn<SuperElem> =
    |elem, _, _| Ok(HtmlElem::new(tag::sup).with_body(Some(elem.body.clone())).pack());

const UNDERLINE_RULE: ShowFn<UnderlineElem> = |elem, _, _| {
    // Note: In modern HTML, `<u>` is not the underline element, but
    // rather an "Unarticulated Annotation" element (see HTML spec
    // 4.5.22). Using `text-decoration` instead is recommended by MDN.
    Ok(HtmlElem::new(tag::span)
        .with_css(css::Properties::new().with("text-decoration", "underline"))
        .with_body(Some(elem.body.clone()))
        .pack())
};

const OVERLINE_RULE: ShowFn<OverlineElem> = |elem, _, _| {
    Ok(HtmlElem::new(tag::span)
        .with_css(css::Properties::new().with("text-decoration", "overline"))
        .with_body(Some(elem.body.clone()))
        .pack())
};

const STRIKE_RULE: ShowFn<StrikeElem> =
    |elem, _, _| Ok(HtmlElem::new(tag::s).with_body(Some(elem.body.clone())).pack());

const HIGHLIGHT_RULE: ShowFn<HighlightElem> =
    |elem, _, _| Ok(HtmlElem::new(tag::mark).with_body(Some(elem.body.clone())).pack());

const SMALLCAPS_RULE: ShowFn<SmallcapsElem> = |elem, _, styles| {
    let variant = if elem.all.get(styles) { "all-small-caps" } else { "small-caps" };
    Ok(HtmlElem::new(tag::span)
        .with_css(css::Properties::new().with("font-variant-caps", variant))
        .with_body(Some(elem.body.clone()))
        .pack())
};

const RAW_RULE: ShowFn<RawElem> = |elem, _, styles| {
    let lines = elem.lines.as_deref().unwrap_or_default();

    let mut seq = EcoVec::with_capacity((2 * lines.len()).saturating_sub(1));
    for (i, line) in lines.iter().enumerate() {
        if i != 0 {
            seq.push(LinebreakElem::shared().clone());
        }

        seq.push(line.clone().pack());
    }

    let lang = elem.lang.get_ref(styles);
    let code = HtmlElem::new(tag::code)
        .with_optional_attr(const { HtmlAttr::constant("data-lang") }, lang.clone())
        .with_body(Some(Content::sequence(seq)))
        .pack()
        .spanned(elem.span());

    Ok(if elem.block.get(styles) {
        BlockElem::packed(
            HtmlElem::new(tag::pre)
                .with_body(Some(code))
                .pack()
                .spanned(elem.span()),
        )
    } else {
        code
    })
};

/// This is used by `RawElem::synthesize` through a routine.
///
/// It's a temporary workaround until `TextElem::fill` is supported in HTML
/// export.
#[doc(hidden)]
pub fn html_span_filled(content: Content, color: Color) -> Content {
    let span = content.span();
    HtmlElem::new(tag::span)
        .with_css(css::Properties::build(()).with("color", color).finish())
        .with_body(Some(content))
        .pack()
        .spanned(span)
}

const RAW_LINE_RULE: ShowFn<RawLine> = |elem, _, _| Ok(elem.body.clone());

// Also check `PATCHED_IMAGE_RULE` in `docs/src/main.rs` when editing this.
const IMAGE_RULE: ShowFn<ImageElem> = |elem, engine, styles| {
    let image = elem.decode(engine, styles)?;

    let mut attrs = HtmlAttrs::new();
    let src = typst_svg::WebImage::new(&image).to_base64_url();
    attrs.push(attr::src, src);

    if let Some(alt) = elem.alt.get_cloned(styles) {
        attrs.push(attr::alt, alt);
    }

    // The `width` and `height` properties on the HTML element are only used to
    // reserve space while the browser is fetching. They are integers. Still, in
    // case of fractional image sizes, rounding is better than nothing and will
    // not disrupt the aspect ratio of the final image.
    let cast = |v: f64| eco_format!("{}", v.round().saturating_as::<i64>());
    attrs.push(attr::width, cast(image.width()));
    attrs.push(attr::height, cast(image.height()));

    let mut css = css::Properties::build((engine, elem.span()));

    // TODO: Exclude in semantic profile.
    if let Some(value) = typst_svg::convert_image_scaling(image.scaling()) {
        css.push("image-rendering", value);
    }

    // TODO: Exclude in semantic profile?
    match elem.width.get(styles) {
        Smart::Auto => {}
        Smart::Custom(rel) => css.push("width", rel),
    }

    // TODO: Exclude in semantic profile?
    match elem.height.get(styles) {
        Sizing::Auto => {}
        Sizing::Rel(rel) => css.push("height", rel),
        Sizing::Fr(_) => {}
    }

    Ok(BlockElem::packed(
        HtmlElem::new(tag::img)
            .with_attrs(attrs)
            .with_css(css.finish())
            .pack()
            .spanned(elem.span()),
    ))
};

const EQUATION_RULE: ShowFn<EquationElem> = |elem, engine, styles| {
    let arenas = Arenas::default();
    let item = resolve_equation(
        elem,
        engine,
        Locator::synthesize(elem.location().unwrap()),
        &arenas,
        styles,
    )?;

    let block = elem.block.get(styles);
    let body = convert_math_to_nodes(item, engine, styles, block)?;
    let span = elem.span();
    let number = if block {
        elem.numbering
            .get_ref(styles)
            .as_ref()
            .map(|numbering| {
                Counter::of(EquationElem::ELEM)
                    .display_at(engine, elem.location().unwrap(), styles, numbering, span)
                    .map(|content| content.plain_text())
            })
            .transpose()?
    } else {
        None
    };
    let mut math = HtmlElem::new(tag::mathml::math)
        .with_body(Some(Content::sequence(body)))
        .with_optional_attr(attr::mathml::display, block.then_some("block"));
    if let Some(number) = number {
        math = math.with_attr(attr::data_typst_equation_number, number);
    }
    let math = math.pack().spanned(span);

    Ok(if block { BlockElem::packed(math) } else { math })
};

/// Returns the body of a MathML `HtmlElem`, if the content is one.
#[doc(hidden)]
pub fn html_mathml_body<'a>(
    content: &'a Content,
    styles: StyleChain<'a>,
) -> Option<Option<&'a Content>> {
    let elem = content.to_packed::<HtmlElem>()?;
    tag::mathml::is_mathml(elem.tag).then(|| elem.body.get_ref(styles).as_ref())
}
