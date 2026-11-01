//! The “Connect your site” page, with generated reverse-proxy settings.

use axum::extract::State;
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Response};
use serde_json::json;

use crate::auth::{self, AdminUser};
use crate::error::AppResult;
use crate::routes::request_origin;
use crate::state::AppState;

use super::{admin_context, origin_warning, render};

pub async fn integration(
    State(state): State<AppState>,
    user: AdminUser,
    headers: HeaderMap,
) -> AppResult<Response> {
    let s = state.settings();
    let upstream = state
        .listen
        .replace("0.0.0.0", "127.0.0.1")
        .replace("[::]", "[::1]");
    let host = url::Url::parse(&s.base_url)
        .ok()
        .and_then(|u| u.host_str().map(str::to_string))
        .unwrap_or_default();
    let mut ctx = admin_context(&s, Some(&user), "integration");
    ctx.insert("upstream".into(), json!(upstream));
    ctx.insert("host".into(), json!(host));
    ctx.insert("path".into(), json!(s.base_path));
    ctx.insert("is_subpath".into(), json!(!s.base_path.is_empty()));
    ctx.insert("origin_warning".into(), json!(origin_warning(&headers, &s)));
    ctx.insert("seen_origin".into(), json!(request_origin(&headers)));
    ctx.insert("https_seen".into(), json!(auth::is_https(&headers)));
    Ok(render(&state, "integration", &ctx)?.into_response())
}
