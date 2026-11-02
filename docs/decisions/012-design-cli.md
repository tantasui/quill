# 012: The design commands

## Context

`quill design check` and `quill design compile` bring the DESIGN.md compiler to the terminal. The plan sets the output format and the exit codes for a design with and without errors. It leaves open what happens when there's no verdict to give, where output goes, and how to print text from a file that may be someone else's.

## Options and choices

**Where rendering lives.** It could be in the `quill` binary or in `quill-design`. It's in `quill-design`, returning strings, so the crate stays free of I/O and rendering is tested without running a process. Both surfaces also share one way of cutting source excerpts, so a fix to carets reaches both.

**When a file can't be read.** Exit code 1 would be simplest, but then a script can't tell "this design is bad" from "this design was never looked at", and a missing file in CI would read as a design failure. We use 2, which clap already uses for bad arguments.

**Which stream.** `check`'s report is its product, so it goes to standard output. For `compile` the CSS is the product, so diagnostics go to standard error and `> tokens.css` always gives a stylesheet. rustc sends everything to standard error, but it has no report to pipe.

**Notes from `compile`.** A typical file produces many notes about choices the compiler made. Next to output that's about to be used they bury the warnings, so `compile` prints errors and warnings, and its closing count shows that notes exist.

**Text from the file.** Excerpts and messages quote the file. Printed raw, escape sequences could rewrite the output or set the clipboard, and bidirectional controls could reorder a line. Each such character is replaced with `�`, one for one, so carets stay aligned. `--json` keeps values exact, because a program reading it needs the real value and JSON already escapes control characters below U+0020. A person printing those values is warned in the docs.

**Color.** Hand-written ANSI codes, around a dozen lines, against a crate such as `anstyle`. The palette is five colors that never change, so a dependency buys nothing. It's on when the stream is a terminal and `NO_COLOR` is unset.

## Trade-offs

- Exit code 2 goes beyond what the plan states. A script testing for exactly 1 won't count a missing file as a failed design; a script testing for non-zero behaves the same either way.
- Replacing control characters means the terminal doesn't show a file's exact bytes. That only matters for a file that contains them, and the file itself is still there to inspect.
- `--base-path` is an extra flag, needed because the commands don't read a blog's settings. Reading them would tie a command that needs no blog to a data folder.
