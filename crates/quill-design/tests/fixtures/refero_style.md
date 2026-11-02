# Northwind Exchange

**Theme:** light

A black-and-white editorial system punctuated by a single high-voltage accent.

## Colors

| Name | Value | Token | Role |
|---|---|---|---|
| Highlighter Yellow | `#e4f222` | `--color-highlighter` | Primary CTA fill |
| Ink | `#0c0a08` | `--color-ink` | Primary text, headlines |
| Graphite | `#6d6c6b` | `--color-graphite` | Muted text, captions |
| Obsidian | `#1a1917` | `--color-obsidian` | Dark section backgrounds |

### Surfaces

| Name | Value | Token | Role |
|---|---|---|---|
| Paper | `#f4f2f0` | `--color-paper` | Page canvas |
| White | `#ffffff` | `--color-white` | Card surfaces |
| Fog | `#e5e7eb` | `--color-fog` | Hairline borders |

## Typography

### Lausanne

**Substitute:** Söhne, IBM Plex Sans, or Inter
**Weights:** 400

A single weight (400) forces hierarchy through size and tracking rather than weight contrast.

### Berkeley Mono

**Substitute:** JetBrains Mono

### Type Scale

| Role | Size | Line height | Weight |
|---|---|---|---|
| Display | 64px | 1.0 | 400 |
| Body | 16px | 1.5 | 400 |

## Shapes

| Name | Radius |
|---|---|
| Control | 6px |
| Input | 10px |
| Card | 16px |
| Pill | 9999px |

## Elevation

No box-shadow on anything. Borders do the elevation work.

## CSS Variables

```css
:root {
  --color-ink: #0c0a08;
  --color-paper: #f4f2f0;
  --color-highlighter: #e4f222;
  --radius-card: 16px;
  --section-gap: 64-128px;
  --page-max-width: 1200px;
}
```

## Quick Color Reference

- text: #0c0a08
- background: #f4f2f0
- card surface: #ffffff
- border: #e5e7eb
- muted text: #6d6c6b
- primary action: #e4f222 (filled action)

## Example Component Prompts

1. Hero headline: 64px weight 400, #0c0a08, on #f4f2f0 canvas.
2. Content card: #ffffff fill, 1px #e5e7eb border, 16px radius. No shadow.

## Do's and Don'ts

- Never bold.
- Use photography of real people, never illustrations.
- Keep sentences short.
