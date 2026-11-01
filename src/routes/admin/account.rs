//! The account page: changing the administrator's password.

use axum::extract::{Query, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Form;
use serde::Deserialize;
use serde_json::json;

use crate::auth::{self, AdminUser};
use crate::error::AppResult;
use crate::state::AppState;

use super::{admin_context, notice_text, redirect, render, NoticeQuery};

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct AccountForm {
    #[serde(rename = "_csrf")]
    csrf: String,
    current_password: String,
    new_password: String,
    new_password_confirm: String,
}

pub async fn account_form(
    State(state): State<AppState>,
    user: AdminUser,
    Query(q): Query<NoticeQuery>,
) -> AppResult<Response> {
    let s = state.settings();
    let mut ctx = admin_context(&s, Some(&user), "account");
    ctx.insert("notice".into(), json!(notice_text(q.notice.as_deref())));
    ctx.insert("min_password".into(), json!(auth::MIN_PASSWORD_LEN));
    Ok(render(&state, "account", &ctx)?.into_response())
}

pub async fn account_submit(
    State(state): State<AppState>,
    user: AdminUser,
    headers: HeaderMap,
    Form(form): Form<AccountForm>,
) -> AppResult<Response> {
    if let Err(r) = user.check_csrf(&form.csrf) {
        return Ok(r);
    }
    let s = state.settings();
    let hash: String = sqlx::query_scalar("SELECT password_hash FROM users WHERE id = ?")
        .bind(user.id)
        .fetch_one(&state.db)
        .await?;
    let mut errors = Vec::new();
    if !auth::verify_password(&form.current_password, &hash) {
        errors.push("Your current password is incorrect.".to_string());
    }
    if form.new_password.chars().count() < auth::MIN_PASSWORD_LEN {
        errors.push(format!(
            "Use a new password of at least {} characters.",
            auth::MIN_PASSWORD_LEN
        ));
    } else if form.new_password != form.new_password_confirm {
        errors.push("The new passwords don't match.".into());
    }
    if !errors.is_empty() {
        let mut ctx = admin_context(&s, Some(&user), "account");
        ctx.insert("errors".into(), json!(errors));
        ctx.insert("min_password".into(), json!(auth::MIN_PASSWORD_LEN));
        return Ok((
            StatusCode::UNPROCESSABLE_ENTITY,
            render(&state, "account", &ctx)?,
        )
            .into_response());
    }
    sqlx::query("UPDATE users SET password_hash = ? WHERE id = ?")
        .bind(auth::hash_password(&form.new_password)?)
        .bind(user.id)
        .execute(&state.db)
        .await?;
    // Sign out every session, then give this browser a fresh one.
    sqlx::query("DELETE FROM sessions WHERE user_id = ?")
        .bind(user.id)
        .execute(&state.db)
        .await?;
    let token = auth::create_session(&state.db, user.id).await?;
    let mut res = redirect(&s, "/admin/account?notice=password");
    res.headers_mut().insert(
        header::SET_COOKIE,
        auth::session_cookie(&token, auth::is_https(&headers)).parse()?,
    );
    Ok(res)
}
