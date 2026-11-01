# 005: Layouts as data, presets as layout + tokens + stylesheet

## Context

In 0.1, each theme had its own page templates, so the only way to change a blog's structure was to switch theme. 0.2 separates style (tokens, and later an imported DESIGN.md) from structure, so two blogs with the same style can look different. The three themes had to become data without changing how they look.

## Options

- **Keep per-theme templates and add options inside them.** Every new option multiplies across themes, and a variant from one theme can't be used with another.
- **One set of component templates driven by layout data.** Each component variant exists once and works with any style.

## Choice

Component partials in `templates/public/components/`, chosen by a typed `Layout` from the `quill-design` crate. A preset is a TOML file holding a layout and tokens.

`Layout` can only be created through validation, including when it's deserialized. `LayoutDraft` is the editable form. So a stored or submitted layout can't reach a template unchecked, and templates don't need defensive code.

## Where this differs from the plan

The plan described a preset as "a layout plus default tokens", with `tag_rail` as a home section. Reproducing 0.1 exactly showed that needs a few changes:

1. **`tag_rail` is its own setting, not a home section.** Devlog's sidebar appears on every page, including articles and the archive. As a sidebar it also has no position in an ordered list, so a home-section entry would imply reordering it does something.
2. **A `page_width` setting was added** (`narrow`, `regular`, `wide`). The three themes have different page widths, and a grid feed needs more room than a reading column.
3. **Layouts carry a few labels and flags.** Devlog's navigation says `posts / archive / rss`, Gazette's says `Front page`, and only some show tags or a feed link. These are wording and structure, so they live with the layout rather than in templates.
4. **Presets keep a small stylesheet of finishing touches.** Italic metadata, Devlog's `~/` prompt and Gazette's drop caps can't be expressed by colors and fonts without dozens of extra tokens. These stylesheets use tokens only, so they work with any palette.

## Proof that the presets didn't change

Each preset was screenshotted in headless Chrome before and after, across 7 pages, 2 widths and 2 color schemes: 84 images per run, compared pixel by pixel. Repeat runs of the new code were 84/84 identical to 0.1.

Occasional runs differ by a few dozen anti-aliased pixels along image edges on mobile. The unchanged 0.1 binary shows the same noise compared with itself, in different images each run. Hover states aren't covered by screenshots; those rules were moved or renamed mechanically.

## Trade-offs

- **The screenshot check isn't in the repository or CI.** It needs Chrome and a frozen 0.1 build, and it answers a one-time question. The automated tests check that every combination renders, not how it looks.
- **Some combinations are valid but plain.** Mixing a preset's finishing touches with another preset's header works, but hasn't had a designer's eye on every pairing.
- **Empty-state wording lost a detail.** Devlog's `0 posts` was styled as code in 0.1 and is now plain text, because labels are plain text. It only shows on a blog with no articles.
