//! The media library: listing, uploading and deleting images.

use axum::extract::{Multipart, Path, Query, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::{Form, Json};
use chrono::Utc;
use serde_json::json;

use crate::auth::{self, AdminUser};
use crate::error::AppResult;
use crate::markdown;
use crate::models;
use crate::routes::public::{self, valid_media_name};
use crate::routes::MAX_UPLOAD_BYTES;
use crate::state::AppState;

use super::{admin_context, notice_text, redirect, render, CsrfForm, NoticeQuery};

/// Identifies an image by its leading bytes; the uploaded filename and declared
/// type are not trusted. SVG is refused because it can carry scripts.
pub fn detect_image(bytes: &[u8]) -> Option<(&'static str, &'static str)> {
    match bytes {
        [0x89, b'P', b'N', b'G', ..] => Some(("png", "image/png")),
        [0xFF, 0xD8, 0xFF, ..] => Some(("jpg", "image/jpeg")),
        [b'G', b'I', b'F', b'8', ..] => Some(("gif", "image/gif")),
        [b'R', b'I', b'F', b'F', _, _, _, _, b'W', b'E', b'B', b'P', ..] => {
            Some(("webp", "image/webp"))
        }
        [_, _, _, _, b'f', b't', b'y', b'p', b'a', b'v', b'i', b'f', ..] => {
            Some(("avif", "image/avif"))
        }
        _ => None,
    }
}

pub async fn media(
    State(state): State<AppState>,
    user: AdminUser,
    Query(q): Query<NoticeQuery>,
) -> AppResult<Response> {
    let s = state.settings();
    let files = models::list_media(&state.db).await?;
    let mut used = std::collections::HashSet::new();
    for a in models::all_articles(&state.db).await? {
        used.extend(markdown::media_references(&a.content));
        used.extend(markdown::media_references(&a.cover_image));
    }
    let mut ctx = admin_context(&s, Some(&user), "media");
    ctx.insert(
        "files".into(),
        json!(files
            .iter()
            .map(|m| json!({
                "id": m.id,
                "url": format!("{}/media/{}", s.base_path, m.filename),
                "reference": format!("/media/{}", m.filename),
                "name": m.original_name,
                "size": m.size,
                "date": public::format_date(m.created_at),
                "in_use": used.contains(&m.filename),
            }))
            .collect::<Vec<_>>()),
    );
    ctx.insert("notice".into(), json!(notice_text(q.notice.as_deref())));
    ctx.insert("max_mb".into(), json!(MAX_UPLOAD_BYTES >> 20));
    Ok(render(&state, "media", &ctx)?.into_response())
}

fn upload_error(wants_json: bool, status: StatusCode, msg: &str) -> Response {
    if wants_json {
        (status, Json(json!({"error": msg}))).into_response()
    } else {
        (status, msg.to_string()).into_response()
    }
}

pub async fn upload_media(
    State(state): State<AppState>,
    user: AdminUser,
    headers: HeaderMap,
    mut multipart: Multipart,
) -> AppResult<Response> {
    let s = state.settings();
    let wants_json = headers
        .get(header::ACCEPT)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.contains("application/json"));
    let mut csrf = headers
        .get("x-csrf-token")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_string();
    let mut file: Option<(String, axum::body::Bytes)> = None;
    loop {
        let field = match multipart.next_field().await {
            Ok(Some(f)) => f,
            Ok(None) => break,
            Err(e) => {
                let msg = if e.status() == StatusCode::PAYLOAD_TOO_LARGE {
                    format!(
                        "That file is too large. The limit is {} MB.",
                        MAX_UPLOAD_BYTES >> 20
                    )
                } else {
                    "The upload was interrupted. Please try again.".to_string()
                };
                return Ok(upload_error(wants_json, e.status(), &msg));
            }
        };
        match field.name() {
            Some("_csrf") => csrf = field.text().await.unwrap_or_default(),
            Some("file") => {
                let name = field.file_name().unwrap_or("upload").to_string();
                match field.bytes().await {
                    Ok(bytes) => file = Some((name, bytes)),
                    Err(e) => {
                        let msg = format!(
                            "That file is too large or the upload was interrupted. The limit is {} MB.",
                            MAX_UPLOAD_BYTES >> 20
                        );
                        return Ok(upload_error(wants_json, e.status(), &msg));
                    }
                }
            }
            _ => {}
        }
    }
    if let Err(r) = user.check_csrf(&csrf) {
        return Ok(r);
    }
    let Some((original_name, bytes)) = file.filter(|(_, b)| !b.is_empty()) else {
        return Ok(upload_error(
            wants_json,
            StatusCode::BAD_REQUEST,
            "Choose a file to upload.",
        ));
    };
    if bytes.len() > MAX_UPLOAD_BYTES {
        return Ok(upload_error(
            wants_json,
            StatusCode::PAYLOAD_TOO_LARGE,
            &format!(
                "That file is too large. The limit is {} MB.",
                MAX_UPLOAD_BYTES >> 20
            ),
        ));
    }
    let Some((ext, mime)) = detect_image(&bytes) else {
        return Ok(upload_error(
            wants_json,
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "Only PNG, JPEG, GIF, WebP and AVIF images can be uploaded.",
        ));
    };
    let filename = format!("{}.{ext}", auth::random_hex(12));
    tokio::fs::create_dir_all(state.media_dir()).await?;
    tokio::fs::write(state.media_dir().join(&filename), &bytes).await?;
    let original_name: String = original_name
        .chars()
        .filter(|c| !c.is_control())
        .take(200)
        .collect();
    sqlx::query("INSERT INTO media (filename, original_name, content_type, size, created_at) VALUES (?, ?, ?, ?, ?)")
        .bind(&filename)
        .bind(&original_name)
        .bind(mime)
        .bind(bytes.len() as i64)
        .bind(Utc::now())
        .execute(&state.db)
        .await?;
    let reference = format!("/media/{filename}");
    if wants_json {
        Ok(Json(json!({"reference": reference, "url": format!("{}{reference}", s.base_path), "name": original_name})).into_response())
    } else {
        Ok(redirect(&s, "/admin/media?notice=uploaded"))
    }
}

pub async fn delete_media(
    State(state): State<AppState>,
    user: AdminUser,
    Path(id): Path<i64>,
    Form(form): Form<CsrfForm>,
) -> AppResult<Response> {
    if let Err(r) = user.check_csrf(&form.csrf) {
        return Ok(r);
    }
    let filename: Option<String> = sqlx::query_scalar("SELECT filename FROM media WHERE id = ?")
        .bind(id)
        .fetch_optional(&state.db)
        .await?;
    if let Some(name) = filename.filter(|n| valid_media_name(n)) {
        match tokio::fs::remove_file(state.media_dir().join(&name)).await {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
        sqlx::query("DELETE FROM media WHERE id = ?")
            .bind(id)
            .execute(&state.db)
            .await?;
    }
    Ok(redirect(
        &state.settings(),
        "/admin/media?notice=media_deleted",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_images_by_content() {
        assert_eq!(detect_image(b"\x89PNG\r\n\x1a\n...").unwrap().0, "png");
        assert_eq!(detect_image(b"RIFF\0\0\0\0WEBPVP8 ").unwrap().0, "webp");
        assert!(detect_image(b"<svg xmlns=").is_none());
        assert!(detect_image(b"GIF").is_none());
    }
}
