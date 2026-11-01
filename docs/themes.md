# Themes

Quill ships three themes. Each renders the same content, and switching never changes article URLs or data.

| Theme | Character | Home page |
|---|---|---|
| **Editorial** | Minimal serif reading column | Titles, dates and excerpts in a single column |
| **Devlog** | Developer blog: sans-serif text, monospace accents, tag sidebar | Compact rows with ISO dates and tag chips |
| **Gazette** | Traditional publication: masthead, section bar, drop caps | Lead story with cover image, then a three-column story grid |

All three adapt to phones and follow the reader's light or dark preference.

## The template contract

Templates use [MiniJinja](https://docs.rs/minijinja) (Jinja2 syntax) and are compiled into the binary.

A theme with id `name` consists of:

- `templates/themes/name/base.html`: required. Defines the page frame and a `{% block content %}`. Include `public/_head.html` inside `<head>` and `public/_banner.html` right after `<body>`.
- `static/themes/name.css`: required. Loaded after the shared `static/themes/common.css`.
- `templates/themes/name/<page>.html`: optional overrides for `index`, `article`, `tag`, `archive` or `not_found`. Pages without an override use `templates/public/<page>.html`.

Register the theme in `src/themes.rs`. The test suite renders every page in every registered theme.

### Variables available on every page

| Variable | Contents |
|---|---|
| `site.name`, `site.description`, `site.url`, `site.language`, `site.author`, `site.footer` | Blog settings |
| `base` | Base path, e.g. `/blog` (empty for a subdomain). Prefix every internal link with it. |
| `theme`, `layout`, `version` | Active theme id, its base template, and the version (for cache busting) |
| `nav_tags` | Most-used tags: `name`, `count`, `url` |
| `feed_url`, `canonical`, `page_title`, `meta_description` | Feed and SEO values |
| `preview`, `preview_label` | Set when the dashboard renders a preview |

Page-specific variables: `articles` and `pagination` (`current`, `total`, `newer_url`, `older_url`) on index and tag pages; `tag` on tag pages; `article`, `newer` and `older` on article pages; `years` on the archive.

Each article provides `title`, `url`, `abs_url`, `excerpt`, `has_excerpt`, `cover_image`, `date`, `date_short`, `date_iso`, `reading_minutes`, `author` and `tags` (each with `name` and `url`), plus `html` on article pages.

### Design tokens

`common.css` styles article prose, pagination and accessibility helpers using CSS custom properties. A theme mostly sets these on `:root`:

```css
--bg --surface --text --muted --accent --border --code-bg
--font-body --font-heading --font-mono --measure --radius
```

The planned `design.md` importer will produce values for these same tokens, so imported designs work with every theme.
