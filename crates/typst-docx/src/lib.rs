//! Typst's DOCX (WordprocessingML) exporter — proof of concept.
//!
//! Converts Typst's evaluated semantic structure (the HTML DOM produced by
//! `typst-html`) into an editable `.docx` with native Word styles, lists and
//! tables. This is the "semantic" half of the dual semantic+layout pipeline;
//! geometry refinement from the paged layout is future work.

use base64::Engine as _;
use std::collections::HashMap;
use std::io::{Cursor, Write};

use ecow::eco_format;
use typst_html::{HtmlDocument, HtmlElement, HtmlNode, HtmlTag, attr, tag};
use typst_library::diag::StrResult;
use typst_library::layout::Abs;
use typst_layout::PagedDocument;
use zip::ZipWriter;
use zip::write::SimpleFileOptions;

pub mod layout;

pub use layout::layout_json;

/// Convert a Typst HTML document into DOCX bytes.
///
/// When the paged `layout` is provided, the typography resolved by the compiler
/// is measured from it and baked into the generated Word styles, so the `.docx`
/// matches what Typst actually rendered.
pub fn docx(
    document: &HtmlDocument,
    layout: Option<&PagedDocument>,
) -> StrResult<Vec<u8>> {
    let all = layout.map(layout::collect_runs).unwrap_or_default();
    let body_runs: Vec<layout::Run> = all
        .iter()
        .filter(|run| run.region == layout::PageRegion::Body)
        .cloned()
        .collect();
    let header_runs: Vec<layout::Run> = all
        .iter()
        .filter(|run| run.region == layout::PageRegion::Header && run.page == 1)
        .cloned()
        .collect();
    let footer_runs: Vec<layout::Run> = all
        .iter()
        .filter(|run| run.region == layout::PageRegion::Footer && run.page == 1)
        .cloned()
        .collect();

    let all_images = layout.map(layout::collect_images).unwrap_or_default();
    let body_images: Vec<layout::ImageInfo> = all_images
        .iter()
        .filter(|image| image.region == layout::PageRegion::Body)
        .cloned()
        .collect();
    let header_images: Vec<layout::ImageInfo> = all_images
        .iter()
        .filter(|image| image.region == layout::PageRegion::Header && image.page == 1)
        .cloned()
        .collect();
    let footer_images: Vec<layout::ImageInfo> = all_images
        .iter()
        .filter(|image| image.region == layout::PageRegion::Footer && image.page == 1)
        .cloned()
        .collect();

    let (content_left, content_right) = layout
        .and_then(|doc| doc.pages().first())
        .map(|page| {
            let size = page.frame.size();
            (page.margin.left.to_pt(), (size.x - page.margin.right).to_pt())
        })
        .unwrap_or((0.0, 450.0));
    let content_width_pt = content_right - content_left;

    let mut em = Emitter::new(&body_runs, content_width_pt, false, body_images);
    if let Some(body) = find_body(document.root()) {
        for child in &body.children {
            em.block(child);
        }
    }
    em.flush_all_rules();

    let mut measured = finalize_measurements(&em.style_samples);
    compute_spacing(&body_runs, &mut measured, &em.blocks, content_left);

    // The page header/footer are emitted structurally by the HTML export (grids,
    // rules, alignment); their typography is recovered from the layout runs in
    // that region.
    let header = region_from_html(
        document.root(),
        tag::header,
        &header_runs,
        content_width_pt,
        header_images,
    );
    let footer = region_from_html(
        document.root(),
        tag::footer,
        &footer_runs,
        content_width_pt,
        footer_images,
    );
    let (header_dist, footer_dist) = header_footer_distances(&all, layout);

    let document_xml = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n\
         <w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\" \
         xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\">\
         <w:body>{}{}</w:body></w:document>",
        em.out,
        sect_pr(layout, header.is_some(), footer.is_some(), header_dist, footer_dist)
    );

    package(
        &document_xml,
        &styles(&measured),
        &numbering(
            &em.ordered_num_ids,
            measured
                .get("ListParagraph")
                .map(|m| (m.indent_pt * 20.0).round() as i64)
                .unwrap_or(720),
        ),
        header.as_deref(),
        footer.as_deref(),
        &em.images,
        &em.hyperlinks,
    )
}

/// Build a header/footer part from the HTML `<header>`/`<footer>` element,
/// typed by the layout runs in that region.
fn region_from_html(
    root: &HtmlElement,
    tag_name: HtmlTag,
    runs: &[layout::Run],
    content_width_pt: f64,
    images: Vec<layout::ImageInfo>,
) -> Option<String> {
    let el = find_element(root, tag_name)?;
    let is_footer = tag_name == tag::footer;
    let mut em = Emitter::new(runs, content_width_pt, is_footer, images);
    for child in &el.children {
        em.block(child);
    }
    let root_name = if tag_name == tag::header { "w:hdr" } else { "w:ftr" };
    Some(format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n\
         <{root_name} xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\" \
         xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\">\
         {}</{root_name}>",
        em.out
    ))
}

/// Find the first descendant element with the given tag.
fn find_element<'a>(el: &'a HtmlElement, tag_name: HtmlTag) -> Option<&'a HtmlElement> {
    if el.tag == tag_name {
        return Some(el);
    }
    for child in &el.children {
        if let HtmlNode::Element(child) = child {
            if let Some(found) = find_element(child, tag_name) {
                return Some(found);
            }
        }
    }
    None
}

fn find_body(el: &HtmlElement) -> Option<&HtmlElement> {
    if el.tag == tag::body {
        return Some(el);
    }
    for child in &el.children {
        if let HtmlNode::Element(child) = child {
            if let Some(found) = find_body(child) {
                return Some(found);
            }
        }
    }
    None
}

/// A single formatted run of text.
#[derive(Clone)]
struct Run {
    text: String,
    bold: bool,
    italic: bool,
    mono: bool,
    /// Whether this run is a hard line break (`<br>` / `\`).
    br: bool,
    /// The external URL this run links to, if any.
    href: Option<String>,
}

/// Typography measured from the paged layout for a given Word style.
#[derive(Clone)]
struct Measured {
    family: String,
    size_pt: f64,
    bold: bool,
    italic: bool,
    color: String,
    /// The block spacing below this style, in points.
    after_pt: f64,
    /// The left indentation of this style, in points.
    indent_pt: f64,
    /// The baseline-to-baseline line pitch of this style, in points.
    line_pt: f64,
}

/// Resolved typography of a single run, measured from the layout.
#[derive(Clone, PartialEq)]
struct Typography {
    family: String,
    size_pt: f64,
    bold: bool,
    italic: bool,
    color: String,
}

/// A semantic block (paragraph) emitted from the HTML tree.
struct Block {
    style: String,
    text: String,
}

/// An embedded image part.
struct Media {
    rel_id: String,
    name: String,
    data: Vec<u8>,
    ext: String,
    content_type: String,
}

/// Accumulates the document body XML.
struct Emitter<'a> {
    out: String,
    runs: &'a [layout::Run],
    /// Per-style samples of resolved typography, reduced to a representative
    /// value after emission.
    style_samples: HashMap<&'static str, Vec<Typography>>,
    blocks: Vec<Block>,
    /// Monotonic cursor into `runs` used to match inline runs to layout runs.
    cursor: usize,
    /// The current paragraph alignment (`w:jc` value), set while recursing into
    /// an aligned container.
    align: Option<String>,
    /// Next numbering id handed out to a new ordered list.
    next_num_id: u32,
    /// Ordered-list numbering ids that were handed out.
    ordered_num_ids: Vec<u32>,
    /// Direct left indent (twips) for the next paragraph, if any.
    indent: Option<i64>,
    /// Body horizontal rules, in document order.
    body_rules: Vec<layout::Rule>,
    /// Next unconsumed body rule.
    rule_cursor: usize,
    /// The y of the most recently matched block, for interleaving rules.
    last_y: f64,
    /// The available content width in points, for resolving grid tracks.
    content_width_pt: f64,
    /// Whether this emitter is building a footer (so a digits-only run becomes
    /// a `PAGE` field).
    is_footer: bool,
    /// Placed images, for sizing an `<img>` (in document order).
    layout_images: Vec<layout::ImageInfo>,
    /// Next unconsumed layout image.
    image_cursor: usize,
    /// Embedded image parts collected while emitting.
    images: Vec<Media>,
    /// External hyperlink targets, in order of first use.
    hyperlinks: Vec<String>,
}

impl Emitter<'_> {
    /// Create an emitter over the given layout runs.
    fn new(
        runs: &[layout::Run],
        content_width_pt: f64,
        is_footer: bool,
        layout_images: Vec<layout::ImageInfo>,
    ) -> Emitter<'_> {
        Emitter {
            out: String::new(),
            runs,
            style_samples: HashMap::new(),
            blocks: Vec::new(),
            cursor: 0,
            align: None,
            next_num_id: 100,
            ordered_num_ids: Vec::new(),
            indent: None,
            body_rules: Vec::new(),
            rule_cursor: 0,
            last_y: 0.0,
            content_width_pt,
            is_footer,
            layout_images,
            image_cursor: 0,
            images: Vec::new(),
            hyperlinks: Vec::new(),
        }
    }

    /// The relationship id for an external URL, allocating one if new.
    fn hyperlink_id(&mut self, url: &str) -> String {
        if let Some(index) = self.hyperlinks.iter().position(|h| h == url) {
            return format!("rId{}", 200 + index + 1);
        }
        self.hyperlinks.push(url.to_string());
        format!("rId{}", 200 + self.hyperlinks.len())
    }

    fn block(&mut self, node: &HtmlNode) {
        match node {
            HtmlNode::Element(el) => self.block_el(el),
            HtmlNode::Text(text, _) => {
                let trimmed = text.trim();
                if !trimmed.is_empty() {
                    let runs = vec![Run {
                        text: trimmed.to_string(),
                        bold: false,
                        italic: false,
                        mono: false,
                        br: false,
                        href: None,
                    }];
                    self.paragraph("Normal", &runs, None);
                }
            }
            _ => {}
        }
    }

    fn block_el(&mut self, el: &HtmlElement) {
        let t = el.tag;
        // Typst's HTML export reserves <h1> for the document title (`#set
        // document(title: ...)`) and offsets section headings by one, so `=` ->
        // <h2>, `==` -> <h3>, ... Map `=` -> Heading 1, `==` -> Heading 2, ...
        // (and the document title -> Title).
        if t == tag::h1 {
            let runs = inline(&el.children, false, false, false, None);
            self.paragraph("Title", &runs, None);
        } else if t == tag::h2 {
            let runs = inline(&el.children, false, false, false, None);
            self.paragraph("Heading1", &runs, None);
        } else if t == tag::h3 {
            let runs = inline(&el.children, false, false, false, None);
            self.paragraph("Heading2", &runs, None);
        } else if t == tag::h4 {
            let runs = inline(&el.children, false, false, false, None);
            self.paragraph("Heading3", &runs, None);
        } else if t == tag::h5 || t == tag::h6 {
            let runs = inline(&el.children, false, false, false, None);
            self.paragraph("Heading4", &runs, None);
        } else if t == tag::p {
            let runs = inline(&el.children, false, false, false, None);
            self.paragraph("Normal", &runs, None);
        } else if t == tag::blockquote {
            let runs = inline(&el.children, false, true, false, None);
            self.paragraph("Quote", &runs, None);
        } else if t == tag::pre {
            let runs = inline(&el.children, false, false, true, None);
            self.paragraph("Code", &runs, None);
        } else if t == tag::figcaption || t == tag::caption {
            let runs = inline(&el.children, false, true, false, None);
            self.paragraph("Caption", &runs, None);
        } else if t == tag::ul {
            if list_style_none(el) {
                self.plain_list(el, 0);
            } else {
                self.list(el, false, 1, 0);
            }
        } else if t == tag::ol {
            if list_style_none(el) {
                self.plain_list(el, 0);
            } else {
                let num_id = self.alloc_num_id();
                self.list(el, true, num_id, 0);
            }
        } else if t == tag::table {
            self.table(el);
        } else if t == tag::img {
            self.image(el);
        } else if t == tag::header || t == tag::footer {
            // Written to the DOCX header/footer parts separately.
        } else if t == tag::hr {
            let style = el.attrs.get(attr::style).map(|s| s.as_str()).unwrap_or("");
            let (thickness, color) = parse_hr_style(style);
            self.out.push_str(&rule_paragraph(thickness, &color, 0));
        } else if el.attrs.get(attr::class).map(|c| c.as_str()) == Some("pagebreak") {
            self.out
                .push_str("<w:p><w:r><w:br w:type=\"page\"/></w:r></w:p>");
        } else {
            // div/section/figure/body/... : honor a `text-align`, then either
            // recurse (block children) or emit inline content as one paragraph.
            let previous = self.align.clone();
            if let Some(align) = el.attrs.get(attr::style).and_then(|s| parse_text_align(s)) {
                self.align = Some(align);
            }
            if has_block_child(el) {
                for child in &el.children {
                    self.block(child);
                }
            } else {
                let runs = inline(&el.children, false, false, false, None);
                if !runs.is_empty() {
                    self.paragraph("Normal", &runs, None);
                }
            }
            self.align = previous;
        }
    }

    fn paragraph(&mut self, style: &str, runs: &[Run], num: Option<(u32, u32)>) {
        let typos = self.measure_runs(runs);
        self.flush_rules();
        self.record_sample(style, &typos);
        self.blocks.push(Block {
            style: style.to_string(),
            text: runs.iter().map(|r| r.text.as_str()).collect(),
        });
        self.out.push_str("<w:p><w:pPr>");
        self.out
            .push_str(&format!("<w:pStyle w:val=\"{style}\"/>"));
        if let Some((num_id, ilvl)) = num {
            self.out.push_str(&format!(
                "<w:numPr><w:ilvl w:val=\"{ilvl}\"/><w:numId w:val=\"{num_id}\"/></w:numPr>"
            ));
        } else if let Some(indent) = self.indent {
            self.out.push_str(&format!("<w:ind w:left=\"{indent}\"/>"));
        }
        if let Some(align) = self.align.clone() {
            self.out.push_str(&format!("<w:jc w:val=\"{align}\"/>"));
        }
        self.out.push_str("</w:pPr>");
        for (run, typo) in runs.iter().zip(typos.iter()) {
            self.run(run, typo.as_ref());
        }
        self.out.push_str("</w:p>");
    }

    /// Record the dominant resolved typography of a block as a sample for its
    /// style. The representative value is chosen after emission, so an outlier
    /// block (e.g. the large company name in a letterhead) can't skew a style.
    fn record_sample(&mut self, style: &str, typos: &[Option<Typography>]) {
        let key: &'static str = match style {
            "Normal" => "Normal",
            "Title" => "Title",
            "Heading1" => "Heading1",
            "Heading2" => "Heading2",
            "Heading3" => "Heading3",
            "Heading4" => "Heading4",
            "Quote" => "Quote",
            "Caption" => "Caption",
            "Code" => "Code",
            "ListParagraph" => "ListParagraph",
            _ => return,
        };
        let values: Vec<Typography> = typos.iter().flatten().cloned().collect();
        if let Some(typo) = mode_typography(&values) {
            self.style_samples.entry(key).or_default().push(typo);
        }
    }

    /// Measure the resolved typography of each run by matching it to a layout
    /// run. Returns a direct override for every matched run.
    fn measure_runs(&mut self, runs: &[Run]) -> Vec<Option<Typography>> {
        let mut overrides = vec![None; runs.len()];
        let mut cursor = self.cursor;
        let mut first_y: Option<f64> = None;

        for (index, run) in runs.iter().enumerate() {
            if run.br {
                continue;
            }
            let target = collapse(&run.text);
            if target.len() < 3 {
                continue;
            }

            let found = if cursor < self.runs.len() {
                self.runs[cursor..].iter().position(|layout_run| {
                    let text = collapse(&layout_run.text);
                    !text.is_empty() && (text.starts_with(&target) || target.starts_with(&text))
                })
            } else {
                None
            };

            if let Some(offset) = found {
                let layout_run = &self.runs[cursor + offset];
                cursor += offset + 1;
                if first_y.is_none() {
                    first_y = Some(layout_run.y_pt);
                }
                overrides[index] = Some(Typography {
                    family: layout_run.family.clone(),
                    size_pt: layout_run.size_pt,
                    bold: layout_run.bold,
                    italic: layout_run.italic,
                    color: layout_run.color.clone(),
                });
            }
        }

        if let Some(y) = first_y {
            self.last_y = y;
        }
        self.cursor = cursor;
        overrides
    }

    /// Emit any body rule that sits above the current block, so rules are
    /// interleaved with the HTML-driven body in document order.
    fn flush_rules(&mut self) {
        while self.rule_cursor < self.body_rules.len()
            && self.body_rules[self.rule_cursor].y_pt < self.last_y
        {
            let rule = self.body_rules[self.rule_cursor].clone();
            self.out.push_str(&rule_paragraph(rule.thickness_pt, &rule.color, 0));
            self.rule_cursor += 1;
        }
    }

    /// Emit any body rules left after the last block.
    fn flush_all_rules(&mut self) {
        while self.rule_cursor < self.body_rules.len() {
            let rule = self.body_rules[self.rule_cursor].clone();
            self.out.push_str(&rule_paragraph(rule.thickness_pt, &rule.color, 0));
            self.rule_cursor += 1;
        }
    }

    fn run(&mut self, run: &Run, typo: Option<&Typography>) {
        if run.br {
            self.out.push_str("<w:r><w:br/></w:r>");
            return;
        }
        // A digits-only run in a footer is the page number: emit a PAGE field
        // so it updates on every page instead of repeating the literal.
        if self.is_footer {
            let text = run.text.trim();
            if !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit()) {
                let mut rpr = String::new();
                if let Some(typo) = typo {
                    let size = (typo.size_pt * 2.0).round().max(2.0) as i64;
                    rpr.push_str(&format!(
                        "<w:rFonts w:ascii=\"{0}\" w:hAnsi=\"{0}\" w:cs=\"{0}\"/>",
                        escape_xml(&typo.family)
                    ));
                    if typo.bold {
                        rpr.push_str("<w:b/>");
                    }
                    if typo.italic {
                        rpr.push_str("<w:i/>");
                    }
                    rpr.push_str(&format!(
                        "<w:color w:val=\"{}\"/><w:sz w:val=\"{size}\"/><w:szCs w:val=\"{size}\"/>",
                        typo.color
                    ));
                }
                self.out.push_str(&format!(
                    "<w:fldSimple w:instr=\" PAGE \"><w:r><w:rPr>{rpr}</w:rPr>\
                     <w:t xml:space=\"preserve\">{text}</w:t></w:r></w:fldSimple>"
                ));
                return;
            }
        }
        let hyperlink = match &run.href {
            Some(url) => {
                let id = self.hyperlink_id(url);
                self.out.push_str(&format!("<w:hyperlink r:id=\"{id}\">"));
                Some(id)
            }
            None => None,
        };
        self.out.push_str("<w:r>");
        if let Some(typo) = typo {
            let size = (typo.size_pt * 2.0).round().max(2.0) as i64;
            self.out.push_str("<w:rPr>");
            self.out.push_str(&format!(
                "<w:rFonts w:ascii=\"{0}\" w:hAnsi=\"{0}\" w:cs=\"{0}\"/>",
                escape_xml(&typo.family)
            ));
            if typo.bold {
                self.out.push_str("<w:b/>");
            }
            if typo.italic {
                self.out.push_str("<w:i/>");
            }
            self.out.push_str(&format!(
                "<w:color w:val=\"{}\"/><w:sz w:val=\"{size}\"/><w:szCs w:val=\"{size}\"/>",
                typo.color
            ));
            self.out.push_str("</w:rPr>");
        } else if run.bold || run.italic || run.mono {
            self.out.push_str("<w:rPr>");
            if run.mono {
                self.out.push_str(
                    "<w:rFonts w:ascii=\"Consolas\" w:hAnsi=\"Consolas\" w:cs=\"Consolas\"/>",
                );
            }
            if run.bold {
                self.out.push_str("<w:b/>");
            }
            if run.italic {
                self.out.push_str("<w:i/>");
            }
            self.out.push_str("</w:rPr>");
        }
        self.out.push_str("<w:t xml:space=\"preserve\">");
        self.out.push_str(&escape_xml(&run.text));
        self.out.push_str("</w:t></w:r>");
        if hyperlink.is_some() {
            self.out.push_str("</w:hyperlink>");
        }
    }

    /// Emit a list, recursing into nested lists. `ordered` selects whether a
    /// nested list continues this list's numbering or starts its own.
    fn list(&mut self, el: &HtmlElement, ordered: bool, num_id: u32, level: u32) {
        for child in &el.children {
            let HtmlNode::Element(li) = child else { continue };
            if li.tag != tag::li {
                continue;
            }
            // Gather inline text from non-list children; recurse into nested lists.
            let mut runs = Vec::new();
            for grand in &li.children {
                match grand {
                    HtmlNode::Element(g)
                        if g.tag == tag::ul || g.tag == tag::ol =>
                    {
                        // handled below
                    }
                    _ => collect_inline(grand, false, false, false, None, &mut runs),
                }
            }
            if runs.iter().all(|r| r.text.trim().is_empty()) {
                runs = vec![Run {
                    text: String::new(),
                    bold: false,
                    italic: false,
                    mono: false,
                    br: false,
                    href: None,
                }];
            }
            self.paragraph("ListParagraph", &runs, Some((num_id, level.min(8))));
            for grand in &li.children {
                if let HtmlNode::Element(g) = grand {
                    if g.tag == tag::ul {
                        self.list(g, false, 1, level + 1);
                    } else if g.tag == tag::ol {
                        let nested = if ordered { num_id } else { self.alloc_num_id() };
                        self.list(g, true, nested, level + 1);
                    }
                }
            }
        }
    }

    /// Emit a list whose HTML carries no markers (e.g. an outline), as plain
    /// indented paragraphs without Word numbering.
    fn plain_list(&mut self, el: &HtmlElement, level: u32) {
        for child in &el.children {
            let HtmlNode::Element(li) = child else { continue };
            if li.tag != tag::li {
                continue;
            }
            let mut runs = Vec::new();
            for grand in &li.children {
                match grand {
                    HtmlNode::Element(g) if g.tag == tag::ul || g.tag == tag::ol => {}
                    _ => collect_inline(grand, false, false, false, None, &mut runs),
                }
            }
            self.indent = Some(360 * (level as i64 + 1));
            self.paragraph("ListParagraph", &runs, None);
            self.indent = None;

            for grand in &li.children {
                if let HtmlNode::Element(g) = grand {
                    if g.tag == tag::ul || g.tag == tag::ol {
                        self.plain_list(g, level + 1);
                    }
                }
            }
        }
    }

    /// Allocate a fresh numbering id for a new ordered list so its counter
    /// restarts at 1.
    fn alloc_num_id(&mut self) -> u32 {
        let id = self.next_num_id;
        self.next_num_id += 1;
        self.ordered_num_ids.push(id);
        id
    }

    /// Measure a text's width from the layout runs, if a run matches.
    fn measure_width(&self, text: &str) -> Option<f64> {
        let target = collapse(text);
        if target.is_empty() {
            return None;
        }
        self.runs
            .iter()
            .find(|run| collapse(&run.text) == target)
            .map(|run| run.width_pt)
    }

    /// Emit an `<img>` as an inline drawing, embedding the image data.
    fn image(&mut self, el: &HtmlElement) {
        let Some(src) = el.attrs.get(attr::src) else { return };
        let Some((mime, data)) = parse_data_uri(src) else { return };
        let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(data) else {
            return;
        };

        let ext = mime_to_ext(mime);
        let index = self.images.len() + 1;
        let rel_id = format!("rId{}", 100 + index);
        self.images.push(Media {
            rel_id: rel_id.clone(),
            name: format!("image{index}.{ext}"),
            data: bytes,
            ext: ext.to_string(),
            content_type: mime.to_string(),
        });

        let (width, height) = self
            .layout_images
            .get(self.image_cursor)
            .map(|image| (image.width_pt, image.height_pt))
            .unwrap_or((100.0, 100.0));
        self.image_cursor += 1;
        let cx = (width * 12700.0).round() as i64;
        let cy = (height * 12700.0).round() as i64;

        self.out.push_str(&format!(
            "<w:p><w:pPr><w:spacing w:before=\"0\" w:after=\"0\" w:line=\"240\" \
             w:lineRule=\"auto\"/></w:pPr>\
             <w:r><w:drawing><wp:inline \
             xmlns:wp=\"http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing\" \
             distT=\"0\" distB=\"0\" distL=\"0\" distR=\"0\">\
             <wp:extent cx=\"{cx}\" cy=\"{cy}\"/>\
             <wp:docPr id=\"{index}\" name=\"Picture {index}\"/>\
             <a:graphic xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\">\
             <a:graphicData uri=\"http://schemas.openxmlformats.org/drawingml/2006/picture\">\
             <pic:pic xmlns:pic=\"http://schemas.openxmlformats.org/drawingml/2006/picture\">\
             <pic:nvPicPr><pic:cNvPr id=\"{index}\" name=\"Picture {index}\"/>\
             <pic:cNvPicPr/></pic:nvPicPr>\
             <pic:blipFill><a:blip r:embed=\"{rel_id}\"/>\
             <a:stretch><a:fillRect/></a:stretch></pic:blipFill>\
             <pic:spPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"{cx}\" cy=\"{cy}\"/></a:xfrm>\
             <a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom></pic:spPr>\
             </pic:pic></a:graphicData></a:graphic></wp:inline></w:drawing></w:r></w:p>"
        ));
    }

    fn table(&mut self, el: &HtmlElement) {
        let mut rows: Vec<&HtmlElement> = Vec::new();
        collect_rows(el, &mut rows);
        if rows.is_empty() {
            return;
        }
        let cols = rows
            .iter()
            .map(|r| r.children.iter().filter(|c| matches!(c, HtmlNode::Element(e) if e.tag == tag::td || e.tag == tag::th)).count())
            .max()
            .unwrap_or(0);
        if cols == 0 {
            return;
        }

        let style = el.attrs.get(attr::style).map(|s| s.as_str()).unwrap_or("");
        let mut col_chars = vec![0usize; cols];
        let mut col_content: Vec<Option<f64>> = vec![None; cols];
        for row in &rows {
            let mut index = 0;
            for cell in &row.children {
                let HtmlNode::Element(c) = cell else { continue };
                if c.tag != tag::td && c.tag != tag::th {
                    continue;
                }
                if index < cols {
                    col_chars[index] = col_chars[index].max(text_len(c));
                    if let Some(width) = self.measure_width(&text_of(c)) {
                        col_content[index] = Some(col_content[index].unwrap_or(0.0).max(width));
                    }
                }
                index += 1;
            }
        }
        let (mut widths, gap) = resolve_tracks(style, self.content_width_pt, &col_content, &col_chars);
        if widths.len() != cols {
            widths = vec![3000; cols];
        }
        let gap_half = gap / 2;
        let total: i64 = widths.iter().sum();

        self.out.push_str(&format!(
            "<w:tbl><w:tblPr><w:tblStyle w:val=\"TableGrid\"/>\
             <w:tblW w:w=\"{total}\" w:type=\"dxa\"/>\
             <w:tblLayout w:type=\"fixed\"/><w:tblCellMar>\
             <w:left w:w=\"{gap_half}\" w:type=\"dxa\"/>\
             <w:right w:w=\"{gap_half}\" w:type=\"dxa\"/></w:tblCellMar>\
             <w:tblLook w:val=\"04A0\"/></w:tblPr><w:tblGrid>"
        ));
        for width in &widths {
            self.out.push_str(&format!("<w:gridCol w:w=\"{width}\"/>"));
        }
        self.out.push_str("</w:tblGrid>");

        for row in rows {
            self.out.push_str("<w:tr>");
            let mut cells = 0;
            for cell in &row.children {
                let HtmlNode::Element(c) = cell else { continue };
                if c.tag != tag::td && c.tag != tag::th {
                    continue;
                }
                let width = widths.get(cells).copied().unwrap_or(3000);
                cells += 1;
                self.out.push_str("<w:tc><w:tcPr>");
                if c.tag == tag::th {
                    self.out
                        .push_str("<w:shd w:val=\"clear\" w:color=\"auto\" w:fill=\"0B3C5D\"/>");
                }
                self.out
                    .push_str(&format!("<w:tcW w:w=\"{width}\" w:type=\"dxa\"/></w:tcPr>"));
                let runs = inline(&c.children, c.tag == tag::th, false, false, None);
                // Cells always contain at least one paragraph.
                if runs.is_empty() {
                    self.paragraph("Normal", &[], None);
                } else {
                    self.paragraph("Normal", &runs, None);
                }
                self.out.push_str("</w:tc>");
            }
            for index in cells..cols {
                let width = widths.get(index).copied().unwrap_or(3000);
                self.out.push_str(&format!(
                    "<w:tc><w:tcPr><w:tcW w:w=\"{width}\" w:type=\"dxa\"/></w:tcPr><w:p/></w:tc>"
                ));
            }
            self.out.push_str("</w:tr>");
        }
        self.out.push_str("</w:tbl>");
        // A table must be followed by a paragraph.
        self.paragraph("Normal", &[], None);
    }
}

fn collect_rows<'a>(el: &'a HtmlElement, rows: &mut Vec<&'a HtmlElement>) {
    if el.tag == tag::tr {
        rows.push(el);
        return;
    }
    for child in &el.children {
        if let HtmlNode::Element(child) = child {
            collect_rows(child, rows);
        }
    }
}

fn inline(
    nodes: &[HtmlNode],
    bold: bool,
    italic: bool,
    mono: bool,
    href: Option<&str>,
) -> Vec<Run> {
    let mut runs = Vec::new();
    for node in nodes {
        collect_inline(node, bold, italic, mono, href, &mut runs);
    }
    runs
}

fn collect_inline(
    node: &HtmlNode,
    bold: bool,
    italic: bool,
    mono: bool,
    href: Option<&str>,
    runs: &mut Vec<Run>,
) {
    match node {
        HtmlNode::Text(text, _) => {
            if !text.is_empty() {
                runs.push(Run {
                    text: text.to_string(),
                    bold,
                    italic,
                    mono,
                    br: false,
                    href: href.map(str::to_string),
                });
            }
        }
        HtmlNode::Element(el) => {
            let t = el.tag;
            let (b, i, m) = if t == tag::strong || t == tag::b {
                (true, italic, mono)
            } else if t == tag::em || t == tag::i {
                (bold, true, mono)
            } else if t == tag::code || t == tag::kbd || t == tag::samp {
                (bold, italic, true)
            } else if t == tag::br {
                runs.push(Run {
                    text: " ".to_string(),
                    bold,
                    italic,
                    mono,
                    br: true,
                    href: None,
                });
                return;
            } else {
                (bold, italic, mono)
            };
            // An external link applies to its children.
            let child_href = if t == tag::a {
                el.attrs
                    .get(attr::href)
                    .map(|h| h.as_str())
                    .filter(|h| h.starts_with("http://") || h.starts_with("https://"))
                    .or(href)
            } else {
                href
            };
            for child in &el.children {
                collect_inline(child, b, i, m, child_href, runs);
            }
        }
        _ => {}
    }
}

/// Whether an element has any block-level child (so it isn't inline content).
fn has_block_child(el: &HtmlElement) -> bool {
    el.children.iter().any(|child| {
        matches!(child, HtmlNode::Element(e) if matches!(
            e.tag,
            tag::p | tag::div | tag::section | tag::table | tag::ul | tag::ol | tag::li
                | tag::h1 | tag::h2 | tag::h3 | tag::h4 | tag::h5 | tag::h6
                | tag::figure | tag::blockquote | tag::pre | tag::hr
                | tag::header | tag::footer | tag::img | tag::figcaption
        ))
    })
}

/// Whether a list element requests no markers (`list-style-type: none`).
fn list_style_none(el: &HtmlElement) -> bool {
    el.attrs
        .get(attr::style)
        .map(|style| {
            style
                .split(';')
                .any(|d| d.replace(' ', "").starts_with("list-style-type:none"))
        })
        .unwrap_or(false)
}

/// Resolve a `grid-template-columns` / `column-gap` style into Word column
/// widths (twips) and a gutter (twips), against the available content width.
fn resolve_tracks(
    style: &str,
    available_pt: f64,
    col_content: &[Option<f64>],
    col_chars: &[usize],
) -> (Vec<i64>, i64) {
    let mut fixed: Vec<Option<f64>> = Vec::new();
    let mut flex: Vec<f64> = Vec::new();
    let mut gap = 0.0;

    for declaration in style.split(';') {
        let declaration = declaration.trim();
        if let Some(value) = declaration.strip_prefix("grid-template-columns:") {
            let mut index = 0;
            for track in value.split_whitespace() {
                if let Some(number) = track.strip_suffix("fr") {
                    fixed.push(None);
                    flex.push(number.trim().parse().unwrap_or(1.0));
                } else if let Some(number) = track.strip_suffix("pt") {
                    fixed.push(Some(number.trim().parse().unwrap_or(0.0)));
                    flex.push(0.0);
                } else if let Some(number) = track.strip_suffix("em") {
                    fixed.push(Some(number.trim().parse::<f64>().unwrap_or(0.0) * 10.5));
                    flex.push(0.0);
                } else {
                    // `auto`: the measured content width from the layout, with
                    // a tolerance (Word's metrics and half-point size rounding
                    // differ from Typst, so an exact width would wrap). Falls
                    // back to a character-count estimate.
                    let measured = col_content.get(index).copied().flatten();
                    let estimate = col_chars.get(index).copied().unwrap_or(0) as f64 * 5.25;
                    fixed.push(Some(measured.unwrap_or(estimate) + 20.0));
                    flex.push(0.0);
                }
                index += 1;
            }
        } else if let Some(value) = declaration.strip_prefix("column-gap:") {
            gap = value
                .trim()
                .strip_suffix("pt")
                .and_then(|n| n.trim().parse().ok())
                .unwrap_or(0.0);
        }
    }

    if fixed.is_empty() {
        return (Vec::new(), 0);
    }

    let total_gap = gap * (fixed.len() as f64 - 1.0).max(0.0);
    let free = (available_pt - total_gap).max(0.0);
    let fixed_sum: f64 = fixed.iter().flatten().sum();
    let flex_sum: f64 = flex.iter().sum();
    let unit = if flex_sum > 0.0 {
        (free - fixed_sum).max(0.0) / flex_sum
    } else {
        0.0
    };

    let widths = fixed
        .iter()
        .zip(flex.iter())
        .map(|(fixed, flex)| ((fixed.unwrap_or(0.0) + flex * unit) * 20.0).round() as i64)
        .collect();

    (widths, (gap * 20.0).round() as i64)
}

/// The concatenated text of an element.
fn text_of(el: &HtmlElement) -> String {
    let mut out = String::new();
    collect_text(el, &mut out);
    out
}

fn collect_text(el: &HtmlElement, out: &mut String) {
    for child in &el.children {
        match child {
            HtmlNode::Text(text, _) => out.push_str(text),
            HtmlNode::Element(element) => collect_text(element, out),
            _ => {}
        }
    }
}

/// The number of characters in an element's text (for `auto` track sizing).
fn text_len(el: &HtmlElement) -> usize {
    let mut count = 0;
    for child in &el.children {
        match child {
            HtmlNode::Text(text, _) => count += text.chars().count(),
            HtmlNode::Element(element) => count += text_len(element),
            _ => {}
        }
    }
    count
}

/// Parse a `data:<mime>;base64,<data>` URI into (mime, base64 data).
fn parse_data_uri(src: &str) -> Option<(&str, &str)> {
    let rest = src.strip_prefix("data:")?;
    let (meta, data) = rest.split_once(',')?;
    if !meta.contains("base64") {
        return None;
    }
    Some((meta.split(';').next()?, data))
}

/// Map an image MIME type to a file extension.
fn mime_to_ext(mime: &str) -> &str {
    match mime {
        "image/jpeg" | "image/jpg" => "jpg",
        "image/gif" => "gif",
        "image/bmp" => "bmp",
        "image/tiff" => "tiff",
        "image/svg+xml" => "svg",
        "image/webp" => "webp",
        _ => "png",
    }
}

/// Parse a `border-top-width`/`border-top-color` style into (thickness pt, color).
fn parse_hr_style(style: &str) -> (f64, String) {
    let mut thickness = 0.5;
    let mut color = "000000".to_string();
    for declaration in style.split(';') {
        let declaration = declaration.trim();
        if let Some(value) = declaration.strip_prefix("border-top-width:") {
            thickness = value
                .trim()
                .strip_suffix("pt")
                .and_then(|n| n.trim().parse().ok())
                .unwrap_or(0.5);
        } else if let Some(value) = declaration.strip_prefix("border-top-color:") {
            color = value.trim().trim_start_matches('#').to_ascii_uppercase();
        }
    }
    (thickness, color)
}

/// Map a CSS `text-align` declaration to a Word `w:jc` value.
fn parse_text_align(style: &str) -> Option<String> {
    for declaration in style.split(';') {
        if let Some(value) = declaration.trim().strip_prefix("text-align:") {
            return Some(match value.trim() {
                "center" => "center".into(),
                "right" => "right".into(),
                "justify" => "both".into(),
                _ => "left".into(),
            });
        }
    }
    None
}

/// Normalize text for matching: keep alphanumerics, collapse whitespace and
/// lowercase, so quotes, dashes and other decoration don't break the match.
fn collapse(text: &str) -> String {
    let filtered: String = text
        .chars()
        .filter(|c| c.is_alphanumeric() || c.is_whitespace())
        .collect();
    filtered.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
}

/// Derive per-style block spacing and indentation from the layout.
///
/// Only the space *below* each block is recorded; `space_before` is left at
/// zero so consecutive blocks don't double up their spacing. Word reflows text,
/// so intra-paragraph line spacing is deliberately not measured.
fn compute_spacing(
    runs: &[layout::Run],
    measured: &mut HashMap<&'static str, Measured>,
    blocks: &[Block],
    margin_left_pt: f64,
) {
    // Locate each block's first run with a monotonic cursor, so repeated text
    // maps to successive occurrences.
    let mut firsts: Vec<Option<usize>> = Vec::with_capacity(blocks.len());
    let mut cursor = 0;
    for block in blocks {
        let target = collapse(&block.text);
        let found = if target.is_empty() || cursor >= runs.len() {
            None
        } else {
            let prefix: String = target.chars().take(24).collect();
            runs[cursor..]
                .iter()
                .position(|run| {
                    let text = collapse(&run.text);
                    !text.is_empty()
                        && (text.starts_with(&prefix) || prefix.starts_with(&text))
                })
                .map(|offset| cursor + offset)
        };
        if let Some(first) = found {
            cursor = first + 1;
        }
        firsts.push(found);
    }

    let mut after: HashMap<&str, Vec<f64>> = HashMap::new();
    let mut indent: HashMap<&str, Vec<f64>> = HashMap::new();
    let mut line: HashMap<&str, Vec<f64>> = HashMap::new();

    for (index, block) in blocks.iter().enumerate() {
        let Some(first) = firsts[index] else { continue };
        let Some(last) = block_last(runs, &firsts, index) else { continue };
        let style = block.style.as_str();

        indent
            .entry(style)
            .or_default()
            .push((runs[first].x_pt - margin_left_pt).max(0.0));

        // Baseline-to-baseline distance between the block's own lines.
        for j in first..last {
            let delta = runs[j + 1].y_pt - runs[j].y_pt;
            if delta > 0.5 {
                line.entry(style).or_default().push(delta);
            }
        }

        let Some(next) = firsts.get(index + 1).copied().flatten() else { continue };
        if runs[next].page == runs[first].page {
            let top = runs[next].y_pt - runs[next].ascent_pt;
            let bottom = runs[last].y_pt + runs[last].descent_pt;
            let gap = top - bottom;
            if gap > -0.5 {
                after.entry(style).or_default().push(gap.max(0.0));
            }
        }
    }

    for (style, m) in measured.iter_mut() {
        if let Some(samples) = after.get(*style) {
            m.after_pt = median(samples);
        }
        if let Some(samples) = indent.get(*style) {
            m.indent_pt = median(samples);
        }
        if let Some(samples) = line.get(*style) {
            m.line_pt = mode_value(samples).unwrap_or(0.0);
        }
    }
}

/// The last layout run belonging to block `k`: the line before the next block's
/// first run, or the final run on the page.
fn block_last(runs: &[layout::Run], firsts: &[Option<usize>], k: usize) -> Option<usize> {
    let first = firsts[k]?;
    let page = runs[first].page;

    if let Some(Some(next)) = firsts.get(k + 1).copied() {
        if runs[next].page == page && next > first {
            return Some(next - 1);
        }
    }

    let mut last = first;
    for (offset, run) in runs[first..].iter().enumerate() {
        if run.page != page {
            break;
        }
        last = first + offset;
    }
    Some(last)
}

/// The median of a non-empty sample set.
fn median(samples: &[f64]) -> f64 {
    let mut values = samples.to_vec();
    values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    values[values.len() / 2]
}

/// Reduce per-style typography samples to a representative value (the mode).
fn finalize_measurements(
    samples: &HashMap<&'static str, Vec<Typography>>,
) -> HashMap<&'static str, Measured> {
    let mut measured = HashMap::new();
    for (style, list) in samples {
        if let Some(typo) = mode_typography(list) {
            measured.insert(
                *style,
                Measured {
                    family: typo.family,
                    size_pt: typo.size_pt,
                    bold: typo.bold,
                    italic: typo.italic,
                    color: typo.color,
                    after_pt: 0.0,
                    indent_pt: 0.0,
                    line_pt: 0.0,
                },
            );
        }
    }
    measured
}

/// The most frequent value in a sample set (bucketed to 0.5), robust to
/// outliers such as a paragraph that was resolved to include extra lines.
fn mode_value(samples: &[f64]) -> Option<f64> {
    let mut counts: HashMap<i64, (usize, f64)> = HashMap::new();
    for &value in samples {
        let bucket = (value * 2.0).round() as i64;
        let entry = counts.entry(bucket).or_insert((0, value));
        entry.0 += 1;
    }
    counts
        .into_iter()
        .max_by(|a, b| a.1.0.cmp(&b.1.0).then(b.0.cmp(&a.0)))
        .map(|(_, (_, value))| value)
}

/// The most frequent typography in a sample set.
fn mode_typography(list: &[Typography]) -> Option<Typography> {
    let mut counts: Vec<(Typography, usize)> = Vec::new();
    for typo in list {
        match counts.iter_mut().find(|(existing, _)| existing == typo) {
            Some((_, count)) => *count += 1,
            None => counts.push((typo.clone(), 1)),
        }
    }
    counts.into_iter().max_by_key(|(_, count)| *count).map(|(typo, _)| typo)
}

fn escape_xml(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            '\n' => out.push(' '),
            '\r' => {}
            '\t' => out.push(' '),
            c if (c as u32) < 0x20 => {}
            c => out.push(c),
        }
    }
    out
}

/// Build the section properties (`w:sectPr`) from the layout's page geometry,
/// so `#set page(...)` is reflected in the `.docx`. Falls back to A4.
fn sect_pr(
    layout: Option<&PagedDocument>,
    header: bool,
    footer: bool,
    header_dist: Option<i64>,
    footer_dist: Option<i64>,
) -> String {
    let header_dist = header_dist.unwrap_or(708);
    let footer_dist = footer_dist.unwrap_or(708);
    let (pg_sz, pg_mar) = match layout.and_then(|doc| doc.pages().first()) {
        Some(page) => {
            let size = page.frame.size();
            let margin = page.margin;
            let twips = |abs: Abs| (abs.to_pt() * 20.0).round() as i64;
            (
                format!(
                    "<w:pgSz w:w=\"{}\" w:h=\"{}\"/>",
                    twips(size.x),
                    twips(size.y)
                ),
                format!(
                    "<w:pgMar w:top=\"{}\" w:right=\"{}\" w:bottom=\"{}\" w:left=\"{}\" \
                     w:header=\"{header_dist}\" w:footer=\"{footer_dist}\" w:gutter=\"0\"/>",
                    twips(margin.top),
                    twips(margin.right),
                    twips(margin.bottom),
                    twips(margin.left),
                ),
            )
        }
        None => (
            "<w:pgSz w:w=\"11906\" w:h=\"16838\"/>".to_string(),
            format!(
                "<w:pgMar w:top=\"1134\" w:right=\"1134\" w:bottom=\"1134\" w:left=\"1134\" \
                 w:header=\"{header_dist}\" w:footer=\"{footer_dist}\" w:gutter=\"0\"/>"
            ),
        ),
    };

    let mut refs = String::new();
    if header {
        refs.push_str("<w:headerReference w:type=\"default\" r:id=\"rId3\"/>");
    }
    if footer {
        refs.push_str("<w:footerReference w:type=\"default\" r:id=\"rId4\"/>");
    }

    format!("<w:sectPr>{refs}{pg_sz}{pg_mar}</w:sectPr>")
}

/// The distances from the page edges to the header/footer, in twips, measured
/// from the layout so they match Typst's header/footer placement.
fn header_footer_distances(
    runs: &[layout::Run],
    layout: Option<&PagedDocument>,
) -> (Option<i64>, Option<i64>) {
    let Some(page) = layout.and_then(|doc| doc.pages().first()) else {
        return (None, None);
    };
    let height = page.frame.size().y.to_pt();

    let header_top = runs
        .iter()
        .filter(|run| run.region == layout::PageRegion::Header)
        .map(|run| run.y_pt - run.ascent_pt)
        .fold(f64::INFINITY, f64::min);
    let footer_bottom = runs
        .iter()
        .filter(|run| run.region == layout::PageRegion::Footer)
        .map(|run| run.y_pt + run.descent_pt)
        .fold(f64::NEG_INFINITY, f64::max);

    let header = header_top.is_finite().then(|| (header_top * 20.0).round() as i64);
    let footer = footer_bottom
        .is_finite()
        .then(|| ((height - footer_bottom) * 20.0).round() as i64);
    (header, footer)
}

/// Render a horizontal rule as an empty paragraph with a bottom border.
fn rule_paragraph(thickness_pt: f64, color: &str, before: i64) -> String {
    let sz = (thickness_pt * 8.0).round().clamp(2.0, 96.0) as i64;
    format!(
        "<w:p><w:pPr><w:pBdr><w:bottom w:val=\"single\" w:sz=\"{sz}\" w:space=\"0\" \
         w:color=\"{color}\"/></w:pBdr>\
         <w:spacing w:before=\"{before}\" w:after=\"0\" w:line=\"20\" w:lineRule=\"exact\"/>\
         </w:pPr></w:p>"
    )
}

fn package(
    document_xml: &str,
    styles: &str,
    numbering: &str,
    header: Option<&str>,
    footer: Option<&str>,
    images: &[Media],
    hyperlinks: &[String],
) -> StrResult<Vec<u8>> {
    let cursor = Cursor::new(Vec::new());
    let mut zip = ZipWriter::new(cursor);
    let opts = SimpleFileOptions::default();

    let write = |zip: &mut ZipWriter<Cursor<Vec<u8>>>,
                 name: &str,
                 data: &str|
     -> StrResult<()> {
        zip.start_file(name, opts)
            .map_err(|e| eco_format!("zip error: {e}"))?;
        zip.write_all(data.as_bytes())
            .map_err(|e| eco_format!("zip write error: {e}"))?;
        Ok(())
    };

    write(
        &mut zip,
        "[Content_Types].xml",
        &content_types(header.is_some(), footer.is_some(), images),
    )?;
    write(&mut zip, "_rels/.rels", ROOT_RELS)?;
    write(&mut zip, "word/document.xml", document_xml)?;
    write(&mut zip, "word/styles.xml", styles)?;
    write(&mut zip, "word/numbering.xml", numbering)?;
    write(
        &mut zip,
        "word/_rels/document.xml.rels",
        &document_rels(header.is_some(), footer.is_some(), images, hyperlinks),
    )?;
    if let Some(header) = header {
        write(&mut zip, "word/header1.xml", header)?;
    }
    if let Some(footer) = footer {
        write(&mut zip, "word/footer1.xml", footer)?;
    }
    for media in images {
        zip.start_file(format!("word/media/{}", media.name), opts)
            .map_err(|e| eco_format!("zip error: {e}"))?;
        zip.write_all(&media.data)
            .map_err(|e| eco_format!("zip write error: {e}"))?;
    }

    let cursor = zip.finish().map_err(|e| eco_format!("zip finish error: {e}"))?;
    Ok(cursor.into_inner())
}

/// The package root relationships.
const ROOT_RELS: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/>
</Relationships>"#;

/// The `[Content_Types].xml` part, including header/footer and image overrides.
fn content_types(header: bool, footer: bool, images: &[Media]) -> String {
    let mut out = String::from(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
<Default Extension="xml" ContentType="application/xml"/>
<Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>
<Override PartName="/word/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml"/>
<Override PartName="/word/numbering.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.numbering+xml"/>"#,
    );
    if header {
        out.push_str("<Override PartName=\"/word/header1.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.header+xml\"/>");
    }
    if footer {
        out.push_str("<Override PartName=\"/word/footer1.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.footer+xml\"/>");
    }
    let mut seen: Vec<&str> = Vec::new();
    for media in images {
        if !seen.contains(&media.ext.as_str()) {
            seen.push(&media.ext);
            out.push_str(&format!(
                "<Default Extension=\"{}\" ContentType=\"{}\"/>",
                media.ext, media.content_type
            ));
        }
    }
    out.push_str("</Types>");
    out
}

/// The document relationships, including header/footer and image parts.
fn document_rels(
    header: bool,
    footer: bool,
    images: &[Media],
    hyperlinks: &[String],
) -> String {
    let mut out = String::from(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/>
<Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/numbering" Target="numbering.xml"/>"#,
    );
    if header {
        out.push_str("<Relationship Id=\"rId3\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/header\" Target=\"header1.xml\"/>");
    }
    if footer {
        out.push_str("<Relationship Id=\"rId4\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/footer\" Target=\"footer1.xml\"/>");
    }
    for media in images {
        out.push_str(&format!(
            "<Relationship Id=\"{}\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/image\" Target=\"media/{}\"/>",
            media.rel_id, media.name
        ));
    }
    for (index, url) in hyperlinks.iter().enumerate() {
        out.push_str(&format!(
            "<Relationship Id=\"rId{}\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink\" Target=\"{}\" TargetMode=\"External\"/>",
            200 + index + 1,
            escape_xml(url)
        ));
    }
    out.push_str("</Relationships>");
    out
}

/// Font size in Word half-points.
fn half_points(measured: &Measured) -> u32 {
    (measured.size_pt * 2.0).round().max(2.0) as u32
}

/// Build `styles.xml`, patching in the typography measured from the layout.
fn styles(measured: &HashMap<&str, Measured>) -> String {
    let mut s = STYLES.to_string();

    if let Some(normal) = measured.get("Normal") {
        s = s.replace(
            "<w:rFonts w:ascii=\"Calibri\" w:hAnsi=\"Calibri\" w:cs=\"Calibri\"/>",
            &format!(
                "<w:rFonts w:ascii=\"{0}\" w:hAnsi=\"{0}\" w:cs=\"{0}\"/>",
                normal.family
            ),
        );
        let sz = half_points(normal);
        s = s.replace(
            "<w:sz w:val=\"22\"/><w:szCs w:val=\"22\"/>",
            &format!("<w:sz w:val=\"{sz}\"/><w:szCs w:val=\"{sz}\"/>"),
        );
    }

    // Patch the measured heading/title runs, keeping their theme colors.
    let patches = [
        ("Title", "<w:b/><w:color w:val=\"0B3C5D\"/><w:sz w:val=\"56\"/>"),
        ("Heading1", "<w:b/><w:color w:val=\"0B3C5D\"/><w:sz w:val=\"40\"/>"),
        ("Heading2", "<w:b/><w:color w:val=\"1D6FA5\"/><w:sz w:val=\"28\"/>"),
        ("Heading3", "<w:b/><w:color w:val=\"1D6FA5\"/><w:sz w:val=\"24\"/>"),
        ("Heading4", "<w:b/><w:sz w:val=\"22\"/>"),
    ];
    for (style, anchor) in patches {
        if let Some(m) = measured.get(style) {
            let sz = half_points(m);
            let mut rpr = String::new();
            if m.bold {
                rpr.push_str("<w:b/>");
            }
            if m.italic {
                rpr.push_str("<w:i/>");
            }
            rpr.push_str(&format!(
                "<w:color w:val=\"{}\"/><w:sz w:val=\"{sz}\"/>",
                m.color
            ));
            s = s.replace(anchor, &rpr);
        }
    }

    // Patch measured block spacing (space below each block) and line pitch.
    // `atLeast` enforces Typst's baseline pitch (which includes leading, so it
    // is larger than the font's natural line height) without clipping.
    let spacing = |m: &Measured| -> String {
        let after = (m.after_pt * 20.0).round() as i64;
        if m.line_pt > 0.0 {
            let line = (m.line_pt * 20.0).round() as i64;
            format!(
                "<w:spacing w:before=\"0\" w:after=\"{after}\" w:line=\"{line}\" \
                 w:lineRule=\"atLeast\"/>"
            )
        } else {
            format!("<w:spacing w:before=\"0\" w:after=\"{after}\"/>")
        }
    };

    let spacing_patches: [(&str, &str, bool); 6] = [
        ("Title", "<w:spacing w:before=\"240\" w:after=\"120\"/>", false),
        (
            "Heading1",
            "<w:keepNext/><w:spacing w:before=\"360\" w:after=\"120\"/>",
            true,
        ),
        (
            "Heading2",
            "<w:keepNext/><w:spacing w:before=\"240\" w:after=\"80\"/>",
            true,
        ),
        (
            "Heading3",
            "<w:keepNext/><w:spacing w:before=\"200\" w:after=\"60\"/>",
            true,
        ),
        (
            "Heading4",
            "<w:keepNext/><w:spacing w:before=\"180\" w:after=\"60\"/>",
            true,
        ),
        ("Caption", "<w:spacing w:after=\"160\"/>", false),
    ];
    for (style, anchor, keep_next) in spacing_patches {
        if let Some(m) = measured.get(style) {
            let keep = if keep_next { "<w:keepNext/>" } else { "" };
            s = s.replace(anchor, &format!("{keep}{}", spacing(m)));
        }
    }

    // `Normal` has no `pPr` in the template; add one so its line pitch applies.
    if let Some(m) = measured.get("Normal") {
        s = s.replace(
            "<w:style w:type=\"paragraph\" w:default=\"1\" w:styleId=\"Normal\"><w:name w:val=\"Normal\"/><w:uiPriority w:val=\"0\"/><w:qFormat/></w:style>",
            &format!(
                "<w:style w:type=\"paragraph\" w:default=\"1\" w:styleId=\"Normal\"><w:name w:val=\"Normal\"/><w:uiPriority w:val=\"0\"/><w:qFormat/><w:pPr>{}</w:pPr></w:style>",
                spacing(m)
            ),
        );
    }

    if let Some(m) = measured.get("Quote") {
        let left = (m.indent_pt * 20.0).round() as i64;
        s = s.replace(
            "<w:ind w:left=\"567\"/>",
            &format!("<w:ind w:left=\"{left}\"/>"),
        );
    }

    // List paragraphs: measured space-below and line pitch, plus the indent
    // (the numbering definition overrides the indent for numbered lists).
    if let Some(m) = measured.get("ListParagraph") {
        let left = (m.indent_pt * 20.0).round().max(0.0) as i64;
        s = s.replace(
            "<w:pPr><w:ind w:left=\"720\"/></w:pPr>",
            &format!("<w:pPr>{}<w:ind w:left=\"{left}\"/></w:pPr>", spacing(m)),
        );
    }

    s
}

/// The `styles.xml` template, with `Normal` typography and the heading/title
/// `rPr` patched in from the layout measurements.
const STYLES: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
<w:docDefaults>
<w:rPrDefault><w:rPr>
<w:rFonts w:ascii="Calibri" w:hAnsi="Calibri" w:cs="Calibri"/>
<w:sz w:val="22"/><w:szCs w:val="22"/>
</w:rPr></w:rPrDefault>
<w:pPrDefault><w:pPr><w:spacing w:after="120" w:line="276" w:lineRule="auto"/></w:pPr></w:pPrDefault>
</w:docDefaults>
<w:style w:type="paragraph" w:default="1" w:styleId="Normal"><w:name w:val="Normal"/><w:uiPriority w:val="0"/><w:qFormat/></w:style>
<w:style w:type="paragraph" w:styleId="Title"><w:name w:val="Title"/><w:basedOn w:val="Normal"/><w:uiPriority w:val="1"/><w:qFormat/>
<w:pPr><w:spacing w:before="240" w:after="120"/></w:pPr>
<w:rPr><w:b/><w:color w:val="0B3C5D"/><w:sz w:val="56"/></w:rPr></w:style>
<w:style w:type="paragraph" w:styleId="Heading1"><w:name w:val="heading 1"/><w:basedOn w:val="Normal"/><w:next w:val="Normal"/><w:uiPriority w:val="9"/><w:qFormat/>
<w:pPr><w:keepNext/><w:spacing w:before="360" w:after="120"/><w:outlineLvl w:val="0"/></w:pPr>
<w:rPr><w:b/><w:color w:val="0B3C5D"/><w:sz w:val="40"/></w:rPr></w:style>
<w:style w:type="paragraph" w:styleId="Heading2"><w:name w:val="heading 2"/><w:basedOn w:val="Normal"/><w:next w:val="Normal"/><w:uiPriority w:val="9"/><w:qFormat/>
<w:pPr><w:keepNext/><w:spacing w:before="240" w:after="80"/><w:outlineLvl w:val="1"/></w:pPr>
<w:rPr><w:b/><w:color w:val="1D6FA5"/><w:sz w:val="28"/></w:rPr></w:style>
<w:style w:type="paragraph" w:styleId="Heading3"><w:name w:val="heading 3"/><w:basedOn w:val="Normal"/><w:next w:val="Normal"/><w:uiPriority w:val="9"/><w:qFormat/>
<w:pPr><w:keepNext/><w:spacing w:before="200" w:after="60"/><w:outlineLvl w:val="2"/></w:pPr>
<w:rPr><w:b/><w:color w:val="1D6FA5"/><w:sz w:val="24"/></w:rPr></w:style>
<w:style w:type="paragraph" w:styleId="Heading4"><w:name w:val="heading 4"/><w:basedOn w:val="Normal"/><w:next w:val="Normal"/><w:uiPriority w:val="9"/><w:qFormat/>
<w:pPr><w:keepNext/><w:spacing w:before="180" w:after="60"/><w:outlineLvl w:val="3"/></w:pPr><w:rPr><w:b/><w:sz w:val="22"/></w:rPr></w:style>
<w:style w:type="paragraph" w:styleId="Heading5"><w:name w:val="heading 5"/><w:basedOn w:val="Normal"/><w:next w:val="Normal"/><w:uiPriority w:val="9"/><w:qFormat/>
<w:pPr><w:keepNext/><w:outlineLvl w:val="4"/></w:pPr><w:rPr><w:b/><w:i/><w:sz w:val="22"/></w:rPr></w:style>
<w:style w:type="paragraph" w:styleId="Heading6"><w:name w:val="heading 6"/><w:basedOn w:val="Normal"/><w:next w:val="Normal"/><w:uiPriority w:val="9"/><w:qFormat/>
<w:pPr><w:keepNext/><w:outlineLvl w:val="5"/></w:pPr><w:rPr><w:i/><w:sz w:val="22"/></w:rPr></w:style>
<w:style w:type="paragraph" w:styleId="Heading7"><w:name w:val="heading 7"/><w:basedOn w:val="Normal"/><w:next w:val="Normal"/><w:uiPriority w:val="9"/><w:qFormat/>
<w:pPr><w:keepNext/><w:outlineLvl w:val="6"/></w:pPr><w:rPr><w:sz w:val="22"/></w:rPr></w:style>
<w:style w:type="paragraph" w:styleId="Heading8"><w:name w:val="heading 8"/><w:basedOn w:val="Normal"/><w:next w:val="Normal"/><w:uiPriority w:val="9"/><w:qFormat/>
<w:pPr><w:keepNext/><w:outlineLvl w:val="7"/></w:pPr><w:rPr><w:i/><w:sz w:val="22"/></w:rPr></w:style>
<w:style w:type="paragraph" w:styleId="Heading9"><w:name w:val="heading 9"/><w:basedOn w:val="Normal"/><w:next w:val="Normal"/><w:uiPriority w:val="9"/><w:qFormat/>
<w:pPr><w:keepNext/><w:outlineLvl w:val="8"/></w:pPr><w:rPr><w:sz w:val="22"/></w:rPr></w:style>
<w:style w:type="paragraph" w:styleId="Quote"><w:name w:val="Quote"/><w:basedOn w:val="Normal"/><w:uiPriority w:val="29"/><w:qFormat/>
<w:pPr><w:ind w:left="567"/></w:pPr><w:rPr><w:i/><w:color w:val="5A6B7B"/></w:rPr></w:style>
<w:style w:type="paragraph" w:styleId="Caption"><w:name w:val="Caption"/><w:basedOn w:val="Normal"/><w:uiPriority w:val="35"/><w:qFormat/>
<w:pPr><w:spacing w:after="160"/></w:pPr><w:rPr><w:i/><w:color w:val="5A6B7B"/><w:sz w:val="18"/></w:rPr></w:style>
<w:style w:type="paragraph" w:styleId="Code"><w:name w:val="Code"/><w:basedOn w:val="Normal"/><w:uiPriority w:val="30"/><w:qFormat/>
<w:rPr><w:rFonts w:ascii="Consolas" w:hAnsi="Consolas" w:cs="Consolas"/><w:sz w:val="20"/></w:rPr></w:style>
<w:style w:type="paragraph" w:styleId="ListParagraph"><w:name w:val="List Paragraph"/><w:basedOn w:val="Normal"/><w:uiPriority w:val="34"/><w:qFormat/>
<w:pPr><w:ind w:left="720"/></w:pPr></w:style>
<w:style w:type="table" w:styleId="TableGrid"><w:name w:val="Table Grid"/><w:uiPriority w:val="39"/><w:qFormat/>
<w:tblPr><w:tblBorders>
<w:top w:val="single" w:sz="4" w:space="0" w:color="B7C4D0"/>
<w:left w:val="single" w:sz="4" w:space="0" w:color="B7C4D0"/>
<w:bottom w:val="single" w:sz="4" w:space="0" w:color="B7C4D0"/>
<w:right w:val="single" w:sz="4" w:space="0" w:color="B7C4D0"/>
<w:insideH w:val="single" w:sz="4" w:space="0" w:color="B7C4D0"/>
<w:insideV w:val="single" w:sz="4" w:space="0" w:color="B7C4D0"/>
</w:tblBorders></w:tblPr></w:style>
</w:styles>"#;

/// The `numbering.xml` part: a bullet list, a decimal list, and one restarting
/// `w:num` per ordered-list instance so each list starts at 1. `indent` is the
/// measured level-0 indent in twips.
fn numbering(ordered_num_ids: &[u32], indent: i64) -> String {
    let indent = indent.max(0);
    let lvl = |ilvl: i64, fmt: &str, text: &str| {
        format!(
            "<w:lvl w:ilvl=\"{ilvl}\"><w:start w:val=\"1\"/><w:numFmt w:val=\"{fmt}\"/>\
             <w:lvlText w:val=\"{text}\"/><w:lvlJc w:val=\"left\"/>\
             <w:pPr><w:ind w:left=\"{}\" w:hanging=\"{indent}\"/></w:pPr></w:lvl>",
            indent * (ilvl + 1)
        )
    };

    let mut out = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n\
         <w:numbering xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\">\
         <w:abstractNum w:abstractNumId=\"0\">",
    );
    out.push_str(&lvl(0, "bullet", "\u{2022}"));
    out.push_str(&lvl(1, "bullet", "\u{25e6}"));
    out.push_str(&lvl(2, "bullet", "\u{25aa}"));
    out.push_str("</w:abstractNum><w:abstractNum w:abstractNumId=\"1\">");
    out.push_str(&lvl(0, "decimal", "%1."));
    out.push_str(&lvl(1, "lowerLetter", "%2."));
    out.push_str(&lvl(2, "lowerRoman", "%3."));
    out.push_str("</w:abstractNum><w:num w:numId=\"1\"><w:abstractNumId w:val=\"0\"/></w:num>");

    for &id in ordered_num_ids {
        out.push_str(&format!(
            "<w:num w:numId=\"{id}\"><w:abstractNumId w:val=\"1\"/>\
             <w:lvlOverride w:ilvl=\"0\"><w:startOverride w:val=\"1\"/></w:lvlOverride>\
             <w:lvlOverride w:ilvl=\"1\"><w:startOverride w:val=\"1\"/></w:lvlOverride>\
             <w:lvlOverride w:ilvl=\"2\"><w:startOverride w:val=\"1\"/></w:lvlOverride>\
             </w:num>"
        ));
    }
    out.push_str("</w:numbering>");
    out
}
