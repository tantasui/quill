//! The themes page: choosing and previewing a theme.

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Form;
use chrono::Utc;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::auth::AdminUser;
use crate::error::AppResult;
use crate::models::{self, Article, Tag};
use crate::routes::public::{self, article_view};
use crate::settings::Settings;
use crate::state::{AppState, Look};
use crate::themes;

use super::{admin_context, notice_text, redirect, render, NoticeQuery};

pub async fn themes(
    State(state): State<AppState>,
    user: AdminUser,
    Query(q): Query<NoticeQuery>,
) -> AppResult<Response> {
    let s = state.settings();
    let mut ctx = admin_context(&s, Some(&user), "themes");
    ctx.insert(
        "themes".into(),
        json!(themes::ALL
            .iter()
            .map(|t| json!({"id": t.id, "name": t.name, "description": t.description, "active": t.id == s.theme,
                            "preview_url": format!("{}/admin/themes/{}/preview", s.base_path, t.id)}))
            .collect::<Vec<_>>()),
    );
    ctx.insert("notice".into(), json!(notice_text(q.notice.as_deref())));
    Ok(render(&state, "themes", &ctx)?.into_response())
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct ThemeForm {
    #[serde(rename = "_csrf")]
    csrf: String,
    theme: String,
}

pub async fn activate_theme(
    State(state): State<AppState>,
    user: AdminUser,
    Form(form): Form<ThemeForm>,
) -> AppResult<Response> {
    if let Err(r) = user.check_csrf(&form.csrf) {
        return Ok(r);
    }
    let mut s = state.settings();
    let Some(theme) = themes::find(&form.theme) else {
        return Ok((StatusCode::BAD_REQUEST, "Unknown theme.").into_response());
    };
    s.theme = theme.id.to_string();
    state.save_settings(s.clone()).await?;
    state.clear_layout().await?;
    Ok(redirect(&s, "/admin/themes?notice=theme"))
}

/// The home page rendered with any theme. Uses sample articles while the blog
/// has nothing published, so empty installs still get a meaningful preview.
pub async fn preview_theme(
    State(state): State<AppState>,
    _user: AdminUser,
    Path(id): Path<String>,
) -> AppResult<Response> {
    let s = state.settings();
    let Some(theme) = themes::find(&id) else {
        return Ok((StatusCode::NOT_FOUND, "Unknown theme.").into_response());
    };
    let articles = models::list_published(&state.db, s.posts_per_page, 0).await?;
    let look = Look::of_preset(theme);
    let mut ctx = public::base_context(&state, &s, &look).await?;
    let views = if articles.is_empty() {
        sample_articles(&s)
    } else {
        public::article_views(&state, &s, &articles).await?
    };
    public::insert_home_articles(&mut ctx, &look.layout, views, true);
    ctx.insert("pagination".into(), json!({"current": 1, "total": 1}));
    ctx.insert(
        "page_title".into(),
        json!(format!("{} theme preview — {}", theme.name, s.site_name)),
    );
    ctx.insert("is_home".into(), json!(true));
    ctx.insert("preview".into(), json!(true));
    ctx.insert(
        "preview_label".into(),
        json!(format!(
            "Previewing the {} theme. Visitors still see your current theme.",
            theme.name
        )),
    );
    ctx.insert("noindex".into(), json!(true));
    Ok(public::render_page(&state, "index", &ctx)?.into_response())
}

fn sample_articles(s: &Settings) -> Vec<Value> {
    let now = Utc::now();
    let samples = [
        (
            "Welcome to your new blog",
            "This is sample content so you can see how the theme lays out articles. Your own articles will appear here once you publish them.",
            "news",
        ),
        (
            "Writing with Markdown",
            "Headings, lists, links, images, quotes and code blocks are all supported, and the editor toolbar inserts them for you.",
            "guides",
        ),
        (
            "Choosing a theme",
            "Themes change how your blog looks without touching your content. Switch at any time from the Themes page.",
            "design",
        ),
    ];
    samples
        .iter()
        .enumerate()
        .map(|(i, (title, excerpt, tag))| {
            let a = Article {
                id: 0,
                title: title.to_string(),
                slug: slug::slugify(title),
                content: excerpt.to_string(),
                excerpt: excerpt.to_string(),
                status: "published".into(),
                cover_image: String::new(),
                meta_title: String::new(),
                meta_description: String::new(),
                author_id: None,
                published_at: Some(now - chrono::Duration::days(i as i64 * 6)),
                created_at: now,
                updated_at: now,
            };
            article_view(
                s,
                &a,
                &[Tag {
                    id: 0,
                    name: tag.to_string(),
                    slug: tag.to_string(),
                }],
                false,
            )
        })
        .collect()
}
