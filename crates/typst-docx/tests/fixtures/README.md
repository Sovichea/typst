# Typst → DOCX benchmark

One stable, feature-dense Typst document (`benchmark.typ`) used to regression-check
every stage of the conversion pipeline. Run it after each change and compare the
outputs against the previous stage.

## Files

| File | Purpose |
|---|---|
| `benchmark.typ` | The benchmark document (≈5 pages). |
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
| Block spacing & indent | ✅ | ✅ | line spacing not controlled (Word reflows) |
| Lists | ✅ | ✅ | bullets/numbers via numbering.xml |
| **Images** | ✅ | ❌ | `FrameItem::Image` / `<img>` not emitted |
| **Math** | ✅ | ❌ | MathML not converted |
| **Footnotes** | ✅ | ❌ | no footnote part / references |
| **Hyperlinks** | ✅ | ❌ | `<a>` not converted |
| Headers / footers | ✅ | ❌ | not written to the DOCX section |

Bold entries are the current known gaps surfaced by this benchmark.
