# 008: Deriving a dark palette

## Context

A blog's readers can prefer dark mode, but most DESIGN.md files describe one look. Quill needs a dark palette for every design without shipping unreadable text.

## Options

- **Invert in sRGB.** Simple, but it changes hues. Inverting a blue link gives orange.
- **Invert in HSL.** Keeps hue, but HSL lightness isn't perceptual: yellow and blue at the same "lightness" look very different, so contrast is unpredictable.
- **Flip lightness in OKLCH.** OKLCH lightness tracks what people see, and hue stays put.
- **Offer no dark mode for imported designs.** Honest, but a regression from the built-in presets.

## Choice

OKLCH, in two steps:

1. **Flip neutrals.** Lightness is inverted and clamped: backgrounds to 0.16–0.24, so they stay off pure black, and text to 0.86–0.97. Hue is kept and chroma reduced to 80%, so a warm paper becomes a warm charcoal.
2. **Keep accents.** An accent keeps its lightness and hue, with 90% chroma, because saturated colors look stronger on dark pages. Accents too dark to see are lifted.

Then text, muted text and link colors have their lightness raised until they pass 4.5:1. `on_accent` is chosen by contrast. A test runs 1,440 palette combinations through this and checks every rule.

Explicit dark tokens always win. A partial dark palette keeps what the file gives and fills in the rest from the derived one. If explicit dark tokens have unreadable text, the whole derived palette is used instead and a warning explains why.

A design that is dark by nature, with no light tokens, uses its palette for both schemes rather than inventing a light version the designer never made.

## Trade-offs

- **It's a good default, not a designer's palette.** The diagnostics say a palette was derived and list its values, so anyone who wants more can add dark tokens to the file.
- **Sub-4.5:1 borders are allowed.** WCAG's text rule doesn't apply to decorative borders.
- **Rounding.** Colors are stored as 8-bit sRGB, so values shift slightly through OKLCH. Contrast is always checked on the final 8-bit color.
