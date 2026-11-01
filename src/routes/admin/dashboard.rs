//! The overview page shown after signing in.

use axum::extract::{Query, State};
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Response};
use serde_json::json;

use crate::auth::AdminUser;
use crate::error::AppResult;
use crate::models;
use crate::state::AppState;
use crate::themes;

use super::articles::article_rows;
use super::{admin_context, notice_text, origin_warning, render, NoticeQuery};

pub async fn dashboard(
    State(state): State<AppState>,
    user: AdminUser,
    headers: HeaderMap,
    Query(q): Query<NoticeQuery>,
) -> AppResult<Response> {
    let s = state.settings();
    let counts = models::counts(&state.db).await?;
    let drafts = models::list_admin(&state.db, "draft", 5).await?;
    let recent = models::list_admin(&state.db, "published", 5).await?;
    let scheduled = models::list_admin(&state.db, "scheduled", 5).await?;
    let mut ctx = admin_context(&s, Some(&user), "dashboard");
    ctx.insert("counts".into(), json!(counts));
    ctx.insert(
        "drafts".into(),
        json!(article_rows(&state, &s, &drafts).await?),
    );
    ctx.insert(
        "recent".into(),
        json!(article_rows(&state, &s, &recent).await?),
    );
    ctx.insert(
        "scheduled".into(),
        json!(article_rows(&state, &s, &scheduled).await?),
    );
    ctx.insert("notice".into(), json!(notice_text(q.notice.as_deref())));
    ctx.insert("origin_warning".into(), json!(origin_warning(&headers, &s)));
    ctx.insert(
        "theme_name".into(),
        json!(themes::find(&s.theme).map(|t| t.name)),
    );
    let total = counts.published + counts.scheduled + counts.drafts;
    ctx.insert(
        "checklist".into(),
        json!([
            {"done": true, "label": "Create your administrator account"},
            {"done": total > 0, "label": "Write your first article", "url": format!("{}/admin/articles/new", s.base_path)},
            {"done": counts.published > 0, "label": "Publish it"},
            {"done": origin_warning(&headers, &s).is_none(), "label": "Connect the blog to your website", "url": format!("{}/admin/integration", s.base_path)},
        ]),
    );
    Ok(render(&state, "dashboard", &ctx)?.into_response())
}
