//! What the extractors find in a design file, before any of it is trusted.
//! Values stay as raw text here; normalizing happens in one later step.

use std::collections::HashMap;

use super::diagnostic::{Code, Diagnostic, Span};

/// Where a value came from. When sources disagree, the earliest variant wins.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Origin {
    Yaml,
    Css,
    Table,
    List,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Scheme {
    Light,
    Dark,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Token {
    /// Everything known about what this value is for: its name, and any role
    /// or description written beside it.
    pub name: String,
    pub value: String,
    pub span: Span,
    pub origin: Origin,
    pub scheme: Scheme,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FontRole {
    Body,
    Heading,
    Mono,
    /// Not stated. Used for whichever roles nothing else claims.
    Any,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct FontDecl {
    pub family: String,
    pub role: FontRole,
    pub substitutes: Vec<String>,
    pub span: Span,
    pub origin: Origin,
}

/// A sentence of prose that might state a design rule.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ProseLine {
    pub text: String,
    pub span: Span,
    /// The headings above this line, lowercased.
    pub context: String,
}

#[derive(Debug, Default)]
pub(crate) struct Extracted {
    pub name: Option<String>,
    pub theme: Option<Scheme>,
    pub colors: Vec<Token>,
    pub fonts: Vec<FontDecl>,
    pub heading_weights: Vec<Token>,
    pub radii: Vec<Token>,
    pub shadows: Vec<Token>,
    pub measures: Vec<Token>,
    pub page_widths: Vec<Token>,
    pub prose: Vec<ProseLine>,
    /// Descriptions written beside a color name in prose, such as
    /// "**Primary (#1A1C1E):** A deep ink used for core text".
    pub descriptions: HashMap<String, String>,
    pub diagnostics: Vec<Diagnostic>,
    pub unused: Vec<(String, Span)>,
}

impl Extracted {
    /// Reports one value that can't be used because it's malformed.
    pub fn invalid(&mut self, name: &str, value: &str, span: Span, reason: &str) {
        self.diagnostics.push(
            Diagnostic::new(
                Code::InvalidValue,
                format!("`{name}` has an invalid value `{value}`, so it was ignored"),
            )
            .at(span)
            .help(reason),
        );
    }
}
