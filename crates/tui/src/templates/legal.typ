// Korean legal document template for legal-ko
// Data is passed via `sys.inputs.v` as a dictionary
//
// Required keys:
//   doc_type: "law" | "precedent" | "admrule" | "ordinance"
//   title: document title
//   body: document body in Typst markup (converted from markdown by Rust)
//
// Optional keys (vary by doc_type):
//   category, department, case_number, court, date, case_type,
//   rule_type, agency, ordinance_type, region

#import sys: inputs
#let data = inputs.v

// ── Page setup ──────────────────────────────────────────────────────────

#set page(
  paper: "a4",
  margin: (x: 2cm, y: 2.5cm),
  footer: context align(center)[
    #text(size: 8pt, fill: rgb("#888"))[
      legal-ko
      #h(1fr)
      #counter(page).display("1 / 1", both: true)
    ]
  ],
)

#set text(size: 10pt, lang: "ko")
#set par(justify: true, leading: 0.8em)
#set heading(numbering: none)

// Style headings
#show heading.where(level: 1): it => {
  v(12pt)
  text(size: 14pt, weight: "bold")[#it.body]
  v(6pt)
}

#show heading.where(level: 2): it => {
  v(10pt)
  text(size: 12pt, weight: "bold")[#it.body]
  v(4pt)
}

#show heading.where(level: 3): it => {
  v(8pt)
  text(size: 11pt, weight: "bold")[#it.body]
  v(3pt)
}

// ── Cover header ────────────────────────────────────────────────────────

#let subtitle = if data.doc_type == "law" {
  let parts = ()
  if data.at("category", default: "") != "" { parts.push(data.category) }
  if data.at("department", default: "") != "" { parts.push(data.department) }
  parts.join(" · ")
} else if data.doc_type == "precedent" {
  let parts = ()
  if data.at("case_number", default: "") != "" { parts.push(data.case_number) }
  if data.at("court", default: "") != "" { parts.push(data.court) }
  if data.at("case_type", default: "") != "" { parts.push(data.case_type) }
  if data.at("date", default: "") != "" { parts.push(data.date) }
  parts.join(" · ")
} else if data.doc_type == "admrule" {
  let parts = ()
  if data.at("rule_type", default: "") != "" { parts.push(data.rule_type) }
  if data.at("agency", default: "") != "" { parts.push(data.agency) }
  if data.at("date", default: "") != "" { parts.push(data.date) }
  parts.join(" · ")
} else if data.doc_type == "ordinance" {
  let parts = ()
  if data.at("ordinance_type", default: "") != "" { parts.push(data.ordinance_type) }
  if data.at("region", default: "") != "" { parts.push(data.region) }
  if data.at("date", default: "") != "" { parts.push(data.date) }
  parts.join(" · ")
} else {
  ""
}

#let type_label = if data.doc_type == "law" {
  "법률"
} else if data.doc_type == "precedent" {
  "판례"
} else if data.doc_type == "admrule" {
  "행정규칙"
} else if data.doc_type == "ordinance" {
  "자치법규"
} else {
  "문서"
}

// Header block
#align(center)[
  #v(0.5cm)
  #text(size: 9pt, fill: rgb("#666"), tracking: 0.1em)[#upper(type_label)]
  #v(6pt)
  #text(size: 18pt, weight: "bold")[#data.title]
  #if subtitle != "" {
    v(4pt)
    text(size: 10pt, fill: rgb("#555"))[#subtitle]
  }
  #v(8pt)
  #line(length: 50%, stroke: 0.5pt + rgb("#ccc"))
  #v(16pt)
]

// ── Body ────────────────────────────────────────────────────────────────

#eval(mode: "markup", data.body)
