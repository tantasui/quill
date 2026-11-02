//! YAML tokens: the front matter block and interleaved ` ```yaml ` fences. The
//! parser is event-based and aliases aren't expanded, so a hostile file can't
//! multiply itself into a huge document.

use saphyr_parser::{Event, Parser};

use super::diagnostic::{Code, Diagnostic, Span};
use super::extract::{Extracted, FontDecl, FontRole, Origin, Scheme, Token};
use crate::Length;

const MAX_DEPTH: usize = 16;

/// Finds a front matter block. Returns the YAML text, the number of source
/// lines before it, and the byte offset where the Markdown body starts.
pub(crate) fn split_front_matter(source: &str) -> Option<(&str, usize, usize)> {
    let start = usize::from(source.starts_with('\u{feff}')) * '\u{feff}'.len_utf8();
    let rest = &source[start..];
    let first_line_end = rest.find('\n')?;
    if rest[..first_line_end].trim_end() != "---" {
        return None;
    }
    let yaml_start = first_line_end + 1;
    let mut offset = yaml_start;
    for line in rest[yaml_start..].split_inclusive('\n') {
        let fence = line.trim_end();
        if fence == "---" || fence == "..." {
            return Some((&rest[yaml_start..offset], 1, start + offset + line.len()));
        }
        offset += line.len();
    }
    None
}

enum Frame {
    Map { key: Option<String> },
    Seq,
}

/// Reads a YAML document into `out`. `line_offset` is how many source lines
/// come before the text, so reported lines match the original file.
pub(crate) fn extract(text: &str, line_offset: u32, out: &mut Extracted) {
    let mut stack: Vec<Frame> = Vec::new();
    let mut path: Vec<String> = Vec::new();
    for item in Parser::new_from_str(text) {
        let (event, range) = match item {
            Ok(item) => item,
            Err(error) => {
                let span = Span {
                    line: error.marker().line() as u32 + line_offset,
                    column: error.marker().col() as u32 + 1,
                    len: 1,
                };
                out.diagnostics.push(
                    Diagnostic::new(
                        Code::MalformedYaml,
                        format!("this YAML couldn't be read: {}", error.info()),
                    )
                    .at(span)
                    .help("Tokens from this block were skipped. Check its indentation and quotes."),
                );
                return;
            }
        };
        match event {
            Event::MappingStart(..) | Event::SequenceStart(..) => {
                if stack.len() >= MAX_DEPTH {
                    let span = Span {
                        line: range.start.line() as u32 + line_offset,
                        column: range.start.col() as u32 + 1,
                        len: 1,
                    };
                    out.diagnostics.push(
                        Diagnostic::new(Code::MalformedYaml, "this YAML is nested too deeply")
                            .at(span)
                            .help("Tokens from this block were skipped."),
                    );
                    return;
                }
                if let Some(Frame::Map { key: Some(key) }) = stack.last() {
                    path.push(key.clone());
                } else {
                    path.push(String::new());
                }
                let is_map = matches!(event, Event::MappingStart(..));
                stack.push(if is_map {
                    Frame::Map { key: None }
                } else {
                    Frame::Seq
                });
            }
            Event::MappingEnd | Event::SequenceEnd => {
                stack.pop();
                path.pop();
                if let Some(Frame::Map { key }) = stack.last_mut() {
                    *key = None;
                }
            }
            Event::Scalar(value, ..) => {
                let span = Span {
                    line: range.start.line() as u32 + line_offset,
                    column: range.start.col() as u32 + 1,
                    len: (value.chars().count() as u32).max(1),
                };
                match stack.last_mut() {
                    Some(Frame::Map { key }) => match key.take() {
                        None => *key = Some(value.to_string()),
                        Some(name) => {
                            let full: Vec<&str> = path
                                .iter()
                                .skip(1)
                                .map(String::as_str)
                                .chain([name.as_str()])
                                .collect();
                            apply(&full, &value, span, out);
                        }
                    },
                    Some(Frame::Seq) | None => {}
                }
            }
            Event::Alias(_) => {
                if let Some(Frame::Map { key }) = stack.last_mut() {
                    *key = None;
                }
            }
            _ => {}
        }
    }
}

fn apply(path: &[&str], value: &str, span: Span, out: &mut Extracted) {
    let token = |name: &str, scheme| Token {
        name: name.to_string(),
        value: value.to_string(),
        span,
        origin: Origin::Yaml,
        scheme,
    };
    let lower: Vec<String> = path.iter().map(|p| p.to_ascii_lowercase()).collect();
    let parts: Vec<&str> = lower.iter().map(String::as_str).collect();
    match parts.as_slice() {
        ["name"] => out.name = Some(value.to_string()),
        ["theme"] | ["mode"] | ["appearance"] => match value.to_ascii_lowercase().as_str() {
            "dark" => out.theme = Some(Scheme::Dark),
            "light" => out.theme = Some(Scheme::Light),
            _ => {}
        },
        ["version"] | ["description"] | ["omitted"] => {}
        ["colors" | "colours" | "color", "dark", name]
        | ["dark", "colors" | "colours", name]
        | ["colors" | "colours" | "color", name, "dark"] => {
            out.colors.push(token(name, Scheme::Dark));
        }
        ["colors" | "colours" | "color", "light", name]
        | ["light", "colors" | "colours", name]
        | ["colors" | "colours" | "color", name, "light"]
        | ["colors" | "colours" | "color", name] => {
            out.colors.push(token(name, Scheme::Light));
        }
        ["typography", level, "fontfamily"] | ["fonts" | "font", level, "fontfamily"] => {
            out.fonts.push(FontDecl {
                family: value.to_string(),
                role: font_role(level),
                substitutes: Vec::new(),
                span,
                origin: Origin::Yaml,
            });
        }
        ["fonts" | "font", level] => out.fonts.push(FontDecl {
            family: value.to_string(),
            role: font_role(level),
            substitutes: Vec::new(),
            span,
            origin: Origin::Yaml,
        }),
        ["typography", level, "fontweight"] => {
            if font_role(level) == FontRole::Heading {
                out.heading_weights.push(token(level, Scheme::Light));
            }
        }
        ["rounded" | "radius" | "radii", level] => out.radii.push(token(level, Scheme::Light)),
        ["shadow" | "shadows" | "elevation", level] => {
            out.shadows.push(token(level, Scheme::Light));
        }
        ["layout", name @ ("measure" | "content-width" | "reading-width")] => {
            out.measures.push(token(name, Scheme::Light));
        }
        ["layout", name @ ("max-width" | "page-width" | "container")] => {
            out.page_widths.push(token(name, Scheme::Light));
        }
        ["spacing", level] => {
            if !value.starts_with('{') && value.parse::<f32>().is_err() {
                if let Err(error) = Length::parse(value) {
                    let name = format!("spacing.{level}");
                    out.invalid(&name, value, span, &error.to_string());
                    return;
                }
            }
            out.unused.push((format!("spacing.{level}"), span));
        }
        _ => out.unused.push((path.join("."), span)),
    }
}

pub(crate) fn font_role(name: &str) -> FontRole {
    let name = name.to_ascii_lowercase();
    let has = |words: &[&str]| words.iter().any(|w| name.contains(w));
    if has(&["mono", "code"]) {
        FontRole::Mono
    } else if has(&["head", "display", "title", "serif-display"])
        || (name.len() == 2 && name.starts_with('h'))
    {
        FontRole::Heading
    } else if has(&[
        "body",
        "text",
        "label",
        "caption",
        "paragraph",
        "sans",
        "ui",
    ]) {
        FontRole::Body
    } else {
        FontRole::Any
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(text: &str) -> Extracted {
        let mut out = Extracted::default();
        extract(text, 1, &mut out);
        out
    }

    #[test]
    fn splits_front_matter_and_finds_the_body() {
        let source = "---\nname: X\n---\n\n# Body\n";
        let (yaml, lines, body) = split_front_matter(source).unwrap();
        assert_eq!(yaml, "name: X\n");
        assert_eq!(lines, 1);
        assert_eq!(&source[body..], "\n# Body\n");

        let crlf = "---\r\nname: X\r\n---\r\nBody";
        assert_eq!(split_front_matter(crlf).unwrap().0, "name: X\r\n");
        let bom = "\u{feff}---\nname: X\n---\n";
        assert_eq!(split_front_matter(bom).unwrap().0, "name: X\n");
        assert_eq!(split_front_matter("# No front matter\n---\n"), None);
        assert_eq!(split_front_matter("---\nname: unclosed\n"), None);
        assert_eq!(split_front_matter("---"), None);
    }

    #[test]
    fn reads_the_spec_format_with_source_lines() {
        let out = run(r##"name: Daylight
colors:
  primary: "#1A1C1E"
  neutral: "#F7F5F2"
typography:
  h1:
    fontFamily: Public Sans
    fontWeight: 600
  body-md:
    fontFamily: Inter
    fontWeight: 400
rounded:
  md: 4px
"##);
        assert_eq!(out.name.as_deref(), Some("Daylight"));
        assert_eq!(out.colors.len(), 2);
        assert_eq!(out.colors[0].name, "primary");
        assert_eq!(out.colors[0].value, "#1A1C1E");
        assert_eq!(out.colors[0].span.line, 4, "counts the opening --- line");
        assert_eq!(out.colors[0].span.column, 12);
        assert_eq!(out.fonts[0].family, "Public Sans");
        assert_eq!(out.fonts[0].role, FontRole::Heading);
        assert_eq!(out.fonts[1].role, FontRole::Body);
        assert_eq!(
            out.heading_weights.len(),
            1,
            "only heading levels set the weight"
        );
        assert_eq!(out.heading_weights[0].value, "600");
        assert_eq!(out.radii[0].value, "4px");
        assert!(out.diagnostics.is_empty());
    }

    #[test]
    fn reads_dark_tokens_in_any_of_the_common_shapes() {
        let out = run("colors:\n  dark:\n    bg: \"#000\"\n  text:\n    light: \"#111\"\n    dark: \"#eee\"\n");
        let schemes: Vec<_> = out
            .colors
            .iter()
            .map(|c| (c.name.as_str(), c.scheme))
            .collect();
        assert_eq!(
            schemes,
            [
                ("bg", Scheme::Dark),
                ("text", Scheme::Light),
                ("text", Scheme::Dark)
            ]
        );
    }

    #[test]
    fn flags_invalid_spacing_but_not_scales_it_ignores() {
        let out = run("spacing:\n  section: 64-128px\n  md: 16px\n  base: 8\ncomponents:\n  button:\n    padding: 4px\n");
        assert_eq!(out.diagnostics.len(), 1);
        assert_eq!(out.diagnostics[0].code, Code::InvalidValue);
        assert!(out.diagnostics[0].message.contains("64-128px"));
        assert_eq!(out.unused.len(), 3);
    }

    #[test]
    fn bad_yaml_is_a_diagnostic_not_a_failure() {
        let out = run("colors:\n  a: [unclosed\n  b: \"#000");
        assert_eq!(out.diagnostics[0].code, Code::MalformedYaml);
    }

    #[test]
    fn deep_nesting_and_aliases_are_harmless() {
        let deep = "a: ".to_string() + &"[".repeat(5000) + &"]".repeat(5000);
        assert_eq!(run(&deep).diagnostics[0].code, Code::MalformedYaml);
        let bomb = "a: &a [x, x]\nb: &b [*a, *a]\nc: &c [*b, *b]\ncolors:\n  text: *c\n";
        let out = run(bomb);
        assert!(out.colors.is_empty());
    }
}
