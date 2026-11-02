# 007: Reading YAML tokens

## Context

The DESIGN.md format puts tokens in YAML, either as front matter or in fenced blocks. Diagnostics need the line and column of every value, and the input comes from anyone's file, so it must be safe to parse.

## Options

- **`serde_yaml`:** the usual choice, but it's archived, and it deserializes to values with no positions attached.
- **`yaml-rust2`:** maintained, but it pulls in `encoding_rs` and `hashlink`, which are large for what's needed here.
- **`saphyr-parser`:** an event-based YAML 1.2 parser with positions on each event, and small dependencies (`arraydeque`, `thiserror`).
- **A handwritten subset:** no dependency and exact positions, but it would fail on valid YAML that the real format allows, like flow maps and block scalars.

## Choice

`saphyr-parser`, walking its events and flattening each value into a path such as `colors.primary`.

## Security properties

- **No alias expansion.** A "billion laughs" file has nothing to multiply, because alias events are simply skipped.
- **Depth limit.** Nesting past 16 levels stops that block with a diagnostic. The spec needs about 3.
- **Size limit.** The whole file is capped at 256 KB before any parsing starts.

## Trade-offs

- **Pre-1.0 crate.** Its API may change between releases, but only `yaml.rs` touches it, so an upgrade or swap stays contained.
- **Aliases don't resolve.** A token written as `*anchor` is ignored. The spec's own reference form, `{colors.primary}`, is supported instead.
