use std::collections::HashMap;

use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::{FromRow, SqlitePool};

#[derive(Debug, Clone, FromRow)]
pub struct Article {
    pub id: i64,
    pub title: String,
    pub slug: String,
    pub content: String,
    pub excerpt: String,
    pub status: String,
    pub cover_image: String,
    pub meta_title: String,
    pub meta_description: String,
    pub author_id: Option<i64>,
    pub published_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Article {
    pub fn is_published(&self) -> bool {
        self.status == "published"
    }

    /// Published with a publication date still in the future.
    pub fn is_scheduled(&self, now: DateTime<Utc>) -> bool {
        self.is_published() && self.published_at.is_some_and(|p| p > now)
    }

    pub fn state_label(&self, now: DateTime<Utc>) -> &'static str {
        if self.is_scheduled(now) {
            "scheduled"
        } else if self.is_published() {
            "published"
        } else {
            "draft"
        }
    }
}

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct Tag {
    pub id: i64,
    pub name: String,
    pub slug: String,
}

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct TagCount {
    pub name: String,
    pub slug: String,
    pub count: i64,
}

#[derive(Debug, Clone, Default)]
pub struct ArticleInput {
    pub title: String,
    pub slug: String,
    pub content: String,
    pub excerpt: String,
    pub cover_image: String,
    pub meta_title: String,
    pub meta_description: String,
    pub status: String,
    pub published_at: Option<DateTime<Utc>>,
    pub tags: Vec<String>,
}

const PUBLIC: &str = "status = 'published' AND published_at <= ?";

// ---- Public queries -------------------------------------------------------

pub async fn list_published(db: &SqlitePool, limit: i64, offset: i64) -> Result<Vec<Article>> {
    Ok(sqlx::query_as(&format!(
        "SELECT * FROM articles WHERE {PUBLIC} ORDER BY published_at DESC, id DESC LIMIT ? OFFSET ?"
    ))
    .bind(Utc::now())
    .bind(limit)
    .bind(offset)
    .fetch_all(db)
    .await?)
}

pub async fn count_published(db: &SqlitePool) -> Result<i64> {
    Ok(
        sqlx::query_scalar(&format!("SELECT COUNT(*) FROM articles WHERE {PUBLIC}"))
            .bind(Utc::now())
            .fetch_one(db)
            .await?,
    )
}

pub async fn list_published_by_tag(
    db: &SqlitePool,
    tag_id: i64,
    limit: i64,
    offset: i64,
) -> Result<Vec<Article>> {
    Ok(sqlx::query_as(&format!(
        "SELECT * FROM articles WHERE {PUBLIC}
           AND id IN (SELECT article_id FROM article_tags WHERE tag_id = ?)
         ORDER BY published_at DESC, id DESC LIMIT ? OFFSET ?"
    ))
    .bind(Utc::now())
    .bind(tag_id)
    .bind(limit)
    .bind(offset)
    .fetch_all(db)
    .await?)
}

pub async fn count_published_by_tag(db: &SqlitePool, tag_id: i64) -> Result<i64> {
    Ok(sqlx::query_scalar(&format!(
        "SELECT COUNT(*) FROM articles WHERE {PUBLIC}
           AND id IN (SELECT article_id FROM article_tags WHERE tag_id = ?)"
    ))
    .bind(Utc::now())
    .bind(tag_id)
    .fetch_one(db)
    .await?)
}

pub async fn find_published(db: &SqlitePool, slug: &str) -> Result<Option<Article>> {
    Ok(sqlx::query_as(&format!(
        "SELECT * FROM articles WHERE slug = ? AND {PUBLIC}"
    ))
    .bind(slug)
    .bind(Utc::now())
    .fetch_optional(db)
    .await?)
}

/// The current slug of a published article that used to live at `old_slug`.
pub async fn find_redirect(db: &SqlitePool, old_slug: &str) -> Result<Option<String>> {
    Ok(sqlx::query_scalar(&format!(
        "SELECT a.slug FROM slug_redirects r JOIN articles a ON a.id = r.article_id
         WHERE r.old_slug = ? AND a.{PUBLIC}"
    ))
    .bind(old_slug)
    .bind(Utc::now())
    .fetch_optional(db)
    .await?)
}

/// The published articles immediately older and newer than `a`.
pub async fn neighbours(
    db: &SqlitePool,
    a: &Article,
) -> Result<(Option<Article>, Option<Article>)> {
    let Some(at) = a.published_at else {
        return Ok((None, None));
    };
    let older = sqlx::query_as(&format!(
        "SELECT * FROM articles WHERE {PUBLIC} AND (published_at, id) < (?, ?)
         ORDER BY published_at DESC, id DESC LIMIT 1"
    ))
    .bind(Utc::now())
    .bind(at)
    .bind(a.id)
    .fetch_optional(db)
    .await?;
    let newer = sqlx::query_as(&format!(
        "SELECT * FROM articles WHERE {PUBLIC} AND (published_at, id) > (?, ?)
         ORDER BY published_at ASC, id ASC LIMIT 1"
    ))
    .bind(Utc::now())
    .bind(at)
    .bind(a.id)
    .fetch_optional(db)
    .await?;
    Ok((older, newer))
}

pub async fn find_tag(db: &SqlitePool, slug: &str) -> Result<Option<Tag>> {
    Ok(sqlx::query_as("SELECT * FROM tags WHERE slug = ?")
        .bind(slug)
        .fetch_optional(db)
        .await?)
}

/// Tags with their number of publicly visible articles, most used first.
pub async fn public_tag_counts(db: &SqlitePool, limit: i64) -> Result<Vec<TagCount>> {
    Ok(sqlx::query_as(&format!(
        "SELECT t.name, t.slug, COUNT(*) AS count
         FROM tags t
         JOIN article_tags at ON at.tag_id = t.id
         JOIN articles a ON a.id = at.article_id
         WHERE a.{PUBLIC}
         GROUP BY t.id ORDER BY count DESC, t.name LIMIT ?"
    ))
    .bind(Utc::now())
    .bind(limit)
    .fetch_all(db)
    .await?)
}

pub async fn tags_for(db: &SqlitePool, article_ids: &[i64]) -> Result<HashMap<i64, Vec<Tag>>> {
    let mut map: HashMap<i64, Vec<Tag>> = HashMap::new();
    if article_ids.is_empty() {
        return Ok(map);
    }
    let placeholders = vec!["?"; article_ids.len()].join(",");
    let sql = format!(
        "SELECT at.article_id, t.id, t.name, t.slug FROM article_tags at
         JOIN tags t ON t.id = at.tag_id
         WHERE at.article_id IN ({placeholders}) ORDER BY t.name"
    );
    let mut q = sqlx::query_as::<_, (i64, i64, String, String)>(&sql);
    for id in article_ids {
        q = q.bind(id);
    }
    for (article_id, id, name, slug) in q.fetch_all(db).await? {
        map.entry(article_id)
            .or_default()
            .push(Tag { id, name, slug });
    }
    Ok(map)
}

// ---- Administration ---------------------------------------------------------

pub async fn get(db: &SqlitePool, id: i64) -> Result<Option<Article>> {
    Ok(sqlx::query_as("SELECT * FROM articles WHERE id = ?")
        .bind(id)
        .fetch_optional(db)
        .await?)
}

/// All articles for the dashboard. `filter` is `all`, `draft`, `published` or
/// `scheduled`.
pub async fn list_admin(db: &SqlitePool, filter: &str, limit: i64) -> Result<Vec<Article>> {
    let condition = match filter {
        "draft" => "status = 'draft'",
        "published" => "status = 'published' AND published_at <= ?1",
        "scheduled" => "status = 'published' AND published_at > ?1",
        _ => "?1 IS NOT NULL",
    };
    Ok(sqlx::query_as(&format!(
        "SELECT * FROM articles WHERE {condition}
         ORDER BY CASE WHEN status = 'draft' THEN updated_at ELSE published_at END DESC, id DESC
         LIMIT ?2"
    ))
    .bind(Utc::now())
    .bind(limit)
    .fetch_all(db)
    .await?)
}

pub async fn all_articles(db: &SqlitePool) -> Result<Vec<Article>> {
    Ok(sqlx::query_as("SELECT * FROM articles ORDER BY id")
        .fetch_all(db)
        .await?)
}

#[derive(Debug, Default, Serialize)]
pub struct Counts {
    pub published: i64,
    pub scheduled: i64,
    pub drafts: i64,
}

pub async fn counts(db: &SqlitePool) -> Result<Counts> {
    let (published, scheduled, drafts): (i64, i64, i64) = sqlx::query_as(
        "SELECT
            COALESCE(SUM(status = 'published' AND published_at <= ?1), 0),
            COALESCE(SUM(status = 'published' AND published_at > ?1), 0),
            COALESCE(SUM(status = 'draft'), 0)
         FROM articles",
    )
    .bind(Utc::now())
    .fetch_one(db)
    .await?;
    Ok(Counts {
        published,
        scheduled,
        drafts,
    })
}

/// Another article already using `slug`, if any.
pub async fn slug_owner(
    db: &SqlitePool,
    slug: &str,
    except_id: Option<i64>,
) -> Result<Option<String>> {
    Ok(
        sqlx::query_scalar("SELECT title FROM articles WHERE slug = ? AND id IS NOT ?")
            .bind(slug)
            .bind(except_id)
            .fetch_optional(db)
            .await?,
    )
}

/// `base`, or `base-2`, `base-3`… whichever is free.
pub async fn unique_slug(db: &SqlitePool, base: &str, except_id: Option<i64>) -> Result<String> {
    let mut candidate = base.to_string();
    let mut n = 2;
    while slug_owner(db, &candidate, except_id).await?.is_some() {
        candidate = format!("{base}-{n}");
        n += 1;
    }
    Ok(candidate)
}

pub async fn create(db: &SqlitePool, input: &ArticleInput, author_id: i64) -> Result<i64> {
    let now = Utc::now();
    let mut tx = db.begin().await?;
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO articles (title, slug, content, excerpt, status, cover_image, meta_title,
                               meta_description, author_id, published_at, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) RETURNING id",
    )
    .bind(&input.title)
    .bind(&input.slug)
    .bind(&input.content)
    .bind(&input.excerpt)
    .bind(&input.status)
    .bind(&input.cover_image)
    .bind(&input.meta_title)
    .bind(&input.meta_description)
    .bind(author_id)
    .bind(input.published_at)
    .bind(now)
    .bind(now)
    .fetch_one(&mut *tx)
    .await?;
    sqlx::query("DELETE FROM slug_redirects WHERE old_slug = ?")
        .bind(&input.slug)
        .execute(&mut *tx)
        .await?;
    set_tags(&mut tx, id, &input.tags).await?;
    tx.commit().await?;
    Ok(id)
}

pub async fn update(db: &SqlitePool, existing: &Article, input: &ArticleInput) -> Result<()> {
    let mut tx = db.begin().await?;
    if existing.slug != input.slug {
        if existing.is_published() {
            sqlx::query(
                "INSERT OR REPLACE INTO slug_redirects (old_slug, article_id) VALUES (?, ?)",
            )
            .bind(&existing.slug)
            .bind(existing.id)
            .execute(&mut *tx)
            .await?;
        }
        sqlx::query("DELETE FROM slug_redirects WHERE old_slug = ?")
            .bind(&input.slug)
            .execute(&mut *tx)
            .await?;
    }
    sqlx::query(
        "UPDATE articles SET title = ?, slug = ?, content = ?, excerpt = ?, status = ?,
             cover_image = ?, meta_title = ?, meta_description = ?, published_at = ?, updated_at = ?
         WHERE id = ?",
    )
    .bind(&input.title)
    .bind(&input.slug)
    .bind(&input.content)
    .bind(&input.excerpt)
    .bind(&input.status)
    .bind(&input.cover_image)
    .bind(&input.meta_title)
    .bind(&input.meta_description)
    .bind(input.published_at)
    .bind(Utc::now())
    .bind(existing.id)
    .execute(&mut *tx)
    .await?;
    set_tags(&mut tx, existing.id, &input.tags).await?;
    tx.commit().await?;
    Ok(())
}

pub async fn delete(db: &SqlitePool, id: i64) -> Result<()> {
    let mut tx = db.begin().await?;
    sqlx::query("DELETE FROM articles WHERE id = ?")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    delete_orphan_tags(&mut tx).await?;
    tx.commit().await?;
    Ok(())
}

async fn set_tags(
    tx: &mut sqlx::SqliteConnection,
    article_id: i64,
    names: &[String],
) -> Result<()> {
    sqlx::query("DELETE FROM article_tags WHERE article_id = ?")
        .bind(article_id)
        .execute(&mut *tx)
        .await?;
    for name in names {
        let slug = slug::slugify(name);
        if slug.is_empty() {
            continue;
        }
        let tag_id: i64 = sqlx::query_scalar(
            "INSERT INTO tags (name, slug) VALUES (?, ?)
             ON CONFLICT (slug) DO UPDATE SET slug = excluded.slug RETURNING id",
        )
        .bind(name)
        .bind(&slug)
        .fetch_one(&mut *tx)
        .await?;
        sqlx::query("INSERT OR IGNORE INTO article_tags (article_id, tag_id) VALUES (?, ?)")
            .bind(article_id)
            .bind(tag_id)
            .execute(&mut *tx)
            .await?;
    }
    delete_orphan_tags(tx).await
}

async fn delete_orphan_tags(tx: &mut sqlx::SqliteConnection) -> Result<()> {
    sqlx::query("DELETE FROM tags WHERE id NOT IN (SELECT tag_id FROM article_tags)")
        .execute(&mut *tx)
        .await?;
    Ok(())
}

/// Splits a comma-separated tag list, dropping blanks and duplicates.
pub fn parse_tags(input: &str) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    input
        .split(',')
        .map(|t| t.trim().chars().take(50).collect::<String>())
        .filter(|t| !t.is_empty() && seen.insert(slug::slugify(t)))
        .take(20)
        .collect()
}

// ---- Media ------------------------------------------------------------------

#[derive(Debug, Clone, FromRow)]
pub struct Media {
    pub id: i64,
    pub filename: String,
    pub original_name: String,
    pub content_type: String,
    pub size: i64,
    pub created_at: DateTime<Utc>,
}

pub async fn list_media(db: &SqlitePool) -> Result<Vec<Media>> {
    Ok(
        sqlx::query_as("SELECT * FROM media ORDER BY created_at DESC, id DESC")
            .fetch_all(db)
            .await?,
    )
}
