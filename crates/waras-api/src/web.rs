//! Penyajian UI statis dari `waras-web` (rust-embed) + fallback router.
//!
//! Halaman: `/` -> Login, `/dashboard`, `/registrasi`, `/rme`, `/farmasi`,
//! `/billing`. Aset dilayani apa adanya dari embed (`/css/*`, `/js/*`,
//! `/icons/*`, `/assets/*`, `/waras-tile-sharp.png`) dengan MIME dari
//! `waras_web::mime_for`. Fallback: path `/api/*` yang tidak dikenal ->
//! JSON 404; path lain -> redirect ke `/` (deep link yang salah mendarat
//! di Login, yang kemudian menjaga lewat guard 401).

use axum::{
    body::Body,
    extract::Path,
    http::{header, Uri},
    response::{IntoResponse, Redirect, Response},
};

use crate::error::ApiError;

fn serve_asset(path: &str) -> Response {
    match waras_web::get(path) {
        Some(file) => Response::builder()
            .status(200)
            .header(header::CONTENT_TYPE, waras_web::mime_for(path))
            .body(Body::from(file.data.into_owned()))
            .unwrap_or_else(|_| {
                ApiError::internal("gagal menyusun respons aset").into_response()
            }),
        None => ApiError::not_found(format!("aset {path} tidak ditemukan")).into_response(),
    }
}

pub async fn serve_index() -> Response {
    serve_asset("index.html")
}

pub async fn serve_dashboard() -> Response {
    serve_asset("dashboard.html")
}

pub async fn serve_registrasi() -> Response {
    serve_asset("registrasi.html")
}

pub async fn serve_rme() -> Response {
    serve_asset("rme.html")
}

pub async fn serve_farmasi() -> Response {
    serve_asset("farmasi.html")
}

pub async fn serve_billing() -> Response {
    serve_asset("billing.html")
}

pub async fn serve_tile() -> Response {
    serve_asset("waras-tile-sharp.png")
}

/// `/assets/<sisa>` — sisa path dipakai apa adanya di dalam embed.
pub async fn serve_assets(Path(path): Path<String>) -> Response {
    serve_asset(&path)
}

/// `/css/<file>` -> `css/<file>` di dalam embed.
pub async fn serve_css(Path(path): Path<String>) -> Response {
    serve_asset(&format!("css/{path}"))
}

/// `/js/<file>` -> `js/<file>` di dalam embed.
pub async fn serve_js(Path(path): Path<String>) -> Response {
    serve_asset(&format!("js/{path}"))
}

/// `/icons/<file>` -> `icons/<file>` di dalam embed.
pub async fn serve_icons(Path(path): Path<String>) -> Response {
    serve_asset(&format!("icons/{path}"))
}

pub async fn fallback(uri: Uri) -> Response {
    let path = uri.path();
    if path.starts_with("/api/") {
        ApiError::not_found(format!("endpoint {path} tidak ditemukan")).into_response()
    } else {
        Redirect::to("/").into_response()
    }
}
