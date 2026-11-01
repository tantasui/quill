# 001: rustfmt width

## Context

The 0.1 code was written to a wide line length by hand and never run through rustfmt, so `cargo fmt --check` failed. CI needs that check to pass, and the goal was the setting that disturbs the existing code least.

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

No value passes with zero changes. The code was never consistent with any single rustfmt configuration, so some diff is unavoidable.

## Choice

`max_width = 160` with default heuristics. Values from 156 to 164 tie on diff size, and 160 is the round number in that range.

## Trade-offs

- **Very wide lines.** 160 columns is well past the Rust norm of 100 and won't fit side by side on a laptop screen. It was chosen to keep the formatting commit small, not because it reads best.
- **Long chains collapse.** rustfmt scales its other limits with `max_width`, so some method chains that used to span several lines now sit on one.
- **Reversible.** Moving to the default later is one config change and one mechanical commit of about 1,600 lines. If the project gets outside contributors, the default is worth reconsidering, since it is what Rust developers expect.
