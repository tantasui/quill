use std::fmt;

use serde::{Deserialize, Serialize};

use crate::TokenError;

const MAX_FAMILIES: usize = 16;
const MAX_NAME_LEN: usize = 64;
const GENERIC: &[&str] = &[
    "serif",
    "sans-serif",
    "monospace",
    "cursive",
    "fantasy",
    "system-ui",
    "ui-serif",
    "ui-sans-serif",
    "ui-monospace",
    "ui-rounded",
];

/// An ordered list of font family names. Names are restricted to characters
/// that can't end a CSS string or declaration.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct FontStack(Vec<String>);

impl FontStack {
    /// Parses a CSS-style list: `"Iowan Old Style", Georgia, serif`.
    pub fn parse(input: &str) -> Result<Self, TokenError> {
        let error = |reason: &str| TokenError::new("font stack", input, reason);
        let mut families = Vec::new();
        for raw in input.split(',') {
            let name = raw.trim();
            let name = name
                .strip_prefix('"')
                .and_then(|n| n.strip_suffix('"'))
                .or_else(|| name.strip_prefix('\'').and_then(|n| n.strip_suffix('\'')))
                .unwrap_or(name)
                .trim();
            if name.is_empty() {
                return Err(error("every entry needs a font name"));
            }
            if name.chars().count() > MAX_NAME_LEN {
                return Err(error("a font name is too long"));
            }
            let allowed = |c: char| c.is_alphanumeric() || matches!(c, ' ' | '-' | '_' | '.');
            if !name.chars().all(allowed) {
                return Err(error(
                    "font names may contain only letters, numbers, spaces, hyphens, underscores and dots",
                ));
            }
            families.push(name.to_string());
        }
        if families.len() > MAX_FAMILIES {
            return Err(error("too many fonts in one stack"));
        }
        Ok(FontStack(families))
    }

    pub fn to_css(&self) -> String {
        let entries: Vec<String> = self.0.iter().map(|name| css_name(name)).collect();
        entries.join(", ")
    }
}

/// Quotes a family name unless it's a generic keyword or a plain CSS identifier.
fn css_name(name: &str) -> String {
    let is_generic = GENERIC.contains(&name);
    let is_identifier = !name.contains(' ') && !name.starts_with(|c: char| c.is_ascii_digit());
    if is_generic || is_identifier {
        name.to_string()
    } else {
        format!("\"{name}\"")
    }
}

impl TryFrom<String> for FontStack {
    type Error = TokenError;
    fn try_from(s: String) -> Result<Self, TokenError> {
        FontStack::parse(&s)
    }
}

impl From<FontStack> for String {
    fn from(f: FontStack) -> String {
        f.to_css()
    }
}

impl fmt::Display for FontStack {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_css())
    }
}

/// A CSS font weight from 1 to 1000.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "u16", into = "u16")]
pub struct FontWeight(u16);

impl FontWeight {
    pub fn new(value: u16) -> Result<Self, TokenError> {
        if (1..=1000).contains(&value) {
            Ok(FontWeight(value))
        } else {
            Err(TokenError::new(
                "font weight",
                &value.to_string(),
                "it must be from 1 to 1000",
            ))
        }
    }

    /// Parses a number, `normal` or `bold`.
    pub fn parse(input: &str) -> Result<Self, TokenError> {
        match input.trim().to_ascii_lowercase().as_str() {
            "normal" => Ok(FontWeight(400)),
            "bold" => Ok(FontWeight(700)),
            text => {
                let value = text.parse().map_err(|_| {
                    TokenError::new("font weight", input, "use a number, normal or bold")
                })?;
                FontWeight::new(value)
            }
        }
    }

    pub fn to_css(self) -> String {
        self.0.to_string()
    }
}

impl TryFrom<u16> for FontWeight {
    type Error = TokenError;
    fn try_from(value: u16) -> Result<Self, TokenError> {
        FontWeight::new(value)
    }
}

impl From<FontWeight> for u16 {
    fn from(w: FontWeight) -> u16 {
        w.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_stacks_with_correct_quoting() {
        let stack = FontStack::parse(
            r#""Iowan Old Style", 'Book Antiqua', Georgia, -apple-system, system-ui, 3Dumb, serif"#,
        )
        .unwrap();
        assert_eq!(
            stack.to_css(),
            r#""Iowan Old Style", "Book Antiqua", Georgia, -apple-system, system-ui, "3Dumb", serif"#
        );
    }

    #[test]
    fn rejects_anything_that_could_escape_the_declaration() {
        for input in [
            "",
            "Georgia,",
            ",Georgia",
            "Georgia; color: red",
            "Georgia}",
            "\"Geo\"rgia\"",
            "url(x)",
            "Georgia\\",
            "a/b",
            "<script>",
        ] {
            assert!(
                FontStack::parse(input).is_err(),
                "{input} should be rejected"
            );
        }
        let long_name = "x".repeat(MAX_NAME_LEN + 1);
        assert!(FontStack::parse(&long_name).is_err());
        let many = vec!["Georgia"; MAX_FAMILIES + 1].join(",");
        assert!(FontStack::parse(&many).is_err());
    }

    #[test]
    fn parses_weights() {
        assert_eq!(FontWeight::parse("normal").unwrap().to_css(), "400");
        assert_eq!(FontWeight::parse("BOLD").unwrap().to_css(), "700");
        assert_eq!(FontWeight::parse("650").unwrap().to_css(), "650");
        for input in ["0", "1001", "heavy", "-1", "400; x"] {
            assert!(
                FontWeight::parse(input).is_err(),
                "{input} should be rejected"
            );
        }
    }
}
