# Quill

[![CI](https://github.com/tantasui/quill/actions/workflows/ci.yml/badge.svg)](https://github.com/tantasui/quill/actions/workflows/ci.yml)

Add a blog to the website you already have. Quill is a small, self-hosted blogging engine: one binary, one SQLite file, three built-in themes and a browser dashboard for writing and publishing. Nothing to build and no separate frontend to maintain.

> "Quill" is a working title.

- **Lives on your domain.** Serve it at `example.com/blog` or `blog.example.com`. Every link, asset, feed and canonical URL follows the public address you set.
- **Publish from the browser.** A Markdown editor with a formatting toolbar, live preview, drag-and-drop images, drafts, scheduling, tags, cover images and SEO fields. You never need a terminal or a database client.
- **Looks good out of the box.** Three responsive themes (*Editorial*, *Devlog* and *Gazette*) with dark mode. Preview any of them on your own content and switch at any time without touching your articles.
- **Secure defaults.** Argon2 password hashing, HttpOnly session cookies, CSRF tokens and origin checks, login throttling, sanitized Markdown, content-sniffed image uploads (no SVG) and a strict Content-Security-Policy. A one-time setup code stops anyone else from claiming a fresh install.
- **Yours to keep.** Built-in backup and restore, automatic snapshots before upgrades, and a Markdown export that works with other tools.
- **Search-friendly.** Server-rendered HTML, canonical URLs, Open Graph tags, an Atom feed, a sitemap, `robots.txt`, and automatic redirects when you rename an article.

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

Open `http://localhost:3000/admin/setup`, enter the setup code, name your blog, set its public address, create your account and pick a theme. Then open **Connect your site** in the dashboard: it generates the nginx, Caddy or Apache configuration for your address.

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
```

| Setting | Flag | Environment | Default |
|---|---|---|---|
| Data folder | `--data-dir` | `QUILL_DATA_DIR` | `./data` |
| Listen address | `--listen` | `QUILL_LISTEN` | `127.0.0.1:3000` |
| Public address (overrides dashboard) | `--base-url` | `QUILL_BASE_URL` | set during setup |
| Fixed setup code | `--setup-code` | `QUILL_SETUP_CODE` | random |
| Log level | | `QUILL_LOG` | `info` |

Backups, upgrades and recovery: [docs/operations.md](docs/operations.md). Themes and the template contract: [docs/themes.md](docs/themes.md).

## Development

```sh
cargo test        # unit + end-to-end tests (in-memory database)
cargo run         # debug builds read templates and CSS from disk, so edits show on reload
```

Layout: `src/routes/public.rs` (blog pages, feeds), `src/routes/admin.rs` (dashboard), `src/models.rs` (queries), `src/auth.rs` (sessions, CSRF), `src/markdown.rs` (rendering and sanitizing), `templates/` and `static/` (embedded into the binary at build time), `migrations/` (applied on startup).

## Decisions for the open questions in the PRD

| PRD question | Choice in this release | Why |
|---|---|---|
| Hosting target | Native binary on a VPS, plus a Docker image | One codebase covers both; shared PHP hosting can't run a server process. |
| `/blog` or subdomain | Both | One public-address setting decides the base path. Proxies may keep or strip the prefix. |
| Editor | Markdown with toolbar, live preview, side-by-side mode and in-theme preview | Portable content and no heavy client bundle; the toolbar covers authors who don't know Markdown. |
| Database | SQLite (WAL mode) | Zero setup, single-file backups, plenty for one author's blog. |
| `design.md` importer | Not yet (Milestone 5) | Themes already use CSS design tokens, which the importer can target later. |
| Users | Single administrator | Per the PRD's MVP assumption; the schema keeps a `role` column for later. |
| Content export | Markdown + YAML front matter, plus media | Readable by most static-site generators and blogging tools. |
