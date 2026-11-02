//! Turns the colors a file provides into a complete, readable palette. Missing
//! roles are filled in, and the WCAG contrast rules are enforced here.

use std::collections::HashMap;

use super::diagnostic::{Code, Diagnostic, Span};
use super::extract::Scheme;
use super::roles::Role;
use crate::{Color, Palette};

/// WCAG 2.x AA for body text.
pub(crate) const MIN_CONTRAST: f64 = 4.5;

const WHITE: Color = Color::rgb(255, 255, 255);
const BLACK: Color = Color::rgb(0, 0, 0);

#[derive(Clone, Copy, Debug)]
pub(crate) struct Slot {
    pub color: Color,
    pub span: Span,
}

pub(crate) type Slots = HashMap<Role, Slot>;

/// Builds one palette. `fallback` supplies roles the file doesn't set. Returns
/// `None`, having reported why, if the main text can't be read on the background.
pub(crate) fn build(
    slots: &Slots,
    scheme: Scheme,
    fallback: Option<&Palette>,
    failure: Code,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<Palette> {
    let label = match scheme {
        Scheme::Light => "",
        Scheme::Dark => " (dark)",
    };
    let mut derived: Vec<&str> = Vec::new();
    let get = |role: Role| slots.get(&role).copied();

    let default_bg = match scheme {
        Scheme::Light => WHITE,
        Scheme::Dark => Color::rgb(0x11, 0x11, 0x11),
    };
    let bg = match get(Role::Bg) {
        Some(slot) => {
            flatten_note(slot, Role::Bg, default_bg, diagnostics);
            slot.color.flatten_over(default_bg)
        }
        None => {
            derived.push("bg");
            fallback.map_or(default_bg, |p| p.bg)
        }
    };
    let flat = |role: Role, diagnostics: &mut Vec<Diagnostic>| {
        get(role).map(|slot| {
            flatten_note(slot, role, bg, diagnostics);
            slot.color.flatten_over(bg)
        })
    };

    let text = match flat(Role::Text, diagnostics) {
        Some(text) => text,
        None => {
            derived.push("text");
            let candidate = fallback
                .map(|p| p.text)
                .filter(|t| t.contrast_ratio(bg) >= MIN_CONTRAST);
            candidate.unwrap_or_else(|| readable_ink(bg))
        }
    };
    let ratio = text.contrast_ratio(bg);
    if ratio < MIN_CONTRAST {
        diagnostics.push(
            Diagnostic::new(
                failure,
                format!(
                    "text {text} on background {bg}{label} has a contrast of {ratio:.2}:1, below the {MIN_CONTRAST}:1 needed to read comfortably"
                ),
            )
            .at_opt(get(Role::Text).map(|s| s.span))
            .help("Make the text darker or the background lighter (or the reverse for a dark design)."),
        );
        return None;
    }

    let mut surface = match flat(Role::Surface, diagnostics) {
        Some(surface) => surface,
        None => {
            derived.push("surface");
            fallback.map_or_else(|| bg.mix(text, 0.03), |p| p.surface)
        }
    };
    let ratio = text.contrast_ratio(surface);
    if ratio < MIN_CONTRAST {
        diagnostics.push(
            Diagnostic::new(
                Code::SurfaceContrast,
                format!(
                    "text {text} on surface {surface}{label} has a contrast of {ratio:.2}:1, so cards use the page background {bg} instead"
                ),
            )
            .at_opt(get(Role::Surface).map(|s| s.span))
            .help("Choose a surface color closer to the background."),
        );
        surface = bg;
    }

    let border = flat(Role::Border, diagnostics).unwrap_or_else(|| {
        derived.push("border");
        fallback.map_or_else(|| bg.mix(text, 0.15), |p| p.border)
    });

    let muted_given = flat(Role::Muted, diagnostics);
    let muted_start = muted_given.unwrap_or_else(|| {
        derived.push("muted");
        fallback.map_or_else(|| bg.mix(text, 0.6), |p| p.muted)
    });
    let muted = until_readable(muted_start, text, &[bg, surface]);
    if muted != muted_start && muted_given.is_some() {
        let ratio = muted_start.contrast_ratio(bg);
        diagnostics.push(
            Diagnostic::new(
                Code::MutedContrast,
                format!(
                    "muted text {muted_start}{label} has a contrast of {ratio:.2}:1 on {bg}, so it was changed to {muted}"
                ),
            )
            .at_opt(get(Role::Muted).map(|s| s.span))
            .help(format!("Use a color with at least {MIN_CONTRAST}:1 contrast to keep your own value.")),
        );
    }

    // A file that names a link color but no fill has told us its accent.
    let fill_given = flat(Role::AccentFill, diagnostics);
    let link_given = flat(Role::AccentText, diagnostics);
    let accent_fill = fill_given.or(link_given).unwrap_or_else(|| {
        derived.push("accent_fill");
        fallback.map_or(text, |p| p.accent_fill)
    });

    let accent_candidate = link_given.unwrap_or(accent_fill);
    let accent_text = if accent_candidate.contrast_ratio(bg) >= MIN_CONTRAST {
        accent_candidate
    } else {
        let ratio = accent_candidate.contrast_ratio(bg);
        diagnostics.push(
            Diagnostic::new(
                Code::AccentNotText,
                format!(
                    "accent {accent_candidate}{label} is only {ratio:.2}:1 as text on {bg}, so it's used as a fill only and links use the text color {text} with an underline"
                ),
            )
            .at_opt(get(Role::AccentText).or(get(Role::AccentFill)).map(|s| s.span))
            .help("This is normal for a bright accent. Set a separate link color to style links."),
        );
        text
    };

    let on_given = flat(Role::OnAccent, diagnostics);
    let on_accent = match on_given.filter(|c| c.contrast_ratio(accent_fill) >= MIN_CONTRAST) {
        Some(on_accent) => on_accent,
        None => {
            let chosen = [text, bg, BLACK, WHITE]
                .into_iter()
                .find(|c| c.contrast_ratio(accent_fill) >= MIN_CONTRAST)
                .unwrap_or_else(|| readable_ink(accent_fill));
            if on_given.is_some() {
                diagnostics.push(
                    Diagnostic::new(
                        Code::OnAccentChosen,
                        format!(
                            "text on the accent fill{label} didn't have enough contrast, so it uses {chosen} ({:.2}:1 on {accent_fill})",
                            chosen.contrast_ratio(accent_fill)
                        ),
                    )
                    .at_opt(get(Role::OnAccent).map(|s| s.span)),
                );
            }
            chosen
        }
    };

    let code_bg =
        flat(Role::CodeBg, diagnostics).unwrap_or_else(|| fallback.map_or(surface, |p| p.code_bg));

    if !derived.is_empty() && fallback.is_none() {
        diagnostics.push(Diagnostic::new(
            Code::RoleDerived,
            format!(
                "the file doesn't set {}{label}, so {} worked out from its other colors",
                derived.join(", "),
                if derived.len() == 1 {
                    "it was"
                } else {
                    "they were"
                }
            ),
        ));
    }

    Some(Palette {
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
    })
}

fn flatten_note(slot: Slot, role: Role, over: Color, diagnostics: &mut Vec<Diagnostic>) {
    if !slot.color.is_opaque() {
        diagnostics.push(
            Diagnostic::new(
                Code::AlphaFlattened,
                format!(
                    "{} {} is transparent, so it was flattened to {} over {over} to check its contrast",
                    role.name(),
                    slot.color,
                    slot.color.flatten_over(over)
                ),
            )
            .at(slot.span),
        );
    }
}

/// Black or white, whichever reads better. One of them always passes AA.
pub(crate) fn readable_ink(on: Color) -> Color {
    let dark = Color::rgb(0x11, 0x11, 0x11);
    let light = Color::rgb(0xf5, 0xf5, 0xf5);
    if dark.contrast_ratio(on) >= light.contrast_ratio(on) {
        dark
    } else {
        light
    }
}

/// Mixes `start` toward `target` until it's readable on all of `backgrounds`.
pub(crate) fn until_readable(start: Color, target: Color, backgrounds: &[Color]) -> Color {
    let passes = |c: Color| {
        backgrounds
            .iter()
            .all(|b| c.contrast_ratio(*b) >= MIN_CONTRAST)
    };
    if passes(start) {
        return start;
    }
    (1..=20)
        .map(|step| start.mix(target, f64::from(step) * 0.05))
        .find(|c| passes(*c))
        .unwrap_or(target)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SPAN: Span = Span {
        line: 1,
        column: 1,
        len: 1,
    };

    fn slots(pairs: &[(Role, &str)]) -> Slots {
        pairs
            .iter()
            .map(|(role, hex)| {
                let slot = Slot {
                    color: Color::parse(hex).unwrap(),
                    span: SPAN,
                };
                (*role, slot)
            })
            .collect()
    }

    fn build_light(pairs: &[(Role, &str)]) -> (Option<Palette>, Vec<Diagnostic>) {
        let mut diagnostics = Vec::new();
        let palette = build(
            &slots(pairs),
            Scheme::Light,
            None,
            Code::TextContrast,
            &mut diagnostics,
        );
        (palette, diagnostics)
    }

    fn codes(diagnostics: &[Diagnostic]) -> Vec<Code> {
        diagnostics.iter().map(|d| d.code).collect()
    }

    fn hex(color: Color) -> String {
        color.to_css()
    }

    /// The case from the plan: Ramp's yellow is 1.1:1 as text but 16:1 as a fill
    /// behind its ink.
    #[test]
    fn a_bright_accent_becomes_fill_only() {
        let (palette, diagnostics) = build_light(&[
            (Role::Bg, "#f4f2f0"),
            (Role::Text, "#0c0a08"),
            (Role::AccentFill, "#e4f222"),
        ]);
        let palette = palette.unwrap();
        assert_eq!(hex(palette.accent_fill), "#e4f222");
        assert_eq!(
            hex(palette.accent_text),
            "#0c0a08",
            "links use the text color"
        );
        assert_eq!(hex(palette.on_accent), "#0c0a08");
        assert!(palette.on_accent.contrast_ratio(palette.accent_fill) > 15.0);
        assert!(codes(&diagnostics).contains(&Code::AccentNotText));
        let note = diagnostics
            .iter()
            .find(|d| d.code == Code::AccentNotText)
            .unwrap();
        assert!(note.message.contains("1.1"), "{}", note.message);
    }

    #[test]
    fn a_readable_accent_is_used_for_links() {
        let (palette, diagnostics) = build_light(&[
            (Role::Bg, "#ffffff"),
            (Role::Text, "#111111"),
            (Role::AccentFill, "#1f5fbf"),
        ]);
        let palette = palette.unwrap();
        assert_eq!(hex(palette.accent_text), "#1f5fbf");
        assert_eq!(
            hex(palette.on_accent),
            "#111111".replace("111111", "ffffff")
        );
        assert!(!codes(&diagnostics).contains(&Code::AccentNotText));
    }

    #[test]
    fn unreadable_text_is_an_error() {
        let (palette, diagnostics) = build_light(&[(Role::Bg, "#ffffff"), (Role::Text, "#cccccc")]);
        assert!(palette.is_none());
        assert_eq!(diagnostics[0].code, Code::TextContrast);
        assert_eq!(diagnostics[0].span, Some(SPAN));
        assert!(
            diagnostics[0].message.contains("1.61"),
            "{}",
            diagnostics[0].message
        );
    }

    #[test]
    fn exactly_four_point_five_is_enough() {
        // #767676 on white is the well-known AA boundary color: about 4.54:1.
        let (palette, _) = build_light(&[(Role::Bg, "#ffffff"), (Role::Text, "#767676")]);
        assert!(palette.is_some());
        let (palette, _) = build_light(&[(Role::Bg, "#ffffff"), (Role::Text, "#777777")]);
        assert!(palette.is_none(), "#777777 is 4.48:1");
    }

    #[test]
    fn a_low_contrast_surface_falls_back_to_the_background() {
        let (palette, diagnostics) = build_light(&[
            (Role::Bg, "#ffffff"),
            (Role::Text, "#555555"),
            (Role::Surface, "#888888"),
        ]);
        assert_eq!(palette.unwrap().surface, Color::rgb(255, 255, 255));
        assert!(codes(&diagnostics).contains(&Code::SurfaceContrast));
    }

    #[test]
    fn low_contrast_muted_text_is_adjusted() {
        let (palette, diagnostics) = build_light(&[
            (Role::Bg, "#ffffff"),
            (Role::Text, "#111111"),
            (Role::Muted, "#bbbbbb"),
        ]);
        let palette = palette.unwrap();
        assert!(palette.muted.contrast_ratio(palette.bg) >= MIN_CONTRAST);
        assert_ne!(hex(palette.muted), "#bbbbbb");
        assert!(codes(&diagnostics).contains(&Code::MutedContrast));
    }

    #[test]
    fn missing_roles_are_derived_and_reported() {
        let (palette, diagnostics) = build_light(&[(Role::Bg, "#fafafa"), (Role::Text, "#222222")]);
        let palette = palette.unwrap();
        assert!(palette.muted.contrast_ratio(palette.bg) >= MIN_CONTRAST);
        assert_eq!(palette.accent_fill, palette.text);
        let note = diagnostics
            .iter()
            .find(|d| d.code == Code::RoleDerived)
            .unwrap();
        assert!(
            note.message.contains("surface, border, muted, accent_fill"),
            "{}",
            note.message
        );
    }

    #[test]
    fn transparent_text_is_flattened_first() {
        let (palette, diagnostics) = build_light(&[
            (Role::Bg, "#ffffff"),
            (Role::Text, "#000000"),
            (Role::Border, "rgba(0, 0, 0, 0.1)"),
        ]);
        let palette = palette.unwrap();
        assert!(palette.border.is_opaque());
        assert_eq!(hex(palette.border), "#e5e5e5");
        assert!(codes(&diagnostics).contains(&Code::AlphaFlattened));
    }

    #[test]
    fn a_link_color_serves_as_the_accent_when_no_fill_is_named() {
        let (palette, _) = build_light(&[
            (Role::Bg, "#ffffff"),
            (Role::Text, "#111111"),
            (Role::AccentText, "#0b57d0"),
        ]);
        let palette = palette.unwrap();
        assert_eq!(hex(palette.accent_fill), "#0b57d0");
        assert_eq!(hex(palette.accent_text), "#0b57d0");
    }

    #[test]
    fn a_given_on_accent_is_kept_only_if_readable() {
        let (palette, _) = build_light(&[
            (Role::Bg, "#ffffff"),
            (Role::Text, "#111111"),
            (Role::AccentFill, "#1f5fbf"),
            (Role::OnAccent, "#eeeeee"),
        ]);
        assert_eq!(hex(palette.unwrap().on_accent), "#eeeeee");
        let (palette, diagnostics) = build_light(&[
            (Role::Bg, "#ffffff"),
            (Role::Text, "#111111"),
            (Role::AccentFill, "#1f5fbf"),
            (Role::OnAccent, "#2060c0"),
        ]);
        let palette = palette.unwrap();
        assert!(palette.on_accent.contrast_ratio(palette.accent_fill) >= MIN_CONTRAST);
        assert!(codes(&diagnostics).contains(&Code::OnAccentChosen));
    }
}
