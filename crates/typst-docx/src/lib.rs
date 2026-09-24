//! Typst's DOCX (WordprocessingML) exporter — proof of concept.
//!
//! Converts Typst's evaluated semantic structure (the HTML DOM produced by
//! `typst-html`) into an editable `.docx` with native Word styles, lists and
//! tables. This is the "semantic" half of the dual semantic+layout pipeline;
//! geometry refinement from the paged layout is future work.

use std::collections::HashMap;
use std::io::{Cursor, Write};

use ecow::eco_format;
use typst_html::{HtmlDocument, HtmlElement, HtmlNode, tag};
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
    let runs = layout.map(layout::collect_runs).unwrap_or_default();
    let mut em = Emitter { out: String::new(), runs: &runs, measured: HashMap::new() };

    if let Some(body) = find_body(document.root()) {
        for child in &body.children {
            em.block(child);
        }
    }

    let document_xml = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n\
         <w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\" \
         xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\">\
         <w:body>{}{}</w:body></w:document>",
        em.out,
        sect_pr(layout)
    );

    package(&document_xml, &styles(&em.measured))
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
}

/// Typography measured from the paged layout for a given Word style.
#[derive(Clone)]
struct Measured {
    family: String,
    size_pt: f64,
    bold: bool,
    italic: bool,
    color: String,
}

/// Accumulates the document body XML.
struct Emitter<'a> {
    out: String,
    runs: &'a [layout::Run],
    measured: HashMap<&'static str, Measured>,
}

impl Emitter<'_> {
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
            let runs = inline(&el.children, false, false, false);
            self.paragraph("Title", &runs, None);
        } else if t == tag::h2 {
            let runs = inline(&el.children, false, false, false);
            self.paragraph("Heading1", &runs, None);
        } else if t == tag::h3 {
            let runs = inline(&el.children, false, false, false);
            self.paragraph("Heading2", &runs, None);
        } else if t == tag::h4 {
            let runs = inline(&el.children, false, false, false);
            self.paragraph("Heading3", &runs, None);
        } else if t == tag::h5 || t == tag::h6 {
            let runs = inline(&el.children, false, false, false);
            self.paragraph("Heading4", &runs, None);
        } else if t == tag::p {
            let runs = inline(&el.children, false, false, false);
            self.paragraph("Normal", &runs, None);
        } else if t == tag::blockquote {
            let runs = inline(&el.children, false, true, false);
            self.paragraph("Quote", &runs, None);
        } else if t == tag::pre {
            let runs = inline(&el.children, false, false, true);
            self.paragraph("Code", &runs, None);
        } else if t == tag::figcaption || t == tag::caption {
            let runs = inline(&el.children, false, true, false);
            self.paragraph("Caption", &runs, None);
        } else if t == tag::ul {
            self.list(el, false, 0);
        } else if t == tag::ol {
            self.list(el, true, 0);
        } else if t == tag::table {
            self.table(el);
        } else if t == tag::hr {
            // skip
        } else {
            // div/section/figure/body/... : recurse
            for child in &el.children {
                self.block(child);
            }
        }
    }

    fn paragraph(&mut self, style: &str, runs: &[Run], num: Option<(u32, u32)>) {
        self.measure(style, runs);
        self.out.push_str("<w:p><w:pPr>");
        self.out
            .push_str(&format!("<w:pStyle w:val=\"{style}\"/>"));
        if let Some((num_id, ilvl)) = num {
            self.out.push_str(&format!(
                "<w:numPr><w:ilvl w:val=\"{ilvl}\"/><w:numId w:val=\"{num_id}\"/></w:numPr>"
            ));
        }
        self.out.push_str("</w:pPr>");
        for run in runs {
            self.run(run);
        }
        self.out.push_str("</w:p>");
    }

    /// Record the layout-measured typography for a style, once, by matching the
    /// paragraph's leading text against a shaped run.
    fn measure(&mut self, style: &str, runs: &[Run]) {
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
            _ => return,
        };
        if self.measured.contains_key(key) {
            return;
        }

        let para = collapse(&runs.iter().map(|r| r.text.as_str()).collect::<String>());
        if para.is_empty() {
            return;
        }
        let prefix: String = para.chars().take(24).collect();

        for run in self.runs {
            if collapse(&run.text).starts_with(&prefix) {
                self.measured.insert(
                    key,
                    Measured {
                        family: run.family.clone(),
                        size_pt: run.size_pt,
                        bold: run.bold,
                        italic: run.italic,
                        color: run.color.clone(),
                    },
                );
                return;
            }
        }
    }

    fn run(&mut self, run: &Run) {
        self.out.push_str("<w:r>");
        if run.bold || run.italic || run.mono {
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
    }

    fn list(&mut self, el: &HtmlElement, ordered: bool, level: u32) {
        let num_id = if ordered { 2 } else { 1 };
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
                    _ => collect_inline(grand, false, false, false, &mut runs),
                }
            }
            if runs.iter().all(|r| r.text.trim().is_empty()) {
                runs = vec![Run {
                    text: String::new(),
                    bold: false,
                    italic: false,
                    mono: false,
                }];
            }
            self.paragraph("ListParagraph", &runs, Some((num_id, level.min(8))));
            for grand in &li.children {
                if let HtmlNode::Element(g) = grand {
                    if g.tag == tag::ul {
                        self.list(g, false, level + 1);
                    } else if g.tag == tag::ol {
                        self.list(g, true, level + 1);
                    }
                }
            }
        }
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

        self.out.push_str(
            "<w:tbl><w:tblPr><w:tblStyle w:val=\"TableGrid\"/>\
             <w:tblW w:w=\"0\" w:type=\"auto\"/><w:tblLook w:val=\"04A0\"/></w:tblPr><w:tblGrid>",
        );
        for _ in 0..cols {
            self.out.push_str("<w:gridCol w:w=\"3000\"/>");
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
                cells += 1;
                self.out.push_str("<w:tc><w:tcPr>");
                if c.tag == tag::th {
                    self.out
                        .push_str("<w:shd w:val=\"clear\" w:color=\"auto\" w:fill=\"0B3C5D\"/>");
                }
                self.out.push_str("<w:tcW w:w=\"3000\" w:type=\"dxa\"/></w:tcPr>");
                let runs = inline(&c.children, c.tag == tag::th, false, false);
                // Cells always contain at least one paragraph.
                if runs.is_empty() {
                    self.paragraph("Normal", &[], None);
                } else {
                    self.paragraph("Normal", &runs, None);
                }
                self.out.push_str("</w:tc>");
            }
            for _ in cells..cols {
                self.out.push_str(
                    "<w:tc><w:tcPr><w:tcW w:w=\"3000\" w:type=\"dxa\"/></w:tcPr><w:p/></w:tc>",
                );
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

fn inline(nodes: &[HtmlNode], bold: bool, italic: bool, mono: bool) -> Vec<Run> {
    let mut runs = Vec::new();
    for node in nodes {
        collect_inline(node, bold, italic, mono, &mut runs);
    }
    runs
}

fn collect_inline(
    node: &HtmlNode,
    bold: bool,
    italic: bool,
    mono: bool,
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
                });
                return;
            } else {
                (bold, italic, mono)
            };
            for child in &el.children {
                collect_inline(child, b, i, m, runs);
            }
        }
        _ => {}
    }
}

/// Collapse runs of whitespace in a string for text matching.
fn collapse(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
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
fn sect_pr(layout: Option<&PagedDocument>) -> String {
    let Some(page) = layout.and_then(|doc| doc.pages().first()) else {
        return A4_SECT_PR.to_string();
    };
    let size = page.frame.size();
    let margin = page.margin;
    let twips = |abs: Abs| (abs.to_pt() * 20.0).round() as i64;
    format!(
        "<w:sectPr>\
         <w:pgSz w:w=\"{}\" w:h=\"{}\"/>\
         <w:pgMar w:top=\"{}\" w:right=\"{}\" w:bottom=\"{}\" w:left=\"{}\" \
         w:header=\"708\" w:footer=\"708\" w:gutter=\"0\"/>\
         </w:sectPr>",
        twips(size.x),
        twips(size.y),
        twips(margin.top),
        twips(margin.right),
        twips(margin.bottom),
        twips(margin.left),
    )
}

const A4_SECT_PR: &str = "<w:sectPr>\
    <w:pgSz w:w=\"11906\" w:h=\"16838\"/>\
    <w:pgMar w:top=\"1134\" w:right=\"1134\" w:bottom=\"1134\" w:left=\"1134\" \
    w:header=\"708\" w:footer=\"708\" w:gutter=\"0\"/>\
    </w:sectPr>";

fn package(document_xml: &str, styles: &str) -> StrResult<Vec<u8>> {
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

    write(&mut zip, "[Content_Types].xml", CONTENT_TYPES)?;
    write(&mut zip, "_rels/.rels", ROOT_RELS)?;
    write(&mut zip, "word/document.xml", document_xml)?;
    write(&mut zip, "word/styles.xml", styles)?;
    write(&mut zip, "word/numbering.xml", NUMBERING)?;
    write(&mut zip, "word/_rels/document.xml.rels", DOC_RELS)?;

    let cursor = zip.finish().map_err(|e| eco_format!("zip finish error: {e}"))?;
    Ok(cursor.into_inner())
}

const CONTENT_TYPES: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
<Default Extension="xml" ContentType="application/xml"/>
<Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>
<Override PartName="/word/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml"/>
<Override PartName="/word/numbering.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.numbering+xml"/>
</Types>"#;

const ROOT_RELS: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/>
</Relationships>"#;

const DOC_RELS: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/>
<Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/numbering" Target="numbering.xml"/>
</Relationships>"#;

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

const NUMBERING: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:numbering xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
<w:abstractNum w:abstractNumId="0">
<w:lvl w:ilvl="0"><w:start w:val="1"/><w:numFmt w:val="bullet"/><w:lvlText w:val="•"/><w:lvlJc w:val="left"/><w:pPr><w:ind w:left="720" w:hanging="360"/></w:pPr></w:lvl>
<w:lvl w:ilvl="1"><w:start w:val="1"/><w:numFmt w:val="bullet"/><w:lvlText w:val="◦"/><w:lvlJc w:val="left"/><w:pPr><w:ind w:left="1440" w:hanging="360"/></w:pPr></w:lvl>
<w:lvl w:ilvl="2"><w:start w:val="1"/><w:numFmt w:val="bullet"/><w:lvlText w:val="▪"/><w:lvlJc w:val="left"/><w:pPr><w:ind w:left="2160" w:hanging="360"/></w:pPr></w:lvl>
</w:abstractNum>
<w:abstractNum w:abstractNumId="1">
<w:lvl w:ilvl="0"><w:start w:val="1"/><w:numFmt w:val="decimal"/><w:lvlText w:val="%1."/><w:lvlJc w:val="left"/><w:pPr><w:ind w:left="720" w:hanging="360"/></w:pPr></w:lvl>
<w:lvl w:ilvl="1"><w:start w:val="1"/><w:numFmt w:val="lowerLetter"/><w:lvlText w:val="%2."/><w:lvlJc w:val="left"/><w:pPr><w:ind w:left="1440" w:hanging="360"/></w:pPr></w:lvl>
<w:lvl w:ilvl="2"><w:start w:val="1"/><w:numFmt w:val="lowerRoman"/><w:lvlText w:val="%3."/><w:lvlJc w:val="left"/><w:pPr><w:ind w:left="2160" w:hanging="360"/></w:pPr></w:lvl>
</w:abstractNum>
<w:num w:numId="1"><w:abstractNumId w:val="0"/></w:num>
<w:num w:numId="2"><w:abstractNumId w:val="1"/></w:num>
</w:numbering>"#;
