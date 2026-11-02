# Quill

[![CI](https://github.com/tantasui/quill/actions/workflows/ci.yml/badge.svg)](https://github.com/tantasui/quill/actions/workflows/ci.yml)

Add a blog to the website you already have. Quill is a small, self-hosted blogging engine: one binary, one SQLite file, three built-in presets and a browser dashboard for writing and publishing. Nothing to build and no separate frontend to maintain.

- **Lives on your domain.** Serve it at `example.com/blog` or `blog.example.com`. Every link, asset, feed and canonical URL follows the public address you set.
- **Publish from the browser.** A Markdown editor with a formatting toolbar, live preview, drag-and-drop images, drafts, scheduling, tags, cover images and SEO fields. You never need a terminal or a database client.
- **Looks good out of the box.** Three responsive presets (*Editorial*, *Devlog* and *Gazette*) with dark mode. Preview any of them on your own content and switch at any time without touching your articles.
- **A layout you arrange yourself.** Choose the header, feed style, sidebar, footer and article layout, and reorder the home page, with a live preview. Two blogs on the same preset don't have to look the same.
- **Paste a DESIGN.md, get that look.** Paste the file you'd give your coding agent, and your blog takes its colors, fonts and shapes. Every decision is listed with its source line, contrast is checked, and a preview on your own articles comes before anything goes live.
- **Secure defaults.** Argon2 password hashing, HttpOnly session cookies, CSRF tokens and origin checks, login throttling, sanitized Markdown, content-sniffed image uploads (no SVG) and a strict Content-Security-Policy. A one-time setup code stops anyone else from claiming a fresh install.
- **Yours to keep.** Built-in backup and restore, automatic snapshots before upgrades, and a Markdown export that works with other tools.
- **Search-friendly.** Server-rendered HTML, canonical URLs, Open Graph tags, an Atom feed, a sitemap, `robots.txt`, and automatic redirects when you rename an article.

## Screenshots

<!-- TODO: add dashboard and preset screenshots, plus a demo GIF. -->
*Screenshots and a demo GIF are coming.*

## Why Quill?

Quill is for people who already have a website and want a blog on it, written from a browser, with one small program to look after. It is young and deliberately narrow, and the projects below are more mature. Pick whichever fits.

| | Quill | [Ghost](https://ghost.org) | [WriteFreely](https://writefreely.org) | [Zola](https://www.getzola.org) |
|---|---|---|---|---|
| **What it is** | Self-hosted blog with a dashboard | Publishing platform with memberships and newsletters | Distraction-free writing with federation | Static site generator |
| **What you run** | One binary | [Node.js LTS, MySQL 8 and nginx on Ubuntu](https://docs.ghost.org/faq/supported-databases) | [One binary](https://writefreely.org/start) | [Nothing at runtime](https://www.getzola.org/): it renders static files |
| **Storage** | SQLite | [MySQL 8, the only supported production database](https://docs.ghost.org/faq/supported-databases) | [MySQL or SQLite](https://writefreely.org/docs/main/admin/config) | [Markdown files](https://www.getzola.org/documentation/getting-started/overview/) |
| **Where you write** | Browser (Markdown) | Browser | [Browser](https://writefreely.org/) | Your text editor, then build and deploy |

- **Choose Ghost** if you want [memberships, paid subscriptions, newsletters and a theme marketplace](https://ghost.org/pricing/). Quill has none of these. Ghost also asks more of your server, and [Ghost(Pro)](https://ghost.org/pricing/) hosts it for you if you'd rather not run one at all.
- **Choose WriteFreely** if you want [ActivityPub federation or a community of writers](https://writefreely.org/) on one instance. Quill doesn't federate and has a single administrator.
- **Choose Zola** if you're happy writing in files and deploying from a terminal. With [no server or database to manage](https://www.getzola.org/), a static site has less to look after and less to secure than Quill will ever have.
- **Choose Quill** if you want a dashboard and your blog at `example.com/blog` on the site you already run, without operating a database server or a build pipeline.

## Quick start

**Docker**

```sh
docker compose up -d
docker compose logs quill      # copy the setup code
```

**Native binary** (Linux or macOS; Windows should work but is untested. Rust is only needed to build it.)

```sh
cargo build --release
./target/release/quill         # prints the address and setup code
```

Open `http://localhost:3000/admin/setup`, enter the setup code, name your blog, set its public address, create your account and pick a preset. Then open **Connect your site** in the dashboard: it generates the nginx, Caddy or Apache configuration for your address.

Full instructions: [docs/install.md](docs/install.md).

## Where it runs

| Environment | Supported | Notes |
|---|---|---|
| Linux VPS or dedicated server | Yes | Binary + systemd ([deploy/quill.service](deploy/quill.service)) behind nginx, Caddy or Apache. |
| Docker / Docker Compose | Yes | Persist `/data` in a volume. |
| Container platforms with a persistent disk (e.g. Fly.io volumes, Render disks) | Should work | Untested. Point `QUILL_DATA_DIR` at the mounted disk. |
| Shared hosting that only runs PHP | No | Quill is a long-running server process; PHP-only hosts can't run it. |
| Static or serverless hosting | No | Needs a persistent process and disk. |

## Commands

```
quill [serve]                 Run the server (default)
quill backup [--out DIR]      Snapshot database + media (safe while running)
quill restore DIR             Restore a backup (stop the server first)
quill export [--out DIR]      Export articles as Markdown with front matter
quill reset-password [USER]   Set a new admin password
quill design check FILE       Check a DESIGN.md: diagnostics and an exit code
quill design compile FILE     Compile a DESIGN.md to CSS
```

| Setting | Flag | Environment | Default |
|---|---|---|---|
| Data folder | `--data-dir` | `QUILL_DATA_DIR` | `./data` |
| Listen address | `--listen` | `QUILL_LISTEN` | `127.0.0.1:3000` |
| Public address (overrides dashboard) | `--base-url` | `QUILL_BASE_URL` | set during setup |
| Fixed setup code | `--setup-code` | `QUILL_SETUP_CODE` | random |
| Log level | | `QUILL_LOG` | `info` |

Backups, upgrades and recovery: [docs/operations.md](docs/operations.md). Presets, layouts and tokens: [docs/themes.md](docs/themes.md). Importing designs: [docs/design-import.md](docs/design-import.md).

## Development

```sh
cargo fmt --all --check                                      # formatting
cargo clippy --workspace --all-targets -- -D warnings        # lints
cargo test --workspace                                       # unit + end-to-end tests (in-memory database)
cargo run                                                    # debug builds read templates and CSS from disk, so edits show on reload
```

Code: `crates/quill-design/` (tokens, layouts and presets; no I/O), `src/routes/public.rs` (blog pages, feeds), `src/routes/admin/` (dashboard, one module per area), `src/models.rs` (queries), `src/auth.rs` (sessions, CSRF), `src/markdown.rs` (rendering and sanitizing), `templates/` and `static/` (embedded into the binary at build time), `migrations/` (applied on startup).

CI runs the first three on every push and pull request.

## Design decisions

The longer reasoning behind individual choices is in [docs/decisions/](docs/decisions/).

| Question | Choice | Why |
|---|---|---|
| Hosting target | Native binary on a VPS, plus a Docker image | One codebase covers both; shared PHP hosting can't run a server process. |
| `/blog` or subdomain | Both | One public-address setting decides the base path. Proxies may keep or strip the prefix. |
| Editor | Markdown with toolbar, live preview, side-by-side mode and a preview on your blog's design | Portable content and no heavy client bundle; the toolbar covers authors who don't know Markdown. |
| Database | SQLite (WAL mode) | Zero setup, single-file backups, plenty for one author's blog. |
| DESIGN.md import | A deterministic compiler with typed tokens; no AI and no URL fetching | The same file always gives the same result, and nothing from a file reaches a stylesheet as text. |
| Users | Single administrator | Keeps the first releases simple; the schema keeps a `role` column for later. |
| Content export | Markdown + YAML front matter, plus media | Readable by most static-site generators and blogging tools. |

## License

[MIT](LICENSE). The bundled fonts are under the SIL Open Font License; see [static/fonts/licenses/](static/fonts/licenses/).
