//! Works out what a color is for, from its name and whatever is written beside it.

/// The job a color does in a palette.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Role {
    Bg,
    Surface,
    Text,
    Muted,
    Border,
    AccentFill,
    OnAccent,
    AccentText,
    CodeBg,
}

impl Role {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Role::Bg => "bg",
            Role::Surface => "surface",
            Role::Text => "text",
            Role::Muted => "muted",
            Role::Border => "border",
            Role::AccentFill => "accent_fill",
            Role::OnAccent => "on_accent",
            Role::AccentText => "accent_text",
            Role::CodeBg => "code_bg",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Mapping {
    Role(Role),
    /// A palette slot such as `primary`, which the spec leaves open. In its own
    /// example `primary` is dark ink, so the name alone doesn't say.
    Ambiguous,
    /// A color for something Quill doesn't draw, such as error states.
    Unused,
    Unknown,
}

/// Lowercases and splits into space-separated words, padded so a phrase can be
/// matched on whole words.
pub(crate) fn words(text: &str) -> String {
    let mut out = String::from(" ");
    for c in text.chars() {
        if c.is_alphanumeric() {
            out.extend(c.to_lowercase());
        } else if !out.ends_with(' ') {
            out.push(' ');
        }
    }
    if !out.ends_with(' ') {
        out.push(' ');
    }
    out
}

fn has_any(padded: &str, phrases: &[&str]) -> bool {
    phrases.iter().any(|p| padded.contains(&format!(" {p} ")))
}

// The order matters: more specific phrases come first, so "muted text" is muted
// rather than text, and "on primary" isn't taken for the primary color itself.
const RULES: &[(&[&str], Role)] = &[
    (
        &[
            "on primary",
            "on accent",
            "on brand",
            "on cta",
            "on action",
            "button text",
            "cta text",
            "primary foreground",
            "accent foreground",
            "primary text on",
        ],
        Role::OnAccent,
    ),
    (
        &[
            "card foreground",
            "popover foreground",
            "on surface",
            "on background",
        ],
        Role::Text,
    ),
    (
        &["code", "code bg", "code background", "code block"],
        Role::CodeBg,
    ),
    (
        &["link", "links", "hyperlink", "accent text", "text accent"],
        Role::AccentText,
    ),
    (
        &[
            "muted",
            "secondary text",
            "text secondary",
            "tertiary text",
            "text tertiary",
            "subtle",
            "subdued",
            "caption",
            "captions",
            "placeholder",
            "metadata",
            "meta",
        ],
        Role::Muted,
    ),
    (
        &[
            "border",
            "borders",
            "hairline",
            "hairlines",
            "divider",
            "dividers",
            "stroke",
            "separator",
        ],
        Role::Border,
    ),
    (
        &[
            "surface", "surfaces", "card", "cards", "panel", "panels", "elevated", "raised",
            "sheet",
        ],
        Role::Surface,
    ),
    (
        &[
            "canvas",
            "background",
            "backgrounds",
            "bg",
            "backdrop",
            "page",
            "pages",
            "paper",
        ],
        Role::Bg,
    ),
    (
        &[
            "primary action",
            "primary actions",
            "cta",
            "call to action",
            "button",
            "buttons",
            "action",
            "actions",
            "accent",
            "brand",
            "interactive",
            "highlight",
        ],
        Role::AccentFill,
    ),
    (
        &[
            "text",
            "ink",
            "foreground",
            "fg",
            "body",
            "heading",
            "headings",
            "headline",
            "headlines",
            "content",
        ],
        Role::Text,
    ),
];

const UNUSED: &[&str] = &[
    "error",
    "danger",
    "destructive",
    "success",
    "warning",
    "info",
    "hover",
    "active",
    "focus",
    "pressed",
    "disabled",
    "chart",
    "sidebar",
    "overlay",
    "ring",
    "selection",
    "shadow",
];

const AMBIGUOUS: &[&str] = &["primary", "secondary", "tertiary", "neutral", "quaternary"];

/// Maps a name, plus any description beside it, to a role.
pub(crate) fn map(name: &str) -> Mapping {
    let padded = words(name.strip_prefix("--").unwrap_or(name));
    if has_any(&padded, UNUSED) {
        return Mapping::Unused;
    }
    for (phrases, role) in RULES {
        if has_any(&padded, phrases) {
            // A dark band inside a light page isn't the page's dark mode.
            let is_area = matches!(role, Role::Bg | Role::Surface);
            if is_area && has_any(&padded, &["dark", "inverse", "inverted"]) {
                return Mapping::Unused;
            }
            return Mapping::Role(*role);
        }
    }
    if has_any(&padded, AMBIGUOUS) {
        Mapping::Ambiguous
    } else {
        Mapping::Unknown
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_names_seen_in_real_files() {
        for (name, role) in [
            ("text", Role::Text),
            ("--color-ink", Role::Text),
            ("background", Role::Bg),
            ("Page canvas", Role::Bg),
            ("--bg", Role::Bg),
            ("Paper", Role::Bg),
            ("card surface", Role::Surface),
            ("--surface-1", Role::Surface),
            ("border", Role::Border),
            ("Hairline dividers", Role::Border),
            ("muted text", Role::Muted),
            ("--text-secondary", Role::Muted),
            ("Secondary text, captions", Role::Muted),
            ("primary action", Role::AccentFill),
            (
                "Highlighter Yellow --color-highlighter Primary CTA fill",
                Role::AccentFill,
            ),
            ("--accent", Role::AccentFill),
            ("brand", Role::AccentFill),
            ("on-primary", Role::OnAccent),
            ("--primary-foreground", Role::OnAccent),
            ("button text", Role::OnAccent),
            ("--link", Role::AccentText),
            ("Link color", Role::AccentText),
            ("--code-bg", Role::CodeBg),
            ("on-surface", Role::Text),
            ("--card-foreground", Role::Text),
            ("Ink --color-ink Primary text, headlines", Role::Text),
        ] {
            assert_eq!(map(name), Mapping::Role(role), "{name}");
        }
    }

    #[test]
    fn leaves_open_what_the_name_cannot_settle() {
        for name in ["primary", "--color-secondary", "tertiary", "neutral"] {
            assert_eq!(map(name), Mapping::Ambiguous, "{name}");
        }
        for name in ["Highlighter Yellow", "--brand-500x", "foo"] {
            let mapped = map(name);
            assert!(
                matches!(mapped, Mapping::Unknown | Mapping::Role(_)),
                "{name}"
            );
        }
        assert_eq!(map("--color-blue-500"), Mapping::Unknown);
    }

    #[test]
    fn skips_colors_quill_does_not_draw() {
        for name in [
            "error",
            "--color-destructive",
            "success",
            "--button-hover",
            "Obsidian: dark section backgrounds",
            "--surface-inverse",
        ] {
            assert_eq!(map(name), Mapping::Unused, "{name}");
        }
    }

    #[test]
    fn matches_whole_words_only() {
        assert_eq!(map("context"), Mapping::Unknown, "not `text`");
        assert_eq!(map("backpage"), Mapping::Unknown, "not `page`");
        assert_eq!(map("cardinal"), Mapping::Unknown, "not `card`");
    }
}
