# 010: How the DESIGN.md compiler treats input

## Context

DESIGN.md files come from many sources in several shapes, and some contain mistakes. The result becomes a stylesheet on a live blog, so the compiler has to be forgiving about format and strict about safety.

## Decisions

**Values are never copied through.** Each value is parsed into a type such as `Color` or `Length`, and CSS is written from that type. A file can't inject CSS or markup, and malformed values can't reach a stylesheet. Unsupported color forms like `lab()` are rejected with a diagnostic rather than passed along.

**One pipeline with an explicit precedence.** Extractors produce raw candidates tagged with their source and line, and a single normalize step chooses between them: YAML first, then CSS, tables and lists. Disagreements between sources get a note, so the author can see which value won.

**Only unreadable main text is an error.** A design with unreadable body text has no automatic fix that respects the author's intent. Everything else has a safe fallback, reported as a warning or note:

| Problem | What happens |
|---|---|
| Accent fails as text | It stays a fill; links use the text color, underlined |
| Muted text fails | Nudged toward the text color until it passes |
| Surface fails | Cards use the page background |
| Explicit dark text fails | The derived dark palette is used |
| Text on the accent fails | Replaced by whichever of text, background, black or white passes |

`Compilation::design` is `None` whenever there is an error, so code that applies a design can't reach one that failed. The level belongs to the diagnostic code, so a problem can't be an error in one place and a warning in another.

**`primary` doesn't automatically mean accent.** The spec's own example uses `primary` for dark ink and `neutral` for the page background. So those names first look for a description in the file, then fall back to the color itself: the most colorful is the accent, and neutrals go by lightness. Each inference is reported.

**Rules are recognized by phrase, with no AI.** Results are deterministic and explainable. A rule that isn't recognized is listed as "not applied", so nothing is silently dropped.

**Hostile input is bounded.** The file is limited to 256 KB and parsing is linear. A test runs 16 pathological inputs under a time limit, and another runs 3,000 mangled files and checks that nothing panics and that whatever passes is still readable. These tests found two real quadratic slowdowns during development.

## Trade-offs

- **Role matching is heuristic.** An oddly named color can land in the wrong role. Diagnostics and the preview exist so a person catches that before applying.
- **Multi-layer shadows are rejected.** Only one shadow is supported, which is enough for cards, so a layered one becomes a warning.
- **Snapshots lock in wording.** Rewording a diagnostic means updating snapshots, which also makes the change visible in review.
