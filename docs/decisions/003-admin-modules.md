# 003: Splitting the dashboard handlers into modules

## Context

`src/routes/admin.rs` reached about 1,000 lines holding every dashboard feature. Milestones 1, 2 and 4 each add dashboard pages (layouts, design import, domains), so it would keep growing.

## Options

- **Keep one file**, relying on section comments. No churn, but it only gets worse.
- **Split by layer**: forms in one file, handlers in another. Reads tidily, but a single feature change then touches several files.
- **Split by feature**: one module per area of the dashboard, with shared helpers in `mod.rs`.

## Choice

Split by feature, using the section boundaries already in the file:

| Module | Contents |
|---|---|
| `mod.rs` | Shared helpers: template context, rendering, redirects, flash notices, the address-mismatch warning |
| `setup` | First-run wizard |
| `login` | Sign in and out |
| `dashboard` | Overview page |
| `articles` | Article list and editor |
| `media` | Media library and uploads |
| `appearance` | Themes (layouts and design import join it later) |
| `settings` | Site settings |
| `integration` | "Connect your site" |
| `account` | Password change |

The plan suggested `auth`; this uses `login` so it can't be confused with `crate::auth`, which holds sessions and password hashing.

## Why this needs so few `pub` changes

In Rust, a private item is visible in the module that defines it **and in that module's descendants**. So the helpers in `admin/mod.rs` stay private and the child modules still reach them with `use super::{...}`. Siblings are different: `dashboard` can't see private items in `articles`, so `article_rows` became `pub(super)`, meaning visible to the parent and everything under it. That is the only visibility change in the split.

## Trade-offs

- **Handler names stutter in places**, such as `admin::articles::articles` and `admin::media::media`. Keeping names unchanged keeps each commit a pure move. They can be renamed in a later commit.
- **The route table now names each module**, so `routes/mod.rs` shows at a glance where each handler lives.
- **`git blame` needs help across the move.** `git log --follow` and `git blame -C` trace lines back through the split.
