//! The DESIGN.md compiler: parse, extract, normalize, validate, compile.
//!
//! It reads whatever a file provides, maps it onto Quill's semantic roles and
//! checks the result. Nothing from the input reaches a stylesheet as text: every
//! value is parsed into a typed token, and CSS is written back from that.

mod css;
mod dark;
pub mod diagnostic;
mod extract;
pub mod fonts;
mod markdown;
mod normalize;
mod palette;
mod prose;
pub mod report;
mod roles;
mod yaml;

use std::fmt::Write;

use serde::{Deserialize, Serialize};

use crate::{DesignTokens, FontWeight, Length, Palette, Shadow};
use diagnostic::{Code, Diagnostic, LineIndex};
pub use extract::Scheme;

/// The largest file the compiler accepts.
pub const MAX_SOURCE_BYTES: usize = 256 * 1024;

/// Which line of the file a token's value came from.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenOrigin {
    pub token: String,
    pub line: u32,
}

/// A design that passed every check and is safe to apply.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CompiledDesign {
    pub name: Option<String>,
    pub tokens: DesignTokens,
    /// Ids of the bundled fonts the tokens use.
    pub fonts: Vec<String>,
    /// Set when the file states a heading weight, to override the weights a
    /// preset's stylesheet gives its titles.
    pub title_weight: Option<FontWeight>,
    pub page_width: Option<Length>,
    pub origins: Vec<TokenOrigin>,
}

impl CompiledDesign {
    /// The stylesheet for this design. `base_path` prefixes font URLs.
    pub fn to_css(&self, base_path: &str) -> String {
        let mut css = self.font_faces(base_path);
        css.push_str(&self.tokens.to_css());
        css.push_str(&self.overrides());
        css
    }

    /// The same with one color scheme forced on, for previewing either scheme
    /// whatever the viewer's own preference is.
    pub fn to_css_forced(&self, base_path: &str, scheme: Scheme) -> String {
        let palette: &Palette = match scheme {
            Scheme::Light => &self.tokens.light,
            Scheme::Dark => &self.tokens.dark,
        };
        let mut css = self.font_faces(base_path);
        css.push_str(&self.tokens.to_css_forced(palette));
        css.push_str(&self.overrides());
        css
    }

    fn font_faces(&self, base_path: &str) -> String {
        let mut css = String::new();
        for font in self.fonts.iter().filter_map(|id| fonts::bundled_by_id(id)) {
            for face in font.faces {
                let _ = writeln!(
                    css,
                    "@font-face {{ font-family: \"{}\"; src: url(\"{base_path}/static/fonts/{}\") format(\"woff2\"); font-weight: {}; font-style: {}; font-display: swap; }}",
                    font.family, face.file, font.weights, face.style
                );
            }
        }
        if !css.is_empty() {
            css.push('\n');
        }
        css
    }

    /// Values preset stylesheets read through fallbacks, so a design can reach
    /// the finishing touches its tokens don't cover.
    fn overrides(&self) -> String {
        let mut css = String::from("\n:root {\n");
        let _ = writeln!(
            css,
            "  --design-font-ui: {};",
            self.tokens.font_body.to_css()
        );
        let _ = writeln!(css, "  --image-radius: {};", self.tokens.radius.to_css());
        if let Some(weight) = self.title_weight {
            let _ = writeln!(css, "  --title-weight: {};", weight.to_css());
        }
        css.push_str("}\n");
        if let Some(width) = self.page_width {
            let _ = writeln!(
                css,
                "\nbody.width-regular, body.width-wide {{ --page-width: {}; }}",
                width.to_css()
            );
        }
        css
    }
}

/// The result of compiling a file. `design` is present only when there are no
/// errors, so a design that failed can't be applied by accident.
#[derive(Clone, Debug)]
pub struct Compilation {
    pub design: Option<CompiledDesign>,
    pub diagnostics: Vec<Diagnostic>,
}

impl Compilation {
    pub fn has_errors(&self) -> bool {
        self.design.is_none()
    }

    fn failed(diagnostic: Diagnostic) -> Self {
        Compilation {
            design: None,
            diagnostics: vec![diagnostic],
        }
    }
}

/// Compiles an uploaded file, which might not be text at all.
pub fn compile_bytes(bytes: &[u8]) -> Compilation {
    match std::str::from_utf8(bytes) {
        Ok(source) => compile(source),
        // The size limit applies first: there's no reason to scan a huge file.
        Err(_) if bytes.len() > MAX_SOURCE_BYTES => too_large(bytes.len()),
        Err(error) => Compilation::failed(
            Diagnostic::new(
                Code::NotUtf8,
                format!(
                    "this file isn't UTF-8 text (the first problem is at byte {})",
                    error.valid_up_to()
                ),
            )
            .help("Save it as UTF-8 Markdown and try again."),
        ),
    }
}

fn too_large(bytes: usize) -> Compilation {
    Compilation::failed(
        Diagnostic::new(
            Code::SourceTooLarge,
            format!(
                "this file is {} KB, and the limit is {} KB",
                bytes / 1024,
                MAX_SOURCE_BYTES / 1024
            ),
        )
        .help("A DESIGN.md file is normally far smaller. Check that this is the right file."),
    )
}

pub fn compile(source: &str) -> Compilation {
    if source.len() > MAX_SOURCE_BYTES {
        return too_large(source.len());
    }

    // Parse and extract.
    let index = LineIndex::new(source);
    let mut extracted = extract::Extracted::default();
    let body_start = match yaml::split_front_matter(source) {
        Some((text, lines_before, body_start)) => {
            yaml::extract(text, lines_before as u32, &mut extracted);
            body_start
        }
        None => {
            if source.trim_start_matches('\u{feff}').starts_with("---\n") {
                extracted.diagnostics.push(
                    Diagnostic::new(Code::MalformedYaml, "the front matter is never closed")
                        .at(index.span(0, 3))
                        .help("End the block with a line containing only ---."),
                );
            }
            0
        }
    };
    markdown::extract(source, body_start, &index, &mut extracted);

    // Normalize and validate.
    let mut diagnostics = std::mem::take(&mut extracted.diagnostics);
    let rules = prose::apply(&extracted.prose, &mut diagnostics);
    let fonts = fonts::resolve(&extracted.fonts, &mut diagnostics);
    let values = normalize::scalars(&extracted, &rules, &mut diagnostics);
    let colors = normalize::colors(&extracted, &mut diagnostics);
    let palettes = normalize::palettes(&colors, extracted.theme, &mut diagnostics);
    normalize::report_unused(&extracted, &colors, &mut diagnostics);

    let design = palettes.map(|(light, dark)| {
        let mut origins = colors.origins.clone();
        origins.extend(values.origins.iter().cloned());
        CompiledDesign {
            name: extracted.name.clone(),
            tokens: DesignTokens {
                light,
                dark,
                font_body: fonts.body,
                font_heading: fonts.heading,
                font_mono: fonts.mono,
                heading_weight: values
                    .heading_weight
                    .unwrap_or_else(|| FontWeight::new(700).expect("700 is valid")),
                radius: values.radius,
                measure: values.measure,
                shadow: values.shadow.unwrap_or(Shadow::None),
            },
            fonts: fonts.bundled,
            title_weight: values.heading_weight,
            page_width: values.page_width,
            origins,
        }
    });

    // Most serious first, then in file order. The sort is stable, so messages
    // with no location stay where they were found.
    diagnostics.sort_by_key(|d| (d.level, d.span.map_or(u32::MAX, |s| s.line)));
    Compilation {
        design,
        diagnostics,
    }
}
