//! Auth: extractor `AuthUser` (Bearer token -> `User`), extractor
//! `JsonBody` (body JSON dengan error berbentuk JSON), dan guard RBAC.

use axum::{
    body::Bytes,
    extract::{FromRequest, FromRequestParts, Request},
    http::{header, request::Parts},
};
use serde::de::DeserializeOwned;
use waras_core::{rbac::Action, types::User};

use crate::{error::ApiError, AppState};

/// Pengguna terautentikasi dari header `Authorization: Bearer <token>`.
/// Token dicari lewat `Db::find_user_by_token` (menolak token yang
/// kedaluwarsa atau tidak ada dengan 401).
pub struct AuthUser(pub User);

impl FromRequestParts<AppState> for AuthUser {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let token = parts
            .headers
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .ok_or_else(ApiError::unauthorized)?;
        let user = state
            .db
            .find_user_by_token(token)
            .await
            .map_err(ApiError::from)?;
        match user {
            Some(u) if u.active => Ok(AuthUser(u)),
            _ => Err(ApiError::unauthorized()),
        }
    }
}

/// Body JSON; kegagalan parse menghasilkan 400 JSON (bukan teks polos
/// bawaan Axum) supaya bentuk error konsisten di semua endpoint.
pub struct JsonBody<T>(pub T);

impl<T, S> FromRequest<S> for JsonBody<T>
where
    T: DeserializeOwned,
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        let bytes = Bytes::from_request(req, state)
            .await
            .map_err(|_| ApiError::validation("body permintaan tidak bisa dibaca"))?;
        serde_json::from_slice::<T>(&bytes)
            .map(JsonBody)
            .map_err(|e| ApiError::validation(format!("body JSON tidak valid: {e}")))
    }
}

/// Guard RBAC: tolak dengan 403 bila peran pengguna tidak boleh
/// melakukan aksi ini (matriks di `waras_core::rbac::can`).
pub fn require(user: &User, action: Action) -> Result<(), ApiError> {
    if waras_core::rbac::can(user.role, action) {
        Ok(())
    } else {
        Err(ApiError::forbidden(action.label()))
    }
}
