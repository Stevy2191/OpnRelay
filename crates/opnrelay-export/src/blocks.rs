// SPDX-License-Identifier: AGPL-3.0-only
//! Parses step Markdown into a tiny, safe block model (paragraphs and list
//! items made of styled text runs). Raw HTML, links and images are reduced
//! to plain text, so no renderer ever emits user-controlled markup.

use pulldown_cmark::{Event, Parser, Tag, TagEnd};
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Run {
    #[serde(rename = "t")]
    pub text: String,
    #[serde(rename = "b")]
    pub bold: bool,
    #[serde(rename = "i")]
    pub italic: bool,
    #[serde(rename = "c")]
    pub code: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Block {
    /// `"p"` (paragraph) or `"li"` (bullet item).
    pub kind: &'static str,
    pub runs: Vec<Run>,
}

pub fn parse(md: &str) -> Vec<Block> {
    let mut blocks = Vec::new();
    let mut current: Option<Block> = None;
    let (mut bold, mut italic) = (0u32, 0u32);
    let mut in_item = false;

    let push_text = |cur: &mut Option<Block>, text: &str, b: bool, i: bool, c: bool, item: bool| {
        if text.is_empty() {
            return;
        }
        let block = cur.get_or_insert_with(|| Block {
            kind: if item { "li" } else { "p" },
            runs: Vec::new(),
        });
        if let Some(last) = block.runs.last_mut()
            && last.bold == b
            && last.italic == i
            && last.code == c
        {
            last.text.push_str(text);
            return;
        }
        block.runs.push(Run {
            text: text.to_string(),
            bold: b,
            italic: i,
            code: c,
        });
    };

    for ev in Parser::new(md) {
        match ev {
            Event::Start(Tag::Item) => {
                if let Some(b) = current.take() {
                    blocks.push(b);
                }
                in_item = true;
            }
            Event::End(TagEnd::Item) => {
                if let Some(b) = current.take() {
                    blocks.push(b);
                }
                in_item = false;
            }
            Event::Start(
                Tag::Paragraph | Tag::Heading { .. } | Tag::BlockQuote(_) | Tag::CodeBlock(_),
            ) => {
                if !in_item && let Some(b) = current.take() {
                    blocks.push(b);
                }
            }
            Event::End(
                TagEnd::Paragraph | TagEnd::Heading(_) | TagEnd::BlockQuote(_) | TagEnd::CodeBlock,
            ) => {
                if !in_item && let Some(b) = current.take() {
                    blocks.push(b);
                }
            }
            Event::Start(Tag::Strong) => bold += 1,
            Event::End(TagEnd::Strong) => bold = bold.saturating_sub(1),
            Event::Start(Tag::Emphasis) => italic += 1,
            Event::End(TagEnd::Emphasis) => italic = italic.saturating_sub(1),
            Event::Text(t) | Event::Html(t) | Event::InlineHtml(t) => {
                push_text(&mut current, &t, bold > 0, italic > 0, false, in_item)
            }
            Event::Code(t) => push_text(&mut current, &t, bold > 0, italic > 0, true, in_item),
            Event::SoftBreak => push_text(&mut current, " ", bold > 0, italic > 0, false, in_item),
            Event::HardBreak => push_text(&mut current, "\n", bold > 0, italic > 0, false, in_item),
            _ => {}
        }
    }
    if let Some(b) = current.take() {
        blocks.push(b);
    }
    blocks
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn styles_lists_and_html_as_text() {
        let b = parse(
            "Hello **bold** and *it* `c`\n\n- one\n- **two**\n\n<script>x</script>\n\nSee [link](javascript:x)",
        );
        assert_eq!(b.len(), 5);
        assert_eq!(b[0].kind, "p");
        assert_eq!(
            b[0].runs[1],
            Run {
                text: "bold".into(),
                bold: true,
                italic: false,
                code: false
            }
        );
        assert!(b[0].runs.iter().any(|r| r.code && r.text == "c"));
        assert_eq!(b[1].kind, "li");
        assert!(b[2].runs[0].bold);
        let html: String = b[3].runs.iter().map(|r| r.text.as_str()).collect();
        assert!(
            html.contains("<script>"),
            "html kept as literal text: {html}"
        );
        let link: String = b[4].runs.iter().map(|r| r.text.as_str()).collect();
        assert_eq!(link, "See link", "link targets dropped");
        assert!(parse("").is_empty());
    }
}
