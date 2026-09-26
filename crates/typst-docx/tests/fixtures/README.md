# Typst → DOCX benchmark

The converter-first roadmap and fidelity acceptance gates are in the
[implementation plan](../../IMPLEMENTATION_PLAN.md).

Two feature-dense Typst documents (`benchmark.typ` and `complex-benchmark.typ`)
regression-check the conversion pipeline. Compare the generated DOCX structure
and geometry to the corresponding Typst PDF/layout reference during tool
development.

## Files

| File | Purpose |
|---|---|
| `benchmark.typ` | The benchmark document (≈5 pages). |
| `complex-benchmark.typ` | Textual Typst IR exercising CeTZ diagrams, chart, columns, floats, and equations. |
| `energy-chart.png` | Chart image referenced by the document. |
| `make_chart.py` | Regenerates `energy-chart.png` (Pillow). Committed output. |

## Running each stage

From the repository root, with the CLI built (`cargo build -p typst-cli`):

```sh
FIX=crates/typst-docx/tests/fixtures/benchmark.typ
EXE=target/debug/typst.exe

# 1. Semantic HTML (the converter's input)
$EXE compile --format html --features html $FIX /tmp/bm.html

# 2. Compiler-native layout oracle (geometry, styles, spacing)
$EXE compile --format layout $FIX /tmp/bm.layout.json

# 3. Editable DOCX (the product)
$EXE compile --format docx --features html $FIX /tmp/bm.docx

# 4. PDF — visual reference for fidelity comparison
$EXE compile --format pdf $FIX /tmp/bm.pdf
```

Expected layout output: **5 pages, ~1274 text runs**, A4 (595.3 × 841.9 pt),
left margin 62.4 pt (2.2 cm).

## Feature coverage

The document deliberately exercises:

- metadata (`#set document`) and a visible `#title`
- page size, margins, running header, footer, page numbers
- `#outline` (table of contents)
- headings levels 1–4 with `#set heading(numbering:)`
- justified paragraphs
- inline formatting: `*bold*`, `_italic_`, `#strike`, `#highlight`, sub/superscript, colored text
- inline `code` and block `#raw(..., block: true)`
- math: inline (`$E = m c^2$`) and display
- links (`#link`), footnotes (`#footnote`)
- `#quote`
- bullet, ordered and nested lists; term list (`/ Term: ...`)
- tables: header rows, `colspan`, `align`, a figure-wrapped captioned table
- figure with image and caption
- a callout `#box`
- `#pagebreak`

## Status by feature (keep this current)

| Feature | Layout oracle | DOCX | Notes |
|---|---|---|---|
| Headings / title / outline | ✅ | ✅ (title, H1–4) | outline becomes a list, not a TOC field |
| Paragraphs & inline runs | ✅ | ✅ | bold/italic/code carried |
| Tables (header, colspan) | ✅ | ✅ | column widths still fixed |
| Typography (font/size/color) | ✅ | ✅ | measured from layout |
| Page size & margins | ✅ | ✅ | from `#set page(...)` |
| Block spacing & indent | ✅ | ✅ | space-below + left indent + line pitch (`w:line`, `atLeast`) |
| Line pitch (intra-paragraph) | ✅ | ✅ | measured baseline-to-baseline; `atLeast` avoids clipping. Line *breaking* still follows Word |
| Lists | ✅ | ✅ | numbered lists restart per list; measured indent; bullets/numbers via numbering.xml |
| Outline (TOC) | ✅ | ✅ | rendered as plain indented paragraphs (no Word numbering — the numbers are already in the text) |
| **Multi-line headings / letterheads** | ✅ | ✅ | `<br>` → `<w:br/>`, inline size/color, `#align` → centered |
| **Headers / footers** | ✅ | ✅ | `header-letterhead.typ`; text + measured spacing, `PAGE` field for a trailing page number |
| **Images** | ✅ | ✅ | embedded from the `<img>` data URI, sized from the layout |
| **Math** | ✅ | Partial | OMML fractions, scripts and radicals; equation numbering uses a borderless grid. Complex constructs need coverage checks. |
| **Footnotes** | ✅ | ✅ | endnotes section → `word/footnotes.xml` + `w:footnoteReference` (content formatting is plain) |
| **Hyperlinks** | ✅ | ✅ | external `http(s)` links → `w:hyperlink` + external rel (internal/TOC links not yet) |

This table describes `benchmark.typ`; `complex-benchmark.typ` exercises the
additional column, float and SVG coverage described below.

### Multi-line headings / body letterheads

A body letterhead is one HTML `<p>` with `<br>` separators and inline `<span>`s of
different sizes/colors (or an `#align` wrapper). The converter emits `<w:br/>` for
line breaks, recovers each inline run's resolved family/size/color/bold/italic from
the layout as a direct `rPr`, and applies `<w:jc>` from `#align`. Style-level
typography is chosen as the **mode** across blocks so an outlier (the large company
name) can't skew `Normal`.

### Page header / footer letterhead

`header-letterhead.typ` puts a multi-line letterhead in `#set page(header: ...)`.
The HTML export drops headers/footers entirely (the page set rule is ignored), so
their content and spacing are recovered from the **layout oracle**: runs in the
top/bottom margin become `word/header1.xml` / `word/footer1.xml`, referenced from
`sectPr`, and repeat on every page. A trailing page number becomes a `PAGE` field.

Each header/footer line gets an **exact** line height from the glyph metrics
(`w:line` + `lineRule="exact"`), so it doesn't inherit the body's line pitch and
pad below the baseline. The `w:header`/`w:footer` distances are derived from the
measured header/footer position rather than hardcoded. A line whose runs sit apart
(a left/right `grid`) becomes a **borderless table** with the measured column
widths and cell margins zeroed to match the grid's inset.

Not yet emitted from a header: images.

### Complex drawing benchmark

The complex benchmark keeps all three CeTZ drawings as editable Typst code.
`html.frame(cetz.canvas(...))` leaves the PDF appearance unchanged and exposes
the evaluated frame as an SVG to the HTML/DOCX converter. Build both outputs
with the `html` feature (it makes `html.frame` available even for PDF):

```sh
FIX=crates/typst-docx/tests/fixtures/complex-benchmark.typ
target/debug/typst compile --features html "$FIX" /tmp/complex.pdf
target/debug/typst compile --format layout --features html "$FIX" /tmp/complex.layout.json
target/debug/typst compile --format docx --features html "$FIX" /tmp/complex.docx
```

The SVGs are embedded in the generated DOCX, not stored as replacement assets
in the Typst source. The converter currently drops Typst `columns` during HTML
export, so this benchmark also serves as a source-text coverage check: successful
compilation alone does not mean multi-column prose survived in DOCX.

### Horizontal rules

`#line(length: 100%)` is a stroked shape (not text), so it's recovered from the
layout (`Geometry::Line` with a horizontal direction) and emitted as an empty
paragraph with a bottom border (`w:pBdr`), sized from the stroke thickness and
color. In the body, rules are interleaved with the HTML-driven content by their
vertical position.
