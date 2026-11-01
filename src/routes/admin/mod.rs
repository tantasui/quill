pub mod account;
pub mod appearance;
pub mod integration;
pub mod media;
pub mod settings;

use axum::extract::{Path, Query, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum::{Extension, Form};
use chrono::{DateTime, NaiveDateTime, Utc};
use serde::Deserialize;
use serde_json::{json, Map, Value};

use crate::auth::{self, AdminUser};
use crate::error::AppResult;
use crate::markdown;
use crate::models::{self, Article, ArticleInput, Tag};
use crate::routes::public::{self, media_url};
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

// ---- Login ----------------------------------------------------------------------

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct LoginForm {
    username: String,
    password: String,
}

pub async fn login_form(State(state): State<AppState>) -> AppResult<Response> {
    let s = state.settings();
    if state.needs_setup() {
        return Ok(redirect(&s, "/admin/setup"));
    }
    let mut ctx = admin_context(&s, None, "login");
    ctx.insert("username".into(), json!(""));
    Ok(render(&state, "login", &ctx)?.into_response())
}

pub async fn login_submit(State(state): State<AppState>, headers: HeaderMap, Form(form): Form<LoginForm>) -> AppResult<Response> {
    let s = state.settings();
    if state.needs_setup() {
        return Ok(redirect(&s, "/admin/setup"));
    }
    let username = form.username.trim();
    let fail = |msg: &str| -> AppResult<Response> {
        let mut ctx = admin_context(&s, None, "login");
        ctx.insert("username".into(), json!(username));
        ctx.insert("error".into(), json!(msg));
        Ok((StatusCode::UNAUTHORIZED, render(&state, "login", &ctx)?).into_response())
    };
    if state.login_limiter.is_blocked(username) {
        return fail(
            "Too many failed attempts for this account. Wait 15 minutes and try again, or reset the password from the server with “quill reset-password”.",
        );
    }
    let user: Option<(i64, String)> = sqlx::query_as("SELECT id, password_hash FROM users WHERE username = ?")
        .bind(username)
        .fetch_optional(&state.db)
        .await?;
    // Hash even for unknown users so response timing doesn't reveal which exist.
    let valid = match &user {
        Some((_, hash)) => auth::verify_password(&form.password, hash),
        None => {
            let _ = auth::verify_password(
                &form.password,
                "$argon2id$v=19$m=19456,t=2,p=1$c29tZXNhbHRzb21lc2FsdA$AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
            );
            false
        }
    };
    let Some((user_id, _)) = user.filter(|_| valid) else {
        state.login_limiter.record_failure(username);
        return fail("That username and password don't match.");
    };
    state.login_limiter.clear(username);
    let token = auth::create_session(&state.db, user_id).await?;
    let mut res = redirect(&s, "/admin");
    res.headers_mut()
        .insert(header::SET_COOKIE, auth::session_cookie(&token, auth::is_https(&headers)).parse()?);
    Ok(res)
}

pub async fn logout(State(state): State<AppState>, user: AdminUser, headers: HeaderMap, Form(form): Form<CsrfForm>) -> AppResult<Response> {
    if let Err(r) = user.check_csrf(&form.csrf) {
        return Ok(r);
    }
    if let Some(token) = auth::session_token(&headers) {
        auth::delete_session(&state.db, &token).await?;
    }
    let mut res = redirect(&state.settings(), "/admin/login");
    res.headers_mut().insert(header::SET_COOKIE, auth::clear_session_cookie().parse()?);
    Ok(res)
}

// ---- Dashboard -------------------------------------------------------------------

fn admin_article_row(s: &Settings, a: &Article, tags: &[Tag], now: DateTime<Utc>) -> Value {
    let state = a.state_label(now);
    let date = match state {
        "draft" => format!("Edited {}", public::format_date(a.updated_at)),
        "scheduled" => format!(
            "Goes live {}",
            a.published_at.map(|d| d.format("%B %-d, %Y at %H:%M UTC").to_string()).unwrap_or_default()
        ),
        _ => format!("Published {}", a.published_at.map(public::format_date).unwrap_or_default()),
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

async fn article_rows(state: &AppState, s: &Settings, articles: &[Article]) -> AppResult<Vec<Value>> {
    let ids: Vec<i64> = articles.iter().map(|a| a.id).collect();
    let tags = models::tags_for(&state.db, &ids).await?;
    let now = Utc::now();
    Ok(articles
        .iter()
        .map(|a| admin_article_row(s, a, tags.get(&a.id).map(Vec::as_slice).unwrap_or(&[]), now))
        .collect())
}

pub async fn dashboard(State(state): State<AppState>, user: AdminUser, headers: HeaderMap, Query(q): Query<NoticeQuery>) -> AppResult<Response> {
    let s = state.settings();
    let counts = models::counts(&state.db).await?;
    let drafts = models::list_admin(&state.db, "draft", 5).await?;
    let recent = models::list_admin(&state.db, "published", 5).await?;
    let scheduled = models::list_admin(&state.db, "scheduled", 5).await?;
    let mut ctx = admin_context(&s, Some(&user), "dashboard");
    ctx.insert("counts".into(), json!(counts));
    ctx.insert("drafts".into(), json!(article_rows(&state, &s, &drafts).await?));
    ctx.insert("recent".into(), json!(article_rows(&state, &s, &recent).await?));
    ctx.insert("scheduled".into(), json!(article_rows(&state, &s, &scheduled).await?));
    ctx.insert("notice".into(), json!(notice_text(q.notice.as_deref())));
    ctx.insert("origin_warning".into(), json!(origin_warning(&headers, &s)));
    ctx.insert("theme_name".into(), json!(themes::find(&s.theme).map(|t| t.name)));
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

pub async fn articles(State(state): State<AppState>, user: AdminUser, Query(q): Query<NoticeQuery>) -> AppResult<Response> {
    let s = state.settings();
    let filter = q
        .status
        .as_deref()
        .filter(|f| matches!(*f, "draft" | "published" | "scheduled"))
        .unwrap_or("all");
    let list = models::list_admin(&state.db, filter, 1000).await?;
    let mut ctx = admin_context(&s, Some(&user), "articles");
    ctx.insert("articles".into(), json!(article_rows(&state, &s, &list).await?));
    ctx.insert("filter".into(), json!(filter));
    ctx.insert("counts".into(), json!(models::counts(&state.db).await?));
    ctx.insert("notice".into(), json!(notice_text(q.notice.as_deref())));
    Ok(render(&state, "articles", &ctx)?.into_response())
}

// ---- Editor ------------------------------------------------------------------------

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
            tags: tags.iter().map(|t| t.name.as_str()).collect::<Vec<_>>().join(", "),
            published_at: a.published_at.map(|d| d.format("%Y-%m-%dT%H:%M").to_string()).unwrap_or_default(),
            tz_offset: "0".into(),
            ..Default::default()
        }
    }
}

pub fn parse_local_datetime(value: &str, tz_offset_minutes: &str) -> Result<Option<DateTime<Utc>>, String> {
    let value = value.trim();
    if value.is_empty() {
        return Ok(None);
    }
    let naive = NaiveDateTime::parse_from_str(value, "%Y-%m-%dT%H:%M")
        .or_else(|_| NaiveDateTime::parse_from_str(value, "%Y-%m-%dT%H:%M:%S"))
        .map_err(|_| "The publication date isn't a valid date and time.".to_string())?;
    let offset: i64 = tz_offset_minutes.trim().parse().unwrap_or(0).clamp(-1440, 1440);
    Ok(Some(naive.and_utc() + chrono::Duration::minutes(offset)))
}

async fn build_input(state: &AppState, form: &ArticleForm, existing: Option<&Article>) -> AppResult<Result<ArticleInput, Vec<String>>> {
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
        let base = if base.is_empty() { "article".to_string() } else { base };
        models::unique_slug(&state.db, &base, except).await?
    } else {
        let slug = slug::slugify(form.slug.trim());
        if slug.is_empty() {
            errors.push("The URL slug needs at least one letter or number.".to_string());
        } else if let Some(owner) = models::slug_owner(&state.db, &slug, except).await? {
            errors.push(format!("The URL slug “{slug}” is already used by “{owner}”. Choose another one."));
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
        Ok(None) if status == "published" => Some(existing.and_then(|a| a.published_at).unwrap_or_else(Utc::now)),
        Ok(None) => None,
        Err(e) => {
            errors.push(e);
            None
        }
    };

    let cover_image = form.cover_image.trim().to_string();
    if !(cover_image.is_empty() || cover_image.starts_with("/media/") || cover_image.starts_with("https://") || cover_image.starts_with("http://")) {
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

fn editor_context(s: &Settings, user: &AdminUser, form: &ArticleForm, article: Option<&Article>, errors: &[String], notice: Option<&str>) -> Ctx {
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
    ctx.insert("slug_prefix".into(), json!(format!("{}/posts/", s.base_url)));
    ctx
}

pub async fn new_article(State(state): State<AppState>, user: AdminUser) -> AppResult<Response> {
    let s = state.settings();
    let form = ArticleForm::default();
    Ok(render(&state, "editor", &editor_context(&s, &user, &form, None, &[], None))?.into_response())
}

pub async fn edit_article(State(state): State<AppState>, user: AdminUser, Path(id): Path<i64>, Query(q): Query<NoticeQuery>) -> AppResult<Response> {
    let s = state.settings();
    let Some(a) = models::get(&state.db, id).await? else {
        return public::not_found(State(state), "/admin/missing".parse().unwrap()).await;
    };
    let tags = models::tags_for(&state.db, &[a.id]).await?.remove(&a.id).unwrap_or_default();
    let form = ArticleForm::from_article(&a, &tags);
    let ctx = editor_context(&s, &user, &form, Some(&a), &[], notice_text(q.notice.as_deref()));
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

pub async fn create_article(State(state): State<AppState>, user: AdminUser, Form(form): Form<ArticleForm>) -> AppResult<Response> {
    if let Err(r) = user.check_csrf(&form.csrf) {
        return Ok(r);
    }
    let s = state.settings();
    let input = match build_input(&state, &form, None).await? {
        Ok(input) => input,
        Err(errors) => {
            let ctx = editor_context(&s, &user, &form, None, &errors, None);
            return Ok((StatusCode::UNPROCESSABLE_ENTITY, render(&state, "editor", &ctx)?).into_response());
        }
    };
    let id = models::create(&state.db, &input, user.id).await?;
    let notice = status_notice(None, &input);
    Ok(redirect(&s, &format!("/admin/articles/{id}/edit?notice={notice}")))
}

pub async fn update_article(State(state): State<AppState>, user: AdminUser, Path(id): Path<i64>, Form(form): Form<ArticleForm>) -> AppResult<Response> {
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
            return Ok((StatusCode::UNPROCESSABLE_ENTITY, render(&state, "editor", &ctx)?).into_response());
        }
    };
    models::update(&state.db, &existing, &input).await?;
    let notice = status_notice(Some(&existing), &input);
    Ok(redirect(&s, &format!("/admin/articles/{id}/edit?notice={notice}")))
}

pub async fn delete_article(State(state): State<AppState>, user: AdminUser, Path(id): Path<i64>, Form(form): Form<CsrfForm>) -> AppResult<Response> {
    if let Err(r) = user.check_csrf(&form.csrf) {
        return Ok(r);
    }
    models::delete(&state.db, id).await?;
    Ok(redirect(&state.settings(), "/admin/articles?notice=deleted"))
}

/// Renders unsaved editor contents with the live theme, without saving.
pub async fn preview_article(State(state): State<AppState>, user: AdminUser, Form(form): Form<ArticleForm>) -> AppResult<Response> {
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
        published_at: parse_local_datetime(&form.published_at, &form.tz_offset).ok().flatten().or(Some(now)),
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
    let mut ctx = public::article_context_with_tags(&state, &s, &s.theme, &article, &tags, true).await?;
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
pub async fn preview_markdown(State(state): State<AppState>, user: AdminUser, Form(form): Form<MarkdownForm>) -> Response {
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
        let d = parse_local_datetime("2026-05-01T12:00", "-120").unwrap().unwrap();
        assert_eq!(d.to_rfc3339(), "2026-05-01T10:00:00+00:00");
        assert_eq!(parse_local_datetime("", "0").unwrap(), None);
        assert!(parse_local_datetime("yesterday", "0").is_err());
    }
}
