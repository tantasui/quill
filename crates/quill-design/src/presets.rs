//! Built-in presets: a layout, default tokens and a small stylesheet of finishing
//! touches, under one name. The TOML files are embedded at compile time.

use std::collections::HashMap;
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

use crate::{DesignTokens, Layout};

#[derive(Debug, Serialize)]
pub struct Preset {
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    #[serde(skip)]
    source: &'static str,
}

pub const DEFAULT: &str = "editorial";

pub const ALL: &[Preset] = &[
    Preset {
        id: "editorial",
        name: "Editorial",
        description: "A quiet, minimalist reading column with classic serif typography. Suited to essays and long-form writing.",
        source: include_str!("../presets/editorial.toml"),
    },
    Preset {
        id: "devlog",
        name: "Devlog",
        description: "A modern developer blog: crisp sans-serif text, monospace details, tag sidebar and polished code blocks.",
        source: include_str!("../presets/devlog.toml"),
    },
    Preset {
        id: "gazette",
        name: "Gazette",
        description: "A traditional publication with a masthead, a lead story and a grid of articles with cover images.",
        source: include_str!("../presets/gazette.toml"),
    },
];

pub fn find(id: &str) -> Option<&'static Preset> {
    ALL.iter().find(|p| p.id == id)
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Parsed {
    tokens: DesignTokens,
    layout: Layout,
}

/// Parsed presets, built once. A broken embedded file is a bug in Quill itself,
/// so it panics; a test parses every preset to make sure one never ships.
fn parsed(preset: &Preset) -> &'static Parsed {
    static CACHE: OnceLock<HashMap<&'static str, Parsed>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| {
        ALL.iter()
            .map(|p| {
                let parsed = toml::from_str(p.source)
                    .unwrap_or_else(|e| panic!("preset {} is invalid: {e}", p.id));
                (p.id, parsed)
            })
            .collect()
    });
    &cache[preset.id]
}

impl Preset {
    pub fn tokens(&self) -> &'static DesignTokens {
        &parsed(self).tokens
    }

    pub fn layout(&self) -> &'static Layout {
        &parsed(self).layout
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{HeaderVariant, Palette};

    const AA: f64 = 4.5;

    #[test]
    fn every_preset_parses_and_has_a_unique_id() {
        for preset in ALL {
            assert!(preset.tokens().to_css().starts_with(":root {"));
            assert_eq!(find(preset.id).unwrap().id, preset.id);
            assert_eq!(ALL.iter().filter(|p| p.id == preset.id).count(), 1);
        }
        assert!(find(DEFAULT).is_some());
        assert!(find("nope").is_none());
    }

    #[test]
    fn presets_have_distinct_layouts() {
        let header = |id| find(id).unwrap().layout().header.variant;
        assert_eq!(header("editorial"), HeaderVariant::Centered);
        assert_eq!(header("devlog"), HeaderVariant::Left);
        assert_eq!(header("gazette"), HeaderVariant::Masthead);
    }

    /// The same contrast rules imported designs must meet in Milestone 2.
    #[test]
    fn every_palette_meets_wcag_aa() {
        fn check(id: &str, scheme: &str, p: &Palette) {
            for (name, fg, bg) in [
                ("text on bg", p.text, p.bg),
                ("text on surface", p.text, p.surface),
                ("muted on bg", p.muted, p.bg),
                ("accent_text on bg", p.accent_text, p.bg),
                ("on_accent on accent_fill", p.on_accent, p.accent_fill),
            ] {
                let ratio = fg.contrast_ratio(bg);
                assert!(ratio >= AA, "{id} {scheme}: {name} is {ratio:.2}:1");
            }
        }
        for preset in ALL {
            check(preset.id, "light", &preset.tokens().light);
            check(preset.id, "dark", &preset.tokens().dark);
        }
    }
}
