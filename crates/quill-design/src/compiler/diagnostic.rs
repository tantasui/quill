//! What the compiler tells the person importing a design. Modeled on rustc:
//! a stable code, a level, a message, a source location and a suggested fix.

use std::fmt;

use serde::{Deserialize, Serialize};
use unicode_width::UnicodeWidthChar;

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
        /// `docs/design-import.md`; never reuse or renumber one.
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

/// The source line a span points at, for showing it with a caret underneath.
/// The text is safe to print, and the caret is measured in terminal columns, so
/// it lines up under wide characters.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Excerpt {
    pub text: String,
    /// Columns before the caret, within `text`.
    pub caret_offset: usize,
    /// Columns the caret covers, at least 1.
    pub caret_len: usize,
}

impl Span {
    /// Cuts the line this span points at out of `source`. Long lines, and long
    /// spans, are narrowed to a window.
    pub fn excerpt(self, source: &str) -> Option<Excerpt> {
        const WINDOW: usize = 100;
        let line = source
            .split('\n')
            .nth(self.line.checked_sub(1)? as usize)?
            .trim_end_matches('\r');
        let chars: Vec<char> = line.chars().map(printable_char).collect();
        let start = self.column.saturating_sub(1) as usize;
        let shown = (self.len as usize).clamp(1, WINDOW);
        let from = start.saturating_sub(WINDOW / 2);
        let to = (start + shown + WINDOW / 2).min(chars.len());
        if from > chars.len() {
            return None;
        }
        let caret_start = start.min(to);
        let caret_end = (start + shown).min(to);
        Some(Excerpt {
            text: chars[from..to].iter().collect(),
            caret_offset: columns(&chars[from..caret_start]) + (start - caret_start),
            caret_len: columns(&chars[caret_start..caret_end]).max(1),
        })
    }
}

fn columns(chars: &[char]) -> usize {
    chars.iter().filter_map(|c| c.width()).sum()
}

/// A character that can't change how a terminal shows anything around it. A
/// design file is often someone else's: left alone, escape sequences in it could
/// rewrite the output or set the clipboard, and bidirectional controls could
/// reorder a line so it reads as something it isn't. Each character maps to
/// exactly one, so positions stay right.
fn printable_char(c: char) -> char {
    match c {
        '\t' => ' ',
        '\u{061c}'
        | '\u{200e}'
        | '\u{200f}'
        | '\u{2028}'
        | '\u{2029}'
        | '\u{202a}'..='\u{202e}'
        | '\u{2066}'..='\u{2069}' => char::REPLACEMENT_CHARACTER,
        c if c.is_control() => char::REPLACEMENT_CHARACTER,
        c => c,
    }
}

/// Text from a file, such as a value quoted in a message, made safe to print.
pub(crate) fn printable(text: &str) -> String {
    text.chars().map(printable_char).collect()
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
    fn cuts_excerpts_around_a_span() {
        let source = "first\r\n  --gap: 64-128px;\nlast";
        let span = Span {
            line: 2,
            column: 10,
            len: 8,
        };
        let excerpt = span.excerpt(source).unwrap();
        assert_eq!(excerpt.text, "  --gap: 64-128px;");
        assert_eq!((excerpt.caret_offset, excerpt.caret_len), (9, 8));
        assert_eq!(&excerpt.text[9..17], "64-128px");

        // A very long line is narrowed, and the caret still lines up.
        let long = format!("{}#badvalue{}", "x".repeat(5000), "y".repeat(5000));
        let span = Span {
            line: 1,
            column: 5001,
            len: 9,
        };
        let excerpt = span.excerpt(&long).unwrap();
        assert!(excerpt.text.len() < 200);
        let at: String = excerpt
            .text
            .chars()
            .skip(excerpt.caret_offset)
            .take(9)
            .collect();
        assert_eq!(at, "#badvalue");

        // A span that doesn't match this source is harmless.
        assert!(Span {
            line: 99,
            column: 1,
            len: 1
        }
        .excerpt(source)
        .is_none());
        assert!(Span {
            line: 0,
            column: 1,
            len: 1
        }
        .excerpt(source)
        .is_none());
        assert!(Span {
            line: 1,
            column: 999,
            len: 1
        }
        .excerpt(source)
        .is_none());
    }

    #[test]
    fn narrows_a_very_long_span() {
        let source = format!("--x: {};", "y".repeat(10_000));
        let excerpt = Span {
            line: 1,
            column: 6,
            len: 10_000,
        }
        .excerpt(&source)
        .unwrap();
        assert!(excerpt.text.len() < 200);
        assert_eq!(excerpt.caret_len, 100);
    }

    #[test]
    fn measures_the_caret_in_terminal_columns() {
        // The accented letter and emoji are one character each; the emoji takes
        // two columns and the combining mark takes none.
        let source = "é🎨e\u{301} #bad";
        let excerpt = Span {
            line: 1,
            column: 6,
            len: 4,
        }
        .excerpt(source)
        .unwrap();
        assert_eq!(excerpt.caret_offset, 5);
        assert_eq!(excerpt.caret_len, 4);

        let wide = Span {
            line: 1,
            column: 2,
            len: 1,
        }
        .excerpt(source)
        .unwrap();
        assert_eq!((wide.caret_offset, wide.caret_len), (1, 2));

        // A span just past the end of a line still gets a caret, where it points.
        let past = Span {
            line: 1,
            column: 5,
            len: 1,
        }
        .excerpt("abc")
        .unwrap();
        assert_eq!(
            (past.text.as_str(), past.caret_offset, past.caret_len),
            ("abc", 4, 1)
        );
    }

    #[test]
    fn excerpts_cannot_control_a_terminal() {
        let source = "a\u{1b}]52;c;aGk=\u{7}b\u{1b}[2Jc\u{202e}d\u{9b}e\u{7f}f\tg\rh #bad";
        let chars = source.chars().count() as u32;
        let excerpt = Span {
            line: 1,
            column: chars - 3,
            len: 4,
        }
        .excerpt(source)
        .unwrap();
        assert!(
            !excerpt.text.chars().any(char::is_control),
            "{:?}",
            excerpt.text
        );
        assert!(!excerpt.text.contains('\u{202e}'));
        // Every character was replaced one for one, so the caret still lines up.
        let at: String = excerpt.text.chars().skip(excerpt.caret_offset).collect();
        assert_eq!(at, "#bad");
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
