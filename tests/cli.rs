//! Runs the real `quill` binary, because the exit codes and streams are the
//! commands' contract with scripts.

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use serde_json::Value;

const GOOD: &str = "front_matter.md";
const BAD: &str = "messy.md";

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("crates/quill-design/tests/fixtures")
        .join(name)
}

fn quill(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_quill"))
        .args(args)
        .env_remove("NO_COLOR")
        .output()
        .unwrap()
}

fn check(file: &Path, extra: &[&str]) -> Output {
    let mut args = vec!["design", "check", file.to_str().unwrap()];
    args.extend_from_slice(extra);
    quill(&args)
}

fn stdout(output: &Output) -> String {
    String::from_utf8(output.stdout.clone()).unwrap()
}

fn stderr(output: &Output) -> String {
    String::from_utf8(output.stderr.clone()).unwrap()
}

#[test]
fn check_passes_a_good_design_with_exit_code_0() {
    let output = check(&fixture(GOOD), &[]);
    assert_eq!(output.status.code(), Some(0), "{}", stdout(&output));
    assert!(stdout(&output).contains("ok: this design can be applied"));
}

#[test]
fn check_fails_a_bad_design_with_exit_code_1() {
    let file = fixture(BAD);
    let output = check(&file, &[]);
    assert_eq!(output.status.code(), Some(1));
    let text = stdout(&output);
    assert!(text.contains("error[QD010]: text #cccccc"), "{text}");
    assert!(
        text.contains(&format!("--> {}:4:9\n", file.display())),
        "{text}"
    );
    assert!(
        text.contains(" 4 |   text: \"#cccccc\"\n   |         ^^^^^^^\n"),
        "{text}"
    );
    assert!(text.contains("= help: Make the text darker"), "{text}");
    assert!(
        text.contains("error: this design can't be applied (1 error"),
        "{text}"
    );
    // Not a terminal, so no colors end up in a pipe or a file.
    assert!(!text.contains('\u{1b}'));
}

#[test]
fn warnings_alone_do_not_fail_a_check() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("DESIGN.md");
    std::fs::write(
        &file,
        "---\ncolors:\n  text: \"#111111\"\n  background: \"#ffffff\"\n  accent: nope\n---\n",
    )
    .unwrap();
    let output = check(&file, &[]);
    assert_eq!(output.status.code(), Some(0));
    assert!(stdout(&output).contains("warning[QD005]"));
}

#[test]
fn check_json_gives_a_parseable_report_and_the_same_exit_code() {
    let file = fixture(BAD);
    let output = check(&file, &["--json"]);
    assert_eq!(output.status.code(), Some(1));
    let report: Value = serde_json::from_str(&stdout(&output)).unwrap();
    assert_eq!(report["file"], file.to_str().unwrap());
    assert_eq!(report["valid"], false);
    assert_eq!(report["errors"], 1);

    let first = &report["diagnostics"][0];
    assert_eq!(first["code"], "QD010");
    assert_eq!(first["level"], "error");
    assert_eq!(first["span"]["line"], 4);
    assert_eq!(first["span"]["column"], 9);
    assert!(first["help"].as_str().unwrap().contains("darker"));

    let total = report["errors"].as_u64().unwrap()
        + report["warnings"].as_u64().unwrap()
        + report["notes"].as_u64().unwrap();
    assert_eq!(
        report["diagnostics"].as_array().unwrap().len() as u64,
        total
    );

    let good: Value = serde_json::from_str(&stdout(&check(&fixture(GOOD), &["--json"]))).unwrap();
    assert_eq!(good["valid"], true);
}

#[test]
fn trouble_reading_the_file_is_exit_code_2_and_not_a_verdict() {
    let dir = tempfile::tempdir().unwrap();
    for path in [dir.path().join("missing.md"), dir.path().to_path_buf()] {
        let output = check(&path, &[]);
        assert_eq!(output.status.code(), Some(2), "{}", path.display());
        assert!(
            stderr(&output).contains("could not read"),
            "{}",
            stderr(&output)
        );
        assert!(output.stdout.is_empty());
    }
}

#[test]
fn a_huge_file_is_refused_without_being_read() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("huge.md");
    // Sparse, so this is instant, and reading it all would take gigabytes.
    std::fs::File::create(&file)
        .unwrap()
        .set_len(64 * 1024 * 1024 * 1024)
        .unwrap();
    let output = check(&file, &[]);
    assert_eq!(output.status.code(), Some(1));
    assert!(stdout(&output).contains("error[QD001]: this file is 67108864 KB"));
}

#[test]
fn a_file_that_is_not_text_gets_a_diagnostic() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("image.md");
    std::fs::write(&file, [0x89, b'P', b'N', b'G', 0xff, 0xfe]).unwrap();
    let output = check(&file, &[]);
    assert_eq!(output.status.code(), Some(1));
    let text = stdout(&output);
    assert!(text.contains("error[QD002]"), "{text}");
    assert!(
        text.contains(&format!("--> {}\n", file.display())),
        "{text}"
    );
}

#[test]
fn a_hostile_file_cannot_send_escapes_to_the_terminal() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("evil.md");
    std::fs::write(
        &file,
        "---\ncolors:\n  text: \"#111111\"\n  background: \"#ffffff\"\n  accent: \"\u{1b}]52;c;aGk=\u{7}\u{1b}[2J\u{202e}\"\n---\n",
    )
    .unwrap();
    let output = check(&file, &[]);
    assert!(
        stdout(&output).contains("warning[QD005]"),
        "{}",
        stdout(&output)
    );
    assert!(!output.stdout.contains(&0x1b) && !output.stdout.contains(&0x07));
    assert!(!stdout(&output).contains('\u{202e}'));

    let json = stdout(&check(&file, &["--json"]));
    assert!(!json.contains('\u{1b}'), "{json}");
    let report: Value = serde_json::from_str(&json).unwrap();
    assert!(report["diagnostics"].is_array());
}

#[test]
fn a_reader_going_away_does_not_panic_or_change_the_verdict() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_quill"))
        .args(["design", "check", fixture(BAD).to_str().unwrap()])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    drop(child.stdout.take());
    let output = child.wait_with_output().unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(!stderr(&output).contains("panicked"), "{}", stderr(&output));
}

#[test]
fn design_commands_leave_no_data_folder_behind() {
    let dir = tempfile::tempdir().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_quill"))
        .args(["design", "check", fixture(GOOD).to_str().unwrap()])
        .current_dir(dir.path())
        .env_remove("QUILL_DATA_DIR")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
}

#[test]
fn compile_prints_css_for_light_and_dark_to_stdout() {
    let file = fixture(GOOD);
    let output = quill(&["design", "compile", file.to_str().unwrap()]);
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    let css = stdout(&output);
    assert!(css.contains(":root {"), "{css}");
    assert!(css.contains("@media (prefers-color-scheme: dark)"), "{css}");
    // Only CSS, so `> tokens.css` gives a usable file.
    assert!(!css.contains("this design can be applied"));
    assert!(stderr(&output).contains("ok: this design can be applied"));
}

#[test]
fn compile_writes_to_a_file_with_out() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("tokens.css");
    let source = fixture(GOOD);
    let source = source.to_str().unwrap();
    let output = quill(&["design", "compile", source, "--out", out.to_str().unwrap()]);
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    assert!(output.stdout.is_empty());
    assert!(stderr(&output).contains("Wrote"));
    let printed = quill(&["design", "compile", source]);
    assert_eq!(std::fs::read(&out).unwrap(), printed.stdout);
}

#[test]
fn compile_writes_nothing_when_the_design_has_errors() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("tokens.css");
    std::fs::write(&out, "kept").unwrap();
    let source = fixture(BAD);
    let source = source.to_str().unwrap();

    let output = quill(&["design", "compile", source, "--out", out.to_str().unwrap()]);
    assert_eq!(output.status.code(), Some(1));
    assert!(stderr(&output).contains("error[QD010]"));
    assert_eq!(std::fs::read_to_string(&out).unwrap(), "kept");

    assert!(quill(&["design", "compile", source]).stdout.is_empty());
}

#[test]
fn compile_shows_problems_but_leaves_notes_to_check() {
    let source = fixture(BAD);
    let output = quill(&["design", "compile", source.to_str().unwrap()]);
    let text = stderr(&output);
    assert!(text.contains("warning[QD005]"), "{text}");
    assert!(!text.contains("note["), "{text}");
    // The count still tells the reader there are notes to go and find.
    assert!(text.contains("2 notes)"), "{text}");
    assert!(stdout(&check(&source, &[])).contains("note["));
}

#[test]
fn compile_reports_a_place_it_cannot_write_as_trouble() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("no/such/folder/tokens.css");
    let source = fixture(GOOD);
    let output = quill(&[
        "design",
        "compile",
        source.to_str().unwrap(),
        "--out",
        out.to_str().unwrap(),
    ]);
    assert_eq!(output.status.code(), Some(2));
    assert!(stderr(&output).contains("could not write"));
}

#[test]
fn compile_uses_the_base_path_for_fonts_and_keeps_it_clean() {
    let source = fixture("refero_style.md");
    let source = source.to_str().unwrap();
    let css = |extra: &[&str]| {
        let mut args = vec!["design", "compile", source];
        args.extend_from_slice(extra);
        stdout(&quill(&args))
    };
    assert!(css(&[]).contains("url(\"/static/fonts/"));
    assert!(css(&["--base-path", "/blog"]).contains("url(\"/blog/static/fonts/"));
    assert!(css(&["--base-path=/blog/"]).contains("url(\"/blog/static/fonts/"));

    for bad in ["blog", "/a\"b", "/a b", "/a);}"] {
        let output = quill(&["design", "compile", source, "--base-path", bad]);
        assert_eq!(output.status.code(), Some(2), "{bad}");
        assert!(output.stdout.is_empty(), "{bad}");
    }
}
