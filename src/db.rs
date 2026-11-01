use std::path::Path;
use std::str::FromStr;
use std::time::Duration;

use anyhow::{Context, Result};
use sqlx::migrate::Migrator;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};
use sqlx::SqlitePool;

pub static MIGRATOR: Migrator = sqlx::migrate!("./migrations");

pub const DB_FILE: &str = "quill.db";

pub async fn connect(path: &Path) -> Result<SqlitePool> {
    let opts = SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .synchronous(SqliteSynchronous::Normal)
        .foreign_keys(true)
        .busy_timeout(Duration::from_secs(5));
    SqlitePoolOptions::new()
        .max_connections(8)
        .connect_with(opts)
        .await
        .with_context(|| format!("could not open database {}", path.display()))
}

/// A private in-memory database, used by tests. A single connection keeps every
/// query on the same database.
pub async fn connect_memory() -> Result<SqlitePool> {
    let opts = SqliteConnectOptions::from_str("sqlite::memory:")?.foreign_keys(true);
    Ok(SqlitePoolOptions::new().max_connections(1).connect_with(opts).await?)
}

/// Applies pending migrations. When an existing database is about to be
/// migrated, a snapshot is written to `backup_dir` first so an upgrade can
/// always be rolled back.
pub async fn migrate(pool: &SqlitePool, backup_dir: Option<&Path>) -> Result<()> {
    let has_table: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = '_sqlx_migrations'")
        .fetch_one(pool)
        .await?;

    if has_table > 0 {
        let applied: Vec<i64> = sqlx::query_scalar("SELECT version FROM _sqlx_migrations").fetch_all(pool).await?;
        let pending = MIGRATOR.iter().filter(|m| !applied.contains(&m.version)).count();
        if pending > 0 {
            if let Some(dir) = backup_dir {
                std::fs::create_dir_all(dir)?;
                let file = dir.join(format!("pre-upgrade-{}.db", chrono::Utc::now().format("%Y%m%d-%H%M%S")));
                snapshot(pool, &file).await?;
                tracing::info!("{pending} database migration(s) pending; saved a snapshot to {}", file.display());
            }
        }
    }

    MIGRATOR
        .run(pool)
        .await
        .context("database migration failed; your data is untouched and a snapshot is in the backups folder")
}

/// Writes a consistent copy of the database to `dest`, safe while the server runs.
pub async fn snapshot(pool: &SqlitePool, dest: &Path) -> Result<()> {
    let dest = dest.to_string_lossy().replace('\'', "''");
    sqlx::query(&format!("VACUUM INTO '{dest}'"))
        .execute(pool)
        .await
        .context("could not write database snapshot")?;
    Ok(())
}
