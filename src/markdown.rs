use std::collections::HashSet;
use std::sync::LazyLock;

use pulldown_cmark::{html, CowStr, Event, Options, Parser, Tag, TagEnd};

static SANITIZER: LazyLock<ammonia::Builder<'static>> = LazyLock::new(|| {
    let mut b = ammonia::Builder::default();
    b.add_tags(["input"])
        .add_tag_attributes("code", ["class"])
        .add_tag_attributes("div", ["class", "id"])
        .add_tag_attributes("sup", ["class", "id"])
        .add_tag_attributes("input", ["type", "checked", "disabled"])
        .add_tag_attributes("td", ["style"])
        .add_tag_attributes("th", ["style"])
        .attribute_filter(|element, attribute, value| match (element, attribute) {
            ("input", "type") if value != "checkbox" => None,
            // Tables emit `style="text-align: …"`; allow nothing else.
            ("td" | "th", "style")
                if !matches!(value, "text-align: left" | "text-align: center" | "text-align: right") =>
            {
                None
            }
            _ => Some(value.into()),
        });
    b.url_relative(ammonia::UrlRelative::PassThrough);
    b
});

fn options() -> Options {
    Options::ENABLE_TABLES
        | Options::ENABLE_FOOTNOTES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_SMART_PUNCTUATION
}

/// Renders Markdown to sanitized HTML. Links and images that point at uploaded
/// media (`/media/...`) are prefixed with `media_prefix`, so content keeps
/// working when the blog's base path changes.
pub fn render(markdown: &str, media_prefix: &str) -> String {
    let prefix = |url: CowStr<'_>| -> CowStr<'static> {
        if url.starts_with("/media/") {
            CowStr::from(format!("{media_prefix}{url}"))
        } else {
            CowStr::from(url.into_string())
        }
    };
    let events = Parser::new_ext(markdown, options()).map(|event| match event {
        Event::Start(Tag::Image { link_type, dest_url, title, id }) => Event::Start(Tag::Image {
            link_type,
            dest_url: prefix(dest_url),
            title,
            id,
        }),
        Event::Start(Tag::Link { link_type, dest_url, title, id }) => Event::Start(Tag::Link {
            link_type,
            dest_url: prefix(dest_url),
            title,
            id,
        }),
        other => other,
    });
    let mut out = String::with_capacity(markdown.len() * 3 / 2);
    html::push_html(&mut out, events);
    SANITIZER.clean(&out).to_string()
}

/// Plain text of the first paragraphs, cut at a word boundary, for excerpts and
/// meta descriptions.
pub fn plain_text(markdown: &str, max_chars: usize) -> String {
    let mut out = String::new();
    let mut in_skipped = 0usize;
    for event in Parser::new_ext(markdown, options()) {
        match event {
            Event::Start(Tag::CodeBlock(_) | Tag::Image { .. } | Tag::Heading { .. }) => in_skipped += 1,
            Event::End(TagEnd::CodeBlock | TagEnd::Image | TagEnd::Heading(_)) => {
                in_skipped = in_skipped.saturating_sub(1)
            }
            Event::Text(t) | Event::Code(t) if in_skipped == 0 => out.push_str(&t),
            Event::SoftBreak | Event::HardBreak | Event::End(TagEnd::Paragraph) => out.push(' '),
            _ => {}
        }
        if out.chars().count() > max_chars + 1 {
            break;
        }
    }
    let collapsed = out.split_whitespace().collect::<Vec<_>>().join(" ");
    truncate_words(&collapsed, max_chars)
}

pub fn truncate_words(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let cut: String = text.chars().take(max_chars).collect();
    let cut = match cut.rfind(' ') {
        Some(i) if i > max_chars / 2 => &cut[..i],
        _ => &cut[..],
    };
    format!("{}…", cut.trim_end_matches(|c: char| c.is_ascii_punctuation()))
}

pub fn reading_minutes(markdown: &str) -> usize {
    (markdown.split_whitespace().count() / 225).max(1)
}

/// Upload filenames referenced by a document, so media in use can be flagged.
pub fn media_references(markdown: &str) -> HashSet<String> {
    markdown
        .match_indices("/media/")
        .filter_map(|(i, m)| {
            let rest = &markdown[i + m.len()..];
            let end = rest
                .find(|c: char| !(c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_'))
                .unwrap_or(rest.len());
            (end > 0).then(|| rest[..end].to_string())
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_scripts_and_handlers() {
        let html = render("Hello <script>alert(1)</script><img src=x onerror=alert(1)> [x](javascript:alert(1))", "");
        assert!(!html.contains("<script"));
        assert!(!html.contains("onerror"));
        assert!(!html.contains("javascript:"));
    }

    #[test]
    fn prefixes_media_paths() {
        let html = render("![cat](/media/cat.png) [doc](https://example.com/media/x)", "/blog");
        assert!(html.contains(r#"src="/blog/media/cat.png""#));
        assert!(html.contains(r#"href="https://example.com/media/x""#));
    }

    #[test]
    fn keeps_code_language_and_tasklists() {
        let html = render("```rust\nfn main() {}\n```\n\n- [x] done", "");
        assert!(html.contains(r#"class="language-rust""#));
        assert!(html.contains(r#"type="checkbox""#));
    }

    #[test]
    fn plain_text_excerpt() {
        let text = plain_text("# Title\n\nSome *emphasised* text with `code`.\n\n```\nskip\n```\n\nMore.", 200);
        assert_eq!(text, "Some emphasised text with code. More.");
        assert_eq!(truncate_words("one two three four", 9), "one two…");
    }
}
