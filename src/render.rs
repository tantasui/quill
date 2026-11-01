use std::fmt::Write as _;

use anyhow::{Context, Result};
use minijinja::{AutoEscape, Environment};
use rust_embed::RustEmbed;
use serde::Serialize;

#[derive(RustEmbed)]
#[folder = "templates/"]
struct TemplateFiles;

#[derive(RustEmbed)]
#[folder = "static/"]
pub struct StaticFiles;

/// The template environment. Templates are compiled into the binary; debug
/// builds read them from disk so edits show up on reload.
pub struct Views {
    env: Environment<'static>,
}

impl Default for Views {
    fn default() -> Self {
        Self::new()
    }
}

impl Views {
    pub fn new() -> Self {
        let mut env = Environment::new();
        env.set_loader(|name| {
            Ok(TemplateFiles::get(name).map(|f| String::from_utf8_lossy(&f.data).into_owned()))
        });
        env.set_auto_escape_callback(|name| {
            if name.ends_with(".html") || name.ends_with(".xml") {
                AutoEscape::Html
            } else {
                AutoEscape::None
            }
        });
        // The default HTML escaper also encodes `/`, which turns every URL into
        // `&#x2f;blog&#x2f;…`. Escaping these five characters is sufficient.
        env.set_formatter(|out, state, value| match value.as_str() {
            Some(s) if state.auto_escape() == AutoEscape::Html && !value.is_safe() => {
                for c in s.chars() {
                    match c {
                        '&' => out.write_str("&amp;")?,
                        '<' => out.write_str("&lt;")?,
                        '>' => out.write_str("&gt;")?,
                        '"' => out.write_str("&quot;")?,
                        '\'' => out.write_str("&#39;")?,
                        c => out.write_char(c)?,
                    }
                }
                Ok(())
            }
            _ => minijinja::escape_formatter(out, state, value),
        });
        env.set_trim_blocks(true);
        env.set_lstrip_blocks(true);
        env.add_filter("filesize", |bytes: i64| -> String {
            match bytes {
                b if b >= 1 << 20 => format!("{:.1} MB", b as f64 / (1u64 << 20) as f64),
                b if b >= 1 << 10 => format!("{} KB", b >> 10),
                b => format!("{b} B"),
            }
        });
        Views { env }
    }

    pub fn render(&self, name: &str, ctx: impl Serialize) -> Result<String> {
        let tmpl = self
            .env
            .get_template(name)
            .with_context(|| format!("template {name}"))?;
        tmpl.render(ctx)
            .with_context(|| format!("rendering {name}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every template must parse; this catches syntax errors in themes.
    #[test]
    fn all_templates_compile() {
        let views = Views::new();
        for name in TemplateFiles::iter() {
            views
                .env
                .get_template(&name)
                .unwrap_or_else(|e| panic!("{name}: {e:#}"));
        }
    }
}
