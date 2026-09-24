// Centered multi-line letterhead in the document body (not the page header).
//
// Exercises: `#align(center)` (previously dropped its content), `<br>` line
// breaks, and inline runs with different sizes/colors.
//
//   typst compile --format docx --features html body-letterhead.typ out.docx

#set document(title: "Offer of Employment")
#set page(paper: "a4", margin: (x: 2.2cm, y: 2.4cm))
#set text(font: "Calibri", size: 10.5pt)

#align(center)[
  #text(size: 22pt, weight: "bold", fill: rgb("#0B3C5D"))[Typsastra Office]
  #v(-0.35em)
  #text(size: 8.5pt, fill: luma(45%))[Software for documents]
]
#v(0.3em)
#align(center)[
  #text(size: 8.5pt, fill: luma(35%))[
    123 Innovation Way, Suite 400 · Phnom Penh 12000, Cambodia\
    +855 23 456 789 · hello\@typsastra.example
  ]
]
#v(0.45em)
#line(length: 100%, stroke: 1.2pt + rgb("#0B3C5D"))

#v(0.9em)

#text(size: 8.5pt, fill: luma(45%))[Ref: HR-2026-0142]

#v(0.2em)

#text(size: 8.5pt, fill: luma(45%))[14 March 2026]

#v(1em)

Ms. A. Candidate\
Phnom Penh

#v(1em)

*Subject: Offer of Employment*

#v(0.6em)

Dear Ms. Candidate,

We are pleased to offer you the position of _Senior Engineer_ at Typsastra Office.

#v(0.5em)

Yours sincerely,

#v(1.4em)

*Sovichea Tep*\
Chief Executive Officer
