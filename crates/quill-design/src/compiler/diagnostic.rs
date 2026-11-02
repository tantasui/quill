//! What the compiler tells the person importing a design. Modeled on rustc:
//! a stable code, a level, a message, a source location and a suggested fix.

use std::fmt;

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    /// Blocks applying the design.
    Error,
    Warning,
    Note,
}

impl fmt::Display for Level {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Level::Error => "error",
            Level::Warning => "warning",
            Level::Note => "note",
        })
    }
}

macro_rules! codes {
    ($($variant:ident = $code:literal $level:ident,)*) => {
        /// A stable identifier for each kind of diagnostic. Published in
        /// `docs/design-diagnostics.md`; never reuse or renumber one.
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
        pub enum Code {
            $(#[serde(rename = $code)] $variant,)*
        }

        impl Code {
            pub const ALL: &'static [Code] = &[$(Code::$variant,)*];

            pub fn as_str(self) -> &'static str {
                match self { $(Code::$variant => $code,)* }
            }

            pub fn level(self) -> Level {
                match self { $(Code::$variant => Level::$level,)* }
            }
        }
    };
}

codes! {
    SourceTooLarge = "QD001" Error,
    NotUtf8 = "QD002" Error,
    NoTokens = "QD003" Error,
    MalformedYaml = "QD004" Warning,
    InvalidValue = "QD005" Warning,
    Conflict = "QD006" Note,
    NotUsed = "QD007" Note,
    RoleInferred = "QD008" Note,
    RoleDerived = "QD009" Note,
    TextContrast = "QD010" Error,
    SurfaceContrast = "QD011" Warning,
    MutedContrast = "QD012" Warning,
    AccentNotText = "QD013" Warning,
    OnAccentChosen = "QD014" Note,
    AlphaFlattened = "QD015" Note,
    DarkPaletteRejected = "QD016" Warning,
    FontSwapped = "QD020" Note,
    FontSystem = "QD021" Note,
    DarkDerived = "QD030" Note,
    DarkOnly = "QD031" Note,
    RuleApplied = "QD040" Note,
    RuleNotApplied = "QD041" Note,
    PageWidthIgnored = "QD042" Warning,
}

impl fmt::Display for Code {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A place in the source, for showing the line with a caret under it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Span {
    /// 1-based.
    pub line: u32,
    /// 1-based, counted in characters.
    pub column: u32,
    /// Length in characters on that line, at least 1.
    pub len: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Diagnostic {
    pub code: Code,
    pub level: Level,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub span: Option<Span>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub help: Option<String>,
}

impl Diagnostic {
    pub(crate) fn new(code: Code, message: impl Into<String>) -> Self {
        Diagnostic {
            code,
            level: code.level(),
            message: message.into(),
            span: None,
            help: None,
        }
    }

    pub(crate) fn at(mut self, span: Span) -> Self {
        self.span = Some(span);
        self
    }

    pub(crate) fn at_opt(mut self, span: Option<Span>) -> Self {
        self.span = span;
        self
    }

    pub(crate) fn help(mut self, help: impl Into<String>) -> Self {
        self.help = Some(help.into());
        self
    }
}

/// Converts byte offsets into lines and columns. Everything is looked up rather
/// than scanned, so thousands of tokens on one enormous line stay fast.
pub(crate) struct LineIndex<'a> {
    source: &'a str,
    line_starts: Vec<usize>,
    /// Offsets of UTF-8 continuation bytes, which don't count toward a column.
    continuation_bytes: Vec<usize>,
}

impl<'a> LineIndex<'a> {
    pub(crate) fn new(source: &'a str) -> Self {
        let mut line_starts = vec![0];
        line_starts.extend(source.match_indices('\n').map(|(i, _)| i + 1));
        let continuation_bytes = source
            .bytes()
            .enumerate()
            .filter(|(_, byte)| byte & 0xC0 == 0x80)
            .map(|(i, _)| i)
            .collect();
        LineIndex {
            source,
            line_starts,
            continuation_bytes,
        }
    }

    /// How many characters lie between two byte offsets.
    fn chars_between(&self, start: usize, end: usize) -> usize {
        let before = |offset| self.continuation_bytes.partition_point(|&i| i < offset);
        (end - start) - (before(end) - before(start))
    }

    /// The span of `len` bytes starting at byte `offset`, clipped to one line.
    pub(crate) fn span(&self, offset: usize, len: usize) -> Span {
        let offset = floor_char_boundary(self.source, offset.min(self.source.len()));
        let line = self.line_starts.partition_point(|&start| start <= offset) - 1;
        let end = floor_char_boundary(self.source, (offset + len).min(self.source.len()));
        let visible = self.source[offset..end]
            .find(['\n', '\r'])
            .map_or(end, |i| offset + i);
        Span {
            line: line as u32 + 1,
            column: self.chars_between(self.line_starts[line], offset) as u32 + 1,
            len: self.chars_between(offset, visible).max(1) as u32,
        }
    }
}

fn floor_char_boundary(text: &str, mut index: usize) -> usize {
    while !text.is_char_boundary(index) {
        index -= 1;
    }
    index
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_are_unique_and_well_formed() {
        let mut seen = std::collections::HashSet::new();
        for code in Code::ALL {
            let text = code.as_str();
            assert!(text.starts_with("QD") && text.len() == 5, "{text}");
            assert!(seen.insert(text), "{text} is used twice");
        }
    }

    #[test]
    fn serializes_with_the_stable_code() {
        let d = Diagnostic::new(Code::InvalidValue, "bad").help("fix it");
        let json = serde_json::to_string(&d).unwrap();
        assert_eq!(
            json,
            r#"{"code":"QD005","level":"warning","message":"bad","help":"fix it"}"#
        );
        assert_eq!(serde_json::from_str::<Diagnostic>(&json).unwrap(), d);
    }

    #[test]
    fn finds_lines_and_columns() {
        let index = LineIndex::new("one\ntwo words\r\nthré e\n");
        assert_eq!(
            index.span(0, 3),
            Span {
                line: 1,
                column: 1,
                len: 3
            }
        );
        assert_eq!(
            index.span(8, 5),
            Span {
                line: 2,
                column: 5,
                len: 5
            }
        );
        // Columns count characters, not bytes, and a span stops at the line end.
        let e = "one\ntwo words\r\nthré ".len();
        assert_eq!(
            index.span(e, 99),
            Span {
                line: 3,
                column: 6,
                len: 1
            }
        );
        // An offset inside a multi-byte character doesn't panic.
        let inside = "one\ntwo words\r\nthr".len() + 1;
        assert_eq!(index.span(inside, 1).line, 3);
        assert_eq!(index.span(10_000, 1).line, 4);
    }
}
