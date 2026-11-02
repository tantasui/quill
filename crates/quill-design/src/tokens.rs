use std::fmt::Write;

use serde::{Deserialize, Serialize};

use crate::{Color, FontStack, FontWeight, Length, Shadow};

/// The colors of one color scheme, by what each color is for.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Palette {
    pub bg: Color,
    pub surface: Color,
    pub text: Color,
    pub muted: Color,
    pub border: Color,
    /// Background of filled accents such as buttons.
    pub accent_fill: Color,
    /// Text drawn on `accent_fill`.
    pub on_accent: Color,
    /// Links and other accents drawn as text on `bg`.
    pub accent_text: Color,
    pub code_bg: Color,
    /// A faint accent tint behind small labels. When absent, stylesheets mix one
    /// from `accent_text` and `bg`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accent_soft: Option<Color>,
}

impl Palette {
    fn write_css(&self, out: &mut String, indent: &str) {
        let colors = [
            ("bg", Some(self.bg)),
            ("surface", Some(self.surface)),
            ("text", Some(self.text)),
            ("muted", Some(self.muted)),
            ("border", Some(self.border)),
            ("accent-fill", Some(self.accent_fill)),
            ("on-accent", Some(self.on_accent)),
            ("accent-text", Some(self.accent_text)),
            ("code-bg", Some(self.code_bg)),
            ("accent-soft", self.accent_soft),
        ];
        for (name, color) in colors {
            if let Some(color) = color {
                write_declaration(out, indent, name, &color.to_css());
            }
        }
    }
}

/// A complete visual style. Rendering it to CSS writes every value from its
/// typed form, so nothing from an imported file reaches a stylesheet as text.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DesignTokens {
    pub light: Palette,
    pub dark: Palette,
    pub font_body: FontStack,
    pub font_heading: FontStack,
    pub font_mono: FontStack,
    pub heading_weight: FontWeight,
    pub radius: Length,
    pub measure: Length,
    pub shadow: Shadow,
}

impl DesignTokens {
    /// A `:root` block with the light palette and shared values, followed by
    /// the dark palette for readers who prefer dark mode.
    pub fn to_css(&self) -> String {
        let mut out = String::from(":root {\n");
        self.light.write_css(&mut out, "  ");
        self.write_shared(&mut out);
        out.push_str("}\n\n@media (prefers-color-scheme: dark) {\n  :root {\n");
        self.dark.write_css(&mut out, "    ");
        out.push_str("  }\n}\n");
        out
    }

    /// One palette forced on, whatever the reader's preference. For previews.
    pub fn to_css_forced(&self, palette: &Palette) -> String {
        let mut out = String::from(":root {\n");
        palette.write_css(&mut out, "  ");
        self.write_shared(&mut out);
        out.push_str("}\n");
        out
    }

    fn write_shared(&self, out: &mut String) {
        let values = [
            ("font-body", self.font_body.to_css()),
            ("font-heading", self.font_heading.to_css()),
            ("font-mono", self.font_mono.to_css()),
            ("heading-weight", self.heading_weight.to_css()),
            ("radius", self.radius.to_css()),
            ("measure", self.measure.to_css()),
            ("shadow", self.shadow.to_css()),
        ];
        for (name, value) in &values {
            write_declaration(out, "  ", name, value);
        }
    }
}

fn write_declaration(out: &mut String, indent: &str, name: &str, value: &str) {
    writeln!(out, "{indent}--{name}: {value};").expect("writing to a String can't fail");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn palette(base: u8) -> Palette {
        let color = |offset: u8| Color::rgb(base + offset, base + offset, base + offset);
        Palette {
            bg: color(0),
            surface: color(1),
            text: color(2),
            muted: color(3),
            border: color(4),
            accent_fill: color(5),
            on_accent: color(6),
            accent_text: color(7),
            code_bg: color(8),
            accent_soft: None,
        }
    }

    fn tokens() -> DesignTokens {
        DesignTokens {
            light: palette(0x10),
            dark: Palette {
                accent_soft: Some(Color::rgb(1, 2, 3)),
                ..palette(0x20)
            },
            font_body: FontStack::parse("Source Serif 4, serif").unwrap(),
            font_heading: FontStack::parse("Inter").unwrap(),
            font_mono: FontStack::parse("monospace").unwrap(),
            heading_weight: FontWeight::new(700).unwrap(),
            radius: Length::parse("6px").unwrap(),
            measure: Length::parse("40rem").unwrap(),
            shadow: Shadow::None,
        }
    }

    #[test]
    fn renders_light_and_dark_blocks() {
        assert_eq!(
            tokens().to_css(),
            r#":root {
  --bg: #101010;
  --surface: #111111;
  --text: #121212;
  --muted: #131313;
  --border: #141414;
  --accent-fill: #151515;
  --on-accent: #161616;
  --accent-text: #171717;
  --code-bg: #181818;
  --font-body: "Source Serif 4", serif;
  --font-heading: Inter;
  --font-mono: monospace;
  --heading-weight: 700;
  --radius: 6px;
  --measure: 40rem;
  --shadow: none;
}

@media (prefers-color-scheme: dark) {
  :root {
    --bg: #202020;
    --surface: #212121;
    --text: #222222;
    --muted: #232323;
    --border: #242424;
    --accent-fill: #252525;
    --on-accent: #262626;
    --accent-text: #272727;
    --code-bg: #282828;
    --accent-soft: #010203;
  }
}
"#
        );
    }

    #[test]
    fn deserializing_validates_every_value() {
        let good = serde_json::to_value(tokens()).unwrap();
        let round_trip: DesignTokens = serde_json::from_value(good.clone()).unwrap();
        assert_eq!(round_trip, tokens());

        for (field, value) in [
            ("radius", "64-128px"),
            ("measure", "-4px"),
            ("font_body", "x; color: red"),
            ("shadow", "soft"),
        ] {
            let mut bad = good.clone();
            bad[field] = value.into();
            assert!(
                serde_json::from_value::<DesignTokens>(bad).is_err(),
                "{field}: {value}"
            );
        }
        let mut bad = good.clone();
        bad["light"]["bg"] = "notacolor".into();
        assert!(serde_json::from_value::<DesignTokens>(bad).is_err());
        let mut bad = good;
        bad["unknown_token"] = "1px".into();
        assert!(serde_json::from_value::<DesignTokens>(bad).is_err());
    }
}
