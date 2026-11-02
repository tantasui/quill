//! Derives a dark palette from a light one in OKLCH. Lightness is flipped and
//! hue kept, so the result stays recognizably the same design, then each color
//! is nudged until it passes the same contrast rules as a hand-written palette.

use super::palette::{readable_ink, MIN_CONTRAST};
use crate::{Color, Palette};

/// Raises lightness until `color` is readable on all of `backgrounds`.
fn lighten_until_readable(color: Color, backgrounds: &[Color]) -> Color {
    let (mut lightness, chroma, hue) = color.to_oklch();
    let mut candidate = color;
    while lightness < 1.0 {
        if backgrounds
            .iter()
            .all(|b| candidate.contrast_ratio(*b) >= MIN_CONTRAST)
        {
            return candidate;
        }
        lightness += 0.01;
        candidate = Color::from_oklch(lightness, chroma, hue);
    }
    Color::rgb(255, 255, 255)
}

pub(crate) fn derive(light: &Palette) -> Palette {
    let neutral = |color: Color, min: f64, max: f64| {
        let (l, c, h) = color.to_oklch();
        Color::from_oklch((1.0 - l).clamp(min, max), c * 0.8, h)
    };

    let bg = neutral(light.bg, 0.16, 0.24);
    let (bg_l, bg_c, bg_h) = bg.to_oklch();
    let surface = Color::from_oklch(bg_l + 0.04, bg_c, bg_h);
    let border = Color::from_oklch(bg_l + 0.12, bg_c, bg_h);
    let code_bg = Color::from_oklch(bg_l + 0.03, bg_c, bg_h);

    let text = lighten_until_readable(neutral(light.text, 0.86, 0.97), &[bg, surface]);
    let muted = lighten_until_readable(neutral(light.muted, 0.6, 0.8), &[bg, surface]);

    // Accents keep their lightness where they can, with slightly less chroma:
    // saturated colors look stronger on a dark page.
    let (l, c, h) = light.accent_fill.to_oklch();
    let accent = Color::from_oklch(l, c * 0.9, h);
    let accent_fill = if accent.contrast_ratio(bg) >= 3.0 {
        accent
    } else {
        Color::from_oklch(l.max(0.7), c * 0.9, h)
    };
    let accent_text = lighten_until_readable(accent_fill, &[bg]);
    let on_accent = [bg, text]
        .into_iter()
        .find(|c| c.contrast_ratio(accent_fill) >= MIN_CONTRAST)
        .unwrap_or_else(|| readable_ink(accent_fill));

    Palette {
        bg,
        surface,
        text,
        muted,
        border,
        accent_fill,
        on_accent,
        accent_text,
        code_bg,
        accent_soft: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn light(bg: &str, text: &str, muted: &str, accent: &str) -> Palette {
        let c = |hex| Color::parse(hex).unwrap();
        Palette {
            bg: c(bg),
            surface: c(bg).mix(c(text), 0.03),
            text: c(text),
            muted: c(muted),
            border: c(bg).mix(c(text), 0.15),
            accent_fill: c(accent),
            on_accent: readable_ink(c(accent)),
            accent_text: c(accent),
            code_bg: c(bg),
            accent_soft: None,
        }
    }

    fn assert_readable(p: &Palette, label: &str) {
        for (name, fg, bg) in [
            ("text on bg", p.text, p.bg),
            ("text on surface", p.text, p.surface),
            ("muted on bg", p.muted, p.bg),
            ("muted on surface", p.muted, p.surface),
            ("accent_text on bg", p.accent_text, p.bg),
            ("on_accent on fill", p.on_accent, p.accent_fill),
        ] {
            let ratio = fg.contrast_ratio(bg);
            assert!(ratio >= MIN_CONTRAST, "{label}: {name} is {ratio:.2}:1");
        }
    }

    #[test]
    fn stays_dark_and_readable_for_many_palettes() {
        let backgrounds = [
            "#ffffff", "#f4f2f0", "#fbfaf7", "#fff8e1", "#eef6ff", "#f0f0f0",
        ];
        let inks = ["#000000", "#0c0a08", "#22201c", "#123456", "#3b0a45"];
        let muteds = ["#6d6c6b", "#555555", "#5f5e5a", "#6b4f4f"];
        let accents = [
            "#e4f222", "#0b6bcb", "#9c3a1f", "#ff0000", "#00ff00", "#0000ff", "#ffffff", "#000000",
            "#808080", "#ff00ff", "#b3261e", "#111827",
        ];
        for bg in backgrounds {
            for ink in inks {
                for muted in muteds {
                    for accent in accents {
                        let label = format!("{bg} {ink} {muted} {accent}");
                        let dark = derive(&light(bg, ink, muted, accent));
                        assert_readable(&dark, &label);
                        let (lightness, _, _) = dark.bg.to_oklch();
                        assert!(lightness < 0.3, "{label}: background is {lightness}");
                    }
                }
            }
        }
    }

    #[test]
    fn keeps_the_hue_of_the_accent_and_background() {
        let source = light("#fff8e1", "#22201c", "#6d6c6b", "#0b6bcb");
        let dark = derive(&source);
        let hue = |c: Color| c.to_oklch().2;
        assert!((hue(dark.accent_fill) - hue(source.accent_fill)).abs() < 2.0);
        assert!(
            (hue(dark.bg) - hue(source.bg)).abs() < 25.0,
            "warm stays warm"
        );
    }

    #[test]
    fn a_dark_accent_is_lifted_so_it_can_be_seen() {
        let dark = derive(&light("#ffffff", "#111111", "#555555", "#111827"));
        assert!(dark.accent_fill.contrast_ratio(dark.bg) >= 3.0);
    }

    #[test]
    fn keeps_a_bright_accent_as_it_is() {
        let source = light("#f4f2f0", "#0c0a08", "#6d6c6b", "#e4f222");
        let dark = derive(&source);
        assert!(dark.accent_text.contrast_ratio(dark.bg) >= MIN_CONTRAST);
        let (before, after) = (
            source.accent_fill.to_oklch().0,
            dark.accent_fill.to_oklch().0,
        );
        assert!((before - after).abs() < 0.03, "{before} became {after}");
    }
}
