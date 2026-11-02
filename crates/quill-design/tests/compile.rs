//! Compiles whole files. Fixtures are written by hand to match the formats seen
//! in the wild. Real third-party files stay out of the repository; put them in
//! `local-fixtures/` and run the ignored test below.

use std::fmt::Write;

use quill_design::{compile, compile_bytes, Code, Compilation, Level, Palette, MAX_SOURCE_BYTES};

fn render(result: &Compilation) -> String {
    let mut out = String::new();
    for d in &result.diagnostics {
        let at = d
            .span
            .map_or(String::from("-"), |s| format!("{}:{}", s.line, s.column));
        let _ = writeln!(out, "{}[{}] {at}: {}", d.level, d.code, d.message);
        if let Some(help) = &d.help {
            let _ = writeln!(out, "  help: {help}");
        }
    }
    match &result.design {
        None => out.push_str("\nno design\n"),
        Some(design) => {
            let _ = writeln!(out, "\nname: {:?}", design.name);
            let _ = writeln!(out, "fonts: {:?}", design.fonts);
            let origins: Vec<String> = design
                .origins
                .iter()
                .map(|o| format!("{}:{}", o.token, o.line))
                .collect();
            let _ = writeln!(out, "origins: {}\n", origins.join(" "));
            out.push_str(&design.to_css("/blog"));
        }
    }
    out
}

macro_rules! fixture_test {
    ($name:ident, $file:literal) => {
        #[test]
        fn $name() {
            let source = include_str!(concat!("fixtures/", $file));
            insta::assert_snapshot!(render(&compile(source)));
        }
    };
}

fixture_test!(spec_front_matter, "front_matter.md");
fixture_test!(refero_style, "refero_style.md");
fixture_test!(messy_values_and_low_contrast, "messy.md");
fixture_test!(no_tokens_at_all, "empty.md");
fixture_test!(dark_only_design, "dark_only.md");
fixture_test!(explicit_light_and_dark, "explicit_dark.md");

const ALL_FIXTURES: &[&str] = &[
    include_str!("fixtures/front_matter.md"),
    include_str!("fixtures/refero_style.md"),
    include_str!("fixtures/messy.md"),
    include_str!("fixtures/empty.md"),
    include_str!("fixtures/dark_only.md"),
    include_str!("fixtures/explicit_dark.md"),
];

fn has_code(result: &Compilation, code: Code) -> bool {
    result.diagnostics.iter().any(|d| d.code == code)
}

/// What every design that passes has to satisfy.
fn assert_palette_is_readable(palette: &Palette, context: &str) {
    for (name, fg, bg) in [
        ("text on bg", palette.text, palette.bg),
        ("text on surface", palette.text, palette.surface),
        ("muted on bg", palette.muted, palette.bg),
        ("accent_text on bg", palette.accent_text, palette.bg),
        ("on_accent on fill", palette.on_accent, palette.accent_fill),
    ] {
        let ratio = fg.contrast_ratio(bg);
        assert!(ratio >= 4.5, "{context}: {name} is {ratio:.2}:1");
    }
}

#[test]
fn errors_and_designs_never_coexist() {
    for source in ALL_FIXTURES {
        let result = compile(source);
        let has_error = result.diagnostics.iter().any(|d| d.level == Level::Error);
        assert_eq!(result.design.is_none(), has_error, "{}", &source[..20]);
    }
}

#[test]
fn nothing_from_the_input_reaches_the_stylesheet_as_text() {
    let source = r##"---
name: "x</style><script>alert(1)</script>"
colors:
  text: "#111111"
  background: "#ffffff"
typography:
  body:
    fontFamily: "Inter; } body { display: none } a {"
---

```css
:root {
  --shadow: 0 0 0 #000; background: url(https://evil.example/x);
  --font-heading: x"; color: red; "
}
```
"##;
    let result = compile(source);
    let css = result.design.expect("the colors are fine").to_css("");
    for forbidden in [
        "script",
        "evil",
        "display: none",
        "url(https",
        "color: red",
        "alert",
    ] {
        assert!(!css.contains(forbidden), "{forbidden} in:\n{css}");
    }
}

#[test]
fn enforces_the_size_limit() {
    let limit = "x".repeat(MAX_SOURCE_BYTES);
    assert!(!has_code(&compile(&limit), Code::SourceTooLarge));
    let over = "x".repeat(MAX_SOURCE_BYTES + 1);
    let result = compile(&over);
    assert!(has_code(&result, Code::SourceTooLarge));
    assert!(result.has_errors());
    assert!(has_code(
        &compile_bytes(over.as_bytes()),
        Code::SourceTooLarge
    ));
}

#[test]
fn rejects_files_that_are_not_text() {
    let result = compile_bytes(&[0x89, b'P', b'N', b'G', 0xff, 0xfe]);
    assert!(has_code(&result, Code::NotUtf8));
    assert!(result.has_errors());
}

#[test]
fn diagnostics_put_errors_first() {
    for source in ALL_FIXTURES {
        let levels: Vec<Level> = compile(source)
            .diagnostics
            .iter()
            .map(|d| d.level)
            .collect();
        let mut sorted = levels.clone();
        sorted.sort();
        assert_eq!(levels, sorted);
    }
}

#[test]
fn compiled_designs_survive_being_stored() {
    let design = compile(ALL_FIXTURES[1]).design.unwrap();
    let json = serde_json::to_string(&design).unwrap();
    let back: quill_design::CompiledDesign = serde_json::from_str(&json).unwrap();
    assert_eq!(back, design);
    assert_eq!(back.to_css("/x"), design.to_css("/x"));
}

/// A small deterministic generator, so a failing input can be reproduced.
struct Sequence(u64);

impl Sequence {
    fn next(&mut self, below: usize) -> usize {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((self.0 >> 33) as usize) % below.max(1)
    }
}

/// Mangled input must never panic, and whatever passes must still be readable.
#[test]
fn survives_mangled_files() {
    let mut sequence = Sequence(0xC0FFEE);
    let junk = [
        "---\n", "```css\n", "```\n", "|", "\n", ":", "{", "}", "#", "é", "\u{0}", "(", "- ", "**",
        "var(--x)", "\r\n", "<!--",
    ];
    for round in 0..3000 {
        let source = ALL_FIXTURES[sequence.next(ALL_FIXTURES.len())];
        let mut text: Vec<char> = source.chars().collect();
        for _ in 0..1 + sequence.next(8) {
            let at = sequence.next(text.len() + 1);
            match sequence.next(3) {
                0 => {
                    let end = (at + sequence.next(40)).min(text.len());
                    text.drain(at..end);
                }
                1 => {
                    let piece = junk[sequence.next(junk.len())];
                    let chars: Vec<char> = piece.chars().collect();
                    text.splice(at..at, chars);
                }
                _ => text.truncate(at.max(text.len() / 2)),
            }
        }
        let mangled: String = text.into_iter().collect();
        let result = compile(&mangled);
        if let Some(design) = &result.design {
            let context = format!("round {round}");
            assert_palette_is_readable(&design.tokens.light, &context);
            assert_palette_is_readable(&design.tokens.dark, &context);
            let css = design.to_css("");
            assert!(!css.contains('<'), "round {round}: {css}");
        }
    }
}

/// Pathological shapes that could make a parser slow or overflow its stack.
#[test]
fn stays_fast_on_hostile_structure() {
    let inputs = [
        "- ".repeat(50_000),
        "> ".repeat(50_000),
        "[".repeat(100_000),
        "|a".repeat(50_000),
        "| a |\n|---|\n".repeat(10_000),
        format!("---\n{}\n---\n", "a: {b: ".repeat(20_000)),
        format!("```css\n{}\n```", "{".repeat(100_000)),
        format!("```css\n:root {{{}}}\n```", "--a: var(--a);".repeat(10_000)),
        "*a **b ".repeat(30_000),
        format!("```css\n{}\n```", "/**/".repeat(60_000)),
        format!("```css\n:root {{{}}}\n```", "--a: #111111;".repeat(15_000)),
        format!("---\ncolors:\n{}---\n", "  a: \"#111111\"\n".repeat(10_000)),
        "- text: #111111\n".repeat(15_000),
        "| Text | #111111 |\n".repeat(10_000),
        "- Never use this.\n".repeat(13_000),
        "a(".repeat(100_000),
    ];
    for input in inputs {
        let input = &input[..input.len().min(MAX_SOURCE_BYTES)];
        let started = std::time::Instant::now();
        let _ = compile(input);
        let elapsed = started.elapsed();
        assert!(
            elapsed.as_secs() < 20,
            "took {elapsed:?} on {:?}",
            &input[..20]
        );
    }
}

/// Run against real files you've put in `local-fixtures/`:
/// `cargo test -p quill-design --test compile -- --ignored --nocapture`
#[test]
#[ignore = "needs files in local-fixtures/"]
fn local_fixtures() {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../local-fixtures");
    for entry in std::fs::read_dir(dir).expect("create local-fixtures/ first") {
        let path = entry.unwrap().path();
        let bytes = std::fs::read(&path).unwrap();
        println!(
            "\n=== {}\n{}",
            path.display(),
            render(&compile_bytes(&bytes))
        );
    }
}
