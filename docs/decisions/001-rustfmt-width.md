# 001: rustfmt width

**Status:** the first choice, `max_width = 160`, was reversed the same day in favor of rustfmt's default of 100.

## Context

The 0.1 code was written to a wide line length by hand and never run through rustfmt, so `cargo fmt --check` failed. CI needs that check to pass.

## Options

Lines added plus removed by `cargo fmt`, measured on the 0.1 code:

| `max_width` | Default heuristics | `use_small_heuristics = "Max"` |
|---:|---:|---:|
| 100 (rustfmt default) | 1614 | 917 |
| 120 | 1020 | 626 |
| 140 | 623 | 577 |
| 150 | 548 | 603 |
| 156–164 | **502** | about 606 |
| 170 | 508 | 619 |
| 200 | 540 | 629 |

No value passes with zero changes, because the code was never consistent with any single rustfmt configuration.

## Choice

**The default width of 100, with no `rustfmt.toml`.**

The first attempt optimized for the smallest diff and chose 160. That was the wrong thing to optimize:

- The diff size only matters once, in one commit, and `.git-blame-ignore-revs` removes its main cost by letting `git blame` skip that commit. The line width affects every read of the code afterwards.
- 100 is what Rust developers expect. This code is public and meant to be read, and it now looks like other Rust projects with no explanation needed.
- 160 columns doesn't fit two files side by side on a laptop screen, and it collapsed readable method chains onto one line.
- The switch was cheapest now, before Milestones 1 to 4 add more code.

## Trade-offs

- **More vertical space.** Some signatures and calls now span several lines.
- **A large formatting commit.** It touches 22 Rust files, with about 1,200 lines added and 300 removed, and contains nothing else.
- **The blame-ignore entry depends on that commit's hash.** Rebasing or squash-merging `quill-0.2` would change the hash and leave the entry pointing at nothing. Merge this branch with a merge commit, or update the file after merging.
- **Earlier formatting is still in blame.** The first formatting commit (`de91073`) isn't listed, because it also adds files. Blame for some lines stops there rather than at the original code.
