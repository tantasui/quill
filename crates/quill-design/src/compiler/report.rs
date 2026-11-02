//! Diagnostics as text for a terminal, laid out the way rustc does it: the level
//! and code, where it is, the source line with a caret, then the suggested fix.
//!
//! This returns strings rather than printing, so the crate stays free of I/O.

use std::fmt::Write;

use super::diagnostic::{printable, Diagnostic, Level};

/// How many diagnostics there are at each level.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Tally {
    pub errors: usize,
    pub warnings: usize,
    pub notes: usize,
}

impl Tally {
    pub fn of(diagnostics: &[Diagnostic]) -> Self {
        let mut tally = Tally::default();
        for d in diagnostics {
            match d.level {
                Level::Error => tally.errors += 1,
                Level::Warning => tally.warnings += 1,
                Level::Note => tally.notes += 1,
            }
        }
        tally
    }
}

/// Wraps text in terminal colors, or leaves it alone.
#[derive(Clone, Copy)]
struct Paint(bool);

impl Paint {
    fn apply(self, style: &str, text: &str) -> String {
        if self.0 {
            format!("\u{1b}[{style}m{text}\u{1b}[0m")
        } else {
            text.to_string()
        }
    }

    fn level(self, level: Level, text: &str) -> String {
        let style = match level {
            Level::Error => "1;31",
            Level::Warning => "1;33",
            Level::Note => "1;36",
        };
        self.apply(style, text)
    }

    fn gutter(self, text: &str) -> String {
        self.apply("1;34", text)
    }
}

/// Every diagnostic, in order, each followed by a blank line. `source` is the
/// text the spans point into and `file` is the name to show for it.
pub fn render(diagnostics: &[Diagnostic], source: &str, file: &str, color: bool) -> String {
    // One width for the whole report, so the gutters line up with each other.
    let width = diagnostics
        .iter()
        .filter_map(|d| d.span)
        .map(|span| span.line.to_string().len())
        .max()
        .unwrap_or(1);
    let file = printable(file);
    let mut out = String::new();
    for d in diagnostics {
        render_one(&mut out, d, source, &file, width, Paint(color));
        out.push('\n');
    }
    out
}

fn render_one(
    out: &mut String,
    d: &Diagnostic,
    source: &str,
    file: &str,
    width: usize,
    paint: Paint,
) {
    let pad = " ".repeat(width);
    let head = paint.level(d.level, &format!("{}[{}]", d.level, d.code));
    let message = paint.apply("1", &format!(": {}", printable(&d.message)));
    let _ = writeln!(out, "{head}{message}");

    let arrow = paint.gutter("-->");
    match d.span {
        Some(span) => {
            let _ = writeln!(out, "{pad}{arrow} {file}:{}:{}", span.line, span.column);
        }
        None => {
            let _ = writeln!(out, "{pad}{arrow} {file}");
        }
    }

    let bar = paint.gutter("|");
    if let Some((span, excerpt)) = d.span.and_then(|s| Some((s, s.excerpt(source)?))) {
        let number = paint.gutter(&format!("{:>width$}", span.line));
        let carets = paint.level(d.level, &"^".repeat(excerpt.caret_len));
        let _ = writeln!(out, "{pad} {bar}");
        let line = format!("{number} {bar} {}", excerpt.text);
        let _ = writeln!(out, "{}", line.trim_end());
        let _ = writeln!(
            out,
            "{pad} {bar} {}{carets}",
            " ".repeat(excerpt.caret_offset)
        );
        if d.help.is_some() {
            let _ = writeln!(out, "{pad} {bar}");
        }
    }

    if let Some(help) = &d.help {
        let equals = paint.gutter("=");
        let label = paint.apply("1", "help");
        let _ = writeln!(out, "{pad} {equals} {label}: {}", printable(help));
    }
}

fn counted(n: usize, word: &str) -> String {
    format!("{n} {word}{}", if n == 1 { "" } else { "s" })
}

/// The closing line: whether the design can be used, and the counts.
pub fn summary(diagnostics: &[Diagnostic], color: bool) -> String {
    let paint = Paint(color);
    let tally = Tally::of(diagnostics);
    let counts = format!(
        "{}, {}, {}",
        counted(tally.errors, "error"),
        counted(tally.warnings, "warning"),
        counted(tally.notes, "note")
    );
    if tally.errors > 0 {
        let label = paint.level(Level::Error, "error");
        format!("{label}: this design can't be applied ({counts})\n")
    } else {
        let label = paint.apply("1;32", "ok");
        format!("{label}: this design can be applied ({counts})\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compiler::diagnostic::{Code, Span};

    const SOURCE: &str = "first\n  --gap: 64-128px;\n";

    fn bad_value() -> Diagnostic {
        Diagnostic::new(Code::InvalidValue, "`64-128px` isn't a single length")
            .at(Span {
                line: 2,
                column: 10,
                len: 8,
            })
            .help("use one value")
    }

    #[test]
    fn lays_a_diagnostic_out_like_rustc() {
        assert_eq!(
            render(&[bad_value()], SOURCE, "DESIGN.md", false),
            "\
warning[QD005]: `64-128px` isn't a single length
 --> DESIGN.md:2:10
  |
2 |   --gap: 64-128px;
  |          ^^^^^^^^
  |
  = help: use one value

"
        );
    }

    #[test]
    fn gutters_line_up_across_the_whole_report() {
        let far = Diagnostic::new(Code::NotUsed, "unused").at(Span {
            line: 120,
            column: 1,
            len: 1,
        });
        let text = format!("{}x\n", "\n".repeat(119));
        let out = render(&[far, bad_value()], &text, "f.md", false);
        assert!(out.contains("120 | x\n    | ^\n"), "{out}");
        // Line 2 of this source is empty, which must not leave trailing spaces.
        assert!(out.contains("  --> f.md:2:10\n    |\n  2 |\n"), "{out}");
    }

    #[test]
    fn a_diagnostic_without_a_place_still_names_the_file() {
        let d = Diagnostic::new(Code::NotUtf8, "this file isn't UTF-8 text").help("save it again");
        assert_eq!(
            render(&[d], "", "x.md", false),
            "error[QD002]: this file isn't UTF-8 text\n --> x.md\n  = help: save it again\n\n"
        );
    }

    #[test]
    fn nothing_from_the_file_can_control_the_terminal() {
        let evil = "\u{1b}]52;c;aGk=\u{7}\u{1b}[2J\u{202e}\n";
        let d = Diagnostic::new(Code::InvalidValue, format!("bad value `{evil}`"))
            .at(Span {
                line: 1,
                column: 1,
                len: 2,
            })
            .help(format!("not `{evil}`"));
        let source = format!("{evil}rest");
        for color in [false, true] {
            let out = render(std::slice::from_ref(&d), &source, evil, color);
            // Colors are the only escapes, and they're always Quill's own.
            let without_paint = ["1;31", "1;33", "1;34", "1;36", "1", "0"]
                .iter()
                .fold(out, |text, style| {
                    text.replace(&format!("\u{1b}[{style}m"), "")
                });
            assert!(
                !without_paint
                    .chars()
                    .any(|c| (c.is_control() && c != '\n') || c == '\u{202e}'),
                "{without_paint:?}"
            );
            // A newline in a value can't fake a line of output either: this is
            // the head, arrow, three source lines, a rule, help and a blank line.
            assert_eq!(without_paint.lines().count(), 8, "{without_paint:?}");
        }
    }

    #[test]
    fn color_is_only_added_when_asked_for() {
        assert!(!render(&[bad_value()], SOURCE, "f", false).contains('\u{1b}'));
        let colored = render(&[bad_value()], SOURCE, "f", true);
        assert!(colored.contains("\u{1b}[1;33mwarning[QD005]\u{1b}[0m"));
        assert!(colored.contains("\u{1b}[1;33m^^^^^^^^\u{1b}[0m"));
    }

    #[test]
    fn summarizes_with_the_verdict_and_counts() {
        let note = Diagnostic::new(Code::NotUsed, "unused");
        let error = Diagnostic::new(Code::NoTokens, "nothing here");
        assert_eq!(
            summary(&[], false),
            "ok: this design can be applied (0 errors, 0 warnings, 0 notes)\n"
        );
        assert_eq!(
            summary(&[bad_value(), note.clone(), note.clone()], false),
            "ok: this design can be applied (0 errors, 1 warning, 2 notes)\n"
        );
        assert_eq!(
            summary(&[error, note], false),
            "error: this design can't be applied (1 error, 0 warnings, 1 note)\n"
        );
    }
}
