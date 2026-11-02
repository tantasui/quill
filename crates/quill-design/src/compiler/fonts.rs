//! Chooses fonts. Quill never loads remote fonts, because its Content-Security-
//! Policy forbids them and they'd tell a third party who reads the blog. So
//! families map to a small bundled set, or to fonts already on the reader's device.

use serde::{Deserialize, Serialize};

use super::diagnostic::{Code, Diagnostic, Span};
use super::extract::{FontDecl, FontRole};
use crate::FontStack;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Kind {
    Sans,
    Serif,
    Mono,
}

impl Kind {
    fn system_stack(self) -> &'static str {
        match self {
            Kind::Sans => r#"system-ui, -apple-system, "Segoe UI", Roboto, sans-serif"#,
            Kind::Serif => r#"Georgia, "Times New Roman", Times, serif"#,
            Kind::Mono => r#"ui-monospace, "SF Mono", Menlo, Consolas, monospace"#,
        }
    }
}

#[derive(Debug)]
pub struct Face {
    pub file: &'static str,
    pub style: &'static str,
}

/// An open-licensed font embedded in the binary. Licenses are in `static/fonts/licenses`.
#[derive(Debug)]
pub struct BundledFont {
    pub id: &'static str,
    pub family: &'static str,
    /// The variable font's weight range, as CSS.
    pub weights: &'static str,
    pub faces: &'static [Face],
    kind: Kind,
}

pub const BUNDLED: &[BundledFont] = &[
    BundledFont {
        id: "inter",
        family: "Inter",
        weights: "100 900",
        kind: Kind::Sans,
        faces: &[
            Face {
                file: "inter-latin-wght-normal.woff2",
                style: "normal",
            },
            Face {
                file: "inter-latin-wght-italic.woff2",
                style: "italic",
            },
        ],
    },
    BundledFont {
        id: "ibm-plex-sans",
        family: "IBM Plex Sans",
        weights: "100 700",
        kind: Kind::Sans,
        faces: &[Face {
            file: "ibm-plex-sans-latin-wght-normal.woff2",
            style: "normal",
        }],
    },
    BundledFont {
        id: "source-serif-4",
        family: "Source Serif 4",
        weights: "200 900",
        kind: Kind::Serif,
        faces: &[
            Face {
                file: "source-serif-4-latin-wght-normal.woff2",
                style: "normal",
            },
            Face {
                file: "source-serif-4-latin-wght-italic.woff2",
                style: "italic",
            },
        ],
    },
    BundledFont {
        id: "jetbrains-mono",
        family: "JetBrains Mono",
        weights: "100 800",
        kind: Kind::Mono,
        faces: &[Face {
            file: "jetbrains-mono-latin-wght-normal.woff2",
            style: "normal",
        }],
    },
];

pub(crate) fn bundled_by_id(id: &str) -> Option<&'static BundledFont> {
    BUNDLED.iter().find(|f| f.id == id)
}

fn bundled_by_name(name: &str) -> Option<&'static BundledFont> {
    BUNDLED
        .iter()
        .find(|f| f.family.eq_ignore_ascii_case(name.trim()))
}

fn bundled_default(kind: Kind) -> &'static BundledFont {
    BUNDLED
        .iter()
        .find(|f| f.kind == kind)
        .expect("every kind has a bundled font")
}

/// Fonts safe to name directly, because they ship with common systems.
const SYSTEM_FONTS: &[(&str, Kind)] = &[
    ("system-ui", Kind::Sans),
    ("-apple-system", Kind::Sans),
    ("sans-serif", Kind::Sans),
    ("ui-sans-serif", Kind::Sans),
    ("arial", Kind::Sans),
    ("helvetica", Kind::Sans),
    ("helvetica neue", Kind::Sans),
    ("segoe ui", Kind::Sans),
    ("roboto", Kind::Sans),
    ("verdana", Kind::Sans),
    ("tahoma", Kind::Sans),
    ("trebuchet ms", Kind::Sans),
    ("serif", Kind::Serif),
    ("ui-serif", Kind::Serif),
    ("georgia", Kind::Serif),
    ("times", Kind::Serif),
    ("times new roman", Kind::Serif),
    ("palatino", Kind::Serif),
    ("monospace", Kind::Mono),
    ("ui-monospace", Kind::Mono),
    ("menlo", Kind::Mono),
    ("monaco", Kind::Mono),
    ("consolas", Kind::Mono),
    ("courier", Kind::Mono),
    ("courier new", Kind::Mono),
    ("sf mono", Kind::Mono),
];

fn system_font(name: &str) -> Option<Kind> {
    let lower = name.trim().to_ascii_lowercase();
    SYSTEM_FONTS
        .iter()
        .find(|(font, _)| *font == lower)
        .map(|(_, kind)| *kind)
}

/// Guesses a family's style from its name, for fonts Quill can't bundle.
fn guess_kind(name: &str) -> Kind {
    let lower = name.to_lowercase();
    let has = |words: &[&str]| words.iter().any(|w| lower.contains(w));
    if has(&["mono", "code", "courier", "consol", "menlo", "typewriter"]) {
        Kind::Mono
    } else if has(&["sans", "grotesk", "grotesque", "gothic"]) {
        Kind::Sans
    } else if has(&[
        "serif",
        "garamond",
        "caslon",
        "baskerville",
        "times",
        "georgia",
        "tiempos",
        "playfair",
        "merriweather",
        "lora",
        "didot",
        "bodoni",
        "canela",
        "sectra",
        "freight",
        "kalice",
        "minion",
        "palatino",
        "charter",
        "crimson",
        "spectral",
        "newsreader",
        "fraunces",
    ]) {
        Kind::Serif
    } else {
        Kind::Sans
    }
}

fn stack_for_bundled(font: &BundledFont) -> FontStack {
    let name = if font.family.contains(' ') {
        format!("\"{}\"", font.family)
    } else {
        font.family.to_string()
    };
    FontStack::parse(&format!("{name}, {}", font.kind.system_stack()))
        .expect("bundled names and system stacks are valid")
}

fn system_stack(kind: Kind) -> FontStack {
    FontStack::parse(kind.system_stack()).expect("system stacks are valid")
}

#[derive(Debug)]
pub(crate) struct ResolvedFonts {
    pub body: FontStack,
    pub heading: FontStack,
    pub mono: FontStack,
    /// Ids of the bundled fonts these use, which the stylesheet has to load.
    pub bundled: Vec<String>,
}

struct Choice {
    stack: FontStack,
    bundled: Option<&'static BundledFont>,
}

/// Picks a font for one role and reports any change from what the file asked for.
fn choose(decl: &FontDecl, wanted: Option<Kind>, diagnostics: &mut Vec<Diagnostic>) -> Choice {
    let family = decl.family.trim();
    let mut pick = |font: &'static BundledFont, reason: String, span: Span| {
        diagnostics.push(
            Diagnostic::new(
                Code::FontSwapped,
                format!("`{family}` isn't bundled with Quill, so {} is used instead{reason}", font.family),
            )
            .at(span)
            .help("Quill never loads fonts from other servers. If you have a license, install the font on your site separately."),
        );
        Choice {
            stack: stack_for_bundled(font),
            bundled: Some(font),
        }
    };

    if let Some(font) = bundled_by_name(family) {
        return Choice {
            stack: stack_for_bundled(font),
            bundled: Some(font),
        };
    }
    if let Some(kind) = system_font(family) {
        // Named exactly as written, ahead of the usual fallbacks.
        let is_generic = matches!(
            family.to_ascii_lowercase().as_str(),
            "sans-serif" | "serif" | "monospace" | "system-ui"
        );
        let stack = if is_generic {
            Ok(system_stack(kind))
        } else {
            FontStack::parse(&format!("\"{family}\", {}", kind.system_stack()))
        };
        return Choice {
            stack: stack.unwrap_or_else(|_| system_stack(kind)),
            bundled: None,
        };
    }
    if FontStack::parse(family).is_err() {
        diagnostics.push(
            Diagnostic::new(
                Code::InvalidValue,
                format!("`{family}` isn't a usable font name, so it was ignored"),
            )
            .at(decl.span)
            .help("Font names may contain only letters, numbers, spaces, hyphens, underscores and dots."),
        );
        let kind = wanted.unwrap_or(Kind::Sans);
        return Choice {
            stack: system_stack(kind),
            bundled: None,
        };
    }
    let kind = wanted.unwrap_or_else(|| guess_kind(family));
    let substitute = decl
        .substitutes
        .iter()
        .find_map(|name| bundled_by_name(name));
    if let Some(font) = substitute.filter(|f| wanted.is_none() || Some(f.kind) == wanted) {
        return pick(
            font,
            ", the substitute the file lists".to_string(),
            decl.span,
        );
    }
    let system_substitute = decl
        .substitutes
        .iter()
        .find(|name| system_font(name).is_some());
    if let Some(name) = system_substitute {
        let kind = system_font(name).unwrap_or(kind);
        diagnostics.push(
            Diagnostic::new(
                Code::FontSystem,
                format!("`{family}` isn't bundled with Quill, so the file's substitute `{name}` is used from the reader's device"),
            )
            .at(decl.span),
        );
        let stack = FontStack::parse(&format!("\"{name}\", {}", kind.system_stack()));
        return Choice {
            stack: stack.unwrap_or_else(|_| system_stack(kind)),
            bundled: None,
        };
    }
    pick(
        bundled_default(kind),
        ", the closest match".to_string(),
        decl.span,
    )
}

/// Assigns fonts to the body, heading and code roles.
pub(crate) fn resolve(decls: &[FontDecl], diagnostics: &mut Vec<Diagnostic>) -> ResolvedFonts {
    let mut ordered: Vec<&FontDecl> = decls.iter().collect();
    ordered.sort_by_key(|d| d.origin);
    let first = |role: FontRole| ordered.iter().copied().find(|d| d.role == role);
    let any = first(FontRole::Any);
    let body_decl = first(FontRole::Body).or(any);
    let heading_decl = first(FontRole::Heading).or(any).or(body_decl);
    let mono_decl = first(FontRole::Mono);

    let mut bundled: Vec<String> = Vec::new();
    let mut record = |choice: Choice| {
        if let Some(font) = choice.bundled {
            if !bundled.iter().any(|id| id == font.id) {
                bundled.push(font.id.to_string());
            }
        }
        choice.stack
    };

    let body_choice = body_decl.map(|d| choose(d, None, diagnostics));
    // The same family in two roles should only be reported once.
    let heading_choice = heading_decl.map(|d| {
        if body_decl.is_some_and(|b| b.family == d.family) {
            let mut quiet = Vec::new();
            choose(d, None, &mut quiet)
        } else {
            choose(d, None, diagnostics)
        }
    });
    let mono_choice = mono_decl.map(|d| choose(d, Some(Kind::Mono), diagnostics));

    let body = body_choice.map_or_else(|| system_stack(Kind::Sans), &mut record);
    let heading = heading_choice.map_or_else(|| body.clone(), &mut record);
    let mono = mono_choice.map_or_else(|| system_stack(Kind::Mono), &mut record);
    ResolvedFonts {
        body,
        heading,
        mono,
        bundled,
    }
}

#[cfg(test)]
mod tests {
    use super::super::extract::Origin;
    use super::*;

    const SPAN: Span = Span {
        line: 1,
        column: 1,
        len: 1,
    };

    fn decl(family: &str, role: FontRole, substitutes: &[&str]) -> FontDecl {
        FontDecl {
            family: family.to_string(),
            role,
            substitutes: substitutes.iter().map(|s| s.to_string()).collect(),
            span: SPAN,
            origin: Origin::List,
        }
    }

    fn run(decls: &[FontDecl]) -> (ResolvedFonts, Vec<Diagnostic>) {
        let mut diagnostics = Vec::new();
        let fonts = resolve(decls, &mut diagnostics);
        (fonts, diagnostics)
    }

    #[test]
    fn bundled_families_are_used_directly() {
        let (fonts, diagnostics) = run(&[decl("inter", FontRole::Body, &[])]);
        assert!(fonts.body.to_css().starts_with("Inter, system-ui"));
        assert_eq!(fonts.bundled, ["inter"]);
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn prefers_the_files_listed_substitute() {
        let (fonts, diagnostics) = run(&[decl(
            "Lausanne",
            FontRole::Any,
            &["Söhne", "IBM Plex Sans", "Inter"],
        )]);
        assert!(fonts.body.to_css().starts_with("\"IBM Plex Sans\""));
        assert_eq!(fonts.heading, fonts.body, "one family serves both roles");
        assert_eq!(fonts.bundled, ["ibm-plex-sans"]);
        assert_eq!(diagnostics.len(), 1, "reported once, not per role");
        assert_eq!(diagnostics[0].code, Code::FontSwapped);
        assert!(diagnostics[0].message.contains("substitute"));
    }

    #[test]
    fn guesses_the_closest_bundled_family() {
        for (family, expected) in [
            ("Tiempos Headline", "Source Serif 4"),
            ("Söhne", "Inter"),
            ("Berkeley Mono", "JetBrains Mono"),
            ("Neue Haas Grotesk", "Inter"),
            ("GT Sectra", "Source Serif 4"),
        ] {
            let (fonts, diagnostics) = run(&[decl(family, FontRole::Body, &[])]);
            assert!(
                fonts.body.to_css().contains(expected),
                "{family}: {}",
                fonts.body
            );
            assert_eq!(diagnostics[0].code, Code::FontSwapped, "{family}");
            assert!(diagnostics[0].message.contains("closest"), "{family}");
        }
    }

    #[test]
    fn system_fonts_stay_as_written() {
        let (fonts, diagnostics) = run(&[decl("Georgia", FontRole::Body, &[])]);
        assert!(
            fonts.body.to_css().starts_with("\"Georgia\", Georgia")
                || fonts.body.to_css().starts_with("Georgia")
        );
        assert!(fonts.bundled.is_empty());
        assert!(diagnostics.is_empty());
        let (fonts, _) = run(&[decl("sans-serif", FontRole::Body, &[])]);
        assert!(fonts.body.to_css().starts_with("system-ui"));
    }

    #[test]
    fn a_system_substitute_is_used_if_nothing_is_bundled() {
        let (fonts, diagnostics) = run(&[decl("Söhne", FontRole::Body, &["Arial"])]);
        assert!(fonts.body.to_css().contains("Arial"));
        assert!(fonts.bundled.is_empty());
        assert_eq!(diagnostics[0].code, Code::FontSystem);
    }

    #[test]
    fn roles_get_their_own_fonts() {
        let (fonts, _) = run(&[
            decl("Inter", FontRole::Body, &[]),
            decl("Source Serif 4", FontRole::Heading, &[]),
            decl("JetBrains Mono", FontRole::Mono, &[]),
        ]);
        assert!(fonts.body.to_css().starts_with("Inter"));
        assert!(fonts.heading.to_css().starts_with("\"Source Serif 4\""));
        assert!(fonts.mono.to_css().starts_with("\"JetBrains Mono\""));
        assert_eq!(fonts.bundled.len(), 3);
    }

    #[test]
    fn code_font_is_never_proportional() {
        let (fonts, _) = run(&[decl("Weird Display", FontRole::Mono, &["Inter"])]);
        assert!(fonts.mono.to_css().starts_with("\"JetBrains Mono\""));
    }

    #[test]
    fn hostile_names_never_reach_a_stack() {
        let (fonts, diagnostics) = run(&[decl("x; } body { display: none", FontRole::Body, &[])]);
        assert!(!fonts.body.to_css().contains("display"));
        assert_eq!(diagnostics[0].code, Code::InvalidValue);
    }

    #[test]
    fn without_fonts_it_uses_the_system_stacks() {
        let (fonts, diagnostics) = run(&[]);
        assert!(fonts.body.to_css().starts_with("system-ui"));
        assert_eq!(fonts.heading, fonts.body);
        assert!(fonts.mono.to_css().starts_with("ui-monospace"));
        assert!(fonts.bundled.is_empty());
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn every_bundled_file_exists_and_is_woff2() {
        for font in BUNDLED {
            assert!(bundled_by_id(font.id).is_some());
            for face in font.faces {
                let path = format!(
                    "{}/../../static/fonts/{}",
                    env!("CARGO_MANIFEST_DIR"),
                    face.file
                );
                let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
                assert_eq!(&bytes[..4], b"wOF2", "{path}");
            }
            let license = format!(
                "{}/../../static/fonts/licenses/{}.txt",
                env!("CARGO_MANIFEST_DIR"),
                font.id
            );
            let text =
                std::fs::read_to_string(&license).unwrap_or_else(|e| panic!("{license}: {e}"));
            assert!(
                text.contains("SIL OPEN FONT LICENSE") || text.contains("SIL Open Font License"),
                "{license}"
            );
        }
    }
}
