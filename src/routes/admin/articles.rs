//! The article list and the editor: creating, saving, previewing and deleting.

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Response};
use axum::Form;
use chrono::{DateTime, NaiveDateTime, Utc};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::auth::AdminUser;
use crate::error::AppResult;
use crate::markdown;
use crate::models::{self, Article, ArticleInput, Tag};
use crate::routes::public::{self, media_url};
use crate::settings::Settings;
use crate::state::AppState;

use super::{admin_context, notice_text, redirect, render, CsrfForm, Ctx, NoticeQuery};

fn admin_article_row(s: &Settings, a: &Article, tags: &[Tag], now: DateTime<Utc>) -> Value {
    let state = a.state_label(now);
    let date = match state {
        "draft" => format!("Edited {}", public::format_date(a.updated_at)),
        "scheduled" => format!(
            "Goes live {}",
            a.published_at
                .map(|d| d.format("%B %-d, %Y at %H:%M UTC").to_string())
                .unwrap_or_default()
        ),
        _ => format!(
            "Published {}",
            a.published_at.map(public::format_date).unwrap_or_default()
        ),
    };
    json!({
        "id": a.id,
        "title": a.title,
        "state": state,
        "date": date,
        "tags": tags.iter().map(|t| &t.name).collect::<Vec<_>>(),
        "edit_url": format!("{}/admin/articles/{}/edit", s.base_path, a.id),
        "public_url": (state == "published").then(|| format!("{}/posts/{}", s.base_path, a.slug)),
    })
}

pub(super) async fn article_rows(
    state: &AppState,
    s: &Settings,
    articles: &[Article],
) -> AppResult<Vec<Value>> {
    let ids: Vec<i64> = articles.iter().map(|a| a.id).collect();
    let tags = models::tags_for(&state.db, &ids).await?;
    let now = Utc::now();
    Ok(articles
        .iter()
        .map(|a| admin_article_row(s, a, tags.get(&a.id).map(Vec::as_slice).unwrap_or(&[]), now))
        .collect())
}

pub async fn articles(
    State(state): State<AppState>,
    user: AdminUser,
    Query(q): Query<NoticeQuery>,
) -> AppResult<Response> {
    let s = state.settings();
    let filter = q
        .status
        .as_deref()
        .filter(|f| matches!(*f, "draft" | "published" | "scheduled"))
        .unwrap_or("all");
    let list = models::list_admin(&state.db, filter, 1000).await?;
    let mut ctx = admin_context(&s, Some(&user), "articles");
    ctx.insert(
        "articles".into(),
        json!(article_rows(&state, &s, &list).await?),
    );
    ctx.insert("filter".into(), json!(filter));
    ctx.insert("counts".into(), json!(models::counts(&state.db).await?));
    ctx.insert("notice".into(), json!(notice_text(q.notice.as_deref())));
    Ok(render(&state, "articles", &ctx)?.into_response())
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct ArticleForm {
    #[serde(rename = "_csrf")]
    csrf: String,
    title: String,
    slug: String,
    content: String,
    excerpt: String,
    cover_image: String,
    meta_title: String,
    meta_description: String,
    tags: String,
    /// `datetime-local` value in the author's time zone…
    published_at: String,
    /// …and that zone's offset from UTC in minutes, as reported by the browser
    /// (`Date.getTimezoneOffset`). Missing without JavaScript, meaning UTC.
    tz_offset: String,
    /// `publish`, `draft`, or empty to keep the current status.
    action: String,
}

impl ArticleForm {
    fn from_article(a: &Article, tags: &[Tag]) -> Self {
        ArticleForm {
            title: a.title.clone(),
            slug: a.slug.clone(),
            content: a.content.clone(),
            excerpt: a.excerpt.clone(),
            cover_image: a.cover_image.clone(),
            meta_title: a.meta_title.clone(),
            meta_description: a.meta_description.clone(),
            tags: tags
                .iter()
                .map(|t| t.name.as_str())
                .collect::<Vec<_>>()
                .join(", "),
            published_at: a
                .published_at
                .map(|d| d.format("%Y-%m-%dT%H:%M").to_string())
                .unwrap_or_default(),
            tz_offset: "0".into(),
            ..Default::default()
        }
    }
}

pub fn parse_local_datetime(
    value: &str,
    tz_offset_minutes: &str,
) -> Result<Option<DateTime<Utc>>, String> {
    let value = value.trim();
    if value.is_empty() {
        return Ok(None);
    }
    let naive = NaiveDateTime::parse_from_str(value, "%Y-%m-%dT%H:%M")
        .or_else(|_| NaiveDateTime::parse_from_str(value, "%Y-%m-%dT%H:%M:%S"))
        .map_err(|_| "The publication date isn't a valid date and time.".to_string())?;
    let offset: i64 = tz_offset_minutes
        .trim()
        .parse()
        .unwrap_or(0)
        .clamp(-1440, 1440);
    Ok(Some(naive.and_utc() + chrono::Duration::minutes(offset)))
}

async fn build_input(
    state: &AppState,
    form: &ArticleForm,
    existing: Option<&Article>,
) -> AppResult<Result<ArticleInput, Vec<String>>> {
    let mut errors = Vec::new();
    let title = form.title.split_whitespace().collect::<Vec<_>>().join(" ");
    if title.is_empty() {
        errors.push("Give the article a title.".to_string());
    } else if title.chars().count() > 200 {
        errors.push("Keep the title under 200 characters.".to_string());
    }

    let except = existing.map(|a| a.id);
    let slug = if form.slug.trim().is_empty() {
        let base = slug::slugify(&title);
        let base = if base.is_empty() {
            "article".to_string()
        } else {
            base
        };
        models::unique_slug(&state.db, &base, except).await?
    } else {
        let slug = slug::slugify(form.slug.trim());
        if slug.is_empty() {
            errors.push("The URL slug needs at least one letter or number.".to_string());
        } else if let Some(owner) = models::slug_owner(&state.db, &slug, except).await? {
            errors.push(format!(
                "The URL slug “{slug}” is already used by “{owner}”. Choose another one."
            ));
        }
        slug
    };

    let status = match form.action.as_str() {
        "publish" => "published",
        "draft" => "draft",
        _ => existing.map(|a| a.status.as_str()).unwrap_or("draft"),
    }
    .to_string();
    let published_at = match parse_local_datetime(&form.published_at, &form.tz_offset) {
        Ok(Some(d)) => Some(d),
        Ok(None) if status == "published" => Some(
            existing
                .and_then(|a| a.published_at)
                .unwrap_or_else(Utc::now),
        ),
        Ok(None) => None,
        Err(e) => {
            errors.push(e);
            None
        }
    };

    let cover_image = form.cover_image.trim().to_string();
    if !(cover_image.is_empty()
        || cover_image.starts_with("/media/")
        || cover_image.starts_with("https://")
        || cover_image.starts_with("http://"))
    {
        errors.push("The cover image must be an uploaded file or a full web address starting with https://.".to_string());
    }

    if !errors.is_empty() {
        return Ok(Err(errors));
    }
    Ok(Ok(ArticleInput {
        title,
        slug,
        content: form.content.replace("\r\n", "\n"),
        excerpt: form.excerpt.trim().to_string(),
        cover_image,
        meta_title: form.meta_title.trim().to_string(),
        meta_description: form.meta_description.trim().to_string(),
        status,
        published_at,
        tags: models::parse_tags(&form.tags),
    }))
}

fn editor_context(
    s: &Settings,
    user: &AdminUser,
    form: &ArticleForm,
    article: Option<&Article>,
    errors: &[String],
    notice: Option<&str>,
) -> Ctx {
    let now = Utc::now();
    let mut ctx = admin_context(s, Some(user), "editor");
    ctx.insert(
        "form".into(),
        json!({
            "title": form.title, "slug": form.slug, "content": form.content, "excerpt": form.excerpt,
            "cover_image": form.cover_image, "cover_preview": media_url(&s.base_path, form.cover_image.trim()),
            "meta_title": form.meta_title, "meta_description": form.meta_description, "tags": form.tags,
            "published_at": form.published_at, "tz_offset": form.tz_offset,
        }),
    );
    ctx.insert(
        "article".into(),
        json!(article.map(|a| json!({
            "id": a.id,
            "state": a.state_label(now),
            "action": format!("{}/admin/articles/{}", s.base_path, a.id),
            "public_url": (a.state_label(now) == "published").then(|| format!("{}/posts/{}", s.base_path, a.slug)),
            "updated": public::format_date(a.updated_at),
        }))),
    );
    ctx.insert(
        "form_action".into(),
        json!(article
            .map(|a| format!("{}/admin/articles/{}", s.base_path, a.id))
            .unwrap_or_else(|| format!("{}/admin/articles", s.base_path))),
    );
    ctx.insert("errors".into(), json!(errors));
    ctx.insert("notice".into(), json!(notice));
    ctx.insert(
        "slug_prefix".into(),
        json!(format!("{}/posts/", s.base_url)),
    );
    ctx
}

pub async fn new_article(State(state): State<AppState>, user: AdminUser) -> AppResult<Response> {
    let s = state.settings();
    let form = ArticleForm::default();
    Ok(render(
        &state,
        "editor",
        &editor_context(&s, &user, &form, None, &[], None),
    )?
    .into_response())
}

pub async fn edit_article(
    State(state): State<AppState>,
    user: AdminUser,
    Path(id): Path<i64>,
    Query(q): Query<NoticeQuery>,
) -> AppResult<Response> {
    let s = state.settings();
    let Some(a) = models::get(&state.db, id).await? else {
        return public::not_found(State(state), "/admin/missing".parse().unwrap()).await;
    };
    let tags = models::tags_for(&state.db, &[a.id])
        .await?
        .remove(&a.id)
        .unwrap_or_default();
    let form = ArticleForm::from_article(&a, &tags);
    let ctx = editor_context(
        &s,
        &user,
        &form,
        Some(&a),
        &[],
        notice_text(q.notice.as_deref()),
    );
    Ok(render(&state, "editor", &ctx)?.into_response())
}

fn status_notice(before: Option<&Article>, input: &ArticleInput) -> &'static str {
    let was_published = before.is_some_and(|a| a.is_published());
    match (was_published, input.status.as_str()) {
        (false, "published") if input.published_at.is_some_and(|d| d > Utc::now()) => "scheduled",
        (false, "published") => "published",
        (true, "draft") => "unpublished",
        _ => "saved",
    }
}

pub async fn create_article(
    State(state): State<AppState>,
    user: AdminUser,
    Form(form): Form<ArticleForm>,
) -> AppResult<Response> {
    if let Err(r) = user.check_csrf(&form.csrf) {
        return Ok(r);
    }
    let s = state.settings();
    let input = match build_input(&state, &form, None).await? {
        Ok(input) => input,
        Err(errors) => {
            let ctx = editor_context(&s, &user, &form, None, &errors, None);
            return Ok((
                StatusCode::UNPROCESSABLE_ENTITY,
                render(&state, "editor", &ctx)?,
            )
                .into_response());
        }
    };
    let id = models::create(&state.db, &input, user.id).await?;
    let notice = status_notice(None, &input);
    Ok(redirect(
        &s,
        &format!("/admin/articles/{id}/edit?notice={notice}"),
    ))
}

pub async fn update_article(
    State(state): State<AppState>,
    user: AdminUser,
    Path(id): Path<i64>,
    Form(form): Form<ArticleForm>,
) -> AppResult<Response> {
    if let Err(r) = user.check_csrf(&form.csrf) {
        return Ok(r);
    }
    let s = state.settings();
    let Some(existing) = models::get(&state.db, id).await? else {
        return Ok((StatusCode::NOT_FOUND, "That article no longer exists.").into_response());
    };
    let input = match build_input(&state, &form, Some(&existing)).await? {
        Ok(input) => input,
        Err(errors) => {
            let ctx = editor_context(&s, &user, &form, Some(&existing), &errors, None);
            return Ok((
                StatusCode::UNPROCESSABLE_ENTITY,
                render(&state, "editor", &ctx)?,
            )
                .into_response());
        }
    };
    models::update(&state.db, &existing, &input).await?;
    let notice = status_notice(Some(&existing), &input);
    Ok(redirect(
        &s,
        &format!("/admin/articles/{id}/edit?notice={notice}"),
    ))
}

pub async fn delete_article(
    State(state): State<AppState>,
    user: AdminUser,
    Path(id): Path<i64>,
    Form(form): Form<CsrfForm>,
) -> AppResult<Response> {
    if let Err(r) = user.check_csrf(&form.csrf) {
        return Ok(r);
    }
    models::delete(&state.db, id).await?;
    Ok(redirect(
        &state.settings(),
        "/admin/articles?notice=deleted",
    ))
}

/// Renders unsaved editor contents with the live theme, without saving.
pub async fn preview_article(
    State(state): State<AppState>,
    user: AdminUser,
    Form(form): Form<ArticleForm>,
) -> AppResult<Response> {
    if let Err(r) = user.check_csrf(&form.csrf) {
        return Ok(r);
    }
    let s = state.settings();
    let now = Utc::now();
    let title = if form.title.trim().is_empty() {
        "Untitled article".to_string()
    } else {
        form.title.trim().to_string()
    };
    let article = Article {
        id: 0,
        slug: if form.slug.trim().is_empty() {
            slug::slugify(&title)
        } else {
            slug::slugify(&form.slug)
        },
        title,
        content: form.content.replace("\r\n", "\n"),
        excerpt: form.excerpt.trim().to_string(),
        status: "draft".into(),
        cover_image: form.cover_image.trim().to_string(),
        meta_title: form.meta_title.clone(),
        meta_description: form.meta_description.clone(),
        author_id: Some(user.id),
        published_at: parse_local_datetime(&form.published_at, &form.tz_offset)
            .ok()
            .flatten()
            .or(Some(now)),
        created_at: now,
        updated_at: now,
    };
    let tags: Vec<Tag> = models::parse_tags(&form.tags)
        .into_iter()
        .map(|name| Tag {
            id: 0,
            slug: slug::slugify(&name),
            name,
        })
        .collect();
    let mut ctx =
        public::article_context_with_tags(&state, &s, &s.theme, &article, &tags, true).await?;
    ctx.insert(
        "preview_label".into(),
        json!("Preview — this is how the article will look. It hasn't been saved."),
    );
    ctx.insert("noindex".into(), json!(true));
    Ok(public::render_page(&state, &s.theme, "article", &ctx)?.into_response())
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct MarkdownForm {
    #[serde(rename = "_csrf")]
    csrf: String,
    content: String,
}

/// Live preview pane in the editor.
pub async fn preview_markdown(
    State(state): State<AppState>,
    user: AdminUser,
    Form(form): Form<MarkdownForm>,
) -> Response {
    if let Err(r) = user.check_csrf(&form.csrf) {
        return r;
    }
    Html(markdown::render(&form.content, &state.settings().base_path)).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_datetime_uses_browser_offset() {
        // UTC+2 reports an offset of -120.
        let d = parse_local_datetime("2026-05-01T12:00", "-120")
            .unwrap()
            .unwrap();
        assert_eq!(d.to_rfc3339(), "2026-05-01T10:00:00+00:00");
        assert_eq!(parse_local_datetime("", "0").unwrap(), None);
        assert!(parse_local_datetime("yesterday", "0").is_err());
    }
}
