use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::TokenError;

/// An sRGB color. Every accepted input form is converted to this, so the CSS
/// written back out never contains text copied from the input.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Color {
    r: u8,
    g: u8,
    b: u8,
    a: u8,
}

impl Color {
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Color { r, g, b, a: 255 }
    }

    /// Parses hex (`#rgb`, `#rgba`, `#rrggbb`, `#rrggbbaa`), `rgb()`, `hsl()`
    /// and `oklch()`, including their alpha forms.
    pub fn parse(input: &str) -> Result<Self, TokenError> {
        let error = |reason: &str| TokenError::new("color", input, reason);
        let text = input.trim().to_ascii_lowercase();
        if let Some(hex) = text.strip_prefix('#') {
            return parse_hex(hex).ok_or_else(|| {
                error("hex colors need 3, 4, 6 or 8 digits, such as #fff or #1f5fbf")
            });
        }
        let Some((name, rest)) = text.split_once('(') else {
            return Err(error("use a hex, rgb(), hsl() or oklch() value"));
        };
        let body = rest
            .strip_suffix(')')
            .ok_or_else(|| error("missing closing parenthesis"))?;
        let (parts, alpha) = split_components(body).ok_or_else(|| {
            error("expected three components and an optional alpha, such as rgb(12 34 56 / 50%)")
        })?;
        let a = match alpha {
            Some(text) => parse_alpha(text).ok_or_else(|| error("alpha must be from 0 to 1"))?,
            None => 255,
        };
        let channels = match name {
            "rgb" | "rgba" => parse_rgb(&parts).ok_or_else(|| {
                error("red, green and blue must each be from 0 to 255, or from 0% to 100%")
            })?,
            "hsl" | "hsla" => parse_hsl(&parts).ok_or_else(|| {
                error("hsl() needs a hue, then saturation and lightness from 0% to 100%")
            })?,
            "oklch" => parse_oklch(&parts).ok_or_else(|| {
                error("oklch() needs lightness from 0 to 1, a non-negative chroma, and a hue")
            })?,
            _ => return Err(error("use a hex, rgb(), hsl() or oklch() value")),
        };
        Ok(Color {
            r: channels[0],
            g: channels[1],
            b: channels[2],
            a,
        })
    }

    /// `#rrggbb`, or `#rrggbbaa` when the color isn't fully opaque.
    pub fn to_css(self) -> String {
        if self.a == 255 {
            format!("#{:02x}{:02x}{:02x}", self.r, self.g, self.b)
        } else {
            format!("#{:02x}{:02x}{:02x}{:02x}", self.r, self.g, self.b, self.a)
        }
    }

    /// WCAG 2.x relative luminance, from 0 (black) to 1 (white). Alpha is ignored.
    pub fn relative_luminance(self) -> f64 {
        let linear = |channel: u8| {
            let c = f64::from(channel) / 255.0;
            if c <= 0.04045 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * linear(self.r) + 0.7152 * linear(self.g) + 0.0722 * linear(self.b)
    }

    /// WCAG 2.x contrast ratio, from 1 (identical) to 21 (black on white).
    pub fn contrast_ratio(self, other: Color) -> f64 {
        let (a, b) = (self.relative_luminance(), other.relative_luminance());
        (a.max(b) + 0.05) / (a.min(b) + 0.05)
    }
}

fn parse_hex(hex: &str) -> Option<Color> {
    if !hex.is_ascii() {
        return None;
    }
    let digit = |i: usize, len: usize| u8::from_str_radix(hex.get(i..i + len)?, 16).ok();
    let (r, g, b, a) = match hex.len() {
        3 | 4 => {
            let short = |i| digit(i, 1).map(|d| d * 17);
            let a = if hex.len() == 4 { short(3)? } else { 255 };
            (short(0)?, short(1)?, short(2)?, a)
        }
        6 | 8 => {
            let a = if hex.len() == 8 { digit(6, 2)? } else { 255 };
            (digit(0, 2)?, digit(2, 2)?, digit(4, 2)?, a)
        }
        _ => return None,
    };
    Some(Color { r, g, b, a })
}

/// Splits `12 34 56 / 50%` or `12, 34, 56, 0.5` into three components and an alpha.
fn split_components(body: &str) -> Option<(Vec<&str>, Option<&str>)> {
    let (main, slash_alpha) = match body.split_once('/') {
        Some((main, alpha)) => (main, Some(alpha.trim())),
        None => (body, None),
    };
    let mut parts: Vec<&str> = main
        .split(|c: char| c == ',' || c.is_whitespace())
        .filter(|p| !p.is_empty())
        .collect();
    let comma_alpha = (parts.len() == 4 && slash_alpha.is_none())
        .then(|| parts.pop())
        .flatten();
    (parts.len() == 3).then_some((parts, slash_alpha.or(comma_alpha)))
}

fn parse_number(text: &str) -> Option<f64> {
    text.parse::<f64>().ok().filter(|n| n.is_finite())
}

fn parse_percent(text: &str) -> Option<f64> {
    parse_number(text.strip_suffix('%')?)
}

fn to_byte(unit: f64) -> u8 {
    (unit.clamp(0.0, 1.0) * 255.0).round() as u8
}

fn parse_alpha(text: &str) -> Option<u8> {
    let value = match text.strip_suffix('%') {
        Some(percent) => parse_number(percent)? / 100.0,
        None => parse_number(text)?,
    };
    (0.0..=1.0).contains(&value).then(|| to_byte(value))
}

fn parse_rgb(parts: &[&str]) -> Option<[u8; 3]> {
    let channel = |text: &str| {
        let unit = match text.strip_suffix('%') {
            Some(percent) => parse_number(percent)? / 100.0,
            None => parse_number(text)? / 255.0,
        };
        (0.0..=1.0).contains(&unit).then_some(to_byte(unit))
    };
    Some([channel(parts[0])?, channel(parts[1])?, channel(parts[2])?])
}

fn parse_hue(text: &str) -> Option<f64> {
    parse_number(text.strip_suffix("deg").unwrap_or(text)).map(|h| h.rem_euclid(360.0))
}

fn parse_hsl(parts: &[&str]) -> Option<[u8; 3]> {
    let hue = parse_hue(parts[0])?;
    let unit = |text: &str| {
        let value = parse_percent(text)? / 100.0;
        (0.0..=1.0).contains(&value).then_some(value)
    };
    let (saturation, lightness) = (unit(parts[1])?, unit(parts[2])?);
    let channel = |n: f64| {
        let k = (n + hue / 30.0).rem_euclid(12.0);
        let a = saturation * lightness.min(1.0 - lightness);
        to_byte(lightness - a * (k - 3.0).min(9.0 - k).clamp(-1.0, 1.0))
    };
    Some([channel(0.0), channel(8.0), channel(4.0)])
}

fn parse_oklch(parts: &[&str]) -> Option<[u8; 3]> {
    let lightness = match parts[0].strip_suffix('%') {
        Some(percent) => parse_number(percent)? / 100.0,
        None => parse_number(parts[0])?,
    };
    let chroma = parse_number(parts[1])?;
    let hue = parse_hue(parts[2])?;
    if !(0.0..=1.0).contains(&lightness) || chroma < 0.0 {
        return None;
    }
    Some(oklch_to_srgb(lightness, chroma, hue))
}

/// Björn Ottosson's OKLab to linear sRGB matrices, then the sRGB transfer
/// function. Colors outside the sRGB gamut are clamped per channel.
fn oklch_to_srgb(lightness: f64, chroma: f64, hue_degrees: f64) -> [u8; 3] {
    let hue = hue_degrees.to_radians();
    let (a, b) = (chroma * hue.cos(), chroma * hue.sin());
    let l = (lightness + 0.396_337_777_4 * a + 0.215_803_757_3 * b).powi(3);
    let m = (lightness - 0.105_561_345_8 * a - 0.063_854_172_8 * b).powi(3);
    let s = (lightness - 0.089_484_177_5 * a - 1.291_485_548_0 * b).powi(3);
    let linear = [
        4.076_741_662_1 * l - 3.307_711_591_3 * m + 0.230_969_929_2 * s,
        -1.268_438_004_6 * l + 2.609_757_401_1 * m - 0.341_319_396_5 * s,
        -0.004_196_086_3 * l - 0.703_418_614_7 * m + 1.707_614_701_0 * s,
    ];
    linear.map(|c| {
        let c = c.clamp(0.0, 1.0);
        to_byte(if c <= 0.003_130_8 {
            12.92 * c
        } else {
            1.055 * c.powf(1.0 / 2.4) - 0.055
        })
    })
}

impl FromStr for Color {
    type Err = TokenError;
    fn from_str(s: &str) -> Result<Self, TokenError> {
        Color::parse(s)
    }
}

impl TryFrom<String> for Color {
    type Error = TokenError;
    fn try_from(s: String) -> Result<Self, TokenError> {
        Color::parse(&s)
    }
}

impl From<Color> for String {
    fn from(c: Color) -> String {
        c.to_css()
    }
}

impl fmt::Display for Color {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_css())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_every_form_to_the_same_value() {
        for input in [
            "#1f5fbf",
            "#1F5FBF",
            " #1f5fbf ",
            "rgb(31 95 191)",
            "rgb(31, 95, 191)",
            "rgba(31, 95, 191, 1)",
            "rgb(31 95 191 / 100%)",
        ] {
            assert_eq!(Color::parse(input).unwrap().to_css(), "#1f5fbf", "{input}");
        }
        assert_eq!(Color::parse("#abc").unwrap().to_css(), "#aabbcc");
        assert_eq!(Color::parse("#abcd").unwrap().to_css(), "#aabbccdd");
        assert_eq!(Color::parse("#11223344").unwrap().to_css(), "#11223344");
        assert_eq!(
            Color::parse("rgb(100% 0% 0% / 0.5)").unwrap().to_css(),
            "#ff000080"
        );
    }

    #[test]
    fn converts_hsl() {
        for (input, expected) in [
            ("hsl(0 100% 50%)", "#ff0000"),
            ("hsl(120, 100%, 50%)", "#00ff00"),
            ("hsl(240deg 100% 50%)", "#0000ff"),
            ("hsl(0 0% 100%)", "#ffffff"),
            ("hsl(0 0% 0%)", "#000000"),
            ("hsl(600 100% 50%)", "#0000ff"),
            ("hsla(0, 100%, 50%, 0.5)", "#ff000080"),
        ] {
            assert_eq!(Color::parse(input).unwrap().to_css(), expected, "{input}");
        }
    }

    #[test]
    fn converts_oklch() {
        // Reference values from the CSS Color 4 definition of OKLCH.
        for (input, expected) in [
            ("oklch(1 0 0)", "#ffffff"),
            ("oklch(0 0 0)", "#000000"),
            ("oklch(0.62796 0.25768 29.234)", "#ff0000"),
            ("oklch(0.86644 0.29483 142.495)", "#00ff00"),
            ("oklch(0.45201 0.31321 264.052)", "#0000ff"),
            ("oklch(100% 0 0deg / 50%)", "#ffffff80"),
        ] {
            assert_eq!(Color::parse(input).unwrap().to_css(), expected, "{input}");
        }
    }

    #[test]
    fn clamps_colors_outside_srgb() {
        // This vivid green is outside sRGB: green saturates and red bottoms out
        // instead of the conversion wrapping around or failing.
        let color = Color::parse("oklch(0.9 0.4 150)").unwrap();
        assert_eq!((color.r, color.g), (0, 255));
    }

    #[test]
    fn rejects_invalid_values() {
        for input in [
            "",
            "red",
            "#",
            "#12",
            "#12345",
            "#1234567",
            "#ggg",
            "#é12",
            "rgb(1 2)",
            "rgb(1 2 3 4 5)",
            "rgb(300 0 0)",
            "rgb(-1 0 0)",
            "rgb(1 2 3",
            "rgb(a b c)",
            "rgb(1 2 3 / 2)",
            "hsl(0 120% 50%)",
            "hsl(0 50 50)",
            "oklch(2 0 0)",
            "oklch(0.5 -1 0)",
            "lab(50 0 0)",
            "#fff; background: url(x)",
            "rgb(1 2 3); color: red",
            "rgb(NaN 2 3)",
            "rgb(inf 2 3)",
        ] {
            assert!(Color::parse(input).is_err(), "{input} should be rejected");
        }
    }

    #[test]
    fn contrast_matches_known_values() {
        let black = Color::rgb(0, 0, 0);
        let white = Color::rgb(255, 255, 255);
        assert!((black.contrast_ratio(white) - 21.0).abs() < 0.001);
        assert!((white.contrast_ratio(white) - 1.0).abs() < 0.001);
        // The real-world case from the plan: a yellow accent that fails as text
        // but works as a fill behind dark text.
        let accent = Color::parse("#e4f222").unwrap();
        let canvas = Color::parse("#f4f2f0").unwrap();
        let ink = Color::parse("#0c0a08").unwrap();
        assert!((accent.contrast_ratio(canvas) - 1.1).abs() < 0.05);
        assert!(accent.contrast_ratio(ink) > 15.0);
    }

    #[test]
    fn round_trips_through_serde_as_a_string() {
        let color: Color = serde_json::from_str("\"#ABC\"").unwrap();
        assert_eq!(serde_json::to_string(&color).unwrap(), "\"#aabbcc\"");
        assert!(serde_json::from_str::<Color>("\"nope\"").is_err());
    }
}
