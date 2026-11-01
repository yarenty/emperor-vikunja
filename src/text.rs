//! Task text both ways.
//!
//! Vikunja stores descriptions and comments as HTML (its editor is a rich-text editor). Models
//! write Markdown, so text going in is rendered to HTML with `pulldown-cmark`; text coming out
//! is rendered to plain, Markdown-flavoured text with `html2text`, which parses HTML5 properly
//! (entities, nested lists, tables) instead of guessing with regular expressions.

use pulldown_cmark::{Event, Options, Parser, html};

/// Width used when rendering HTML to text. Wide on purpose: models do not need hard wraps.
const TEXT_WIDTH: usize = 400;

/// Render Markdown to HTML for a Vikunja description or comment.
///
/// Raw HTML inside the Markdown is escaped and shown as text, never passed through: the model
/// writes Markdown, and the HTML that reaches Vikunja is only what the Markdown renderer makes.
pub fn markdown_to_html(markdown: &str) -> String {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TASKLISTS);
    let events = Parser::new_ext(markdown, options).map(|event| match event {
        Event::Html(raw) | Event::InlineHtml(raw) => Event::Text(raw),
        other => other,
    });
    let mut out = String::with_capacity(markdown.len() * 3 / 2);
    html::push_html(&mut out, events);
    out.trim_end().to_string()
}

/// Render a Vikunja HTML description or comment as plain text.
pub fn html_to_text(html: &str) -> String {
    if html.trim().is_empty() {
        return String::new();
    }
    let rendered =
        html2text::from_read(html.as_bytes(), TEXT_WIDTH).unwrap_or_else(|_| html.to_string());
    tidy(&rendered)
}

/// Trim trailing spaces and collapse runs of blank lines.
fn tidy(text: &str) -> String {
    let mut out: Vec<&str> = Vec::new();
    for line in text.lines().map(str::trim_end) {
        if line.is_empty() && out.last().is_none_or(|l| l.is_empty()) {
            continue;
        }
        out.push(line);
    }
    while out.last().is_some_and(|l| l.is_empty()) {
        out.pop();
    }
    out.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markdown_becomes_html() {
        let html = markdown_to_html("# Plan\n\n- **one**\n- `two`\n\n[docs](https://vikunja.io)");
        assert!(html.contains("<h1>Plan</h1>"), "{html}");
        assert!(html.contains("<li><strong>one</strong></li>"), "{html}");
        assert!(html.contains("<code>two</code>"), "{html}");
        assert!(
            html.contains(r#"<a href="https://vikunja.io">docs</a>"#),
            "{html}"
        );
    }

    #[test]
    fn markdown_tables_and_tasklists() {
        let html = markdown_to_html("| a | b |\n|---|---|\n| 1 | 2 |\n\n- [x] done\n- [ ] open");
        assert!(html.contains("<table>"), "{html}");
        assert!(html.contains("checkbox"), "{html}");
    }

    #[test]
    fn raw_html_in_markdown_is_escaped() {
        let html = markdown_to_html("hi <script>alert(1)</script>\n\n<img src=x onerror=y>");
        assert!(!html.contains("<script>"), "{html}");
        assert!(!html.contains("<img"), "{html}");
        assert!(html.contains("&lt;script&gt;"), "{html}");
    }

    #[test]
    fn html_becomes_text() {
        let text = html_to_text(
            "<h2>Steps</h2><p>Do <strong>this</strong> &amp; that.</p><ul><li>one</li><li>two</li></ul><p></p><p></p>",
        );
        assert!(text.contains("Steps"), "{text}");
        assert!(text.contains("this"), "{text}");
        assert!(text.contains("& that."), "{text}");
        assert!(text.contains("one") && text.contains("two"), "{text}");
        assert!(!text.contains('<'), "{text}");
        assert!(!text.contains("\n\n\n"), "{text}");
    }

    #[test]
    fn empty_html_is_empty_text() {
        assert_eq!(html_to_text(""), "");
        assert_eq!(html_to_text("  "), "");
    }

    #[test]
    fn markdown_round_trip_keeps_the_words() {
        let text = html_to_text(&markdown_to_html(
            "Ship **v0.1** on Friday.\n\n1. build\n2. tag",
        ));
        assert!(text.contains("Ship"), "{text}");
        assert!(text.contains("v0.1"), "{text}");
        assert!(text.contains("build") && text.contains("tag"), "{text}");
    }
}
