//! `quill design`: the dashboard's compiler from a terminal, so a file can be
//! checked before it goes anywhere near a blog.

use std::fs::File;
use std::io::{self, IsTerminal, Read, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result};
use clap::Subcommand;
use quill_design::compiler::report::{self, Tally};
use quill_design::{compile_bytes, Compilation, Diagnostic, MAX_SOURCE_BYTES};
use serde::Serialize;

/// The design has errors.
const EXIT_INVALID: u8 = 1;
/// Quill couldn't get as far as an answer, such as with a missing file. This
/// matches what clap uses for bad arguments, and lets a script tell "this design
/// is bad" apart from "this design was never looked at".
const EXIT_TROUBLE: u8 = 2;

#[derive(Subcommand)]
pub enum DesignCommand {
    /// Report every problem and decision in a DESIGN.md file. Exits with 0 when
    /// there are no errors, 1 when there are, and 2 if the file can't be read.
    Check {
        file: PathBuf,
        /// Print one JSON object instead of text.
        #[arg(long)]
        json: bool,
    },
}

pub fn run(command: DesignCommand) -> ExitCode {
    let result = match command {
        DesignCommand::Check { file, json } => check(&file, json),
    };
    match result {
        Ok(code) => code,
        Err(e) => {
            emit(io::stderr(), &format!("\nError: {e:#}\n"));
            ExitCode::from(EXIT_TROUBLE)
        }
    }
}

struct Loaded {
    source: String,
    compilation: Compilation,
}

fn load(path: &Path) -> Result<Loaded> {
    let problem = || format!("could not read {}", path.display());
    let file = File::open(path).with_context(problem)?;
    let size = file.metadata().with_context(problem)?.len();
    if size > MAX_SOURCE_BYTES as u64 {
        return Ok(Loaded {
            source: String::new(),
            compilation: Compilation::too_large(size),
        });
    }
    // The size can be wrong for anything but a regular file, so the read is
    // capped too: one byte over the limit is enough for the compiler to refuse it.
    let mut bytes = Vec::new();
    file.take(MAX_SOURCE_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .with_context(problem)?;
    Ok(Loaded {
        compilation: compile_bytes(&bytes),
        source: String::from_utf8_lossy(&bytes).into_owned(),
    })
}

/// Writes out text, which `println!` would panic on if the reader has gone away,
/// as with `quill design check FILE | head`. Nobody is left to tell, and the exit
/// code should still give the verdict.
fn emit(mut stream: impl Write, text: &str) {
    let _ = stream.write_all(text.as_bytes());
}

/// Whether to color output for a stream, following https://no-color.org.
fn wants_color(stream: &impl IsTerminal) -> bool {
    stream.is_terminal() && std::env::var_os("NO_COLOR").unwrap_or_default().is_empty()
}

fn verdict(compilation: &Compilation) -> ExitCode {
    if compilation.has_errors() {
        ExitCode::from(EXIT_INVALID)
    } else {
        ExitCode::SUCCESS
    }
}

/// The shape of `--json`, which scripts rely on. Documented in
/// `docs/design-import.md`.
#[derive(Serialize)]
struct JsonReport<'a> {
    file: String,
    valid: bool,
    errors: usize,
    warnings: usize,
    notes: usize,
    diagnostics: &'a [Diagnostic],
}

fn check(path: &Path, json: bool) -> Result<ExitCode> {
    let Loaded {
        source,
        compilation,
    } = load(path)?;
    let diagnostics = &compilation.diagnostics;
    let file = path.display().to_string();
    let text = if json {
        let tally = Tally::of(diagnostics);
        let mut line = serde_json::to_string(&JsonReport {
            file,
            valid: !compilation.has_errors(),
            errors: tally.errors,
            warnings: tally.warnings,
            notes: tally.notes,
            diagnostics,
        })?;
        line.push('\n');
        line
    } else {
        let color = wants_color(&io::stdout());
        let mut text = report::render(diagnostics, &source, &file, color);
        text.push_str(&report::summary(diagnostics, color));
        text
    };
    emit(io::stdout(), &text);
    Ok(verdict(&compilation))
}
