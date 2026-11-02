//! Imported designs in the database. Compiling happens in `quill-design`; this
//! module only stores results and tracks which one is live.

use anyhow::Result;
use chrono::{DateTime, Utc};
use quill_design::{CompiledDesign, Diagnostic};
use sqlx::{FromRow, SqlitePool};

/// How many past designs to keep once a new one is applied.
const HISTORY_LIMIT: i64 = 20;

#[derive(Debug, Clone, FromRow)]
pub struct StoredDesign {
    pub id: i64,
    pub name: String,
    pub source: String,
    pub compiled: String,
    pub diagnostics: String,
    pub created_at: DateTime<Utc>,
    pub applied_at: Option<DateTime<Utc>>,
    pub is_active: bool,
}

impl StoredDesign {
    /// `None` if the stored JSON no longer validates, perhaps because a newer
    /// version wrote it. Callers treat that as "this design can't be used".
    pub fn design(&self) -> Option<CompiledDesign> {
        match serde_json::from_str(&self.compiled) {
            Ok(design) => Some(design),
            Err(error) => {
                tracing::warn!("ignoring stored design {}: {error}", self.id);
                None
            }
        }
    }

    pub fn diagnostics(&self) -> Vec<Diagnostic> {
        serde_json::from_str(&self.diagnostics).unwrap_or_default()
    }
}

/// Saves a compiled design for review. Earlier drafts that were never applied
/// are abandoned, so reviewing many files doesn't pile up rows.
pub async fn insert_draft(
    db: &SqlitePool,
    source: &str,
    design: &CompiledDesign,
    diagnostics: &[Diagnostic],
) -> Result<i64> {
    let mut tx = db.begin().await?;
    sqlx::query("DELETE FROM designs WHERE applied_at IS NULL")
        .execute(&mut *tx)
        .await?;
    let id = sqlx::query_scalar(
        "INSERT INTO designs (name, source, compiled, diagnostics, created_at)
         VALUES (?, ?, ?, ?, ?) RETURNING id",
    )
    .bind(design.name.as_deref().unwrap_or("Imported design"))
    .bind(source)
    .bind(serde_json::to_string(design)?)
    .bind(serde_json::to_string(diagnostics)?)
    .bind(Utc::now())
    .fetch_one(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(id)
}

pub async fn get(db: &SqlitePool, id: i64) -> Result<Option<StoredDesign>> {
    Ok(sqlx::query_as("SELECT * FROM designs WHERE id = ?")
        .bind(id)
        .fetch_optional(db)
        .await?)
}

pub async fn active(db: &SqlitePool) -> Result<Option<StoredDesign>> {
    Ok(sqlx::query_as("SELECT * FROM designs WHERE is_active = 1")
        .fetch_optional(db)
        .await?)
}

/// Designs that have been applied, newest first.
pub async fn history(db: &SqlitePool) -> Result<Vec<StoredDesign>> {
    Ok(sqlx::query_as(
        "SELECT * FROM designs WHERE applied_at IS NOT NULL ORDER BY applied_at DESC, id DESC",
    )
    .fetch_all(db)
    .await?)
}

/// Makes a design the live one, in one transaction so the blog is never left
/// without its previous design if this fails partway.
pub async fn activate(db: &SqlitePool, id: i64) -> Result<()> {
    let mut tx = db.begin().await?;
    sqlx::query("UPDATE designs SET is_active = 0 WHERE is_active = 1")
        .execute(&mut *tx)
        .await?;
    sqlx::query("UPDATE designs SET is_active = 1, applied_at = ? WHERE id = ?")
        .bind(Utc::now())
        .bind(id)
        .execute(&mut *tx)
        .await?;
    sqlx::query(
        "DELETE FROM designs WHERE applied_at IS NOT NULL AND is_active = 0 AND id NOT IN (
             SELECT id FROM designs WHERE applied_at IS NOT NULL
             ORDER BY applied_at DESC, id DESC LIMIT ?)",
    )
    .bind(HISTORY_LIMIT)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(())
}

/// Goes back to the preset's own tokens. The design stays in the history.
pub async fn deactivate(db: &SqlitePool) -> Result<()> {
    sqlx::query("UPDATE designs SET is_active = 0 WHERE is_active = 1")
        .execute(db)
        .await?;
    Ok(())
}

/// Removes a draft. Applied designs are history and stay.
pub async fn delete_draft(db: &SqlitePool, id: i64) -> Result<()> {
    sqlx::query("DELETE FROM designs WHERE id = ? AND applied_at IS NULL")
        .bind(id)
        .execute(db)
        .await?;
    Ok(())
}
