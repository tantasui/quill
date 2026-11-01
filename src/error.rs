use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Response};

/// An unexpected failure. The details go to the server log; visitors get a
/// short explanation.
#[derive(Debug)]
pub struct AppError(anyhow::Error);

pub type AppResult<T> = Result<T, AppError>;

impl<E: Into<anyhow::Error>> From<E> for AppError {
    fn from(e: E) -> Self {
        AppError(e.into())
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        tracing::error!("request failed: {:#}", self.0);
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Html(
                "<!doctype html><meta charset=utf-8><title>Something went wrong</title>\
                 <body style=\"font:16px/1.5 system-ui,sans-serif;max-width:36rem;margin:4rem auto;padding:0 1rem\">\
                 <h1>Something went wrong</h1>\
                 <p>The server hit an unexpected error while handling this page. Nothing you entered was lost \
                 unless it was mid-save. Try again in a moment; if it keeps happening, the server log has the details.</p>",
            ),
        )
            .into_response()
    }
}
