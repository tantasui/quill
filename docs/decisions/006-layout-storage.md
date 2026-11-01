# 006: Storing the customized layout

## Context

Someone who rearranges their layout needs that to survive restarts and upgrades. Blogs upgrading from 0.1 have no stored layout and must look unchanged.

## Options

- **A key in the existing `settings` table.** No migration, but a nested JSON document doesn't fit a table of simple values, and it gets loaded with every setting.
- **Normalized tables**, with a row per section and option. Queryable, but nothing needs to query it, and each new option would need a migration.
- **One JSON document in its own table**, limited to a single row by `CHECK (id = 1)`.

## Choice

The JSON document in a `site_layout` table, added by migration `0002`. No row means "use the preset's own layout", so 0.1 blogs need no data migration at all.

The row records which preset it was customized from. Choosing a preset deletes the row, so picking a preset always gives that preset's layout.

## Failure behavior

A stored layout that no longer validates, perhaps written by a newer version before a downgrade, logs a warning and is ignored. The blog falls back to the preset's layout rather than failing to start, and the bad row stays in place until the next save. A blog staying up matters more than one customization.

## Trade-offs

- **Choosing a preset discards customizations.** The dashboard asks for confirmation first. Named saved layouts would avoid this, but nothing in 0.2 needs them.
- **JSON validity isn't checked by the database.** Validation happens in `quill-design` when the row is read, which keeps the rules in one place.
- **Layout changes aren't versioned.** Milestone 2's `designs` table keeps history for designs; layouts have only their current value.
