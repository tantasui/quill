# 004: A separate `quill-design` crate

## Context

Design tokens, layouts and the upcoming DESIGN.md compiler need the same validation rules in the dashboard and in the `quill design` command line. Milestone 2's compiler also needs to be easy to test against many inputs.

## Options

- **A module inside the `quill` crate.** Less setup. But nothing stops that module from reaching for the database or Axum, and only a code review would notice.
- **A separate crate in a Cargo workspace.** The dependency list becomes the boundary: if `sqlx` and `axum` aren't in `quill-design/Cargo.toml`, the code can't use them.

## Choice

A workspace with `crates/quill-design`, depended on by path from the `quill` package at the repository root. It takes values and returns values, with no file, network or database access. Embedded presets use `include_str!`, which happens at compile time.

## Trade-offs

- **One more manifest and a workspace to understand.** That cost is small and only paid once.
- **The Dockerfile has to copy `crates/`.** It was changed in the same commit, so no commit in the history has a broken image build.
- **The crate isn't published.** It is an internal boundary for now, and its API can change freely.
