# 009: Bundling a small set of fonts

## Context

A DESIGN.md often names a commercial font such as Söhne or Lausanne. Quill's Content-Security-Policy forbids remote fonts, and loading them from a font service would tell a third party who reads each blog. Without any fonts of its own, every imported design would fall back to system fonts and lose much of its character.

## Options

- **System fonts only.** No binary growth, but designs lose their typography.
- **Let users upload fonts.** Flexible, but it brings licensing questions and untrusted font files into the dashboard.
- **Bundle a few open-licensed families,** mapped by a clear rule.

## Choice

Four families under the SIL Open Font License, embedded in the binary as variable Latin-subset WOFF2 files, about 290 KB in total:

| Family | Kind | Weights | Italic |
|---|---|---|---|
| Inter | Sans | 100–900 | Yes |
| IBM Plex Sans | Sans | 100–700 | No |
| Source Serif 4 | Serif | 200–900 | Yes |
| JetBrains Mono | Mono | 100–800 | No |

The files come from the `@fontsource-variable` npm packages, version 5.3, and each family's license is in `static/fonts/licenses/`, served alongside the fonts as the license requires. A test checks that every listed file exists, is really WOFF2, and has its license.

A requested font maps in this order:

1. A bundled family of that name.
2. A system font, named as written.
3. The file's listed substitute, if bundled, then if it's a system font.
4. The closest bundled family, guessed from the name: "Tiempos" is a serif, "Berkeley Mono" is monospace, and anything else is sans.

Every swap is reported with a diagnostic. Stylesheets only load the fonts a design uses, and built-in presets load none, so they look the same as before.

## Trade-offs

- **The binary grows by about 290 KB,** whether or not a blog imports a design. That's small next to the binary itself.
- **Latin only.** Other scripts fall back to system fonts through the font stack. Full character sets would add several megabytes.
- **Some italics are synthesized.** Browsers slant IBM Plex Sans and JetBrains Mono rather than using true italics, which saves about 90 KB.
- **The guess is only a guess.** A serif whose name gives no hint becomes Inter. The diagnostic says so, and a `Substitute:` line in the file fixes it.
