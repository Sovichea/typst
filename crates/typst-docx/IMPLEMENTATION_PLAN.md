# Typst → DOCX converter: fidelity implementation plan

Status: active plan · Scope: author a new document in Typst and export an
editable DOCX that reproduces its intended appearance.

## Goal and boundary

Build `typst-docx` as a deterministic converter. Each supported Typst construct
must appear in the DOCX with its content, geometry, typography, and relationships
intact. Prefer native editable Word elements for text and document structure;
render only complex vector artwork, such as CeTZ, to SVG at export time. Keep
the Typst source editable and the DOCX caption native.

This phase does **not** implement an agent, DOCX → Typst import, or synchronization
of user edits made after export. Those require a reliable exporter first and
belong to the later agentic-editing plan in `DesktopEditors/docs/agentic-editing-plan.md`.
Nor does this phase require a visual model or screenshots to judge an export.

"Accurate" means measured against a pinned reference, with explicit feature
coverage and tolerances. Word and Typst use different layout engines, so an
editable paragraph may not share every line break. Such differences are
measured and reported, rather than hidden by rasterizing ordinary text.

## Current conversion path

```text
Typst source ─┬─ paged layout ── PDF reference + geometry/style samples
              └─ semantic HTML DOM ───────────┐
                   layout geometry/style samples ─┴─ typst-docx ── DOCX
```

- The semantic tree provides reading order, text, structure, tables, figures,
  captions, MathML, and links. The paged layout supplies resolved font metrics,
  image dimensions, page geometry, rules, headers, and footers.
- `typst-docx` emits WordprocessingML and package parts. `html.frame` exposes an
  evaluated CeTZ canvas to the HTML DOM; the exporter places its SVG in DOCX
  media and leaves the authored CeTZ code in Typst.
- Semantic HTML currently ignores `columns`, `place`, and some spacing. The
  `complex-benchmark.typ` PDF contains multi-column prose and a floating
  thumbnail that do not fully survive DOCX export. These are **open blockers**,
  not acceptable fallbacks or proof of overall fidelity.

## Per-element export contract

| Typst construct | Preferred DOCX representation | Fidelity checks |
| --- | --- | --- |
| Document/page settings, sections | `sectPr`, page size, margins, section breaks | page dimensions, margins, section starts, page count |
| Paragraphs and inline styles | `w:p`, `w:r`, named styles and explicit run overrides | text/Unicode, font family, size, weight, color, alignment, line pitch, indents and breaks |
| Headings and outline | Heading styles, numbering and navigable targets | hierarchy, visible numbering, spacing, references and TOC content |
| Lists and term lists | Native numbering and paragraphs | levels, start/restart, marker alignment, hanging indent and text order |
| Tables and layout grids | Native tables; borderless grid only for genuine layout | row/column count, spans, widths, insets, borders, cell alignment and split behavior |
| Columns | Word section columns where flow semantics allow; bounded borderless grid only for a fixed, explicitly placed column group | all column text retained, count, gutter, breaks, x positions and reading order |
| Figures, images, captions | DrawingML image with native caption text/sequence | media relationships, crop, scale, aspect ratio, placement and caption attachment |
| CeTZ and other complex frames | SVG media produced from evaluated Typst frame at export | nonempty SVG, aspect ratio, DrawingML extent and caption; source stays Typst |
| Equations and code blocks | OMML; editable text in a borderless layout grid for code | mathematical structure/number, code text, grid geometry and captions |
| Header/footer, rules, footnotes, links | Word page regions, borders, footnotes and relationships | offsets, repetition, clearances, references and contents |
| Floating/rotated objects | DrawingML anchor/transform with wrapping as required | x/y offsets, rotation, text wrap, clipping and page boundary |

Every construct gets an explicit disposition: **native**, **SVG visual
fallback**, or **unsupported with a source-linked diagnostic**. Do not silently
drop content, replace editable prose with an image, or call a compile successful
when a required element is absent.

## Deterministic fidelity harness (converter development only)

For each pinned fixture, compile the *same* Typst revision with the same fonts,
assets, package versions, page settings, and feature flags to (1) Typst PDF,
(2) compiler-native layout JSON, and (3) DOCX. Use the layout JSON as the
machine-readable geometry behind the PDF; do not ask an LLM to compare images.

1. **Coverage gate:** inventory source text, semantic blocks, column groups,
   equation numbers, figures, captions, headers/footers, footnotes, and links.
   Verify the corresponding DOCX objects and reading-order text exist. Loss of
   a paragraph inside `columns` must fail the gate before geometry is scored.
2. **Package gate:** validate ZIP/OOXML parts, media MIME types, internal and
   external relationships, numbering/style ids, SVG content and DrawingML
   extents. Match exported objects back to source constructs.
3. **Geometry gate:** compare page/section size and the measured positions and
   dimensions of paragraphs, baselines, tables, columns, rules, images, drawings,
   captions, and page regions. Use explicit unit conversions and per-element
   tolerances; store differences alongside the source location and page.
4. **Reflow gate:** compare page and line counts, paragraph sequence and
   boundary placement. Tune native Word styles/spacing before accepting a
   deviation. Record unavoidable engine-dependent reflow separately.
5. **Repeatability gate:** pin the toolchain and fonts; normalize volatile DOCX
   package metadata; identical inputs must have identical logical OOXML,
   diagnostics and exported media.

Typsastra Office sdkjs exposes a **separate, read-only DOCX layout IR** through
`GetAgentDocumentSnapshot`. A build-time test harness may use it to measure
the imported DOCX, independently of `typst-docx`'s source/layout model. It
currently covers page, paragraph and table geometry; extend its measurement
coverage for drawings, columns and per-page regions as needed. If actual DOCX
geometry is unavailable, mark that check **unmeasured**: declared OOXML extents
alone do not establish rendered placement. The sdkjs IR does not become the
Typst IR or a runtime prerequisite for export.

## Work sequence and acceptance gates

### 0. Baseline and test contract

- Pin the Typst CLI, CeTZ version, fonts, DOCX reader version and feature flags.
- Capture source/PDF/layout/DOCX inventories for `benchmark.typ` and
  `complex-benchmark.typ`; retain small focused fixtures for each failure.
- Define the object map, measurement units, tolerance policy and diagnostic
  format. Start with content coverage before tuning visual offsets.

**Exit:** the harness identifies the currently missing column text and float;
unknown geometry is reported as unknown, not a passing comparison.

### 1. Complete semantic coverage

- Preserve `columns` bodies and `colbreak` boundaries in the semantic path and
  export a Word representation with the correct count, gutter, flow and order.
- Preserve explicit page breaks and positioned/floating image content; do not
  conflate the body flow with header/footer or decorative frames.
- Reconcile the fixture status table with current math, caption and callout
  implementations; add regressions for omissions.

**Exit:** all authored text, equations, captions and figures in both benchmarks
are present exactly once in DOCX in the intended order. Unsupported constructs
produce a source-linked error instead of an empty container.

### 2. Calibrate native layout element by element

- Page and region geometry, then typography/line pitch, then list and table
  grids, then equations, images, anchored objects, and their captions.
- Compare each element's measured DOCX geometry with the corresponding Typst
  PDF/layout region. Fix systematic unit, font metric, spacing, inset and
  wrapping differences in the converter, not in individual fixtures.
- Verify SVG drawings survive the target DOCX reader with measured display
  bounds. Keep native content around them editable.

**Exit:** the benchmark meets the locked content and geometry tolerances in
the deterministic harness; any unsupported feature has an explicit disposition.

### 3. Qualify and prevent regressions

- Add edge cases for long tables, nested lists, multi-page columns, rotated or
  clipped images, font substitution, non-Latin text, numbered equations, and
  page-boundary figures.
- Run structural and measured-layout checks in CI. Report per-feature coverage,
  object mismatches, page drift and worst geometry error with source locations.

**Exit:** a converter change cannot silently remove an element or exceed a
measured tolerance on the pinned corpus. Only after this gate should the
agentic editing/import work become the active implementation phase.
