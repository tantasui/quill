# Appearance: presets, layouts and tokens

How a Quill blog looks comes from three separate things:

- **Tokens** are the style: colors, fonts, radius and reading width.
- **The layout** is the structure: which header, which feed style, what order the home page is in.
- **A preset** is a named starting point that provides both, plus a small stylesheet of finishing touches.

None of them touch your content. Articles, tags and URLs are the same whatever you choose.

## Presets

| Preset | Character | Layout |
|---|---|---|
| **Editorial** | Minimal serif reading column | Centered header, narrow page, list feed |
| **Devlog** | Developer blog with monospace details | Left header bar, compact rows, tag sidebar |
| **Gazette** | Traditional publication with drop caps | Masthead, lead story, three-column grid, hero covers |

All three adapt to phones and follow the reader's light or dark preference. Their palettes meet WCAG AA contrast, which a test enforces.

## Arranging a layout

Open **Appearance** in the dashboard. Changes show in a live preview and only take effect when you select **Save layout**. Choosing a preset replaces your customized layout with that preset's own.

| Component | Options |
|---|---|
| Page width | `narrow`, `regular`, `wide` |
| Header | `centered`, `left`, `masthead` |
| Tag sidebar, on every page | `left`, `right`, `none` |
| Home sections, in any order | `featured` lead story, `feed` (required), `archive_link` |
| Feed style | `list`, `grid`, `cards`, `compact` |
| Article text width | `narrow`, `wide` |
| Article cover image | `hero`, `inline`, `none` |
| Article details | Show or hide author, date, reading time |
| Footer | `simple`, `columns` |

A narrow page has no room for columns, so a sidebar moves below the content and a grid becomes a single column.

## How it's built

Layouts and tokens are typed data in the `quill-design` crate (`crates/quill-design`), which does no I/O and has no web dependencies.

- **Tokens** become CSS at `/static/tokens/<preset>.css`. Every value is parsed into a type such as `Color` or `Length` and written back out from that type, so text from an input file never reaches a stylesheet.
- **Layouts** can only be created through validation, including when they're read back from the database. A stored layout that no longer validates is ignored with a logged warning.
- **Templates** live in `templates/public/`. `base.html` picks partials from `components/` according to the layout.

### Stylesheets

Loaded in this order:

1. `/static/tokens/<preset>.css`: generated token values.
2. `static/themes/common.css`: article text, pagination and accessibility helpers.
3. `static/themes/components.css`: every layout component in every variant, using tokens only.
4. `static/themes/<preset>.css`: the preset's finishing touches, also using tokens only.

### Design tokens

```css
--bg --surface --text --muted --border --code-bg
--accent-fill --on-accent --accent-text --accent-soft
--font-body --font-heading --font-mono --heading-weight
--measure --radius --shadow
```

The accent has separate roles because one color often can't do every job. `--accent-text` is for links and must be readable on the page background. `--accent-fill` is for filled areas, with `--on-accent` as the text on top. `--accent-soft` is an optional faint tint.

### Adding a preset

1. Add `crates/quill-design/presets/<id>.toml` with `[tokens]` and `[layout]` sections, copying an existing preset as a starting point.
2. Register it in `ALL` in `crates/quill-design/src/presets.rs`.
3. Add `static/themes/<id>.css`, which can be empty.

Tests then render every page with it and check its palettes for contrast.

### Template variables

| Variable | Contents |
|---|---|
| `site.name`, `site.description`, `site.url`, `site.language`, `site.author`, `site.footer` | Blog settings |
| `base` | Base path, e.g. `/blog` (empty for a subdomain). Prefix every internal link with it. |
| `theme`, `version` | Active preset id, and the version for cache busting |
| `layout` | The active layout, with the fields from `LayoutDraft` |
| `nav_tags` | Most-used tags: `name`, `count`, `url` |
| `feed_url`, `canonical`, `page_title`, `meta_description` | Feed and SEO values |
| `preview`, `preview_label` | Set when the dashboard renders a preview |

Page-specific variables: `lead`, `feed_articles` and `pagination` (`current`, `total`, `newer_url`, `older_url`) on the home page; `articles`, `pagination` and `tag` on tag pages; `article`, `newer` and `older` on article pages; `years` on the archive.

Each article provides `title`, `url`, `abs_url`, `excerpt`, `has_excerpt`, `cover_image`, `date`, `date_short`, `date_iso`, `reading_minutes`, `author` and `tags` (each with `name` and `url`), plus `html` on article pages.
