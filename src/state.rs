use std::ops::Deref;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use anyhow::Result;
use chrono::Utc;
use quill_design::{CompiledDesign, Layout};
use sqlx::SqlitePool;

use crate::auth::{self, LoginLimiter};
use crate::designs;
use crate::render::Views;
use crate::settings::Settings;
use crate::themes::{self, Theme};

#[derive(Clone)]
pub struct AppState(Arc<Inner>);

/// How the blog looks: which preset's tokens and stylesheet, on which layout.
#[derive(Clone, Debug)]
pub struct Look {
    pub preset: String,
    pub layout: Layout,
    /// The imported design supplying the tokens, instead of the preset's own.
    pub design: Option<i64>,
}

impl Look {
    pub fn of_preset(preset: &Theme) -> Self {
        Look {
            preset: preset.id.to_string(),
            layout: preset.layout().clone(),
            design: None,
        }
    }
}

pub struct Inner {
    pub db: SqlitePool,
    pub views: Views,
    pub data_dir: PathBuf,
    /// Address the server listens on, shown in proxy configuration snippets.
    pub listen: String,
    pub login_limiter: LoginLimiter,
    settings: RwLock<Settings>,
    /// A customized layout and the preset it was customized from, if any.
    custom_layout: RwLock<Option<(String, Layout)>>,
    /// The live imported design, if any.
    active_design: RwLock<Option<(i64, CompiledDesign)>>,
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
    pub async fn new(
        db: SqlitePool,
        data_dir: PathBuf,
        listen: String,
        setup_code: Option<String>,
    ) -> Result<Self> {
        let settings = Settings::load(&db).await?;
        let custom_layout = load_custom_layout(&db).await?;
        let active_design = load_active_design(&db).await?;
        let users: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users")
            .fetch_one(&db)
            .await?;
        let setup_code = (users == 0).then(|| setup_code.unwrap_or_else(auth::generate_setup_code));
        Ok(AppState(Arc::new(Inner {
            db,
            views: Views::new(),
            data_dir,
            listen,
            login_limiter: LoginLimiter::default(),
            settings: RwLock::new(settings),
            custom_layout: RwLock::new(custom_layout),
            active_design: RwLock::new(active_design),
            setup_code: RwLock::new(setup_code),
        })))
    }

    /// What the blog looks like right now. A customized layout applies only
    /// while the preset it came from is active.
    pub fn look(&self) -> Look {
        let settings = self.settings();
        let preset = themes::find(&settings.theme)
            .or_else(|| themes::find(themes::DEFAULT))
            .expect("the default preset exists");
        let mut look = Look::of_preset(preset);
        if let Some((from, layout)) = &*self.custom_layout.read().unwrap() {
            if from == preset.id {
                look.layout = layout.clone();
            }
        }
        look.design = self
            .active_design
            .read()
            .unwrap()
            .as_ref()
            .map(|(id, _)| *id);
        look
    }

    /// The live design's stylesheet, if `id` is the live one.
    pub fn active_design_css(&self, id: i64) -> Option<String> {
        let base_path = self.settings().base_path;
        let active = self.active_design.read().unwrap();
        active
            .as_ref()
            .filter(|(active_id, _)| *active_id == id)
            .map(|(_, design)| design.to_css(&base_path))
    }

    /// Makes a stored design live.
    pub async fn apply_design(&self, id: i64, design: CompiledDesign) -> Result<()> {
        designs::activate(&self.db, id).await?;
        *self.active_design.write().unwrap() = Some((id, design));
        Ok(())
    }

    /// Goes back to the preset's own tokens.
    pub async fn clear_design(&self) -> Result<()> {
        designs::deactivate(&self.db).await?;
        *self.active_design.write().unwrap() = None;
        Ok(())
    }

    pub fn has_custom_layout(&self) -> bool {
        let look = self.look();
        themes::find(&look.preset).is_some_and(|preset| *preset.layout() != look.layout)
    }

    /// Stores a layout customized from the active preset.
    pub async fn save_layout(&self, layout: Layout) -> Result<()> {
        let preset = self.look().preset;
        sqlx::query(
            "INSERT INTO site_layout (id, preset, layout, updated_at) VALUES (1, ?, ?, ?)
             ON CONFLICT (id) DO UPDATE SET preset = excluded.preset, layout = excluded.layout,
                 updated_at = excluded.updated_at",
        )
        .bind(&preset)
        .bind(layout.to_json())
        .bind(Utc::now())
        .execute(&self.db)
        .await?;
        *self.custom_layout.write().unwrap() = Some((preset, layout));
        Ok(())
    }

    /// Goes back to the active preset's own layout.
    pub async fn clear_layout(&self) -> Result<()> {
        sqlx::query("DELETE FROM site_layout")
            .execute(&self.db)
            .await?;
        *self.custom_layout.write().unwrap() = None;
        Ok(())
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

/// Reads the stored layout. One that no longer validates, perhaps written by a
/// different version, is ignored so the blog keeps working on its preset's layout.
async fn load_custom_layout(db: &SqlitePool) -> Result<Option<(String, Layout)>> {
    let row: Option<(String, String)> =
        sqlx::query_as("SELECT preset, layout FROM site_layout WHERE id = 1")
            .fetch_optional(db)
            .await?;
    Ok(
        row.and_then(|(preset, json)| match Layout::from_json(&json) {
            Ok(layout) => Some((preset, layout)),
            Err(error) => {
                tracing::warn!("ignoring the stored layout: {error}");
                None
            }
        }),
    )
}

/// Reads the live design. One that no longer validates is ignored, like a bad
/// stored layout, so the blog keeps working on its preset's tokens.
async fn load_active_design(db: &SqlitePool) -> Result<Option<(i64, CompiledDesign)>> {
    let stored = designs::active(db).await?;
    Ok(stored.and_then(|row| row.design().map(|design| (row.id, design))))
}
