use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde_json::json;

/// A handler failure. `Status` is an answer the client is meant to see; `Internal`
/// is logged here and reaches the client as a bare 500, so a database message can
/// never leak into a response.
#[derive(Debug)]
pub enum ApiError {
    Status(StatusCode, &'static str),
    Internal(anyhow::Error),
}

pub type ApiResult<T> = Result<T, ApiError>;

impl ApiError {
    pub fn unauthorized() -> Self {
        Self::Status(StatusCode::UNAUTHORIZED, "Unauthorized")
    }

    pub fn forbidden() -> Self {
        Self::Status(StatusCode::FORBIDDEN, "Forbidden")
    }

    pub fn bad_request() -> Self {
        Self::Status(StatusCode::BAD_REQUEST, "Bad request")
    }

    pub fn not_found() -> Self {
        Self::Status(StatusCode::NOT_FOUND, "Not found")
    }
}

impl<E: Into<anyhow::Error>> From<E> for ApiError {
    fn from(err: E) -> Self {
        Self::Internal(err.into())
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        match self {
            Self::Status(status, message) => {
                (status, Json(json!({ "error": message }))).into_response()
            }
            Self::Internal(err) => {
                tracing::error!("request failed: {err:#}");
                let body = Json(json!({ "error": "Internal Server Error" }));
                (StatusCode::INTERNAL_SERVER_ERROR, body).into_response()
            }
        }
    }
}
