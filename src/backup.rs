//! Backup, restore and export. A backup is a plain folder holding a database
//! snapshot and a copy of the uploaded media, so it can be inspected, zipped or
//! synced with ordinary tools.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use chrono::Utc;
use serde_json::json;

use crate::db::{self, DB_FILE};
use crate::models;

fn timestamp() -> String {
    Utc::now().format("%Y%m%d-%H%M%S").to_string()
}

fn copy_dir(from: &Path, to: &Path) -> Result<usize> {
    let mut n = 0;
    if !from.exists() {
        return Ok(0);
    }
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        if entry.file_type()?.is_file() {
            std::fs::copy(entry.path(), to.join(entry.file_name()))?;
            n += 1;
        }
    }
    Ok(n)
}

fn existing_db(data_dir: &Path) -> Result<PathBuf> {
    let path = data_dir.join(DB_FILE);
    if !path.exists() {
        bail!(
            "no blog database at {}. Pass the same --data-dir (or QUILL_DATA_DIR) the server uses.",
            path.display()
        );
    }
    Ok(path)
}

/// Writes a backup folder and returns its path. Safe to run while the server
/// is running.
pub async fn backup(data_dir: &Path, out: Option<PathBuf>) -> Result<PathBuf> {
    let db_path = existing_db(data_dir)?;
    let out = out.unwrap_or_else(|| data_dir.join("backups").join(format!("backup-{}", timestamp())));
    if out.join(DB_FILE).exists() {
        bail!("{} already contains a backup; choose an empty folder", out.display());
    }
    std::fs::create_dir_all(&out).with_context(|| format!("could not create {}", out.display()))?;
    let pool = db::connect(&db_path).await?;
    db::snapshot(&pool, &out.join(DB_FILE)).await?;
    let articles = models::all_articles(&pool).await?.len();
    pool.close().await;
    let media = copy_dir(&data_dir.join("media"), &out.join("media"))?;
    let manifest = json!({
        "quill_version": env!("CARGO_PKG_VERSION"),
        "created_at": Utc::now().to_rfc3339(),
        "articles": articles,
        "media_files": media,
    });
    std::fs::write(out.join("manifest.json"), serde_json::to_string_pretty(&manifest)?)?;
    Ok(out)
}

/// Replaces the blog's data with a backup. The server must be stopped. The
/// current data is moved aside first, never deleted.
pub async fn restore(data_dir: &Path, from: &Path) -> Result<PathBuf> {
    let source_db = from.join(DB_FILE);
    if !source_db.exists() {
        bail!("{} doesn't look like a Quill backup (no {DB_FILE} inside)", from.display());
    }
    // Make sure the snapshot is a readable blog database before touching anything.
    let pool = db::connect(&source_db).await?;
    let check: String = sqlx::query_scalar("PRAGMA integrity_check").fetch_one(&pool).await?;
    let has_articles: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'articles'")
            .fetch_one(&pool)
            .await?;
    pool.close().await;
    if check != "ok" || has_articles == 0 {
        bail!("the backup database failed its integrity check ({check}); nothing was changed");
    }

    std::fs::create_dir_all(data_dir)?;
    let aside = data_dir.join(format!("pre-restore-{}", timestamp()));
    std::fs::create_dir_all(&aside)?;
    for name in [DB_FILE.to_string(), format!("{DB_FILE}-wal"), format!("{DB_FILE}-shm"), "media".to_string()] {
        let path = data_dir.join(&name);
        if path.exists() {
            std::fs::rename(&path, aside.join(&name))
                .with_context(|| format!("could not move {} aside", path.display()))?;
        }
    }
    std::fs::copy(&source_db, data_dir.join(DB_FILE))?;
    copy_dir(&from.join("media"), &data_dir.join("media"))?;
    Ok(aside)
}

fn yaml_str(s: &str) -> String {
    // JSON strings are valid YAML scalars and handle all escaping.
    serde_json::to_string(s).unwrap_or_default()
}

/// Exports every article as a Markdown file with YAML front matter, plus the
/// media folder: a portable copy that other tools (static site generators,
/// other blogging engines) can import.
pub async fn export(data_dir: &Path, out: &Path) -> Result<usize> {
    let pool = db::connect(&existing_db(data_dir)?).await?;
    let articles = models::all_articles(&pool).await?;
    let ids: Vec<i64> = articles.iter().map(|a| a.id).collect();
    let tags = models::tags_for(&pool, &ids).await?;
    pool.close().await;

    std::fs::create_dir_all(out)?;
    for a in &articles {
        let tag_names: Vec<String> = tags.get(&a.id).into_iter().flatten().map(|t| yaml_str(&t.name)).collect();
        let mut fm = vec![
            format!("title: {}", yaml_str(&a.title)),
            format!("slug: {}", yaml_str(&a.slug)),
            format!("status: {}", a.status),
        ];
        if let Some(d) = a.published_at {
            fm.push(format!("date: {}", d.to_rfc3339()));
        }
        fm.push(format!("updated: {}", a.updated_at.to_rfc3339()));
        fm.push(format!("tags: [{}]", tag_names.join(", ")));
        for (key, value) in [
            ("excerpt", &a.excerpt),
            ("cover_image", &a.cover_image),
            ("meta_title", &a.meta_title),
            ("meta_description", &a.meta_description),
        ] {
            if !value.is_empty() {
                fm.push(format!("{key}: {}", yaml_str(value)));
            }
        }
        let doc = format!("---\n{}\n---\n\n{}\n", fm.join("\n"), a.content.trim_end());
        std::fs::write(out.join(format!("{}.md", a.slug)), doc)?;
    }
    copy_dir(&data_dir.join("media"), &out.join("media"))?;
    Ok(articles.len())
}
