use anyhow::Result;
use serde::Serialize;
use sqlx::SqlitePool;

use crate::themes;

#[derive(Clone, Debug, Serialize)]
pub struct Settings {
    pub site_name: String,
    pub description: String,
    /// Public address of the blog without a trailing slash, e.g.
    /// `https://example.com/blog` or `https://blog.example.com`.
    pub base_url: String,
    /// Path part of `base_url` (`/blog`, or empty for a subdomain). Derived.
    pub base_path: String,
    pub theme: String,
    pub author_name: String,
    pub posts_per_page: i64,
    pub language: String,
    pub footer_text: String,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            site_name: "My Blog".into(),
            description: String::new(),
            base_url: "http://localhost:3000".into(),
            base_path: String::new(),
            theme: themes::DEFAULT.into(),
            author_name: String::new(),
            posts_per_page: 10,
            language: "en".into(),
            footer_text: String::new(),
        }
    }
}

impl Settings {
    pub async fn load(db: &SqlitePool) -> Result<Self> {
        let rows: Vec<(String, String)> = sqlx::query_as("SELECT key, value FROM settings")
            .fetch_all(db)
            .await?;
        let mut s = Settings::default();
        for (key, value) in rows {
            match key.as_str() {
                "site_name" => s.site_name = value,
                "description" => s.description = value,
                "base_url" => s.set_base_url(&value),
                "theme" if themes::find(&value).is_some() => s.theme = value,
                "author_name" => s.author_name = value,
                "posts_per_page" => s.posts_per_page = value.parse().unwrap_or(10).clamp(1, 50),
                "language" => s.language = value,
                "footer_text" => s.footer_text = value,
                _ => {}
            }
        }
        Ok(s)
    }

    pub async fn save(&self, db: &SqlitePool) -> Result<()> {
        let pairs = [
            ("site_name", self.site_name.clone()),
            ("description", self.description.clone()),
            ("base_url", self.base_url.clone()),
            ("theme", self.theme.clone()),
            ("author_name", self.author_name.clone()),
            ("posts_per_page", self.posts_per_page.to_string()),
            ("language", self.language.clone()),
            ("footer_text", self.footer_text.clone()),
        ];
        let mut tx = db.begin().await?;
        for (key, value) in pairs {
            sqlx::query(
                "INSERT INTO settings (key, value) VALUES (?, ?)
                 ON CONFLICT (key) DO UPDATE SET value = excluded.value",
            )
            .bind(key)
            .bind(value)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    /// Sets the public URL. Callers validate with [`normalize_base_url`] first.
    pub fn set_base_url(&mut self, value: &str) {
        let value = value.trim_end_matches('/');
        self.base_url = value.to_string();
        self.base_path = url::Url::parse(value)
            .map(|u| u.path().trim_end_matches('/').to_string())
            .unwrap_or_default();
    }

    /// Absolute URL for a blog-relative path such as `/posts/hello`.
    pub fn absolute(&self, path: &str) -> String {
        format!("{}{}", self.base_url, path)
    }

    /// Scheme, host and port of the public URL, e.g. `https://example.com`.
    pub fn origin(&self) -> String {
        url::Url::parse(&self.base_url)
            .map(|u| u.origin().ascii_serialization())
            .unwrap_or_default()
    }
}

/// Validates a user-supplied public URL and returns it in canonical form.
pub fn normalize_base_url(input: &str) -> Result<String, String> {
    let input = input.trim();
    if input.is_empty() {
        return Err(
            "Enter the address where your blog will be visible, such as https://example.com/blog."
                .into(),
        );
    }
    let with_scheme = if input.contains("://") {
        input.to_string()
    } else {
        format!("https://{input}")
    };
    let url = url::Url::parse(&with_scheme).map_err(|_| {
        format!("“{input}” isn't a valid web address. Use a form like https://example.com/blog.")
    })?;
    if url.scheme() != "http" && url.scheme() != "https" {
        return Err("The address must start with http:// or https://.".into());
    }
    if url.host_str().is_none() {
        return Err("The address needs a domain name, such as example.com.".into());
    }
    if url.query().is_some() || url.fragment().is_some() {
        return Err("The address can't contain “?” or “#”. Use only the domain and an optional path like /blog.".into());
    }
    let path = url.path().trim_end_matches('/');
    if path
        .split('/')
        .any(|seg| seg == "admin" || seg == "static" || seg == "media")
    {
        return Err("The path can't include “admin”, “static” or “media”; those names are used by the blog itself.".into());
    }
    Ok(format!("{}{}", url.origin().ascii_serialization(), path))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_urls() {
        assert_eq!(
            normalize_base_url("example.com/blog/").unwrap(),
            "https://example.com/blog"
        );
        assert_eq!(
            normalize_base_url("http://localhost:3000").unwrap(),
            "http://localhost:3000"
        );
        assert_eq!(
            normalize_base_url("https://Blog.Example.com/").unwrap(),
            "https://blog.example.com"
        );
        assert!(normalize_base_url("ftp://example.com").is_err());
        assert!(normalize_base_url("https://example.com/blog?x=1").is_err());
        assert!(normalize_base_url("").is_err());
    }

    #[test]
    fn derives_base_path() {
        let mut s = Settings::default();
        s.set_base_url("https://example.com/blog");
        assert_eq!(s.base_path, "/blog");
        assert_eq!(s.absolute("/posts/a"), "https://example.com/blog/posts/a");
        assert_eq!(s.origin(), "https://example.com");
        s.set_base_url("https://blog.example.com");
        assert_eq!(s.base_path, "");
    }
}
