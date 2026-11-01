//! The site settings page.

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Form;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::auth::AdminUser;
use crate::error::AppResult;
use crate::settings::{normalize_base_url, Settings};
use crate::state::AppState;

use super::{admin_context, notice_text, redirect, render, Ctx, NoticeQuery};

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct SettingsForm {
    #[serde(rename = "_csrf")]
    csrf: String,
    site_name: String,
    description: String,
    base_url: String,
    author_name: String,
    posts_per_page: String,
    language: String,
    footer_text: String,
}

fn settings_context(
    s: &Settings,
    user: &AdminUser,
    form: &Value,
    errors: &[String],
    notice: Option<&str>,
) -> Ctx {
    let mut ctx = admin_context(s, Some(user), "settings");
    ctx.insert("form".into(), form.clone());
    ctx.insert("errors".into(), json!(errors));
    ctx.insert("notice".into(), json!(notice));
    ctx
}

pub async fn settings_form(
    State(state): State<AppState>,
    user: AdminUser,
    Query(q): Query<NoticeQuery>,
) -> AppResult<Response> {
    let s = state.settings();
    let form = json!(s);
    Ok(render(
        &state,
        "settings",
        &settings_context(&s, &user, &form, &[], notice_text(q.notice.as_deref())),
    )?
    .into_response())
}

pub async fn settings_submit(
    State(state): State<AppState>,
    user: AdminUser,
    Form(form): Form<SettingsForm>,
) -> AppResult<Response> {
    if let Err(r) = user.check_csrf(&form.csrf) {
        return Ok(r);
    }
    let current = state.settings();
    let mut errors = Vec::new();
    if form.site_name.trim().is_empty() {
        errors.push("The blog needs a name.".to_string());
    }
    let base_url = normalize_base_url(&form.base_url)
        .map_err(|e| errors.push(e))
        .ok();
    let posts_per_page = match form.posts_per_page.trim().parse::<i64>() {
        Ok(n @ 1..=50) => n,
        _ => {
            errors.push("Articles per page must be a number from 1 to 50.".into());
            current.posts_per_page
        }
    };
    let language = form.language.trim();
    if !(2..=12).contains(&language.len())
        || !language
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-')
    {
        errors.push("Use a language code such as en, fr or pt-BR.".into());
    }
    if !errors.is_empty() {
        let echo = json!({"site_name": form.site_name, "description": form.description, "base_url": form.base_url,
                          "author_name": form.author_name, "posts_per_page": form.posts_per_page,
                          "language": form.language, "footer_text": form.footer_text});
        let ctx = settings_context(&current, &user, &echo, &errors, None);
        return Ok((
            StatusCode::UNPROCESSABLE_ENTITY,
            render(&state, "settings", &ctx)?,
        )
            .into_response());
    }
    let mut s = current;
    s.site_name = form.site_name.trim().to_string();
    s.description = form.description.trim().to_string();
    s.set_base_url(&base_url.unwrap_or_default());
    s.author_name = form.author_name.trim().to_string();
    s.posts_per_page = posts_per_page;
    s.language = language.to_string();
    s.footer_text = form.footer_text.trim().to_string();
    state.save_settings(s.clone()).await?;
    Ok(redirect(&s, "/admin/settings?notice=settings"))
}
