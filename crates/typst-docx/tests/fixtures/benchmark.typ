// Benchmark document for the Typst → DOCX pipeline.
//
// Purpose: exercise the full feature surface the converter aims to support, so
// every stage (semantic HTML, layout oracle, styles, page geometry, spacing)
// can be regression-checked against one stable input.
//
// Stages:
//   typst compile --format html   --features html benchmark.typ out.html
//   typst compile --format layout --features html benchmark.typ out.layout.json
//   typst compile --format docx   --features html benchmark.typ out.docx
//   typst compile --format pdf    benchmark.typ out.pdf   (visual reference)
//
// Feature coverage:
//   metadata + title, page size/margins/header/footer/page numbers, outline,
//   headings 1–4 with numbering, justified paragraphs, bold/italic/strike/
//   highlight/sub/superscript, colored text, inline + block code, math (inline
//   and display), links, footnotes, quote, bullet/ordered/nested lists, term
//   list, tables (header row, colspan, caption), figure with image + caption,
//   callout box, page breaks.

#set document(
  title: "Global Energy Outlook 2026",
  author: "Typsastra Research",
  keywords: ("energy", "benchmark", "typst"),
)

#set page(
  paper: "a4",
  margin: (x: 2.2cm, y: 2.4cm),
  numbering: "1",
  number-align: center,
  header: context [
    #set text(size: 8pt, fill: luma(40%))
    #grid(
      columns: (1fr, auto),
      [Global Energy Outlook 2026],
      align(right)[Typsastra Research],
    )
    #v(-0.4em)
    #line(length: 100%, stroke: 0.5pt + luma(75%))
  ],
  footer: context [
    #set text(size: 8pt, fill: luma(40%))
    #line(length: 100%, stroke: 0.5pt + luma(75%))
    #v(0.2em)
    #align(center)[Page #counter(page).display()]
  ],
)

#set text(font: "Calibri", size: 10.5pt, lang: "en")
#set par(justify: true, leading: 0.65em)
#set heading(numbering: "1.1")

#show heading.where(level: 1): set text(size: 16pt, fill: rgb("#0B3C5D"))
#show heading.where(level: 2): set text(size: 13pt, fill: rgb("#1D6FA5"))
#show heading.where(level: 3): set text(size: 11.5pt, fill: rgb("#1D6FA5"))
#show heading.where(level: 4): set text(size: 10.5pt, style: "italic")

#title[Global Energy Outlook 2026]

#align(center)[#text(size: 11pt, fill: luma(35%))[Typsastra Research · March 2026]]

#v(0.8em)

#outline(title: [Contents], depth: 2)

#pagebreak()

= Executive summary <sec:summary>

Renewable capacity additions reached a record *380 GW* in 2025, up _12%_ year over
year. Solar accounted for the largest share, followed by wind. This report reviews
the drivers, the cost trajectory, and the outlook to 2030.#footnote[All figures are
indicative and are provided for benchmarking only.]

#v(0.4em)

#box(
  fill: rgb("#EAF3FA"),
  inset: 10pt,
  radius: 4pt,
  width: 100%,
)[
  *Key finding.* Levelised costs for utility-scale solar fell below
  #text(fill: rgb("#B23A48"), weight: "bold")[USD 28 per MWh] in the sunniest
  markets, undercutting new combined-cycle gas.
]

== Drivers

The principal drivers are summarised below.

+ Falling module prices
+ Improved cell efficiency
+ Policy support
  + Investment tax credits
  + Competitive auctions

The terminology used throughout is defined in the appendix.

== Regional outlook

#figure(
  table(
    columns: (auto, auto, auto, auto),
    inset: 7pt,
    align: (left, right, right, right),
    table.header([Region], [2024], [2025], [Change]),
    [Africa],   [58],  [71],  [#text(fill: rgb("#2E7D32"))[+22%]],
    [Asia],     [210], [245], [#text(fill: rgb("#2E7D32"))[+17%]],
    [Europe],   [64],  [70],  [#text(fill: rgb("#2E7D32"))[+9%]],
    [Americas], [82],  [84],  [#text(fill: rgb("#2E7D32"))[+2%]],
  ),
  caption: [Capacity additions by region, in gigawatts.],
)

= Technology review

== Solar

Solar is now the cheapest source of new electricity in most markets. The learning
rate has held near 20% for a decade, and manufacturing capacity exceeds demand.

#figure(
  image("energy-chart.png", width: 78%),
  caption: [Projected global capacity additions by source, 2025.],
)

=== Manufacturing

Module manufacturing capacity exceeds demand, compressing margins across the value
chain. Cells and wafers remain the most competitive segments.

==== Supply chain

Polysilicon supply remains concentrated among a small number of producers.

== Wind

Offshore wind faces cost pressure, while onshore remains competitive at good sites.

#quote[
  The cost of wind has fallen faster than almost any other technology in history.
]

== Comparison

#table(
  columns: (auto, 1fr, auto),
  inset: 7pt,
  table.header([Source], [Notes], [LCOE]),
  [Solar],   [Utility-scale, single-axis tracking], [USD 28],
  [Wind],    [Onshore, good resource],              [USD 34],
  [Gas],     [Combined cycle],                      [USD 52],
  [Nuclear], [New build],                           [USD 90],
)

= Modelling

The model solves a least-cost capacity-expansion problem over a 25-year horizon.

#raw(
  "fn lcoe(capex: f64, opex: f64, yield: f64) -> f64 {\n    (capex / yield) + opex\n}",
  lang: "rust",
  block: true,
)

Inline code such as `lcoe()` can be mixed with math such as $E = m c^2$. The
closed form of the Gaussian integral is

$ integral_0^oo e^(-x^2) dif x = sqrt(pi) / 2 $

A footnote reference#footnote[Derivation omitted for brevity.] and an external
link to #link("https://example.com")[the data portal] complete the picture.
Text may also be #strike[struck through] or #highlight[yellowed].

= Outlook

#table(
  columns: (auto, auto, auto),
  inset: 7pt,
  align: (left, right, right),
  table.header([Scenario], [2030 additions], [Share of total]),
  table.cell(colspan: 3, align: center)[*Central scenario*],
  [Solar],   [410], [54%],
  [Wind],    [205], [27%],
  [Other],   [145], [19%],
)

#pagebreak()

= Appendix

== Terminology

/ LCOE: Levelised cost of electricity.
/ Capacity factor: Output as a share of rated capacity.
/ Learning rate: Cost decline per doubling of cumulative capacity.

== Assumptions

- Discount rate: 6%
- Plant lifetime: 25 years
- No explicit carbon price

#v(1em)

#text(size: 9pt, fill: luma(45%))[
  Prepared for internal benchmarking. Contact: research\@typsastra.example.
]
