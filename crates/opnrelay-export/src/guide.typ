// SPDX-License-Identifier: AGPL-3.0-only
// OpnRelay guide template. All content comes from data.json as plain strings,
// which Typst inserts as text: user content is never evaluated as markup.

#let data = json("/data.json")
#let primary = rgb(data.primary)
#let accent = rgb(data.accent)
#let muted = rgb("#5d6673")
#let border = rgb("#dde1e7")

#set document(title: data.title, author: data.author)
#set text(font: ("Inter", "Libertinus Serif"), size: 10.5pt, fill: rgb("#1b1f24"), lang: "en")
#set par(justify: false, leading: 0.65em)

#let with-pages(s) = context {
  let total = counter(page).final().first()
  let current = counter(page).get().first()
  text(s.replace("{pages}", str(total)).replace("{page}", str(current)))
}

#set page(
  paper: "a4",
  margin: (x: 2.2cm, top: 2.8cm, bottom: 2.4cm),
  header: context {
    if not data.cover or counter(page).get().first() > 1 {
      set text(8.5pt, fill: muted)
      grid(
        columns: (auto, 1fr, auto),
        column-gutter: 8pt,
        align: horizon,
        if data.logo != none { image(data.logo, height: 0.7cm) },
        text(data.header),
        if data.classification != none {
          box(fill: rgb("#c4314b"), inset: (x: 5pt, y: 2pt), radius: 2pt, text(fill: white, weight: "bold", data.classification))
        },
      )
      v(-4pt)
      line(length: 100%, stroke: 1.5pt + primary)
    }
  },
  footer: {
    set text(8pt, fill: muted)
    line(length: 100%, stroke: 0.5pt + border)
    v(-4pt)
    grid(
      columns: (1fr, auto),
      with-pages(data.footer),
      if data.watermark != none { text(data.watermark) },
    )
  },
  background: if data.watermark != none {
    rotate(-35deg, text(18pt, fill: luma(236), weight: "bold", data.watermark))
  },
)

#let run(r) = {
  let c = if r.c { box(fill: rgb("#f0f2f5"), inset: (x: 2pt), outset: (y: 2pt), radius: 2pt, text(font: "DejaVu Sans Mono", size: 0.9em, r.t)) } else { text(r.t) }
  if r.b { c = strong(c) }
  if r.i { c = emph(c) }
  c
}

#let body(blocks) = {
  for b in blocks {
    if b.runs.len() > 0 {
      if b.kind == "li" {
        list(for r in b.runs { run(r) })
      } else {
        par(for r in b.runs { run(r) })
      }
    }
  }
}

// ---- Cover ----
#if data.cover {
  v(3cm)
  if data.logo != none { image(data.logo, height: 1.8cm) }
  v(1.2cm)
  if data.classification != none {
    box(fill: rgb("#c4314b"), inset: (x: 8pt, y: 4pt), radius: 3pt, text(fill: white, weight: "bold", data.classification))
    v(0.4cm)
  }
  text(28pt, weight: "bold", fill: primary, data.title)
  v(0.3cm)
  if data.description != "" { text(13pt, fill: muted, data.description) }
  v(1fr)
  line(length: 100%, stroke: 2pt + primary)
  grid(
    columns: (1fr, 1fr, 1fr),
    row-gutter: 4pt,
    text(8pt, fill: muted, "AUTHOR"), text(8pt, fill: muted, "UPDATED"), text(8pt, fill: muted, "VERSION"),
    text(data.author), text(data.date), text(str(data.version)),
  )
  if data.company != "" { v(0.4cm); text(9pt, fill: muted, data.company) }
  pagebreak()
} else {
  text(22pt, weight: "bold", fill: primary, data.title)
  linebreak()
  text(9pt, fill: muted, data.author + " · " + data.date + " · version " + str(data.version))
  if data.description != "" { par(text(11pt, data.description)) }
  v(0.4cm)
}

// ---- Steps ----
#for s in data.steps {
  if s.kind == "heading" {
    v(0.4cm)
    block(below: 0.5cm, {
      text(15pt, weight: "bold", fill: primary, s.title)
      v(-6pt)
      line(length: 100%, stroke: 0.75pt + border)
    })
  } else if s.kind == "tip" or s.kind == "alert" {
    let (bg, fg, label) = if s.kind == "tip" { (rgb("#e7f3ec"), rgb("#18794e"), "Tip") } else { (rgb("#fdecec"), rgb("#c4314b"), "Important") }
    block(fill: bg, inset: 10pt, radius: 5pt, width: 100%, breakable: false, {
      text(weight: "bold", fill: fg, label + if s.title != "" { ": " } else { "" })
      text(weight: "bold", s.title)
      body(s.body)
    })
  } else {
    block(width: 100%, breakable: true, above: 0.6cm, below: 0.6cm, {
      block(breakable: false, sticky: true, grid(
        columns: (auto, 1fr),
        column-gutter: 8pt,
        align: horizon,
        if s.number != none {
          box(width: 20pt, height: 20pt, radius: 10pt, fill: primary, align(center + horizon, text(fill: white, weight: "bold", size: 9pt, str(s.number))))
        },
        text(11.5pt, weight: "semibold", s.title),
      ))
      body(s.body)
      if s.image != none {
        block(stroke: 0.5pt + border, radius: 3pt, clip: true, breakable: false, image(s.image.path, width: s.image.width_cm * 1cm))
      }
    })
  }
}
