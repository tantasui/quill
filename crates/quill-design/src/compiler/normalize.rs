//! Turns raw extracted text into validated values, picks between sources, and
//! decides what each color is for.

use std::collections::HashMap;
use std::ops::RangeInclusive;

use super::dark;
use super::diagnostic::{Code, Diagnostic, Span};
use super::extract::{Extracted, Origin, Scheme, Token};
use super::palette::{self, Slot, Slots};
use super::prose::Rules;
use super::roles::{self, Mapping, Role};
use super::TokenOrigin;
use crate::{Color, FontWeight, Length, Palette, Shadow};

const DEFAULT_RADIUS: &str = "6px";
const DEFAULT_MEASURE: &str = "40rem";
const MEASURE_PX: RangeInclusive<f32> = 320.0..=1200.0;
const PAGE_WIDTH_PX: RangeInclusive<f32> = 640.0..=1800.0;
/// Radii this large mean "pill", not the system's general corner.
const PILL_RADIUS_PX: f32 = 100.0;
/// Below this OKLCH chroma a color reads as a neutral gray.
const NEUTRAL_CHROMA: f64 = 0.04;
/// Colors can reference each other; this stops a loop.
const MAX_REFERENCE_DEPTH: usize = 8;
/// Scale names that mean "the normal one".
const DEFAULT_LEVELS: &[&str] = &[
    "md", "default", "base", "medium", "card", "radius", "shadow",
];

fn short(name: &str) -> String {
    let name = name.trim();
    if name.chars().count() <= 40 {
        name.to_string()
    } else {
        let cut: String = name.chars().take(40).collect();
        format!("{}…", cut.trim_end())
    }
}

#[derive(Debug)]
struct Pending {
    name: String,
    color: Color,
    span: Span,
    scheme: Scheme,
    /// A palette slot such as `primary`, rather than a name that means nothing.
    slot_name: bool,
}

#[derive(Debug, Default)]
pub(crate) struct Colors {
    pub light: Slots,
    pub dark: Slots,
    pub origins: Vec<TokenOrigin>,
    unused: Vec<(String, Span)>,
}

impl Colors {
    fn slots(&self, scheme: Scheme) -> &Slots {
        match scheme {
            Scheme::Light => &self.light,
            Scheme::Dark => &self.dark,
        }
    }

    fn record(&mut self, role: Role, scheme: Scheme, slot: Slot) {
        let slots = match scheme {
            Scheme::Light => &mut self.light,
            Scheme::Dark => &mut self.dark,
        };
        slots.insert(role, slot);
        let suffix = if scheme == Scheme::Dark {
            " (dark)"
        } else {
            ""
        };
        self.origins.push(TokenOrigin {
            token: format!("{}{suffix}", role.name()),
            line: slot.span.line,
        });
    }
}

/// Follows `{colors.primary}` and `var(--ink)` to the value they point at. The
/// lookup table keeps this linear: a file full of references can't make it slow.
fn resolve_reference<'a>(
    value: &'a str,
    by_name: &HashMap<&'a str, &'a str>,
) -> Result<&'a str, &'static str> {
    let mut current = value.trim();
    for _ in 0..MAX_REFERENCE_DEPTH {
        let target = if let Some(path) = current.strip_prefix('{').and_then(|v| v.strip_suffix('}'))
        {
            path.rsplit('.').next().unwrap_or(path)
        } else if let Some(inner) = current
            .strip_prefix("var(")
            .and_then(|v| v.strip_suffix(')'))
        {
            inner.split(',').next().unwrap_or(inner).trim()
        } else {
            return Ok(current);
        };
        current = by_name
            .get(target)
            .map(|value| value.trim())
            .ok_or("it refers to a token this file doesn't define")?;
    }
    Err("its references go in a circle")
}

pub(crate) fn colors(extracted: &Extracted, diagnostics: &mut Vec<Diagnostic>) -> Colors {
    let mut ordered: Vec<&Token> = extracted.colors.iter().collect();
    ordered.sort_by_key(|t| t.origin);

    let mut by_name: HashMap<&str, &str> = HashMap::new();
    for token in &ordered {
        by_name
            .entry(token.name.as_str())
            .or_insert(token.value.as_str());
    }

    let mut colors = Colors::default();
    let mut pending: Vec<Pending> = Vec::new();
    let mut winners: HashMap<(Role, Scheme), (String, Origin, u32)> = HashMap::new();

    for token in ordered {
        let name = short(&token.name);
        let parsed = match resolve_reference(&token.value, &by_name) {
            Ok(value) => Color::parse(value).map_err(|error| error.to_string()),
            Err(reason) => Err(reason.to_string()),
        };
        let color = match parsed {
            Ok(color) => color,
            Err(reason) => {
                diagnostics.push(
                    Diagnostic::new(
                        Code::InvalidValue,
                        format!(
                            "`{name}` has an invalid value `{}`, so it was ignored",
                            token.value
                        ),
                    )
                    .at(token.span)
                    .help(reason),
                );
                continue;
            }
        };
        let mut mapping = roles::map(&token.name);
        if mapping == Mapping::Ambiguous {
            let described = extracted.descriptions.get(&roles::words(&token.name));
            if let Some(Mapping::Role(role)) = described.map(|d| roles::map(d)) {
                mapping = Mapping::Role(role);
            }
        }
        match mapping {
            Mapping::Role(role) => {
                let key = (role, token.scheme);
                if let Some((value, origin, line)) = winners.get(&key) {
                    // Several rows for one role, such as a surface scale, is
                    // normal. Two different sources disagreeing is worth saying.
                    if color.to_css() != *value && token.origin != *origin {
                        diagnostics.push(
                            Diagnostic::new(
                                Code::Conflict,
                                format!(
                                    "`{name}` sets {} to {color}, but line {line} already set it to {value}, which wins",
                                    role.name()
                                ),
                            )
                            .at(token.span),
                        );
                    }
                    continue;
                }
                winners.insert(key, (color.to_css(), token.origin, token.span.line));
                let slot = Slot {
                    color,
                    span: token.span,
                };
                colors.record(role, token.scheme, slot);
            }
            Mapping::Ambiguous | Mapping::Unknown => pending.push(Pending {
                name,
                color,
                span: token.span,
                scheme: token.scheme,
                slot_name: mapping == Mapping::Ambiguous,
            }),
            Mapping::Unused => colors.unused.push((name, token.span)),
        }
    }

    // Palette slots such as `primary` first, then any other names.
    let dark_theme = extracted.theme == Some(Scheme::Dark);
    infer_roles(&mut pending, true, dark_theme, &mut colors, diagnostics);
    infer_roles(&mut pending, false, dark_theme, &mut colors, diagnostics);
    colors
        .unused
        .extend(pending.into_iter().map(|p| (p.name, p.span)));
    colors
}

/// Fills roles names couldn't settle by looking at the colors themselves: the
/// most colorful is the accent, and neutrals go by lightness. Inferred colors
/// are removed from `pending`.
fn infer_roles(
    pending: &mut Vec<Pending>,
    slot_names: bool,
    dark_theme: bool,
    colors: &mut Colors,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for scheme in [Scheme::Light, Scheme::Dark] {
        let dark_palette = dark_theme || scheme == Scheme::Dark;
        let lightness = |p: &Pending| p.color.to_oklch().0;
        let chroma = |p: &Pending| p.color.to_oklch().1;
        let eligible = |p: &Pending| p.slot_name == slot_names && p.scheme == scheme;
        let is_neutral = |p: &Pending| chroma(p) < NEUTRAL_CHROMA;

        // For each open role: which colors qualify, and a score to maximize.
        type Rule<'a> = (
            Role,
            &'a dyn Fn(&Pending) -> bool,
            &'a dyn Fn(&Pending) -> f64,
            &'a str,
        );
        // The spec's convention: `neutral` is the page color even when it's
        // tinted, like a wheat paper, and a colorful `primary` is the key color.
        let named = |p: &Pending, word: &str| roles::words(&p.name).contains(&format!(" {word} "));
        let bg_ok = |p: &Pending| {
            (is_neutral(p) || named(p, "neutral"))
                && if dark_palette {
                    lightness(p) < 0.35
                } else {
                    lightness(p) > 0.8
                }
        };
        let text_ok = |p: &Pending| {
            is_neutral(p)
                && if dark_palette {
                    lightness(p) > 0.7
                } else {
                    lightness(p) < 0.45
                }
        };
        let muted_ok = |p: &Pending| is_neutral(p) && (0.35..=0.75).contains(&lightness(p));
        let accent_ok = |p: &Pending| chroma(p) >= NEUTRAL_CHROMA;
        let outermost = |p: &Pending| {
            let bonus = if named(p, "neutral") { 10.0 } else { 0.0 };
            bonus
                + if dark_palette {
                    -lightness(p)
                } else {
                    lightness(p)
                }
        };
        let innermost = |p: &Pending| {
            if dark_palette {
                lightness(p)
            } else {
                -lightness(p)
            }
        };
        let closest_to_middle = |p: &Pending| -(lightness(p) - 0.55).abs();
        let key_color = |p: &Pending| {
            let bonus = if named(p, "primary") { 10.0 } else { 0.0 };
            bonus + chroma(p)
        };
        let rules: [Rule; 4] = [
            (
                Role::AccentFill,
                &accent_ok,
                &key_color,
                "it's the key color",
            ),
            (Role::Bg, &bg_ok, &outermost, "it's the outermost neutral"),
            (
                Role::Text,
                &text_ok,
                &innermost,
                "it contrasts most with the background",
            ),
            (
                Role::Muted,
                &muted_ok,
                &closest_to_middle,
                "it's a mid-tone neutral",
            ),
        ];

        for (role, qualifies, score, why) in rules {
            if colors.slots(scheme).contains_key(&role) {
                continue;
            }
            let best = pending
                .iter()
                .enumerate()
                .filter(|(_, p)| eligible(p) && qualifies(p))
                .max_by(|(_, a), (_, b)| score(a).total_cmp(&score(b)))
                .map(|(index, _)| index);
            let Some(index) = best else { continue };
            let chosen = pending.remove(index);
            diagnostics.push(
                Diagnostic::new(
                    Code::RoleInferred,
                    format!(
                        "`{}` ({}) is used as {} because {why}",
                        chosen.name,
                        chosen.color,
                        role.name()
                    ),
                )
                .at(chosen.span)
                .help("Describe what the color is for in the file to control this."),
            );
            let slot = Slot {
                color: chosen.color,
                span: chosen.span,
            };
            colors.record(role, scheme, slot);
        }
    }
}

/// Builds both palettes, or reports why it can't.
pub(crate) fn palettes(
    colors: &Colors,
    theme: Option<Scheme>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<(Palette, Palette)> {
    if colors.light.is_empty() && colors.dark.is_empty() {
        diagnostics.push(
            Diagnostic::new(Code::NoTokens, "no colors were found in this file").help(
                "Quill reads YAML front matter, a CSS :root block, color tables, and lists like `- text: #111111`.",
            ),
        );
        return None;
    }

    let background_is_dark = colors
        .light
        .get(&Role::Bg)
        .is_some_and(|slot| slot.color.to_oklch().0 < 0.4);
    let dark_design = theme == Some(Scheme::Dark) || (theme.is_none() && background_is_dark);

    if colors.light.is_empty() || (dark_design && colors.dark.is_empty()) {
        let slots = if colors.dark.is_empty() {
            &colors.light
        } else {
            &colors.dark
        };
        let palette = palette::build(slots, Scheme::Dark, None, Code::TextContrast, diagnostics)?;
        diagnostics.push(Diagnostic::new(
            Code::DarkOnly,
            "this is a dark design with no light colors, so readers see it dark whatever their preference",
        ).help("Add light tokens to offer both."));
        return Some((palette.clone(), palette));
    }

    let light = palette::build(
        &colors.light,
        Scheme::Light,
        None,
        Code::TextContrast,
        diagnostics,
    )?;
    let derived = dark::derive(&light);
    let dark = if colors.dark.is_empty() {
        report_derived(&derived, diagnostics);
        derived
    } else {
        match palette::build(
            &colors.dark,
            Scheme::Dark,
            Some(&derived),
            Code::DarkPaletteRejected,
            diagnostics,
        ) {
            Some(palette) => palette,
            None => {
                report_derived(&derived, diagnostics);
                derived
            }
        }
    };
    Some((light, dark))
}

fn report_derived(palette: &Palette, diagnostics: &mut Vec<Diagnostic>) {
    diagnostics.push(
        Diagnostic::new(
            Code::DarkDerived,
            format!(
                "dark palette derived from the light one: bg {}, surface {}, text {}, muted {}, border {}, accent fill {}, accent text {}",
                palette.bg,
                palette.surface,
                palette.text,
                palette.muted,
                palette.border,
                palette.accent_fill,
                palette.accent_text
            ),
        )
        .help("Add dark tokens to the file to set these yourself."),
    );
}

/// Sizes, weights and shadows. Each has a sensible default, so a file only
/// needs to state what it cares about.
#[derive(Debug)]
pub(crate) struct Scalars {
    pub heading_weight: Option<FontWeight>,
    pub radius: Length,
    pub measure: Length,
    pub shadow: Option<Shadow>,
    pub page_width: Option<Length>,
    pub origins: Vec<TokenOrigin>,
}

/// Parses each token, reporting those that fail, in source-precedence order.
fn parse_all<'a, T>(
    tokens: &'a [Token],
    parse: impl Fn(&str) -> Result<T, crate::TokenError>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Vec<(&'a Token, T)> {
    let mut ordered: Vec<&Token> = tokens.iter().collect();
    ordered.sort_by_key(|t| t.origin);
    let mut parsed = Vec::new();
    for token in ordered {
        // A reference to another scale isn't a mistake, but isn't a value either.
        if token.value.starts_with('{') || token.value.starts_with("var(") {
            continue;
        }
        match parse(&token.value) {
            Ok(value) => parsed.push((token, value)),
            Err(error) => diagnostics.push(
                Diagnostic::new(
                    Code::InvalidValue,
                    format!(
                        "`{}` has an invalid value `{}`, so it was ignored",
                        short(&token.name),
                        token.value
                    ),
                )
                .at(token.span)
                .help(error.to_string()),
            ),
        }
    }
    parsed
}

fn is_default_level(name: &str) -> bool {
    let padded = roles::words(name);
    DEFAULT_LEVELS
        .iter()
        .any(|level| padded.contains(&format!(" {level} ")))
}

pub(crate) fn scalars(
    extracted: &Extracted,
    rules: &Rules,
    diagnostics: &mut Vec<Diagnostic>,
) -> Scalars {
    let mut origins = Vec::new();
    let mut note = |token: &str, line: u32| {
        origins.push(TokenOrigin {
            token: token.to_string(),
            line,
        });
    };

    // A stated rule outranks a token: "no shadows" means no shadows.
    let weights = parse_all(&extracted.heading_weights, FontWeight::parse, diagnostics);
    let heading_weight = rules.heading_weight.or_else(|| {
        weights.first().map(|(token, weight)| {
            note("heading_weight", token.span.line);
            *weight
        })
    });

    let radii = parse_all(&extracted.radii, Length::parse, diagnostics);
    let usable: Vec<_> = radii
        .iter()
        .filter(|(_, r)| r.to_px().is_none_or(|px| px < PILL_RADIUS_PX))
        .collect();
    let radius = if rules.square_corners {
        Length::parse("0").expect("0 is valid")
    } else {
        // The level called "default" or "md", otherwise the middle of the scale.
        let by_name = usable.iter().find(|(t, _)| is_default_level(&t.name));
        let mut by_size = usable.clone();
        by_size.sort_by(|a, b| {
            a.1.to_px()
                .unwrap_or(0.0)
                .total_cmp(&b.1.to_px().unwrap_or(0.0))
        });
        let middle = by_size.get(by_size.len() / 2).copied();
        match by_name.copied().or(middle) {
            Some((token, radius)) => {
                note("radius", token.span.line);
                *radius
            }
            None => Length::parse(DEFAULT_RADIUS).expect("the default is valid"),
        }
    };

    let shadows = parse_all(&extracted.shadows, Shadow::parse, diagnostics);
    let shadow = if rules.no_shadow {
        Some(Shadow::None)
    } else {
        let chosen = shadows
            .iter()
            .find(|(t, s)| is_default_level(&t.name) && *s != Shadow::None)
            .or_else(|| shadows.iter().find(|(_, s)| *s != Shadow::None))
            .or_else(|| shadows.first());
        chosen.map(|(token, shadow)| {
            note("shadow", token.span.line);
            *shadow
        })
    };

    let measures = parse_all(&extracted.measures, Length::parse, diagnostics);
    let mut measure = Length::parse(DEFAULT_MEASURE).expect("the default is valid");
    for (token, value) in &measures {
        if value.to_px().is_some_and(|px| MEASURE_PX.contains(&px)) {
            note("measure", token.span.line);
            measure = *value;
            break;
        }
        diagnostics.push(
            Diagnostic::new(
                Code::InvalidValue,
                format!(
                    "a reading width of `{}` won't work for articles, so it was ignored",
                    token.value
                ),
            )
            .at(token.span)
            .help("Use a fixed size from 20rem to 75rem. Around 40rem reads best."),
        );
    }

    let widths = parse_all(&extracted.page_widths, Length::parse, diagnostics);
    let stated = widths
        .first()
        .map(|(token, value)| (*value, token.span))
        .or(rules.page_width);
    let mut page_width = None;
    if let Some((value, span)) = stated {
        if value.to_px().is_some_and(|px| PAGE_WIDTH_PX.contains(&px)) {
            note("page_width", span.line);
            page_width = Some(value);
        } else {
            diagnostics.push(
                Diagnostic::new(
                    Code::PageWidthIgnored,
                    format!(
                        "a page width of {value} doesn't fit Quill's layouts, so it was ignored"
                    ),
                )
                .at(span)
                .help("Use a fixed size from 640px to 1800px."),
            );
        }
    }

    Scalars {
        heading_weight,
        radius,
        measure,
        shadow,
        page_width,
        origins,
    }
}

/// One summary rather than a note per token, because a file can have hundreds.
pub(crate) fn report_unused(
    extracted: &Extracted,
    colors: &Colors,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let mut unused: Vec<&(String, Span)> = extracted.unused.iter().chain(&colors.unused).collect();
    if unused.is_empty() {
        return;
    }
    unused.sort_by_key(|(_, span)| span.line);
    let first = unused[0].1;
    const SHOWN: usize = 6;
    let names: Vec<&str> = unused.iter().take(SHOWN).map(|(n, _)| n.as_str()).collect();
    let more = unused.len().saturating_sub(SHOWN);
    let list = if more > 0 {
        format!("{} and {more} more", names.join(", "))
    } else {
        names.join(", ")
    };
    diagnostics.push(
        Diagnostic::new(
            Code::NotUsed,
            format!("{} tokens aren't used by Quill: {list}", unused.len()),
        )
        .at(first)
        .help("These are fine to leave in. Quill's templates just have no place for them."),
    );
}
