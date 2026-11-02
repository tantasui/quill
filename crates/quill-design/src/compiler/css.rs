//! Custom properties from fenced CSS blocks. This isn't a CSS parser: it reads
//! `--name: value;` declarations from selectors that set global tokens, and
//! everything else in the block is ignored.

use super::diagnostic::{LineIndex, Span};
use super::extract::{Extracted, FontDecl, FontRole, Origin, Scheme, Token};
use super::yaml::font_role;
use crate::{Color, Length};

/// Reads a CSS block found at byte `offset` of the file.
pub(crate) fn extract(css: &str, offset: usize, index: &LineIndex, out: &mut Extracted) {
    // Comments are blanked rather than removed, so offsets stay correct.
    let cleaned = blank_comments(css);
    let mut dark_depth: Vec<bool> = Vec::new();
    let mut position = 0;
    while let Some(found) = cleaned[position..].find(['{', '}']) {
        let at = position + found;
        if cleaned.as_bytes()[at] == b'}' {
            dark_depth.pop();
            position = at + 1;
            continue;
        }
        let prelude_start = cleaned[position..at]
            .rfind([';', '}'])
            .map_or(position, |i| position + i + 1);
        let prelude = cleaned[prelude_start..at].trim().to_ascii_lowercase();
        if prelude.starts_with('@') {
            dark_depth.push(prelude.contains("dark"));
            position = at + 1;
            continue;
        }
        let end = cleaned[at + 1..]
            .find('}')
            .map_or(cleaned.len(), |i| at + 1 + i);
        let is_global = ["root", "html", "body", "theme", ".dark", ".light", "data-"]
            .iter()
            .any(|word| prelude.contains(word));
        if is_global {
            let dark = prelude.contains("dark") || dark_depth.iter().any(|d| *d);
            let scheme = if dark { Scheme::Dark } else { Scheme::Light };
            read_declarations(&cleaned[at + 1..end], offset + at + 1, scheme, index, out);
        }
        position = (end + 1).min(cleaned.len());
    }
}

fn read_declarations(
    body: &str,
    offset: usize,
    scheme: Scheme,
    index: &LineIndex,
    out: &mut Extracted,
) {
    let mut position = 0;
    for piece in body.split_inclusive(';') {
        let start = position;
        position += piece.len();
        let Some((name, value)) = piece.split_once(':') else {
            continue;
        };
        let name = name.trim();
        let value = value.trim().trim_end_matches(';').trim();
        if !name.starts_with("--") || value.is_empty() {
            continue;
        }
        let value_at = start + piece.find(value).unwrap_or(0);
        let span = index.span(offset + value_at, value.len());
        declaration(name, value, span, scheme, out);
    }
}

fn declaration(name: &str, value: &str, span: Span, scheme: Scheme, out: &mut Extracted) {
    let bare = name.trim_start_matches('-');
    let token = || Token {
        name: name.to_string(),
        value: value.to_string(),
        span,
        origin: Origin::Css,
        scheme,
    };
    let has = |word: &str| bare.contains(word);
    let is_measure = [
        "measure",
        "content-width",
        "reading-width",
        "prose-width",
        "max-width-prose",
    ];
    let is_page = ["page-width", "site-width", "container", "max-width"];
    if has("font")
        && !["size", "weight", "feature", "variation"]
            .iter()
            .any(|w| has(w))
        || has("family")
    {
        out.fonts.push(FontDecl {
            family: first_family(value),
            role: match font_role(bare) {
                FontRole::Any => FontRole::Body,
                role => role,
            },
            substitutes: value.split(',').skip(1).map(unquote).collect(),
            span,
            origin: Origin::Css,
        });
    } else if has("font-weight") {
        if font_role(bare) == FontRole::Heading {
            out.heading_weights.push(token());
        } else {
            out.unused.push((name.to_string(), span));
        }
    } else if has("radius") || has("rounded") {
        out.radii.push(token());
    } else if has("shadow") || has("elevation") {
        out.shadows.push(token());
    } else if is_measure.iter().any(|w| has(w)) {
        out.measures.push(token());
    } else if is_page.iter().any(|w| has(w)) {
        out.page_widths.push(token());
    } else if Color::parse(value).is_ok() || value.starts_with("var(") {
        out.colors.push(token());
    } else if value.starts_with(|c: char| c.is_ascii_digit() || c == '.') {
        // A size Quill doesn't use. It can still be malformed, and saying so
        // tells the author their file has a mistake in it.
        match Length::parse(value) {
            Err(error) if value.parse::<f32>().is_err() => {
                out.invalid(name, value, span, &error.to_string());
            }
            _ => out.unused.push((name.to_string(), span)),
        }
    } else {
        out.unused.push((name.to_string(), span));
    }
}

fn unquote(name: &str) -> String {
    name.trim().trim_matches(['"', '\'']).trim().to_string()
}

fn first_family(value: &str) -> String {
    unquote(value.split(',').next().unwrap_or(value))
}

/// Replaces comments with spaces of the same byte length, keeping newlines, so
/// byte offsets into the result still match the original.
fn blank_comments(css: &str) -> String {
    let mut out = String::with_capacity(css.len());
    let mut rest = css;
    while let Some(start) = rest.find("/*") {
        out.push_str(&rest[..start]);
        let comment = &rest[start..];
        let end = comment[2..].find("*/").map_or(comment.len(), |i| i + 4);
        for c in comment[..end].chars() {
            let filler = if c == '\n' { '\n' } else { ' ' };
            out.extend(std::iter::repeat_n(filler, c.len_utf8()));
        }
        rest = &comment[end..];
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(css: &str) -> Extracted {
        let index = LineIndex::new(css);
        let mut out = Extracted::default();
        extract(css, 0, &index, &mut out);
        out
    }

    #[test]
    fn reads_root_tokens_with_positions() {
        let out = run(":root {\n  --color-ink: #0c0a08;\n  --radius-md: 6px;\n}\n");
        assert_eq!(out.colors[0].name, "--color-ink");
        assert_eq!(out.colors[0].value, "#0c0a08");
        assert_eq!(
            out.colors[0].span,
            Span {
                line: 2,
                column: 16,
                len: 7
            }
        );
        assert_eq!(out.radii[0].value, "6px");
    }

    #[test]
    fn sorts_declarations_by_what_they_are() {
        let out = run(r#":root {
            --font-body: "Söhne", "Inter", sans-serif;
            --font-mono: "Berkeley Mono", monospace;
            --font-weight-heading: 500;
            --font-size-lg: 18px;
            --shadow-card: 0 1px 2px #00000020;
            --content-width: 42rem;
            --max-width: 1200px;
            --z-modal: 100;
            --ease: ease-in;
        }"#);
        assert_eq!(out.fonts[0].family, "Söhne");
        assert_eq!(out.fonts[0].role, FontRole::Body);
        assert_eq!(out.fonts[0].substitutes, ["Inter", "sans-serif"]);
        assert_eq!(out.fonts[1].role, FontRole::Mono);
        assert_eq!(out.heading_weights[0].value, "500");
        assert_eq!(out.shadows[0].name, "--shadow-card");
        assert_eq!(out.measures[0].value, "42rem");
        assert_eq!(out.page_widths[0].value, "1200px");
        assert_eq!(out.unused.len(), 3);
        assert!(out.diagnostics.is_empty());
    }

    #[test]
    fn rejects_the_malformed_gap_from_a_real_file() {
        let out = run(":root { --section-gap: 64-128px; }");
        assert_eq!(out.diagnostics.len(), 1);
        assert!(out.diagnostics[0].message.contains("--section-gap"));
        assert!(out.diagnostics[0].message.contains("64-128px"));
    }

    #[test]
    fn finds_dark_tokens_in_each_form() {
        for css in [
            "@media (prefers-color-scheme: dark) { :root { --bg: #000; } }",
            ".dark { --bg: #000; }",
            "[data-theme=\"dark\"] { --bg: #000; }",
        ] {
            let out = run(css);
            assert_eq!(out.colors[0].scheme, Scheme::Dark, "{css}");
        }
        let out =
            run("@media (min-width: 40rem) { :root { --bg: #fff; } }\n:root { --text: #000; }");
        assert_eq!(out.colors[0].scheme, Scheme::Light);
        assert_eq!(out.colors[1].scheme, Scheme::Light);
    }

    #[test]
    fn ignores_comments_and_component_selectors() {
        let out = run("/* :root { --a: #000; } é */\n.button { --b: #111; }\n:root { --c: #222; /* x */ --d: #333; }");
        let names: Vec<_> = out.colors.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["--c", "--d"]);
        assert_eq!(out.colors[0].span.line, 3);
    }

    #[test]
    fn survives_malformed_css() {
        for css in [
            "",
            "}",
            "{",
            ":root {",
            ":root { --a",
            ":root { --a: ",
            "}}}{{{",
            "/* never closed",
            "@media {",
        ] {
            run(css);
        }
    }
}
