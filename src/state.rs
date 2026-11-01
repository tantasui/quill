use std::ops::Deref;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use anyhow::Result;
use sqlx::SqlitePool;

use crate::auth::{self, LoginLimiter};
use crate::render::Views;
use crate::settings::Settings;

#[derive(Clone)]
pub struct AppState(Arc<Inner>);

pub struct Inner {
    pub db: SqlitePool,
    pub views: Views,
    pub data_dir: PathBuf,
    /// Address the server listens on, shown in proxy configuration snippets.
    pub listen: String,
    pub login_limiter: LoginLimiter,
    settings: RwLock<Settings>,
    /// Present until the first administrator account has been created.
    setup_code: RwLock<Option<String>>,
}

impl Deref for AppState {
    type Target = Inner;
    fn deref(&self) -> &Inner {
        &self.0
    }
}

impl AppState {
    /// `setup_code` is used only if the installation still needs an admin; a
    /// random one is generated when it is `None`.
    pub async fn new(db: SqlitePool, data_dir: PathBuf, listen: String, setup_code: Option<String>) -> Result<Self> {
        let settings = Settings::load(&db).await?;
        let users: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users").fetch_one(&db).await?;
        let setup_code = (users == 0).then(|| setup_code.unwrap_or_else(auth::generate_setup_code));
        Ok(AppState(Arc::new(Inner {
            db,
            views: Views::new(),
            data_dir,
            listen,
            login_limiter: LoginLimiter::default(),
            settings: RwLock::new(settings),
            setup_code: RwLock::new(setup_code),
        })))
    }

    pub fn settings(&self) -> Settings {
        self.settings.read().unwrap().clone()
    }

    pub async fn save_settings(&self, settings: Settings) -> Result<()> {
        settings.save(&self.db).await?;
        *self.settings.write().unwrap() = settings;
        Ok(())
    }

    pub fn needs_setup(&self) -> bool {
        self.setup_code.read().unwrap().is_some()
    }

    pub fn setup_code(&self) -> Option<String> {
        self.setup_code.read().unwrap().clone()
    }

    pub fn finish_setup(&self) {
        *self.setup_code.write().unwrap() = None;
    }

    pub fn media_dir(&self) -> PathBuf {
        self.data_dir.join("media")
    }
}
