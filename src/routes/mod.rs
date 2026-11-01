pub mod admin;
pub mod public;

use axum::body::Body;
use axum::extract::{DefaultBodyLimit, Path, Request, State};
use axum::http::{header, HeaderMap, HeaderValue, Method, StatusCode, Uri};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::Router;
use tower::Layer;
use tower_http::compression::CompressionLayer;
use tower_http::set_header::SetResponseHeaderLayer;
use tower_http::trace::TraceLayer;

use crate::render::StaticFiles;
use crate::state::AppState;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const MAX_UPLOAD_BYTES: usize = 10 * 1024 * 1024;

const CSP: &str = "default-src 'self'; img-src 'self' https: data:; media-src 'self' https:; \
    style-src 'self'; style-src-attr 'unsafe-inline'; script-src 'self'; object-src 'none'; \
    frame-ancestors 'self'; base-uri 'self'; form-action 'self'";

/// The request path as the browser sent it, before the base path was removed.
#[derive(Clone, Debug)]
pub struct OriginalPath(pub String);

pub fn app(state: AppState) -> Router {
    let admin = Router::new()
        .route("/admin", get(admin::dashboard::dashboard))
        .route(
            "/admin/setup",
            get(admin::setup::setup_form).post(admin::setup::setup_submit),
        )
        .route(
            "/admin/login",
            get(admin::login::login_form).post(admin::login::login_submit),
        )
        .route("/admin/logout", post(admin::login::logout))
        .route(
            "/admin/articles",
            get(admin::articles::articles).post(admin::articles::create_article),
        )
        .route("/admin/articles/new", get(admin::articles::new_article))
        .route(
            "/admin/articles/preview",
            post(admin::articles::preview_article),
        )
        .route(
            "/admin/articles/{id}",
            post(admin::articles::update_article),
        )
        .route(
            "/admin/articles/{id}/edit",
            get(admin::articles::edit_article),
        )
        .route(
            "/admin/articles/{id}/delete",
            post(admin::articles::delete_article),
        )
        .route("/admin/preview", post(admin::articles::preview_markdown))
        .route(
            "/admin/themes",
            get(admin::appearance::themes).post(admin::appearance::activate_theme),
        )
        .route(
            "/admin/themes/{id}/preview",
            get(admin::appearance::preview_theme),
        )
        .route(
            "/admin/settings",
            get(admin::settings::settings_form).post(admin::settings::settings_submit),
        )
        .route("/admin/integration", get(admin::integration::integration))
        .route(
            "/admin/account",
            get(admin::account::account_form).post(admin::account::account_submit),
        )
        .route(
            "/admin/media",
            get(admin::media::media)
                .post(admin::media::upload_media)
                .layer(DefaultBodyLimit::max(MAX_UPLOAD_BYTES + 64 * 1024)),
        )
        .route("/admin/media/{id}/delete", post(admin::media::delete_media))
        .layer(SetResponseHeaderLayer::overriding(
            header::CACHE_CONTROL,
            HeaderValue::from_static("no-store"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            header::HeaderName::from_static("x-robots-tag"),
            HeaderValue::from_static("noindex, nofollow"),
        ));

    let inner = Router::new()
        .route("/", get(public::home))
        .route("/page/{n}", get(public::home_page))
        .route("/posts/{slug}", get(public::article))
        .route("/tags/{slug}", get(public::tag))
        .route("/archive", get(public::archive))
        .route("/feed.xml", get(public::feed))
        .route("/sitemap.xml", get(public::sitemap))
        .route("/robots.txt", get(public::robots))
        .route("/media/{name}", get(public::media))
        .route("/static/{*path}", get(static_file))
        .route("/healthz", get(|| async { "ok" }))
        .merge(admin)
        .fallback(public::not_found)
        .layer(middleware::from_fn(check_origin))
        .with_state(state.clone());

    // Rewriting the path has to happen before routing, so the base-path
    // middleware wraps the finished router rather than being a route layer.
    let routed = middleware::from_fn_with_state(state, strip_base_path).layer(inner);

    Router::new()
        .fallback_service(routed)
        .layer(SetResponseHeaderLayer::if_not_present(
            header::X_CONTENT_TYPE_OPTIONS,
            HeaderValue::from_static("nosniff"),
        ))
        .layer(SetResponseHeaderLayer::if_not_present(
            header::REFERRER_POLICY,
            HeaderValue::from_static("strict-origin-when-cross-origin"),
        ))
        .layer(SetResponseHeaderLayer::if_not_present(
            header::X_FRAME_OPTIONS,
            HeaderValue::from_static("SAMEORIGIN"),
        ))
        .layer(SetResponseHeaderLayer::if_not_present(
            header::CONTENT_SECURITY_POLICY,
            HeaderValue::from_static(CSP),
        ))
        .layer(CompressionLayer::new())
        .layer(TraceLayer::new_for_http())
}

/// Serves the blog under its configured base path (e.g. `/blog`). Requests
/// whose prefix was already removed by a reverse proxy are accepted as-is, so
/// both `proxy_pass` styles work.
async fn strip_base_path(State(state): State<AppState>, mut req: Request, next: Next) -> Response {
    let base = state.settings().base_path;
    let path = req.uri().path().to_string();
    req.extensions_mut().insert(OriginalPath(path.clone()));
    if !base.is_empty() {
        let stripped = if path == base {
            Some("/".to_string())
        } else {
            path.strip_prefix(&base)
                .filter(|rest| rest.starts_with('/'))
                .map(str::to_string)
        };
        if let Some(stripped) = stripped {
            let pq = match req.uri().query() {
                Some(q) => format!("{stripped}?{q}"),
                None => stripped,
            };
            let mut parts = req.uri().clone().into_parts();
            parts.path_and_query = pq.parse().ok();
            if let Ok(uri) = Uri::from_parts(parts) {
                *req.uri_mut() = uri;
            }
        }
    }
    next.run(req).await
}

/// Rejects state-changing requests sent by other websites. Authenticated forms
/// also carry a CSRF token; this covers the login and setup forms too.
async fn check_origin(req: Request, next: Next) -> Response {
    if matches!(*req.method(), Method::GET | Method::HEAD | Method::OPTIONS) {
        return next.run(req).await;
    }
    if let Some(origin) = req
        .headers()
        .get(header::ORIGIN)
        .and_then(|v| v.to_str().ok())
    {
        let origin_host = origin.split_once("://").map(|(_, h)| h).unwrap_or("");
        let host = request_host(req.headers());
        if origin == "null" || host.is_none_or(|h| !h.eq_ignore_ascii_case(origin_host)) {
            return (StatusCode::FORBIDDEN, "Cross-site request blocked.").into_response();
        }
    }
    next.run(req).await
}

/// The host the browser used, preferring what a reverse proxy reports.
pub fn request_host(headers: &HeaderMap) -> Option<&str> {
    headers
        .get("x-forwarded-host")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split(',').next())
        .map(str::trim)
        .or_else(|| headers.get(header::HOST).and_then(|v| v.to_str().ok()))
}

/// `scheme://host` as seen by the browser.
pub fn request_origin(headers: &HeaderMap) -> Option<String> {
    let scheme = if crate::auth::is_https(headers) {
        "https"
    } else {
        "http"
    };
    request_host(headers).map(|h| format!("{scheme}://{}", h.to_ascii_lowercase()))
}

async fn static_file(Path(path): Path<String>) -> Response {
    match StaticFiles::get(&path) {
        Some(file) => {
            let mime = mime_guess::from_path(&path).first_or_octet_stream();
            (
                [
                    (header::CONTENT_TYPE, mime.as_ref().to_string()),
                    // Asset URLs carry ?v=<version>, so they can be cached for a day safely.
                    (header::CACHE_CONTROL, "public, max-age=86400".to_string()),
                ],
                Body::from(file.data.into_owned()),
            )
                .into_response()
        }
        None => (StatusCode::NOT_FOUND, "Not found").into_response(),
    }
}
