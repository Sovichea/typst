// Repeating multi-line letterhead in the page header (appears on every page).
//
// Exercises the header/footer path: the HTML export drops page headers and
// footers entirely, so their content and spacing are recovered from the layout
// oracle and written as Word header/footer parts.
//
//   typst compile --format docx --features html header-letterhead.typ out.docx
//   typst compile --format layout --features html header-letterhead.typ out.json
//
// Expected: out.docx contains word/header1.xml (3 lines) and word/footer1.xml,
// and sectPr references them; the header repeats on every page.

#set document(title: "Header Letterhead")
#set page(
  paper: "a4",
  margin: (x: 2.2cm, y: 2.6cm),
  header: [
    #text(size: 15pt, weight: "bold", fill: rgb("#0B3C5D"))[Typsastra Office]
    #v(-0.25em)
    #text(size: 8pt, fill: luma(45%))[Software for documents]
    #v(0.15em)
    #text(size: 8pt, fill: luma(35%))[123 Innovation Way, Suite 400 · Phnom Penh 12000, Cambodia]
    #v(0.1em)
    #line(length: 100%, stroke: 1pt + rgb("#0B3C5D"))
  ],
  footer: context [
    #text(size: 8pt, fill: luma(45%))[Confidential · Page #counter(page).display()]
  ],
)
#set text(font: "Calibri", size: 10.5pt)

= Introduction

This letter uses a repeating multi-line letterhead in the page header.

#lorem(60)

= Details

#lorem(90)

#pagebreak()

= Second page

#lorem(120)
