//! Pemetaan error ke respons JSON `{ "error": { "code", "message" } }`
//! (KONTRAK §3) dengan status HTTP dari `WarasError::http_status`.

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;
use waras_core::WarasError;

/// Error API: pembungkus `WarasError` yang bisa jadi respons HTTP.
#[derive(Debug)]
pub struct ApiError(pub WarasError);

impl ApiError {
    pub fn validation(msg: impl Into<String>) -> Self {
        Self(WarasError::Validation(msg.into()))
    }

    pub fn not_found(msg: impl Into<String>) -> Self {
        Self(WarasError::NotFound(msg.into()))
    }

    pub fn unauthorized() -> Self {
        Self(WarasError::Unauthorized)
    }

    pub fn forbidden(action: &str) -> Self {
        Self(WarasError::Forbidden(format!(
            "anda tidak diizinkan untuk {action}"
        )))
    }

    pub fn internal(msg: impl Into<String>) -> Self {
        Self(WarasError::Internal(msg.into()))
    }

    /// Kode error untuk JSON (sesuai contoh kontrak: `NOT_FOUND`, dst.).
    pub fn code(&self) -> &'static str {
        match &self.0 {
            WarasError::NotFound(_) => "NOT_FOUND",
            WarasError::Validation(_) => "VALIDATION",
            WarasError::Blocked(_) => "BLOCKED",
            WarasError::WrongPatient(_) => "WRONG_PATIENT",
            WarasError::Unprocessable(_) => "UNPROCESSABLE",
            WarasError::Unauthorized => "UNAUTHORIZED",
            WarasError::Forbidden(_) => "FORBIDDEN",
            WarasError::Conflict(_) => "CONFLICT",
            WarasError::Database(_) | WarasError::Internal(_) => "INTERNAL_ERROR",
        }
    }
}

impl From<WarasError> for ApiError {
    fn from(e: WarasError) -> Self {
        Self(e)
    }
}

impl From<serde_json::Error> for ApiError {
    fn from(e: serde_json::Error) -> Self {
        Self::internal(format!("gagal menyusun JSON: {e}"))
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status = StatusCode::from_u16(self.0.http_status())
            .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        let body = Json(json!({
            "error": { "code": self.code(), "message": self.0.to_string() }
        }));
        (status, body).into_response()
    }
}
