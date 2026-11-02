//! Importing a DESIGN.md: paste or upload, read the diagnostics, preview it on
//! real content, then apply or cancel. Nothing is applied automatically.

use axum::extract::{Multipart, Path, Query, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Form;
use quill_design::{
    compile_bytes, CompiledDesign, Diagnostic, Level, Palette, Scheme, SourceLines,
};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::auth::AdminUser;
use crate::designs::{self, StoredDesign};
use crate::error::AppResult;
use crate::routes::public;
use crate::settings::Settings;
use crate::state::AppState;

use super::appearance::render_preview;
use super::{admin_context, notice_text, redirect, render, CsrfForm, Ctx, NoticeQuery};

/// The request limit, above the compiler's own 256 KB, so a slightly oversized
/// file gets the compiler's clear message rather than a dropped connection.
pub const MAX_UPLOAD_BYTES: usize = 2 * 1024 * 1024;

fn diagnostic_rows(source: &str, diagnostics: &[Diagnostic]) -> Vec<Value> {
    let lines = SourceLines::new(source);
    diagnostics
        .iter()
        .map(|d| {
            let excerpt = d.span.and_then(|span| lines.excerpt(span));
            json!({
                "code": d.code.as_str(),
                "level": d.level,
                "message": d.message,
                "line": d.span.map(|s| s.line),
                "column": d.span.map(|s| s.column),
                "help": d.help,
                "excerpt": excerpt.as_ref().map(|e| e.text.clone()),
                "caret": excerpt.as_ref().map(|e| {
                    format!("{}{}", " ".repeat(e.caret_offset), "^".repeat(e.caret_len))
                }),
            })
        })
        .collect()
}

fn count(diagnostics: &[Diagnostic], level: Level) -> usize {
    diagnostics.iter().filter(|d| d.level == level).count()
}

fn import_context(s: &Settings, user: &AdminUser, history: &[StoredDesign]) -> Ctx {
    let mut ctx = admin_context(s, Some(user), "appearance");
    ctx.insert(
        "history".into(),
        json!(history
            .iter()
            .map(|d| json!({
                "id": d.id,
                "name": d.name,
                "active": d.is_active,
                "applied": d.applied_at.map(public::format_date),
                "review_url": format!("{}/admin/appearance/design/{}", s.base_path, d.id),
            }))
            .collect::<Vec<_>>()),
    );
    ctx.insert(
        "max_kb".into(),
        json!(quill_design::MAX_SOURCE_BYTES / 1024),
    );
    ctx
}

pub async fn import_form(
    State(state): State<AppState>,
    user: AdminUser,
    Query(q): Query<NoticeQuery>,
) -> AppResult<Response> {
    let s = state.settings();
    let history = designs::history(&state.db).await?;
    let mut ctx = import_context(&s, &user, &history);
    ctx.insert("notice".into(), json!(notice_text(q.notice.as_deref())));
    Ok(render(&state, "design_import", &ctx)?.into_response())
}

/// Re-renders the form with what went wrong and what was submitted.
async fn import_failed(
    state: &AppState,
    user: &AdminUser,
    status: StatusCode,
    source: &str,
    diagnostics: &[Diagnostic],
    message: Option<&str>,
) -> AppResult<Response> {
    let s = state.settings();
    let history = designs::history(&state.db).await?;
    let mut ctx = import_context(&s, user, &history);
    ctx.insert("source".into(), json!(source));
    ctx.insert(
        "diagnostics".into(),
        json!(diagnostic_rows(source, diagnostics)),
    );
    ctx.insert("failed".into(), json!(true));
    ctx.insert("message".into(), json!(message));
    Ok((status, render(state, "design_import", &ctx)?).into_response())
}

pub async fn import_submit(
    State(state): State<AppState>,
    user: AdminUser,
    mut multipart: Multipart,
) -> AppResult<Response> {
    let mut csrf = String::new();
    let mut pasted = String::new();
    let mut uploaded: Option<Vec<u8>> = None;
    let too_large = "That's too large to be a DESIGN.md. The limit is 256 KB.";
    loop {
        let field = match multipart.next_field().await {
            Ok(Some(field)) => field,
            Ok(None) => break,
            Err(error) => {
                let status = error.status();
                return import_failed(&state, &user, status, "", &[], Some(too_large)).await;
            }
        };
        let name = field.name().unwrap_or_default().to_string();
        let bytes = match field.bytes().await {
            Ok(bytes) => bytes,
            Err(error) => {
                let status = error.status();
                return import_failed(&state, &user, status, "", &[], Some(too_large)).await;
            }
        };
        match name.as_str() {
            "_csrf" => csrf = String::from_utf8_lossy(&bytes).into_owned(),
            "source" => pasted = String::from_utf8_lossy(&bytes).into_owned(),
            "file" if !bytes.is_empty() => uploaded = Some(bytes.to_vec()),
            _ => {}
        }
    }
    if let Err(response) = user.check_csrf(&csrf) {
        return Ok(response);
    }

    // A chosen file wins over the text box, since choosing one is deliberate.
    let bytes = uploaded.unwrap_or_else(|| pasted.into_bytes());
    if bytes.iter().all(u8::is_ascii_whitespace) {
        let message = "Paste a DESIGN.md or choose a file first.";
        return import_failed(
            &state,
            &user,
            StatusCode::BAD_REQUEST,
            "",
            &[],
            Some(message),
        )
        .await;
    }
    // Compiling is pure CPU work, so it stays off the threads serving requests.
    let (compilation, bytes) = tokio::task::spawn_blocking(move || {
        let compilation = compile_bytes(&bytes);
        (compilation, bytes)
    })
    .await?;
    let source = String::from_utf8_lossy(&bytes).into_owned();

    let Some(design) = &compilation.design else {
        // Echoing an enormous file back into the page would help nobody.
        let echo = if source.len() <= quill_design::MAX_SOURCE_BYTES {
            source.as_str()
        } else {
            ""
        };
        return import_failed(
            &state,
            &user,
            StatusCode::UNPROCESSABLE_ENTITY,
            echo,
            &compilation.diagnostics,
            None,
        )
        .await;
    };
    let id = designs::insert_draft(&state.db, &source, design, &compilation.diagnostics).await?;
    Ok(redirect(
        &state.settings(),
        &format!("/admin/appearance/design/{id}"),
    ))
}

fn palette_rows(palette: &Palette) -> Vec<Value> {
    [
        ("Background", palette.bg),
        ("Surface", palette.surface),
        ("Text", palette.text),
        ("Muted text", palette.muted),
        ("Border", palette.border),
        ("Accent fill", palette.accent_fill),
        ("Text on accent", palette.on_accent),
        ("Links", palette.accent_text),
        ("Code background", palette.code_bg),
    ]
    .iter()
    .map(|(name, color)| json!({"name": name, "value": color.to_css()}))
    .collect()
}

pub async fn review(
    State(state): State<AppState>,
    user: AdminUser,
    Path(id): Path<i64>,
) -> AppResult<Response> {
    let s = state.settings();
    let Some(row) = designs::get(&state.db, id).await? else {
        return public::not_found(State(state), "/admin/missing".parse().unwrap()).await;
    };
    let Some(design) = row.design() else {
        let message = "This design was saved by a different version of Quill and can't be used. Import it again.";
        return import_failed(
            &state,
            &user,
            StatusCode::CONFLICT,
            &row.source,
            &[],
            Some(message),
        )
        .await;
    };
    let diagnostics = row.diagnostics();
    let mut ctx = admin_context(&s, Some(&user), "appearance");
    ctx.insert(
        "design".into(),
        json!({
            "id": row.id,
            "name": row.name,
            "active": row.is_active,
            "applied": row.applied_at.is_some(),
            "fonts": font_summary(&design),
            "light": palette_rows(&design.tokens.light),
            "dark": palette_rows(&design.tokens.dark),
        }),
    );
    ctx.insert(
        "diagnostics".into(),
        json!(diagnostic_rows(&row.source, &diagnostics)),
    );
    ctx.insert(
        "warnings".into(),
        json!(count(&diagnostics, Level::Warning)),
    );
    ctx.insert("notes".into(), json!(count(&diagnostics, Level::Note)));
    let root = format!("{}/admin/appearance/design/{}", s.base_path, row.id);
    ctx.insert("preview_url".into(), json!(format!("{root}/preview")));
    ctx.insert("apply_url".into(), json!(format!("{root}/apply")));
    ctx.insert("cancel_url".into(), json!(format!("{root}/cancel")));
    Ok(render(&state, "design_review", &ctx)?.into_response())
}

fn font_summary(design: &CompiledDesign) -> Value {
    json!({
        "body": design.tokens.font_body.to_css(),
        "heading": design.tokens.font_heading.to_css(),
        "mono": design.tokens.font_mono.to_css(),
    })
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct PreviewQuery {
    page: String,
    scheme: String,
}

fn forced_scheme(name: &str) -> Option<Scheme> {
    match name {
        "light" => Some(Scheme::Light),
        "dark" => Some(Scheme::Dark),
        _ => None,
    }
}

/// A real page, on the current layout, wearing a design that isn't live yet.
pub async fn preview(
    State(state): State<AppState>,
    _user: AdminUser,
    Path(id): Path<i64>,
    Query(q): Query<PreviewQuery>,
) -> AppResult<Response> {
    let s = state.settings();
    if designs::get(&state.db, id).await?.is_none() {
        return Ok((StatusCode::NOT_FOUND, "That design no longer exists.").into_response());
    }
    let scheme = forced_scheme(&q.scheme).map_or("auto", |_| q.scheme.as_str());
    let tokens_url = format!(
        "{}/admin/appearance/design/{id}/tokens.css?scheme={scheme}",
        s.base_path
    );
    let label = "Previewing this design on your current layout. It isn't live.";
    render_preview(&state, &s, &state.look(), &q.page, label, Some(tokens_url)).await
}

pub async fn preview_tokens(
    State(state): State<AppState>,
    _user: AdminUser,
    Path(id): Path<i64>,
    Query(q): Query<PreviewQuery>,
) -> AppResult<Response> {
    let base_path = state.settings().base_path;
    let design = designs::get(&state.db, id)
        .await?
        .and_then(|row| row.design());
    let Some(design) = design else {
        return Ok((StatusCode::NOT_FOUND, "Not found").into_response());
    };
    let css = match forced_scheme(&q.scheme) {
        Some(scheme) => design.to_css_forced(&base_path, scheme),
        None => design.to_css(&base_path),
    };
    Ok(([(header::CONTENT_TYPE, "text/css; charset=utf-8")], css).into_response())
}

pub async fn apply(
    State(state): State<AppState>,
    user: AdminUser,
    Path(id): Path<i64>,
    Form(form): Form<CsrfForm>,
) -> AppResult<Response> {
    if let Err(response) = user.check_csrf(&form.csrf) {
        return Ok(response);
    }
    let design = designs::get(&state.db, id)
        .await?
        .and_then(|row| row.design());
    let Some(design) = design else {
        return Ok((StatusCode::NOT_FOUND, "That design can't be used.").into_response());
    };
    state.apply_design(id, design).await?;
    Ok(redirect(
        &state.settings(),
        "/admin/appearance?notice=design",
    ))
}

pub async fn cancel(
    State(state): State<AppState>,
    user: AdminUser,
    Path(id): Path<i64>,
    Form(form): Form<CsrfForm>,
) -> AppResult<Response> {
    if let Err(response) = user.check_csrf(&form.csrf) {
        return Ok(response);
    }
    designs::delete_draft(&state.db, id).await?;
    Ok(redirect(&state.settings(), "/admin/appearance/design"))
}

/// Back to the preset's own colors and fonts, keeping the layout as it is.
pub async fn clear(
    State(state): State<AppState>,
    user: AdminUser,
    Form(form): Form<CsrfForm>,
) -> AppResult<Response> {
    if let Err(response) = user.check_csrf(&form.csrf) {
        return Ok(response);
    }
    state.clear_design().await?;
    Ok(redirect(
        &state.settings(),
        "/admin/appearance?notice=design_removed",
    ))
}
