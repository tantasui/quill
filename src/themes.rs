//! Built-in themes.
//!
//! The template contract: every theme provides `templates/themes/<id>/base.html`
//! defining the blocks `title`, `head` and `content`, and a stylesheet at
//! `static/themes/<id>.css`. A theme may also override any page template
//! (`index`, `article`, `tag`, `archive`, `not_found`) by adding
//! `templates/themes/<id>/<page>.html`; otherwise the shared version in
//! `templates/public/` is used. Content never depends on the active theme.

use serde::Serialize;

#[derive(Debug, Clone, Copy, Serialize)]
pub struct Theme {
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
}

pub const DEFAULT: &str = "editorial";

pub const ALL: &[Theme] = &[
    Theme {
        id: "editorial",
        name: "Editorial",
        description: "A quiet, minimalist reading column with classic serif typography. Suited to essays and long-form writing.",
    },
    Theme {
        id: "devlog",
        name: "Devlog",
        description: "A modern developer blog: crisp sans-serif text, monospace details, tag sidebar and polished code blocks.",
    },
    Theme {
        id: "gazette",
        name: "Gazette",
        description: "A traditional publication with a masthead, a lead story and a grid of articles with cover images.",
    },
];

pub fn find(id: &str) -> Option<&'static Theme> {
    ALL.iter().find(|t| t.id == id)
}
