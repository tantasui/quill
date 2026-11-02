use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::{header, StatusCode, Uri};
use axum::response::{Html, IntoResponse, Redirect, Response};
use chrono::{DateTime, Datelike, Utc};
use quill_design::{HomeSection, Layout};
use serde::Deserialize;
use serde_json::{json, Map, Value};

use crate::error::AppResult;
use crate::markdown;
use crate::models::{self, Article, Tag};
use crate::settings::Settings;
use crate::state::{AppState, Look};

use super::VERSION;

type Ctx = Map<String, Value>;

/// Values every public template can rely on (the template contract).
pub async fn base_context(state: &AppState, s: &Settings, look: &Look) -> AppResult<Ctx> {
    let tags = models::public_tag_counts(&state.db, 15).await?;
    let mut ctx = Ctx::new();
    ctx.insert(
        "site".into(),
        json!({
            "name": s.site_name,
            "description": s.description,
            "url": s.base_url,
            "language": s.language,
            "author": s.author_name,
            "footer": s.footer_text,
        }),
    );
    ctx.insert("base".into(), json!(s.base_path));
    let tokens = match look.design {
        Some(id) => format!("design-{id}"),
        None => look.preset.clone(),
    };
    ctx.insert(
        "tokens_url".into(),
        json!(format!(
            "{}/static/tokens/{tokens}.css?v={VERSION}",
            s.base_path
        )),
    );
    ctx.insert("theme".into(), json!(look.preset));
    ctx.insert("layout".into(), json!(look.layout));
    ctx.insert("version".into(), json!(VERSION));
    ctx.insert("feed_url".into(), json!(s.absolute("/feed.xml")));
    ctx.insert(
        "nav_tags".into(),
        json!(tags
            .iter()
            .map(|t| json!({"name": t.name, "count": t.count, "url": format!("{}/tags/{}", s.base_path, t.slug)}))
            .collect::<Vec<_>>()),
    );
    ctx.insert("year".into(), json!(Utc::now().year()));
    Ok(ctx)
}

pub fn render_page(state: &AppState, page: &str, ctx: &Ctx) -> AppResult<Html<String>> {
    Ok(Html(
        state.views.render(&format!("public/{page}.html"), ctx)?,
    ))
}

/// Splits articles between the home page's sections. A `featured` section takes
/// the newest article, but only on the first page.
pub fn insert_home_articles(ctx: &mut Ctx, layout: &Layout, views: Vec<Value>, first_page: bool) {
    let mut views = views.into_iter();
    if first_page && layout.home.contains(&HomeSection::Featured) {
        ctx.insert("lead".into(), json!(views.next()));
    }
    ctx.insert("feed_articles".into(), json!(views.collect::<Vec<_>>()));
}

/// Resolves an article image reference to a URL, prefixing uploaded media
/// with `prefix` (the base path, or the absolute base URL for feeds).
pub fn media_url(prefix: &str, reference: &str) -> String {
    if reference.starts_with("/media/") {
        format!("{prefix}{reference}")
    } else {
        reference.to_string()
    }
}

pub fn format_date(d: DateTime<Utc>) -> String {
    d.format("%B %-d, %Y").to_string()
}

/// The view model handed to templates for one article.
pub fn article_view(s: &Settings, a: &Article, tags: &[Tag], full: bool) -> Value {
    let date = a.published_at.unwrap_or(a.updated_at);
    let path = format!("/posts/{}", a.slug);
    let excerpt = if a.excerpt.trim().is_empty() {
        markdown::plain_text(&a.content, 220)
    } else {
        a.excerpt.trim().to_string()
    };
    let mut v = json!({
        "id": a.id,
        "title": a.title,
        "slug": a.slug,
        "url": format!("{}{path}", s.base_path),
        "abs_url": s.absolute(&path),
        "excerpt": excerpt,
        "has_excerpt": !a.excerpt.trim().is_empty(),
        "author": s.author_name,
        "cover_image": media_url(&s.base_path, &a.cover_image),
        "date": format_date(date),
        "date_short": date.format("%b %-d").to_string(),
        "date_iso": date.to_rfc3339(),
        "updated_iso": a.updated_at.to_rfc3339(),
        "year": date.year(),
        "reading_minutes": markdown::reading_minutes(&a.content),
        "tags": tags.iter().map(|t| json!({"name": t.name, "slug": t.slug, "url": format!("{}/tags/{}", s.base_path, t.slug)})).collect::<Vec<_>>(),
    });
    if full {
        v["html"] = json!(markdown::render(&a.content, &s.base_path));
    }
    v
}

pub async fn article_views(
    state: &AppState,
    s: &Settings,
    articles: &[Article],
) -> AppResult<Vec<Value>> {
    let ids: Vec<i64> = articles.iter().map(|a| a.id).collect();
    let tags = models::tags_for(&state.db, &ids).await?;
    Ok(articles
        .iter()
        .map(|a| {
            article_view(
                s,
                a,
                tags.get(&a.id).map(Vec::as_slice).unwrap_or(&[]),
                false,
            )
        })
        .collect())
}

fn pagination(base: &str, page: i64, total_pages: i64) -> Value {
    let url = |n: i64| {
        if n == 1 {
            format!("{base}/")
        } else {
            format!("{base}/page/{n}")
        }
    };
    json!({
        "current": page,
        "total": total_pages,
        "newer_url": (page > 1).then(|| url(page - 1)),
        "older_url": (page < total_pages).then(|| url(page + 1)),
    })
}

fn seo(ctx: &mut Ctx, title: String, description: &str, canonical: String) {
    ctx.insert("page_title".into(), json!(title));
    ctx.insert(
        "meta_description".into(),
        json!(markdown::truncate_words(description, 160)),
    );
    ctx.insert("canonical".into(), json!(canonical));
}

// ---- Home ---------------------------------------------------------------------

#[derive(Deserialize)]
pub struct PageQuery {
    page: Option<i64>,
}

pub async fn home(
    State(state): State<AppState>,
    Query(q): Query<PageQuery>,
) -> AppResult<Response> {
    let s = state.settings();
    render_home(&state, &s, q.page.unwrap_or(1)).await
}

pub async fn home_page(
    State(state): State<AppState>,
    Path(n): Path<String>,
) -> AppResult<Response> {
    let s = state.settings();
    match n.parse::<i64>() {
        Ok(1) => Ok(Redirect::permanent(&format!("{}/", s.base_path)).into_response()),
        Ok(n) if n > 1 => render_home(&state, &s, n).await,
        _ => not_found_page(&state, &s).await,
    }
}

pub async fn render_home(state: &AppState, s: &Settings, page: i64) -> AppResult<Response> {
    if state.needs_setup() {
        let html = state.views.render(
            "public/setup_pending.html",
            json!({"base": s.base_path, "version": VERSION}),
        )?;
        return Ok(Html(html).into_response());
    }
    let per_page = s.posts_per_page;
    let total = models::count_published(&state.db).await?;
    let total_pages = ((total + per_page - 1) / per_page).max(1);
    if page < 1 || page > total_pages {
        return not_found_page(state, s).await;
    }
    let articles = models::list_published(&state.db, per_page, (page - 1) * per_page).await?;
    let look = state.look();
    let mut ctx = base_context(state, s, &look).await?;
    let views = article_views(state, s, &articles).await?;
    insert_home_articles(&mut ctx, &look.layout, views, page == 1);
    ctx.insert(
        "pagination".into(),
        pagination(&s.base_path, page, total_pages),
    );
    let path = if page == 1 {
        "/".to_string()
    } else {
        format!("/page/{page}")
    };
    let title = if page == 1 {
        s.site_name.clone()
    } else {
        format!("{} — page {page}", s.site_name)
    };
    seo(&mut ctx, title, &s.description, s.absolute(&path));
    ctx.insert("is_home".into(), json!(page == 1));
    Ok(render_page(state, "index", &ctx)?.into_response())
}

// ---- Article --------------------------------------------------------------------

pub async fn article(
    State(state): State<AppState>,
    Path(slug): Path<String>,
) -> AppResult<Response> {
    let s = state.settings();
    let Some(a) = models::find_published(&state.db, &slug).await? else {
        if let Some(current) = models::find_redirect(&state.db, &slug).await? {
            return Ok(
                Redirect::permanent(&format!("{}/posts/{current}", s.base_path)).into_response(),
            );
        }
        return not_found_page(&state, &s).await;
    };
    let ctx = article_context(&state, &s, &state.look(), &a, false).await?;
    Ok(render_page(&state, "article", &ctx)?.into_response())
}

/// Context for the article page; also used by the dashboard's preview.
pub async fn article_context(
    state: &AppState,
    s: &Settings,
    look: &Look,
    a: &Article,
    preview: bool,
) -> AppResult<Ctx> {
    let tags = models::tags_for(&state.db, &[a.id])
        .await?
        .remove(&a.id)
        .unwrap_or_default();
    article_context_with_tags(state, s, look, a, &tags, preview).await
}

pub async fn article_context_with_tags(
    state: &AppState,
    s: &Settings,
    look: &Look,
    a: &Article,
    tags: &[Tag],
    preview: bool,
) -> AppResult<Ctx> {
    let mut ctx = base_context(state, s, look).await?;
    let view = article_view(s, a, tags, true);
    let description = if a.meta_description.trim().is_empty() {
        view["excerpt"].as_str().unwrap_or_default().to_string()
    } else {
        a.meta_description.trim().to_string()
    };
    let title = if a.meta_title.trim().is_empty() {
        a.title.clone()
    } else {
        a.meta_title.trim().to_string()
    };
    seo(
        &mut ctx,
        format!("{title} — {}", s.site_name),
        &description,
        view["abs_url"].as_str().unwrap_or_default().to_string(),
    );
    ctx.insert("og_type".into(), json!("article"));
    if !a.cover_image.is_empty() {
        ctx.insert(
            "og_image".into(),
            json!(media_url(&s.base_url, &a.cover_image)),
        );
    }
    if !preview {
        let (older, newer) = models::neighbours(&state.db, a).await?;
        ctx.insert(
            "older".into(),
            json!(older.map(|o| article_view(s, &o, &[], false))),
        );
        ctx.insert(
            "newer".into(),
            json!(newer.map(|n| article_view(s, &n, &[], false))),
        );
    }
    ctx.insert("article".into(), view);
    ctx.insert("preview".into(), json!(preview));
    Ok(ctx)
}

// ---- Tags and archive -------------------------------------------------------------

pub async fn tag(
    State(state): State<AppState>,
    Path(slug): Path<String>,
    Query(q): Query<PageQuery>,
) -> AppResult<Response> {
    let s = state.settings();
    let Some(tag) = models::find_tag(&state.db, &slug).await? else {
        return not_found_page(&state, &s).await;
    };
    let per_page = s.posts_per_page;
    let total = models::count_published_by_tag(&state.db, tag.id).await?;
    let total_pages = ((total + per_page - 1) / per_page).max(1);
    let page = q.page.unwrap_or(1);
    if total == 0 || page < 1 || page > total_pages {
        return not_found_page(&state, &s).await;
    }
    let articles =
        models::list_published_by_tag(&state.db, tag.id, per_page, (page - 1) * per_page).await?;
    let mut ctx = base_context(&state, &s, &state.look()).await?;
    let tag_path = format!("{}/tags/{}", s.base_path, tag.slug);
    ctx.insert(
        "tag".into(),
        json!({"name": tag.name, "slug": tag.slug, "count": total}),
    );
    ctx.insert(
        "articles".into(),
        json!(article_views(&state, &s, &articles).await?),
    );
    ctx.insert(
        "pagination".into(),
        json!({
            "current": page,
            "total": total_pages,
            "newer_url": (page > 1).then(|| format!("{tag_path}?page={}", page - 1)),
            "older_url": (page < total_pages).then(|| format!("{tag_path}?page={}", page + 1)),
        }),
    );
    let description = format!("Articles tagged “{}” on {}.", tag.name, s.site_name);
    seo(
        &mut ctx,
        format!("{} — {}", tag.name, s.site_name),
        &description,
        s.absolute(&format!("/tags/{}", tag.slug)),
    );
    Ok(render_page(&state, "tag", &ctx)?.into_response())
}

pub async fn archive(State(state): State<AppState>) -> AppResult<Response> {
    let s = state.settings();
    let articles = models::list_published(&state.db, i64::MAX, 0).await?;
    let mut years: Vec<Value> = Vec::new();
    for a in &articles {
        let view = article_view(&s, a, &[], false);
        let year = view["year"].clone();
        match years.last_mut() {
            Some(group) if group["year"] == year => {
                group["articles"].as_array_mut().unwrap().push(view)
            }
            _ => years.push(json!({"year": year, "articles": [view]})),
        }
    }
    let mut ctx = base_context(&state, &s, &state.look()).await?;
    ctx.insert("years".into(), json!(years));
    ctx.insert("total".into(), json!(articles.len()));
    seo(
        &mut ctx,
        format!("Archive — {}", s.site_name),
        &format!("Every article published on {}.", s.site_name),
        s.absolute("/archive"),
    );
    Ok(render_page(&state, "archive", &ctx)?.into_response())
}

// ---- Feeds and crawlers --------------------------------------------------------------

pub async fn feed(State(state): State<AppState>) -> AppResult<Response> {
    let s = state.settings();
    let articles = models::list_published(&state.db, 20, 0).await?;
    let entries: Vec<Value> = articles
        .iter()
        .map(|a| {
            let mut v = article_view(&s, a, &[], false);
            // Feed readers need absolute URLs for images and links.
            v["html"] = json!(markdown::render(&a.content, &s.base_url));
            v["cover_image"] = json!(media_url(&s.base_url, &a.cover_image));
            v
        })
        .collect();
    let updated = articles
        .iter()
        .map(|a| a.updated_at)
        .max()
        .unwrap_or_else(Utc::now);
    let xml = state.views.render(
        "public/feed.xml",
        json!({"site": {"name": s.site_name, "description": s.description, "url": s.base_url, "author": s.author_name},
               "entries": entries, "updated": updated.to_rfc3339(), "feed_url": s.absolute("/feed.xml")}),
    )?;
    Ok((
        [(header::CONTENT_TYPE, "application/atom+xml; charset=utf-8")],
        xml,
    )
        .into_response())
}

pub async fn sitemap(State(state): State<AppState>) -> AppResult<Response> {
    let s = state.settings();
    let articles = models::list_published(&state.db, 50_000, 0).await?;
    let tags = models::public_tag_counts(&state.db, 10_000).await?;
    let mut urls = vec![
        json!({"loc": s.absolute("/")}),
        json!({"loc": s.absolute("/archive")}),
    ];
    urls.extend(
        articles
            .iter()
            .map(|a| json!({"loc": s.absolute(&format!("/posts/{}", a.slug)), "lastmod": a.updated_at.format("%Y-%m-%d").to_string()})),
    );
    urls.extend(
        tags.iter()
            .map(|t| json!({"loc": s.absolute(&format!("/tags/{}", t.slug))})),
    );
    let xml = state
        .views
        .render("public/sitemap.xml", json!({"urls": urls}))?;
    Ok((
        [(header::CONTENT_TYPE, "application/xml; charset=utf-8")],
        xml,
    )
        .into_response())
}

pub async fn robots(State(state): State<AppState>) -> Response {
    let s = state.settings();
    let body = format!(
        "User-agent: *\nDisallow: {base}/admin\n\nSitemap: {sitemap}\n",
        base = s.base_path,
        sitemap = s.absolute("/sitemap.xml")
    );
    ([(header::CONTENT_TYPE, "text/plain; charset=utf-8")], body).into_response()
}

// ---- Media and errors ---------------------------------------------------------------------

/// Uploaded files have random names of the form `<hex>.<ext>`; anything else
/// is rejected before touching the filesystem.
pub fn valid_media_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 80
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
        && !name.starts_with('.')
        && !name.contains("..")
}

pub async fn media(State(state): State<AppState>, Path(name): Path<String>) -> AppResult<Response> {
    if !valid_media_name(&name) {
        return Ok((StatusCode::NOT_FOUND, "Not found").into_response());
    }
    match tokio::fs::read(state.media_dir().join(&name)).await {
        Ok(bytes) => {
            let mime = mime_guess::from_path(&name).first_or_octet_stream();
            Ok((
                [
                    (header::CONTENT_TYPE, mime.as_ref().to_string()),
                    (
                        header::CACHE_CONTROL,
                        "public, max-age=31536000, immutable".to_string(),
                    ),
                    (
                        header::CONTENT_SECURITY_POLICY,
                        "default-src 'none'; sandbox".to_string(),
                    ),
                ],
                Body::from(bytes),
            )
                .into_response())
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            Ok((StatusCode::NOT_FOUND, "Not found").into_response())
        }
        Err(e) => Err(e.into()),
    }
}

pub async fn not_found(State(state): State<AppState>, uri: Uri) -> AppResult<Response> {
    let s = state.settings();
    if uri.path().starts_with("/admin") {
        let html = state.views.render(
            "admin/not_found.html",
            json!({"base": s.base_path, "version": VERSION}),
        )?;
        return Ok((StatusCode::NOT_FOUND, Html(html)).into_response());
    }
    not_found_page(&state, &s).await
}

pub async fn not_found_page(state: &AppState, s: &Settings) -> AppResult<Response> {
    if state.needs_setup() {
        return Ok((StatusCode::NOT_FOUND, "Not found").into_response());
    }
    let mut ctx = base_context(state, s, &state.look()).await?;
    ctx.insert(
        "page_title".into(),
        json!(format!("Page not found — {}", s.site_name)),
    );
    ctx.insert("noindex".into(), json!(true));
    let recent = models::list_published(&state.db, 5, 0).await?;
    ctx.insert(
        "recent".into(),
        json!(article_views(state, s, &recent).await?),
    );
    Ok((
        StatusCode::NOT_FOUND,
        render_page(state, "not_found", &ctx)?,
    )
        .into_response())
}
