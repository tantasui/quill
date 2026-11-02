# Importing a DESIGN.md

DESIGN.md files describe a site's visual system for AI coding agents. Quill can read one and give your blog that look, on a layout you arrange yourself. Style comes from the file. Structure comes from your [layout](themes.md), so two blogs using the same file can still look different.

## How to import

1. Open **Appearance**, then **Import a design**.
2. Paste the file, or upload it. The limit is 256 KB. Quill doesn't fetch designs from URLs.
3. Read what was found: every problem and decision is listed, with the line it came from.
4. Check the preview on your own articles. Switch between the home page and an article, and between light and dark.
5. Select **Apply to my blog**, or **Cancel**. Nothing changes on your blog until you apply.

To go back, reopen an earlier design from **Import a design**, select **Back to the preset's colors** on the Appearance page, or choose a preset. Your articles are never affected.

## What Quill reads

Quill takes values from wherever the file provides them. When sources disagree, the earlier one in this list wins, and a note says so:

1. **YAML tokens:** front matter, and ` ```yaml ` blocks, in the [DESIGN.md format](https://github.com/google-labs-code/design.md).
2. **CSS:** ` ```css ` blocks with `:root { --name: value; }`, including `.dark` and `@media (prefers-color-scheme: dark)`.
3. **Tables:** colors with a name and role, Light and Dark columns, type scales, radii and shadows.
4. **Lists:** such as `- text: #0c0a08` or `**Primary (#1A1C1E):** A deep ink used for core text.`

It also reads font sections (family, weights and `Substitute:` lines) and `**Theme:** dark`.

### How colors get their roles

| Role | Names it recognizes |
|---|---|
| Background | canvas, background, bg, page, paper |
| Surface | surface, card, panel, elevated |
| Text | text, ink, foreground, heading, on-surface |
| Muted text | muted, secondary text, subtle, caption, metadata |
| Border | border, hairline, divider, stroke |
| Accent fill | primary action, CTA, button, accent, brand |
| Text on accent | on-primary, on-accent, button text |
| Links | link, accent text |
| Code background | code |

`primary`, `secondary`, `tertiary` and `neutral` don't say what a color is for, so Quill first looks for a description in the file. Failing that, it goes by the color: a colorful `primary` is the accent, a light `neutral` is the page, and other neutrals go by lightness. Each guess is listed so you can check it, and describing the color in the file settles it.

Colors for things Quill doesn't draw, such as error states, are listed as unused.

## What Quill checks

**Readability.** Text must have 4.5:1 contrast with its background, the WCAG AA level.

| If this fails | Then |
|---|---|
| Main text on the background | It's an error, and the design can't be applied |
| Text on the surface | Cards use the page background |
| Muted text | It's adjusted until it passes |
| The accent as text | It stays a fill behind readable text, and links use the text color with an underline |

**Valid values.** A value like `64-128px` is rejected with an explanation, never passed through. CSS is always written from parsed values, so a file can't add anything else to your blog's stylesheet.

**Fonts.** Quill never loads fonts from other servers. It bundles Inter, IBM Plex Sans, Source Serif 4 and JetBrains Mono. Other fonts map to the file's `Substitute:` if it's bundled, otherwise to the closest match, and each swap is listed. System fonts such as Georgia are used as written.

**Dark mode.** Dark tokens in the file are used. Otherwise Quill derives a dark palette that keeps the design's hues and passes the same contrast checks, and lists its values. A design that's dark with no light colors stays dark for everyone.

**Rules in prose.** A few are recognized and applied: "never bold" or "only weight 400" (heading weight), "no box-shadow" (shadows), "sharp corners" (radius) and "page max-width: 1200px" (page width). Other rules are listed as not applied, so you know what to check yourself.

## From the terminal

The `quill design` commands run the same compiler as the dashboard, so the two always agree. They don't need a blog or a data folder, so they work in CI or on a laptop.

### Checking a file

```console
$ quill design check DESIGN.md
error[QD010]: text #cccccc on background #ffffff has a contrast of 1.61:1, below the 4.5:1 needed to read comfortably
 --> DESIGN.md:4:9
  |
4 |   text: "#cccccc"
  |         ^^^^^^^
  |
  = help: Make the text darker or the background lighter (or the reverse for a dark design).

error: this design can't be applied (1 error, 0 warnings, 0 notes)
```

| Exit code | Meaning |
|---|---|
| 0 | No errors. There may be warnings and notes. |
| 1 | The design has errors. |
| 2 | Quill couldn't give an answer, for example because the file is missing or an argument is wrong. |

Output is colored in a terminal, unless `NO_COLOR` is set. Control characters from the file are shown as `�`, so a file can't send commands to your terminal.

With `--json`, the same report is printed as a single JSON object:

```json
{
  "file": "DESIGN.md",
  "valid": false,
  "errors": 1,
  "warnings": 0,
  "notes": 0,
  "diagnostics": [
    {
      "code": "QD010",
      "level": "error",
      "message": "text #cccccc on background #ffffff has a contrast of 1.61:1, below the 4.5:1 needed to read comfortably",
      "span": { "line": 4, "column": 9, "len": 7 },
      "help": "Make the text darker or the background lighter (or the reverse for a dark design)."
    }
  ]
}
```

`span` and `help` are left out when a diagnostic doesn't have them. Columns count characters, starting from 1. Values from the file appear in messages exactly as written, so clean them before printing them somewhere that matters.

### Compiling a file

```sh
quill design compile DESIGN.md                                  # print the CSS
quill design compile DESIGN.md --out tokens.css                 # write it to a file
quill design compile DESIGN.md --out tokens.css --base-path /blog
```

The result is the stylesheet Quill would serve: `@font-face` rules for any bundled fonts, then `:root` tokens for light and dark. Use `--base-path` if your blog isn't at the root of its domain, so the font addresses are right.

Errors and warnings go to standard error, so `> tokens.css` always gives you only CSS. Notes appear only in `check`. If the design has errors, nothing is written and any existing `--out` file is left alone. Exit codes are the same as for `check`.

## Diagnostic codes

The codes are stable, so they can be searched for. Errors stop a design from being applied; warnings and notes don't.

| Code | Level | Meaning |
|---|---|---|
| QD001 | Error | The file is over the 256 KB limit |
| QD002 | Error | The file isn't UTF-8 text |
| QD003 | Error | No colors were found |
| QD004 | Warning | A YAML block couldn't be read, and its tokens were skipped |
| QD005 | Warning | A value is invalid and was ignored |
| QD006 | Note | Two sources disagree; the note says which won |
| QD007 | Note | Tokens Quill has no use for |
| QD008 | Note | A color's role was inferred from the color itself |
| QD009 | Note | A role the file doesn't set was worked out from its other colors |
| QD010 | Error | Main text is unreadable on the background |
| QD011 | Warning | Text is unreadable on the surface, so cards use the background |
| QD012 | Warning | Muted text was adjusted to be readable |
| QD013 | Warning | The accent is unreadable as text, so it's fill-only |
| QD014 | Note | Text on the accent was changed to be readable |
| QD015 | Note | A transparent color was flattened to check its contrast |
| QD016 | Warning | Explicit dark colors are unreadable, so a derived palette is used |
| QD020 | Note | A font isn't bundled and was swapped |
| QD021 | Note | A font isn't bundled, so the file's system font substitute is used |
| QD030 | Note | A dark palette was derived, with its values |
| QD031 | Note | A dark design with no light colors stays dark for everyone |
| QD040 | Note | A rule in the file's prose was applied |
| QD041 | Note | A rule in the file's prose wasn't applied |
| QD042 | Warning | A page width doesn't fit Quill's layouts and was ignored |

## Testing against your own files

Put files in `local-fixtures/`, which is git-ignored, then run:

```sh
cargo test -p quill-design --test compile -- --ignored --nocapture
```
