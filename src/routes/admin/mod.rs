pub mod account;
pub mod appearance;
pub mod articles;
pub mod dashboard;
pub mod integration;
pub mod login;
pub mod media;
pub mod settings;

use axum::extract::State;
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum::{Extension, Form};
use chrono::Utc;
use serde::Deserialize;
use serde_json::{json, Map, Value};

use crate::auth::{self, AdminUser};
use crate::error::AppResult;
use crate::routes::{request_origin, OriginalPath, VERSION};
use crate::settings::{normalize_base_url, Settings};
use crate::state::AppState;
use crate::themes;

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

// ---- Setup ---------------------------------------------------------------------

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct SetupForm {
    setup_code: String,
    site_name: String,
    base_url: String,
    username: String,
    password: String,
    password_confirm: String,
    theme: String,
}

fn setup_context(s: &Settings, form: &SetupForm, errors: &[String]) -> Ctx {
    let mut ctx = admin_context(s, None, "setup");
    ctx.insert(
        "form".into(),
        json!({"site_name": form.site_name, "base_url": form.base_url, "username": form.username, "theme": form.theme, "setup_code": form.setup_code}),
    );
    ctx.insert("themes".into(), json!(themes::ALL));
    ctx.insert("errors".into(), json!(errors));
    ctx.insert("min_password".into(), json!(auth::MIN_PASSWORD_LEN));
    ctx
}

pub async fn setup_form(State(state): State<AppState>, headers: HeaderMap, Extension(original): Extension<OriginalPath>) -> AppResult<Response> {
    let s = state.settings();
    if !state.needs_setup() {
        return Ok(redirect(&s, "/admin/login"));
    }
    // Guess the public URL from how the browser reached us.
    let prefix = original.0.strip_suffix("/admin/setup").unwrap_or("");
    let guess = request_origin(&headers).map(|o| format!("{o}{prefix}")).unwrap_or_default();
    let form = SetupForm {
        site_name: "My Blog".into(),
        base_url: guess,
        theme: themes::DEFAULT.into(),
        ..Default::default()
    };
    Ok(render(&state, "setup", &setup_context(&s, &form, &[]))?.into_response())
}

pub async fn setup_submit(State(state): State<AppState>, headers: HeaderMap, Form(form): Form<SetupForm>) -> AppResult<Response> {
    let mut s = state.settings();
    let Some(code) = state.setup_code() else {
        return Ok(redirect(&s, "/admin/login"));
    };
    let mut errors = Vec::new();
    if state.login_limiter.is_blocked("setup") {
        errors.push("Too many incorrect setup codes. Wait 15 minutes and try again.".to_string());
    } else if auth::normalize_setup_code(&form.setup_code) != auth::normalize_setup_code(&code) {
        state.login_limiter.record_failure("setup");
        errors.push("The setup code doesn't match. Copy it from the server's startup output (for Docker: docker logs <container>).".into());
    }
    if form.site_name.trim().is_empty() {
        errors.push("Give your blog a name. You can change it later.".into());
    }
    let base_url = normalize_base_url(&form.base_url).map_err(|e| errors.push(e)).ok();
    let username = form.username.trim();
    if username.is_empty() || username.len() > 64 || !username.chars().all(|c| c.is_ascii_alphanumeric() || "._-@".contains(c)) {
        errors.push("Choose a username of letters, numbers, dots, dashes or underscores.".into());
    }
    if form.password.chars().count() < auth::MIN_PASSWORD_LEN {
        errors.push(format!("Use a password of at least {} characters.", auth::MIN_PASSWORD_LEN));
    } else if form.password != form.password_confirm {
        errors.push("The two passwords don't match.".into());
    }
    let theme = themes::find(&form.theme).map(|t| t.id).unwrap_or(themes::DEFAULT);
    if !errors.is_empty() {
        let ctx = setup_context(&s, &form, &errors);
        return Ok((StatusCode::UNPROCESSABLE_ENTITY, render(&state, "setup", &ctx)?).into_response());
    }

    let hash = auth::hash_password(&form.password)?;
    let mut tx = state.db.begin().await?;
    let existing: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users").fetch_one(&mut *tx).await?;
    if existing > 0 {
        state.finish_setup();
        return Ok(redirect(&s, "/admin/login"));
    }
    let user_id: i64 = sqlx::query_scalar("INSERT INTO users (username, password_hash, role, created_at) VALUES (?, ?, 'admin', ?) RETURNING id")
        .bind(username)
        .bind(hash)
        .bind(Utc::now())
        .fetch_one(&mut *tx)
        .await?;
    tx.commit().await?;

    s.site_name = form.site_name.trim().to_string();
    s.set_base_url(&base_url.unwrap_or_default());
    s.theme = theme.to_string();
    s.author_name = username.to_string();
    state.save_settings(s.clone()).await?;
    state.finish_setup();
    state.login_limiter.clear("setup");
    tracing::info!("setup complete; administrator “{username}” created");

    let token = auth::create_session(&state.db, user_id).await?;
    let mut res = redirect(&s, "/admin?notice=welcome");
    res.headers_mut()
        .insert(header::SET_COOKIE, auth::session_cookie(&token, auth::is_https(&headers)).parse()?);
    Ok(res)
}
