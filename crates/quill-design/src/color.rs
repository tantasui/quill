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

    /// Parses CSS color names, hex (`#rgb`, `#rgba`, `#rrggbb`, `#rrggbbaa`),
    /// `rgb()`, `hsl()` and `oklch()`, including their alpha forms.
    pub fn parse(input: &str) -> Result<Self, TokenError> {
        let error = |reason: &str| TokenError::new("color", input, reason);
        let text = input.trim().to_ascii_lowercase();
        if let Some(hex) = text.strip_prefix('#') {
            return parse_hex(hex).ok_or_else(|| {
                error("hex colors need 3, 4, 6 or 8 digits, such as #fff or #1f5fbf")
            });
        }
        if let Some(color) = named_color(&text) {
            return Ok(color);
        }
        let Some((name, rest)) = text.split_once('(') else {
            return Err(error(
                "use a CSS color name, or a hex, rgb(), hsl() or oklch() value",
            ));
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
            _ => {
                return Err(error(
                    "use a CSS color name, or a hex, rgb(), hsl() or oklch() value",
                ))
            }
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

    pub fn is_opaque(self) -> bool {
        self.a == 255
    }

    /// This color painted over an opaque `background`, as a reader would see it.
    pub fn flatten_over(self, background: Color) -> Color {
        let alpha = f64::from(self.a) / 255.0;
        let channel = |top: u8, bottom: u8| {
            (f64::from(top) * alpha + f64::from(bottom) * (1.0 - alpha)).round() as u8
        };
        Color::rgb(
            channel(self.r, background.r),
            channel(self.g, background.g),
            channel(self.b, background.b),
        )
    }

    /// Mixes `amount` (0 to 1) of `other` into this color, in sRGB. The result
    /// is opaque.
    pub fn mix(self, other: Color, amount: f64) -> Color {
        let channel =
            |a: u8, b: u8| (f64::from(a) + (f64::from(b) - f64::from(a)) * amount).round() as u8;
        Color::rgb(
            channel(self.r, other.r),
            channel(self.g, other.g),
            channel(self.b, other.b),
        )
    }

    /// Lightness (0 to 1), chroma and hue in degrees. Alpha is ignored.
    pub fn to_oklch(self) -> (f64, f64, f64) {
        let linear = |channel: u8| {
            let c = f64::from(channel) / 255.0;
            if c <= 0.04045 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        };
        let (r, g, b) = (linear(self.r), linear(self.g), linear(self.b));
        let l = (0.412_221_470_8 * r + 0.536_332_536_3 * g + 0.051_445_992_9 * b).cbrt();
        let m = (0.211_903_498_2 * r + 0.680_699_545_1 * g + 0.107_396_956_6 * b).cbrt();
        let s = (0.088_302_461_9 * r + 0.281_718_837_6 * g + 0.629_978_700_5 * b).cbrt();
        let lightness = 0.210_454_255_3 * l + 0.793_617_785_0 * m - 0.004_072_046_8 * s;
        let a = 1.977_998_495_1 * l - 2.428_592_205_0 * m + 0.450_593_709_9 * s;
        let b = 0.025_904_037_1 * l + 0.782_771_766_2 * m - 0.808_675_766_0 * s;
        (
            lightness,
            a.hypot(b),
            b.atan2(a).to_degrees().rem_euclid(360.0),
        )
    }

    /// An opaque color from OKLCH, clamped to the sRGB gamut.
    pub fn from_oklch(lightness: f64, chroma: f64, hue_degrees: f64) -> Color {
        let [r, g, b] = oklch_to_srgb(lightness.clamp(0.0, 1.0), chroma.max(0.0), hue_degrees);
        Color::rgb(r, g, b)
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

fn named_color(name: &str) -> Option<Color> {
    if name == "transparent" {
        return Some(Color {
            r: 0,
            g: 0,
            b: 0,
            a: 0,
        });
    }
    let index = NAMED_COLORS.binary_search_by_key(&name, |(n, _)| n).ok()?;
    let [_, r, g, b] = NAMED_COLORS[index].1.to_be_bytes();
    Some(Color::rgb(r, g, b))
}

/// The CSS named colors, sorted by name.
const NAMED_COLORS: [(&str, u32); 148] = [
    ("aliceblue", 0xf0f8ff),
    ("antiquewhite", 0xfaebd7),
    ("aqua", 0x00ffff),
    ("aquamarine", 0x7fffd4),
    ("azure", 0xf0ffff),
    ("beige", 0xf5f5dc),
    ("bisque", 0xffe4c4),
    ("black", 0x000000),
    ("blanchedalmond", 0xffebcd),
    ("blue", 0x0000ff),
    ("blueviolet", 0x8a2be2),
    ("brown", 0xa52a2a),
    ("burlywood", 0xdeb887),
    ("cadetblue", 0x5f9ea0),
    ("chartreuse", 0x7fff00),
    ("chocolate", 0xd2691e),
    ("coral", 0xff7f50),
    ("cornflowerblue", 0x6495ed),
    ("cornsilk", 0xfff8dc),
    ("crimson", 0xdc143c),
    ("cyan", 0x00ffff),
    ("darkblue", 0x00008b),
    ("darkcyan", 0x008b8b),
    ("darkgoldenrod", 0xb8860b),
    ("darkgray", 0xa9a9a9),
    ("darkgreen", 0x006400),
    ("darkgrey", 0xa9a9a9),
    ("darkkhaki", 0xbdb76b),
    ("darkmagenta", 0x8b008b),
    ("darkolivegreen", 0x556b2f),
    ("darkorange", 0xff8c00),
    ("darkorchid", 0x9932cc),
    ("darkred", 0x8b0000),
    ("darksalmon", 0xe9967a),
    ("darkseagreen", 0x8fbc8f),
    ("darkslateblue", 0x483d8b),
    ("darkslategray", 0x2f4f4f),
    ("darkslategrey", 0x2f4f4f),
    ("darkturquoise", 0x00ced1),
    ("darkviolet", 0x9400d3),
    ("deeppink", 0xff1493),
    ("deepskyblue", 0x00bfff),
    ("dimgray", 0x696969),
    ("dimgrey", 0x696969),
    ("dodgerblue", 0x1e90ff),
    ("firebrick", 0xb22222),
    ("floralwhite", 0xfffaf0),
    ("forestgreen", 0x228b22),
    ("fuchsia", 0xff00ff),
    ("gainsboro", 0xdcdcdc),
    ("ghostwhite", 0xf8f8ff),
    ("gold", 0xffd700),
    ("goldenrod", 0xdaa520),
    ("gray", 0x808080),
    ("green", 0x008000),
    ("greenyellow", 0xadff2f),
    ("grey", 0x808080),
    ("honeydew", 0xf0fff0),
    ("hotpink", 0xff69b4),
    ("indianred", 0xcd5c5c),
    ("indigo", 0x4b0082),
    ("ivory", 0xfffff0),
    ("khaki", 0xf0e68c),
    ("lavender", 0xe6e6fa),
    ("lavenderblush", 0xfff0f5),
    ("lawngreen", 0x7cfc00),
    ("lemonchiffon", 0xfffacd),
    ("lightblue", 0xadd8e6),
    ("lightcoral", 0xf08080),
    ("lightcyan", 0xe0ffff),
    ("lightgoldenrodyellow", 0xfafad2),
    ("lightgray", 0xd3d3d3),
    ("lightgreen", 0x90ee90),
    ("lightgrey", 0xd3d3d3),
    ("lightpink", 0xffb6c1),
    ("lightsalmon", 0xffa07a),
    ("lightseagreen", 0x20b2aa),
    ("lightskyblue", 0x87cefa),
    ("lightslategray", 0x778899),
    ("lightslategrey", 0x778899),
    ("lightsteelblue", 0xb0c4de),
    ("lightyellow", 0xffffe0),
    ("lime", 0x00ff00),
    ("limegreen", 0x32cd32),
    ("linen", 0xfaf0e6),
    ("magenta", 0xff00ff),
    ("maroon", 0x800000),
    ("mediumaquamarine", 0x66cdaa),
    ("mediumblue", 0x0000cd),
    ("mediumorchid", 0xba55d3),
    ("mediumpurple", 0x9370db),
    ("mediumseagreen", 0x3cb371),
    ("mediumslateblue", 0x7b68ee),
    ("mediumspringgreen", 0x00fa9a),
    ("mediumturquoise", 0x48d1cc),
    ("mediumvioletred", 0xc71585),
    ("midnightblue", 0x191970),
    ("mintcream", 0xf5fffa),
    ("mistyrose", 0xffe4e1),
    ("moccasin", 0xffe4b5),
    ("navajowhite", 0xffdead),
    ("navy", 0x000080),
    ("oldlace", 0xfdf5e6),
    ("olive", 0x808000),
    ("olivedrab", 0x6b8e23),
    ("orange", 0xffa500),
    ("orangered", 0xff4500),
    ("orchid", 0xda70d6),
    ("palegoldenrod", 0xeee8aa),
    ("palegreen", 0x98fb98),
    ("paleturquoise", 0xafeeee),
    ("palevioletred", 0xdb7093),
    ("papayawhip", 0xffefd5),
    ("peachpuff", 0xffdab9),
    ("peru", 0xcd853f),
    ("pink", 0xffc0cb),
    ("plum", 0xdda0dd),
    ("powderblue", 0xb0e0e6),
    ("purple", 0x800080),
    ("rebeccapurple", 0x663399),
    ("red", 0xff0000),
    ("rosybrown", 0xbc8f8f),
    ("royalblue", 0x4169e1),
    ("saddlebrown", 0x8b4513),
    ("salmon", 0xfa8072),
    ("sandybrown", 0xf4a460),
    ("seagreen", 0x2e8b57),
    ("seashell", 0xfff5ee),
    ("sienna", 0xa0522d),
    ("silver", 0xc0c0c0),
    ("skyblue", 0x87ceeb),
    ("slateblue", 0x6a5acd),
    ("slategray", 0x708090),
    ("slategrey", 0x708090),
    ("snow", 0xfffafa),
    ("springgreen", 0x00ff7f),
    ("steelblue", 0x4682b4),
    ("tan", 0xd2b48c),
    ("teal", 0x008080),
    ("thistle", 0xd8bfd8),
    ("tomato", 0xff6347),
    ("turquoise", 0x40e0d0),
    ("violet", 0xee82ee),
    ("wheat", 0xf5deb3),
    ("white", 0xffffff),
    ("whitesmoke", 0xf5f5f5),
    ("yellow", 0xffff00),
    ("yellowgreen", 0x9acd32),
];

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
    fn parses_named_colors() {
        assert_eq!(Color::parse("red").unwrap().to_css(), "#ff0000");
        assert_eq!(Color::parse("CornflowerBlue").unwrap().to_css(), "#6495ed");
        assert_eq!(Color::parse("transparent").unwrap().to_css(), "#00000000");
        assert!(
            NAMED_COLORS.windows(2).all(|w| w[0].0 < w[1].0),
            "must stay sorted"
        );
    }

    #[test]
    fn oklch_round_trips() {
        for hex in [
            "#000000", "#ffffff", "#1f5fbf", "#e4f222", "#9c3a1f", "#808080",
        ] {
            let color = Color::parse(hex).unwrap();
            let (l, c, h) = color.to_oklch();
            assert_eq!(Color::from_oklch(l, c, h), color, "{hex}");
        }
        // Neutrals have no chroma, whatever hue the math produces.
        assert!(Color::parse("#808080").unwrap().to_oklch().1 < 0.001);
    }

    #[test]
    fn flattens_and_mixes() {
        let white = Color::rgb(255, 255, 255);
        let black = Color::rgb(0, 0, 0);
        let half_black = Color::parse("rgb(0 0 0 / 50%)").unwrap();
        assert!(!half_black.is_opaque());
        assert_eq!(half_black.flatten_over(white).to_css(), "#7f7f7f");
        assert_eq!(white.mix(black, 0.0), white);
        assert_eq!(white.mix(black, 1.0), black);
        assert_eq!(white.mix(black, 0.5).to_css(), "#808080");
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
            "notacolor",
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
