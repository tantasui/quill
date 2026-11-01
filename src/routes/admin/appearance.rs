//! The Appearance page: choosing a preset and arranging the layout.

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Form;
use chrono::Utc;
use quill_design::{HomeSection, Layout, LayoutError};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::auth::AdminUser;
use crate::error::AppResult;
use crate::models::{self, Article, Tag};
use crate::routes::public::{self, article_view};
use crate::settings::Settings;
use crate::state::{AppState, Look};
use crate::themes;

use super::{admin_context, notice_text, redirect, render, Ctx, NoticeQuery};

/// The editable list of home sections: those in use, in order, then those not
/// in use, so each can be switched on.
fn home_sections(layout: &Layout) -> Vec<Value> {
    let label = |key| match key {
        "featured" => "Lead story",
        "feed" => "Article feed",
        _ => "Archive link",
    };
    let mut rows = Vec::new();
    let mut feed_variant = json!("list");
    for section in &layout.home {
        let key = match section {
            HomeSection::Featured => "featured",
            HomeSection::Feed { variant } => {
                feed_variant = json!(variant);
                "feed"
            }
            HomeSection::ArchiveLink => "archive_link",
        };
        rows.push((key, true));
    }
    for key in ["featured", "feed", "archive_link"] {
        if !rows.iter().any(|(k, _)| *k == key) {
            rows.push((key, false));
        }
    }
    rows.into_iter()
        .map(|(key, enabled)| {
            json!({"key": key, "label": label(key), "enabled": enabled, "variant": feed_variant})
        })
        .collect()
}

fn appearance_context(state: &AppState, s: &Settings, user: &AdminUser) -> Ctx {
    let look = state.look();
    let mut ctx = admin_context(s, Some(user), "appearance");
    ctx.insert(
        "themes".into(),
        json!(themes::ALL
            .iter()
            .map(|t| json!({"id": t.id, "name": t.name, "description": t.description, "active": t.id == s.theme,
                            "preview_url": format!("{}/admin/themes/{}/preview", s.base_path, t.id)}))
            .collect::<Vec<_>>()),
    );
    ctx.insert("customized".into(), json!(state.has_custom_layout()));
    ctx.insert("sections".into(), json!(home_sections(&look.layout)));
    ctx.insert("layout".into(), json!(look.layout));
    ctx
}

pub async fn appearance(
    State(state): State<AppState>,
    user: AdminUser,
    Query(q): Query<NoticeQuery>,
) -> AppResult<Response> {
    let s = state.settings();
    let mut ctx = appearance_context(&state, &s, &user);
    ctx.insert("notice".into(), json!(notice_text(q.notice.as_deref())));
    Ok(render(&state, "appearance", &ctx)?.into_response())
}

/// What the layout form submits. Empty checkbox fields mean "off".
#[derive(Deserialize, Default)]
#[serde(default)]
pub struct LayoutForm {
    #[serde(rename = "_csrf")]
    csrf: String,
    page_width: String,
    header_variant: String,
    tag_rail: String,
    footer_variant: String,
    featured: String,
    featured_order: String,
    feed: String,
    feed_order: String,
    feed_variant: String,
    archive_link: String,
    archive_link_order: String,
    article_measure: String,
    article_cover: String,
    show_author: String,
    show_date: String,
    show_reading_time: String,
    page: String,
}

/// Applies the form to the current layout, keeping what the form doesn't edit,
/// such as labels. The result goes through the same JSON validation as a stored
/// layout, so the dashboard has no validation rules of its own.
fn layout_from_form(current: &Layout, form: &LayoutForm) -> Result<Layout, LayoutError> {
    let position = |text: &str| text.trim().parse::<i32>().unwrap_or(i32::MAX);
    let mut sections = Vec::new();
    if !form.featured.is_empty() {
        sections.push((position(&form.featured_order), json!({"type": "featured"})));
    }
    if !form.feed.is_empty() {
        sections.push((
            position(&form.feed_order),
            json!({"type": "feed", "variant": form.feed_variant}),
        ));
    }
    if !form.archive_link.is_empty() {
        sections.push((
            position(&form.archive_link_order),
            json!({"type": "archive_link"}),
        ));
    }
    sections.sort_by_key(|(order, _)| *order);
    let home: Vec<Value> = sections.into_iter().map(|(_, section)| section).collect();

    let mut value = json!(current);
    value["page_width"] = json!(form.page_width);
    value["tag_rail"] = json!(form.tag_rail);
    value["header"]["variant"] = json!(form.header_variant);
    value["footer"]["variant"] = json!(form.footer_variant);
    value["home"] = json!(home);
    value["article"] = json!({
        "measure": form.article_measure,
        "cover": form.article_cover,
        "show_author": !form.show_author.is_empty(),
        "show_date": !form.show_date.is_empty(),
        "show_reading_time": !form.show_reading_time.is_empty(),
    });
    Layout::from_json(&value.to_string())
}

pub async fn save_layout(
    State(state): State<AppState>,
    user: AdminUser,
    Form(form): Form<LayoutForm>,
) -> AppResult<Response> {
    if let Err(r) = user.check_csrf(&form.csrf) {
        return Ok(r);
    }
    let s = state.settings();
    match layout_from_form(&state.look().layout, &form) {
        Ok(layout) => {
            state.save_layout(layout).await?;
            Ok(redirect(&s, "/admin/appearance?notice=layout"))
        }
        Err(error) => {
            let mut ctx = appearance_context(&state, &s, &user);
            ctx.insert("layout_error".into(), json!(error.to_string()));
            Ok((
                StatusCode::UNPROCESSABLE_ENTITY,
                render(&state, "appearance", &ctx)?,
            )
                .into_response())
        }
    }
}

/// Renders the unsaved form through the real page templates, for the preview
/// frame. Nothing is stored.
pub async fn preview_layout(
    State(state): State<AppState>,
    user: AdminUser,
    Form(form): Form<LayoutForm>,
) -> AppResult<Response> {
    if let Err(r) = user.check_csrf(&form.csrf) {
        return Ok(r);
    }
    let s = state.settings();
    let mut look = state.look();
    look.layout = match layout_from_form(&look.layout, &form) {
        Ok(layout) => layout,
        Err(error) => {
            return Ok((StatusCode::UNPROCESSABLE_ENTITY, error.to_string()).into_response())
        }
    };
    let label = "Previewing your layout. It hasn't been saved.";
    let latest = models::list_published(&state.db, s.posts_per_page, 0).await?;
    if form.page == "article" {
        let (article, tags) = match latest.first() {
            Some(article) => {
                let tags = models::tags_for(&state.db, &[article.id])
                    .await?
                    .remove(&article.id)
                    .unwrap_or_default();
                (article.clone(), tags)
            }
            None => sample_article_data().swap_remove(0),
        };
        let mut ctx =
            public::article_context_with_tags(&state, &s, &look, &article, &tags, true).await?;
        ctx.insert("preview_label".into(), json!(label));
        ctx.insert("noindex".into(), json!(true));
        return Ok(public::render_page(&state, "article", &ctx)?.into_response());
    }
    let mut ctx = public::base_context(&state, &s, &look).await?;
    let views = if latest.is_empty() {
        sample_articles(&s)
    } else {
        public::article_views(&state, &s, &latest).await?
    };
    public::insert_home_articles(&mut ctx, &look.layout, views, true);
    ctx.insert("pagination".into(), json!({"current": 1, "total": 1}));
    ctx.insert("page_title".into(), json!(s.site_name));
    ctx.insert("preview".into(), json!(true));
    ctx.insert("preview_label".into(), json!(label));
    ctx.insert("noindex".into(), json!(true));
    Ok(public::render_page(&state, "index", &ctx)?.into_response())
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
        json!(format!("{} preset preview — {}", theme.name, s.site_name)),
    );
    ctx.insert("is_home".into(), json!(true));
    ctx.insert("preview".into(), json!(true));
    ctx.insert(
        "preview_label".into(),
        json!(format!(
            "Previewing the {} preset. Visitors still see your current look.",
            theme.name
        )),
    );
    ctx.insert("noindex".into(), json!(true));
    Ok(public::render_page(&state, "index", &ctx)?.into_response())
}

fn sample_articles(s: &Settings) -> Vec<Value> {
    sample_article_data()
        .iter()
        .map(|(article, tags)| article_view(s, article, tags, false))
        .collect()
}

fn sample_article_data() -> Vec<(Article, Vec<Tag>)> {
    let now = Utc::now();
    let samples = [
        (
            "Welcome to your new blog",
            "This is sample content so you can see how the layout arranges articles. Your own articles will appear here once you publish them.",
            "news",
        ),
        (
            "Writing with Markdown",
            "Headings, lists, links, images, quotes and code blocks are all supported, and the editor toolbar inserts them for you.",
            "guides",
        ),
        (
            "Choosing a look",
            "Presets and layouts change how your blog looks without touching your content. Switch at any time from the Appearance page.",
            "design",
        ),
    ];
    samples
        .iter()
        .enumerate()
        .map(|(i, (title, excerpt, tag))| {
            let article = Article {
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
            let tags = vec![Tag {
                id: 0,
                name: tag.to_string(),
                slug: tag.to_string(),
            }];
            (article, tags)
        })
        .collect()
}
