#import "@preview/cetz:0.4.2"

#set document(
  title: "Complex Visual Systems Benchmark",
  author: "Typsastra Research",
  keywords: ("benchmark", "diagrams", "cetz", "layout", "docx"),
)

#set page(
  paper: "a4",
  margin: (x: 18mm, top: 20mm, bottom: 22mm),
  header: context [
    #set text(size: 8pt, fill: luma(38%))
    #grid(columns: (1fr, 1fr), gutter: 6pt,
      [Complex Visual Systems], align(right)[Benchmark · 2026])
    #v(-2pt)
    #line(length: 100%, stroke: 0.5pt + luma(78%))
  ],
  footer: context [
    #set text(size: 8pt, fill: luma(38%))
    #line(length: 100%, stroke: 0.5pt + luma(78%))
    #v(2pt)
    #align(center)[Page #counter(page).display()]
  ],
)

#set text(font: "Calibri", size: 10pt)
#set par(justify: true, leading: 0.64em)
#set heading(numbering: "1.1")
#show heading.where(level: 1): set text(size: 17pt, fill: rgb("#123B5D"))
#show heading.where(level: 2): set text(size: 13pt, fill: rgb("#1D6FA5"))
#show heading.where(level: 3): set text(size: 11pt, fill: rgb("#2878A8"))
#show raw.where(block: true): set text(font: "DejaVu Sans Mono", size: 8.5pt)
#show link: set text(fill: rgb("#1D6FA5"))

#title[Complex Visual Systems Benchmark]
#align(center)[
  #text(size: 12pt, fill: luma(35%))[A stress test for layout, drawings, and structured content]
]
#v(10pt)

#grid(columns: (1fr, 1fr, 1fr), gutter: 8pt,
  box(width: 100%, fill: rgb("#EAF3FA"), radius: 5pt, inset: 9pt)[
    *Editorial systems* \
    Headings, notes, captions, and typographic hierarchy.
  ],
  box(width: 100%, fill: rgb("#FCEFEF"), radius: 5pt, inset: 9pt)[
    *Visual systems* \
    Flowcharts, CeTZ networks, and data-rich diagrams.
  ],
  box(width: 100%, fill: rgb("#EFF8EE"), radius: 5pt, inset: 9pt)[
    *Reading paths* \
    Double and triple columns for dense reference material.
  ],
)

#v(12pt)
#box(width: 100%, fill: rgb("#FFF8DF"), radius: 6pt, inset: 10pt)[
  *Design challenge.* Preserve relationships between floating objects, captions,
  equations, and multi-column prose while keeping the document editable.
]

#v(12pt)
#outline(title: [Contents], depth: 2)
#pagebreak()

= Editorial systems

#columns(2, gutter: 12pt)[
  The benchmark begins with ordinary reading material so later drawing tests have
  realistic anchors. Paragraphs are justified, with a controlled baseline pitch and
  small islands of colored text that exercise run-level recovery.

  / Visual hierarchy: Headings, labels, and captions should remain distinct at a glance.

  / Semantic structure: A table can carry meaning, while a borderless grid can carry layout.

  The first column is intentionally dense but not crowded. Its paragraphs include
  punctuation, long words, and repeated line starts so the gutter can be inspected
  against the text edge rather than against a decorative border.

  #colbreak()

  == Nested navigation

  + First-level item
    - Supporting detail
      + A third-level detail
    - Another supporting detail
  + Second-level item

  A short quote follows the list.

  #quote[
    Good layout makes the relationship between parts visible before the reader
    needs to read every part.
  ]

  The second column starts after an explicit break, so its left edge can be
  compared directly with the first column. The nested list and quote provide
  different indentation levels without changing the column width.

  Read the two columns as one continuous argument: the first establishes the
  hierarchy, while the second tests how that hierarchy behaves when it moves to
  a neighboring region.
]

#v(12pt)
#columns(3, gutter: 8pt)[
  *Definitions*

  / Canvas: A bounded coordinate system for drawing.

  / Grid: A layout structure for alignment and placement.

  / Cell: A region containing text or another object.

  / Flow: The order in which content is encountered.

  These definitions are written as complete sentences so the first triple-column
  region has a natural rhythm. The repeated label-and-description pattern also
  makes it easy to spot a drifting edge, an uneven gutter, or an accidental
  change in the baseline.

  #colbreak()

  *Editorial note*

  Captions can sit above or below a figure. Listings use a separate counter from
  equations, while tables retain their own semantic label.

  The middle column is intentionally more explanatory than the glossary beside
  it. Its paragraph breaks and centered heading create a clear vertical rhythm
  while preserving enough text to test the shared gutter.

  #colbreak()

  *Layout vocabulary*

  / Baseline: The invisible line on which text sits.

  / Measure: The available width or height of a region.

  / Reference: A named destination that can be reused.

  The third column contains short entries followed by a longer explanatory note.
  Its final paragraph is deliberately close to the lower edge of the column so
  the benchmark exposes any inconsistent bottom padding between neighboring
  columns.
]

#v(12pt)
#figure(
  table(
    columns: (1.2fr, 1fr, 1fr, 1fr),
    inset: 6pt,
    stroke: 0.5pt + luma(65%),
    table.header([*Layer*], [*Purpose*], [*Object*], [*Check*]),
    [Type], [Semantic meaning], [Heading / table], [Structure],
    [Visual], [Decorative composition], [Shape / image], [Contrast],
    [Spatial], [Relative placement], [Grid / float], [Alignment],
  ),
  kind: table,
  caption: [Layered content model used throughout this benchmark.],
)

#pagebreak()

= Diagrams and decision flows

#figure(
  html.frame(cetz.canvas(length: 0.82cm, {
    import cetz.draw: *

    set-style(stroke: 0.8pt + luma(35%))
    grid((0, 0), (10, 4), step: 2, help-lines: true)

    rect((0, 2), (2, 3.2), fill: rgb("#EAF3FA"), stroke: 1pt + rgb("#1D6FA5"))
    content((1, 2.6), align(center)[*Intake*])
    rect((3, 2), (5.2, 3.2), fill: rgb("#FFF8DF"), stroke: 1pt + rgb("#C28B18"))
    content((4.1, 2.6), align(center)[*Validate*])
    circle((6.6, 2.6), radius: 0.72, fill: rgb("#FCEFEF"), stroke: 1pt + rgb("#B23A48"))
    content((6.6, 2.6), align(center)[Ready?])
    rect((8.2, 3.0), (10, 3.9), fill: rgb("#EFF8EE"), stroke: 1pt + rgb("#2E7D32"))
    content((9.1, 3.45), align(center)[*Publish*])
    rect((8.2, 1.0), (10, 1.9), fill: rgb("#F2F2F2"), stroke: 1pt + luma(45%))
    content((9.1, 1.45), align(center)[*Feedback*])
    rect((3, 0.4), (5.2, 1.4), fill: rgb("#F2F2F2"), stroke: 1pt + luma(45%))
    content((4.1, 0.9), align(center)[*Archive*])

    line((2, 2.6), (3, 2.6), mark: (end: ">"))
    line((5.2, 2.6), (5.88, 2.6), mark: (end: ">"))
    line((7.32, 2.6), (8.2, 3.45), mark: (end: ">"))
    line((7.1, 1.98), (8.5, 1.55), mark: (end: ">"))
    line((6.0, 2.25), (5.0, 1.35), mark: (end: ">"))
    line((8.5, 1.35), (5.2, 1.15), stroke: 0.6pt + luma(50%), mark: (end: ">"))
  })),
  caption: [A multi-stage editorial workflow with validation and feedback paths.],
)

#v(8pt)
#figure(
  html.frame(cetz.canvas(length: 0.8cm, {
    import cetz.draw: *

    set-style(stroke: 0.7pt + luma(40%))
    grid((0, 0), (9, 5), step: 1, help-lines: false)
    circle((1, 3.8), radius: 0.65, fill: rgb("#EAF3FA"), stroke: 1pt + rgb("#1D6FA5"), name: "source")
    content("source.center", [Sources])
    circle((4, 4.5), radius: 0.65, fill: rgb("#FFF8DF"), stroke: 1pt + rgb("#C28B18"), name: "parse")
    content("parse.center", [Parser])
    circle((7, 3.8), radius: 0.65, fill: rgb("#EFF8EE"), stroke: 1pt + rgb("#2E7D32"), name: "index")
    content("index.center", [Index])
    circle((4, 2.1), radius: 0.65, fill: rgb("#FCEFEF"), stroke: 1pt + rgb("#B23A48"), name: "review")
    content("review.center", [Review])
    circle((1, 1.1), radius: 0.65, fill: rgb("#F2F2F2"), stroke: 1pt + luma(45%), name: "archive")
    content("archive.center", [Archive])
    line("source.east", "parse.west", mark: (end: ">"))
    line("parse.east", "index.west", mark: (end: ">"))
    line("parse.south", "review.north", mark: (end: ">"))
    line("review.west", "archive.east", mark: (end: ">"))
    line("review.east", "index.south-west", mark: (end: ">"))
  })),
  caption: [A CeTZ knowledge graph with several branches and converging paths.],
)

#v(8pt)
#figure(
  html.frame(cetz.canvas(length: 0.9cm, {
    import cetz.draw: *

    set-style(stroke: 0.7pt + luma(35%))
    line((0, 0), (7, 0), stroke: 1pt + luma(25%))
    line((0, 0), (0, 4), stroke: 1pt + luma(25%))
    grid((0, 0), (7, 4), step: 1, help-lines: true)
    rect((0.7, 0), (1.7, 2.5), fill: rgb("#F4B942"), stroke: none)
    rect((2.4, 0), (3.4, 3.3), fill: rgb("#3DA9D7"), stroke: none)
    rect((4.1, 0), (5.1, 1.8), fill: rgb("#58B76C"), stroke: none)
    rect((5.8, 0), (6.8, 1.2), fill: rgb("#8D65B5"), stroke: none)
    content((1.2, 2.75), [250])
    content((2.9, 3.55), [330])
    content((4.6, 2.05), [180])
    content((6.3, 1.45), [120])
    content((1.2, -0.28), align(center)[A])
    content((2.9, -0.28), align(center)[B])
    content((4.6, -0.28), align(center)[C])
    content((6.3, -0.28), align(center)[D])
  })),
  caption: [A compact CeTZ bar chart used to test drawing scale and labels.],
)

#pagebreak()

= Equations, listings, and images

#set math.equation(numbering: "(1)")

#figure(
  image("energy-chart.png", width: 88%),
  caption: [A wide chart image used as a stable visual anchor.],
)

#v(8pt)
#place(top + right, float: true, dx: 3mm, dy: 2mm)[
  #rotate(4deg, origin: center)[
    #box(fill: white, stroke: 0.7pt + rgb("#1D6FA5"), radius: 4pt, inset: 4pt)[
      #image("energy-chart.png", width: 43mm)
    ]
  ]
]

A floating thumbnail is placed beside this paragraph. It uses a rotation, a
border, a background, and a narrow width so the document contains several
independent image-placement constraints. The surrounding text continues to
explain the relationship between the visual object and the paragraph.

#v(8pt)
#columns(2, gutter: 10pt)[
  The first equation is a centered Gaussian integral.

  $ integral_(-oo)^oo e^(-x^2) dif x = sqrt(pi) $

  #colbreak()

  The second equation is an aligned system with a label.

  $ a &= b + c quad \
    d &= e - f $ <system>

  A final equation uses a matrix and a radical.

  $ M^2 = mat(1, 0, 0, 1) quad \
    det(M) = 1 $
]

#v(10pt)
#figure(
  raw(
    "fn median(values: &[f64]) -> f64 {\n    let mut sorted = values.to_vec();\n    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());\n    sorted[sorted.len() / 2]\n}",
    lang: "rust",
    block: true,
  ),
  kind: raw,
  caption: [A listing caption tests the raw-block grid and listing sequence.],
)

#figure(
  table(
    columns: (1.3fr, 1fr, 1fr, 1fr),
    inset: 5pt,
    stroke: 0.5pt + luma(65%),
    table.header([*Region*], [*2024*], [*2030*], [*Change*]),
    [North], [120], [190], [+58%],
    [Central], [210], [260], [+24%],
    [South], [180], [165], [-8%],
  ),
  kind: table,
  caption: [A compact table with numeric alignment and a semantic caption.],
)

= Reading paths and references

#columns(3, gutter: 8pt)[
  *Systems glossary*

  / Anchor: A stable point used to position a floating object.

  / Counter: A value that changes according to document order.

  / Frame: A bounded region containing laid-out content.

  / Region: A header, footer, or main text area.

  The first glossary column mixes short definitions with a longer paragraph. That
  mixture is useful for checking both the hanging indent of terms and the amount
  of white space above an explicit column transition.

  #colbreak()

  *Review notes*

  A document can be structurally valid while still being difficult to read. The
  benchmark keeps these concerns visible by alternating prose, diagrams, and
  reference material.

  / Dense pages: Tables and columns should still leave enough breathing room.

  / Sparse pages: A figure may be the primary object and carry a short caption.

  / Long objects: A shape or table should be evaluated near page boundaries.

  The review column is deliberately prose-heavy. Its sentences cross several
  line lengths, which makes the column edge and the gutter visible without
  relying on a background color or a table border.

  #colbreak()

  *Column vocabulary*

  / Gutter: The space between adjacent columns.

  / Break: A transition that continues content in another region.

  / Density: The amount of content placed in a given area.

  The final column is a compact reference list. Its text is longer than the
  neighboring columns so the benchmark can reveal whether the third column keeps
  the same inset, leading, and right edge when it reaches a different height.
]

#v(12pt)
#box(width: 100%, fill: rgb("#EAF3FA"), radius: 8pt, inset: 12pt)[
  *Synthesis.* The final page combines semantic labels, numbered equations,
  nested references, and a compact glossary. A footnote follows this callout to
  exercise page-end composition.#footnote[The callout is deliberately close to
  the bottom region so pagination and footnote placement are visible.]
]

#v(12pt)
#columns(2, gutter: 12pt)[
  == Reference notes

  The benchmark uses ordinary paragraphs as anchors for the visual tests. This
  makes it easier to tell whether a layout difference came from text flow or
  from a drawing object. The left column below is intentionally long enough to
  reach the lower half of the page, giving the adjacent column a clear reference
  edge for comparison.

  - A shape is a semantic object, not a rasterized screenshot.
  - A grid is a layout object, not necessarily data.
  - A caption is a first-class block that can carry a sequence field.

  == Editorial checklist

  Before exporting, verify the page size, margins, header, footer, numbering,
  and the relationship between every caption and its object. This second column
  continues the same baseline and gutter so a small difference in line breaks is
  easy to see in a rendered comparison.

  A final sentence gives the lower column a little more density without turning
  the reference section into a solid block of text.
]
