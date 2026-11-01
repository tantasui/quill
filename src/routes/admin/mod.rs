pub mod account;
pub mod appearance;
pub mod articles;
pub mod dashboard;
pub mod integration;
pub mod login;
pub mod media;
pub mod settings;
pub mod setup;

use axum::http::HeaderMap;
use axum::response::{Html, IntoResponse, Redirect, Response};
use serde::Deserialize;
use serde_json::{json, Map, Value};

use crate::auth::AdminUser;
use crate::error::AppResult;
use crate::routes::{request_origin, VERSION};
use crate::settings::Settings;
use crate::state::AppState;

type Ctx = Map<String, Value>;

fn admin_context(s: &Settings, user: Option<&AdminUser>, section: &str) -> Ctx {
    let mut ctx = Ctx::new();
    ctx.insert("base".into(), json!(s.base_path));
    ctx.insert("version".into(), json!(VERSION));
    ctx.insert("site".into(), json!({"name": s.site_name, "url": s.base_url}));
    ctx.insert("section".into(), json!(section));
    if let Some(u) = user {
        ctx.insert("user".into(), json!({"username": u.username}));
        ctx.insert("csrf".into(), json!(u.csrf));
    }
    ctx
}

fn render(state: &AppState, name: &str, ctx: &Ctx) -> AppResult<Html<String>> {
    Ok(Html(state.views.render(&format!("admin/{name}.html"), ctx)?))
}

fn redirect(s: &Settings, path: &str) -> Response {
    Redirect::to(&format!("{}{path}", s.base_path)).into_response()
}

#[derive(Deserialize, Default)]
pub struct NoticeQuery {
    notice: Option<String>,
    status: Option<String>,
}

fn notice_text(key: Option<&str>) -> Option<&'static str> {
    Some(match key? {
        "welcome" => "Your blog is ready. Write your first article, or look around the settings first.",
        "saved" => "Changes saved.",
        "published" => "Published. The article is now live on your blog.",
        "scheduled" => "Scheduled. The article will appear on your blog at its publication date.",
        "unpublished" => "Moved back to drafts. The article is no longer visible on your blog.",
        "deleted" => "Article deleted.",
        "theme" => "Theme activated. Your articles now use the new design.",
        "settings" => "Settings saved.",
        "password" => "Password changed. Other devices have been signed out.",
        "uploaded" => "Upload complete.",
        "media_deleted" => "File deleted.",
        _ => return None,
    })
}

#[derive(Deserialize, Default)]
pub struct CsrfForm {
    #[serde(rename = "_csrf", default)]
    csrf: String,
}

/// Warns when the dashboard is being viewed from a different address than the
/// configured public URL, the most common source of broken links.
fn origin_warning(headers: &HeaderMap, s: &Settings) -> Option<String> {
    let seen = request_origin(headers)?;
    let configured = s.origin();
    (seen != configured).then(|| {
        format!(
            "You're viewing this dashboard at {seen}, but the blog's public address is set to {}. \
             Links, feeds and sharing previews will point to {configured}. That's expected while you test \
             before connecting your domain; otherwise update the address in Settings.",
            s.base_url
        )
    })
}
