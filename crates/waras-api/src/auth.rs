//! Auth: extractor `AuthUser` (Bearer token -> `User`), extractor
//! `JsonBody` (body JSON dengan error berbentuk JSON), dan guard RBAC.

use std::net::SocketAddr;

use axum::{
    body::Bytes,
    extract::{ConnectInfo, FromRequest, FromRequestParts, Request},
    http::{header, request::Parts},
};
use serde::de::DeserializeOwned;
use waras_core::{rbac::Action, types::User};

use crate::{error::ApiError, AppState};

/// IP peer koneksi TCP, untuk kunci brute-force (`ip:pengguna`).
/// Extractor yang TIDAK PERNAH menolak: bila `ConnectInfo` tidak
/// tersedia (mis. test oneshot tanpa `into_make_service_with_
/// connect_info`), IP-nya `"unknown"` supaya kunci tetap terbentuk.
/// (axum 0.8 tidak punya extractor `Option<ConnectInfo>`.)
///
/// SENGAJA tidak memakai `X-Forwarded-For`: header itu bisa dipalsukan
/// pemanggil, dan pemalsuan akan membuat tiap percobaan memakai kunci
/// baru (lockout tidak pernah tercapai). Di balik nginx, peer-nya
/// adalah nginx (127.0.0.1) untuk semua — kunci tetap membedakan
/// pengguna, jadi lockout per pengguna tetap jalan; lihat FASE-A-A4.md.
pub struct ClientIp(pub String);

impl FromRequestParts<AppState> for ClientIp {
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(
        parts: &mut Parts,
        _state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let ip = parts
            .extensions
            .get::<ConnectInfo<SocketAddr>>()
            .map(|ci| ci.0.ip().to_string())
            .unwrap_or_else(|| "unknown".to_string());
        Ok(ClientIp(ip))
    }
}

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
