//! Text normalization.
//!
//! Job descriptions are stored exactly as the source served them. Every
//! provider so far serves markup: Eightfold's `jobDescription`, JSON-LD
//! `description`, and SmartRecruiters' composed sections are HTML, while a
//! manual paste may be plain text. Anything that reads the text for a person or
//! a model goes through `plain_text` first, so the transformation exists in one
//! place with tests rather than as an instruction repeated in a prompt.
//!
//! The stored row is deliberately **not** rewritten. It is the record of what
//! the site said, Phase 4 may want the markup, and a re-fetch is not
//! guaranteed.

use scraper::node::Node;
use scraper::{ElementRef, Html};

/// The result of normalizing a stored description.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlainText {
    /// The normalized text, truncated to the requested limit.
    pub text: String,
    /// `true` when `text` holds fewer characters than the normalized input.
    pub truncated: bool,
    /// The input length in characters, so a caller can compare markup with
    /// prompt text.
    pub source_chars: usize,
}

/// Convert stored job markup to prompt/display text.
///
/// Deterministic and idempotent: plain text passes through unchanged, and
/// calling it twice equals calling it once.
///
/// Rules:
/// - `script`, `style`, `noscript`, `head`, and `title` are dropped entirely.
/// - Block boundaries (`p`, `div`, `tr`, `h1`-`h6`, lists, and similar) become
///   line breaks; `br` is a single line break; `li` becomes `"- "`.
/// - Entities are decoded by the HTML parser (`&amp;`, `&#39;`, `&nbsp;`).
/// - Runs of spaces are collapsed, each line is trimmed, and runs of blank
///   lines become at most one blank line.
/// - Truncation happens on a character boundary, never inside a character.
pub fn plain_text(raw: &str, max_chars: usize) -> PlainText {
    let text = collapse(walk(raw));
    let source_chars = raw.chars().count();

    let char_count = text.chars().count();
    let truncated = char_count > max_chars;
    let text = if truncated {
        text.chars().take(max_chars).collect()
    } else {
        text
    };

    PlainText {
        text,
        truncated,
        source_chars,
    }
}

/// Walk a fragment, appending its text with block boundaries as newlines.
fn walk(raw: &str) -> String {
    let document = Html::parse_fragment(raw);
    let mut out = String::with_capacity(raw.len());
    element_text(document.root_element(), &mut out);
    out
}

fn element_text(element: ElementRef<'_>, out: &mut String) {
    for child in element.children() {
        match child.value() {
            Node::Text(text) => out.push_str(text),
            Node::Element(_) => {
                if let Some(child) = ElementRef::wrap(child) {
                    child_text(child, out);
                }
            }
            _ => {}
        }
    }
}

fn child_text(element: ElementRef<'_>, out: &mut String) {
    let name = element.value().name();

    // Content that is never part of a description. A fragment parser lifts
    // `title` out of `head`, so it is dropped by name too.
    if matches!(name, "script" | "style" | "noscript" | "head" | "title") {
        return;
    }

    match name {
        "br" => out.push('\n'),
        // A list item opens with a bullet and relies on the next block (or the
        // closing list) for its separation.
        "li" => {
            out.push_str("\n- ");
            element_text(element, out);
        }
        "td" | "th" => {
            element_text(element, out);
            out.push(' ');
        }
        _ if is_block(name) => {
            out.push('\n');
            element_text(element, out);
            out.push('\n');
        }
        _ => element_text(element, out),
    }
}

/// Elements whose boundaries are paragraph or line breaks.
fn is_block(name: &str) -> bool {
    matches!(
        name,
        "p" | "div"
            | "tr"
            | "h1"
            | "h2"
            | "h3"
            | "h4"
            | "h5"
            | "h6"
            | "ul"
            | "ol"
            | "table"
            | "section"
            | "article"
            | "header"
            | "footer"
            | "blockquote"
            | "pre"
            | "dl"
            | "dt"
            | "dd"
    )
}

/// Collapse whitespace without changing the line structure the walker built.
fn collapse(raw: String) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut blank_lines = 0;

    for line in raw.replace('\r', "\n").split('\n') {
        let mut line_out = String::with_capacity(line.len());
        let mut previous_space = false;

        for ch in line.chars() {
            // `&nbsp;` decodes to a non-breaking space; treat it as a space so
            // it collapses like the rest of the whitespace.
            let ch = if ch == '\u{00a0}' || ch == '\t' {
                ' '
            } else {
                ch
            };
            if ch == ' ' {
                if previous_space {
                    continue;
                }
                previous_space = true;
            } else {
                previous_space = false;
            }
            line_out.push(ch);
        }

        let line = line_out.trim();
        if line.is_empty() {
            blank_lines += 1;
            continue;
        }

        if !out.is_empty() {
            out.push('\n');
            if blank_lines > 0 {
                out.push('\n');
            }
        }
        out.push_str(line);
        blank_lines = 0;
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const MAX: usize = usize::MAX;

    fn text(raw: &str) -> String {
        plain_text(raw, MAX).text
    }

    #[test]
    fn plain_text_passes_through_unchanged() {
        let plain = "We are hiring.\n\nApply now.";
        assert_eq!(text(plain), plain);
        assert!(!plain_text(plain, MAX).truncated);
    }

    #[test]
    fn html_tags_are_removed_and_blocks_become_paragraphs() {
        let html = "<p>First paragraph.</p><p>Second <strong>paragraph</strong>.</p>";
        assert_eq!(text(html), "First paragraph.\n\nSecond paragraph.");
    }

    #[test]
    fn list_items_become_bullets() {
        let html = "<ul><li>One</li><li>Two</li></ul>";
        assert_eq!(text(html), "- One\n- Two");
    }

    #[test]
    fn line_breaks_stay_single() {
        assert_eq!(text("one<br>two<br/>three"), "one\ntwo\nthree");
    }

    #[test]
    fn entities_are_decoded() {
        assert_eq!(
            text("<p>R&amp;D &#39;team&#39;&nbsp;here</p>"),
            "R&D 'team' here"
        );
    }

    #[test]
    fn script_style_and_head_content_is_dropped() {
        let html = "<head><title>Title</title><style>p { color: red; }</style></head>\
                    <script>alert('x')</script>\
                    <noscript>Enable JS</noscript>\
                    <p>Visible.</p>";
        assert_eq!(text(html), "Visible.");
    }

    #[test]
    fn indentation_and_blank_runs_are_collapsed() {
        let html = "<div>\n   <p>   A    paragraph.   </p>\n\n\n   <p>B.</p>\n</div>";
        assert_eq!(text(html), "A paragraph.\n\nB.");
    }

    #[test]
    fn normalizing_twice_equals_normalizing_once() {
        let html = "<div><h2>Requirements</h2><ul><li>C++</li><li>SQL</li></ul>\
                    <p>R&amp;D team.</p></div>";
        let once = text(html);
        assert_eq!(text(&once), once, "idempotence: {once:?}");
    }

    #[test]
    fn truncation_stops_on_a_character_boundary() {
        let html = "<p>aaa\u{00e9}\u{00e9}bbb</p>";
        let limited = plain_text(html, 4);

        assert_eq!(limited.text, "aaa\u{00e9}");
        assert!(limited.truncated);
        assert_eq!(limited.text.chars().count(), 4);
        assert_eq!(limited.source_chars, html.chars().count());
    }

    #[test]
    fn a_limit_at_or_above_the_length_is_not_truncated() {
        let limited = plain_text("<p>abcd</p>", 4);
        assert_eq!(limited.text, "abcd");
        assert!(!limited.truncated);
    }

    #[test]
    fn an_empty_description_is_empty() {
        let limited = plain_text("", MAX);
        assert!(limited.text.is_empty());
        assert!(!limited.truncated);
        assert_eq!(limited.source_chars, 0);
    }

    /// The fixture is the JSON-LD block saved from a live Waymo posting; its
    /// `description` is the HTML the provider actually stores.
    #[test]
    fn a_real_provider_description_normalizes_without_tags() {
        let raw = waymo_description();

        let plain = plain_text(&raw, MAX).text;

        assert!(plain.starts_with("Waymo is an autonomous driving technology company"));
        assert!(plain.contains("You will:"));
        assert!(plain.contains("- Design, implement, and optimize low-latency data pipelines"));
        assert!(plain.contains("Data Architecture & Processing"));
        assert!(
            !plain.contains('<') && !plain.contains('>'),
            "tags survived: {plain}"
        );
        assert!(!plain.contains("class=\"content-intro\""));
        assert!(plain.lines().count() > 5, "paragraphs collapsed: {plain}");
    }

    fn waymo_description() -> String {
        const PAGE: &str = include_str!("../tests/fixtures/waymo_job_posting.html");
        let document = Html::parse_document(PAGE);
        let selector =
            scraper::Selector::parse("script[type='application/ld+json']").expect("selector");
        let json = document
            .select(&selector)
            .next()
            .expect("ld+json block")
            .text()
            .collect::<String>();
        let value: serde_json::Value = serde_json::from_str(&json).expect("json");
        value["description"]
            .as_str()
            .expect("description")
            .to_string()
    }
}
