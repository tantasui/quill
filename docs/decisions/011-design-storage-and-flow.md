# 011: Storing and applying imported designs

## Context

Importing a design must never surprise anyone. A person has to see what the file produced and try it on real content before visitors see it, and be able to go back in one click.

## Options

- **Keep drafts in memory until applied.** No rows to clean up, but a restart or a second tab loses the draft, and the preview needs somewhere to read from.
- **Compile on every request from the stored source.** Always current, but a new Quill version could silently change how a live blog looks.
- **Store the source, the compiled result and the diagnostics together,** with a pointer to the live one.

## Choice

The third, in a `designs` table added by migration `0003`:

- **A row with no `applied_at` is a draft.** Starting another import replaces abandoned drafts, so reviewing many files leaves nothing behind.
- **A design with errors is never stored.** The form shows the diagnostics and keeps the text in the box for fixing.
- **A unique partial index allows one live design,** so the database itself rules out two.
- **The 20 most recent applied designs are kept as history.** Applying one again takes a single click.

**Public and preview stylesheets are separate routes.** `/static/tokens/design-<id>.css` only serves the live design, so a draft is never public. Previews use an admin route behind the login, which can also force light or dark regardless of the viewer's own setting.

**A design supplies tokens; the layout stays yours.** Applying a design keeps the current layout and preset stylesheet. "Back to the preset's colors" drops the design and keeps a customized layout. Choosing a preset replaces both, and the dashboard says so first.

**A live design that stops validating is ignored,** with a logged warning, and the blog falls back to its preset. Like stored layouts, this favors a blog that stays up.

## Trade-offs

- **The compiled result can go stale.** If a later version compiles better, an applied design doesn't benefit until it's imported again. That's deliberate: an upgrade shouldn't change how a blog looks. The source is stored, so re-importing stays possible.
- **The whole source is kept per design,** up to 256 KB across at most 21 rows plus one draft. That's small next to one uploaded image.
- **A preset's finishing touches stay on.** Editorial's italic metadata still applies under an imported design. This makes results depend a little on the preset, but drop caps and similar touches are what the preset is for.
