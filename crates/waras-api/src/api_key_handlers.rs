//! Manajemen kunci API publik oleh admin (FASE4.md §5.2).
//!
//! - `GET /api/api-keys` — daftar kunci (TANPA hash; `ApiKey`
//!   tidak menyerialisasi `key_hash`).
//! - `POST /api/api-keys` — buat kunci; kunci penuh (`wrs_` + 32 hex)
//!   dikembalikan HANYA SEKALI di respons ini, tidak pernah disimpan
//!   dan tidak pernah dikembalikan lagi.
//! - `POST /api/api-keys/{id}/revoke` — cabut kunci.
//! Semua endpoint butuh aksi RBAC `ManageApiKeys` (hanya Admin, §3).

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use rand::random;
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use waras_core::rbac::Action;

use crate::{
    auth::{require, AuthUser, JsonBody},
    error::ApiError,
    AppState,
};

type ApiResult<T> = Result<T, ApiError>;

/// Scope yang diizinkan (FASE4.md §5.2). Scope di luar daftar -> 400.
pub const ALLOWED_SCOPES: [&str; 4] = [
    "patients:read",
    "queue:read",
    "encounters:read",
    "fhir:read",
];

/// Audit best-effort (pola `handlers.rs`): kegagalan mencatat audit
/// tidak boleh menggagalkan aksi yang sudah berhasil.
async fn audit(
    db: &waras_db::Db,
    actor: &str,
    action: &str,
    resource_type: &str,
    resource_id: &str,
    details: Value,
) {
    let _ = db
        .log_audit(actor, action, resource_type, resource_id, details)
        .await;
}

/// Kunci baru: `wrs_` + 32 karakter heks acak (16 byte dari RNG
/// kriptografis, total 36 karakter).
pub fn generate_api_key() -> String {
    let bytes: [u8; 16] = random();
    format!("wrs_{}", hex_encode(bytes))
}

/// SHA-256 hex dari kunci penuh (yang disimpan di `api_keys.key_hash`).
pub fn sha256_hex_str(s: &str) -> String {
    hex_encode(Sha256::digest(s.as_bytes()))
}

fn hex_encode(bytes: impl AsRef<[u8]>) -> String {
    bytes
        .as_ref()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// 12 karakter pertama kunci (yang disimpan di `api_keys.key_prefix`).
pub fn key_prefix(key: &str) -> &str {
    &key[..12.min(key.len())]
}

/// Validasi daftar scope: tidak boleh kosong, tiap scope harus ada di
/// `ALLOWED_SCOPES` (duplikat dihapus pemanggil bila perlu).
pub fn validate_scopes(scopes: &[String]) -> ApiResult<()> {
    if scopes.is_empty() {
        return Err(ApiError::validation("scopes wajib diisi"));
    }
    for s in scopes {
        if !ALLOWED_SCOPES.contains(&s.as_str()) {
            return Err(ApiError::validation(format!(
                "scope tidak dikenal: {s}"
            )));
        }
    }
    Ok(())
}

#[derive(Deserialize)]
pub struct CreateApiKeyDto {
    pub name: String,
    pub scopes: Vec<String>,
}

pub async fn list_api_keys(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
) -> ApiResult<Json<Vec<waras_core::types::ApiKey>>> {
    require(&user, Action::ManageApiKeys)?;
    Ok(Json(state.db.list_api_keys().await?))
}

pub async fn create_api_key(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    JsonBody(body): JsonBody<CreateApiKeyDto>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    require(&user, Action::ManageApiKeys)?;
    if body.name.trim().is_empty() {
        return Err(ApiError::validation("nama kunci wajib diisi"));
    }
    validate_scopes(&body.scopes)?;
    // Normalisasi: trim, buang duplikat, simpan dipisah koma.
    let mut scopes: Vec<String> = Vec::new();
    for s in &body.scopes {
        let s = s.trim().to_string();
        if !s.is_empty() && !scopes.contains(&s) {
            scopes.push(s);
        }
    }
    let key = generate_api_key();
    let record = state
        .db
        .create_api_key(
            body.name.trim(),
            key_prefix(&key),
            &sha256_hex_str(&key),
            &scopes.join(","),
            &user.name,
        )
        .await?;
    audit(
        &state.db,
        &user.name,
        "api_key.created",
        "api_key",
        &record.id,
        json!({"name": record.name, "key_prefix": record.key_prefix, "scopes": scopes}),
    )
    .await;
    // Kunci penuh HANYA di sini. Jangan pernah log `key` di mana pun.
    Ok((
        StatusCode::CREATED,
        Json(json!({ "key": key, "api_key": record })),
    ))
}

pub async fn revoke_api_key(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(id): Path<String>,
) -> ApiResult<Json<waras_core::types::ApiKey>> {
    require(&user, Action::ManageApiKeys)?;
    let record = state.db.revoke_api_key(&id).await?;
    audit(
        &state.db,
        &user.name,
        "api_key.revoked",
        "api_key",
        &record.id,
        json!({"name": record.name, "key_prefix": record.key_prefix}),
    )
    .await;
    Ok(Json(record))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    use axum::{
        body::Body,
        http::{Request, StatusCode},
        routing::{get, post},
        Router,
    };
    use serde_json::Value;
    use tower::ServiceExt;
    use waras_bridge::{build_registry, BpjsConfig, BridgeConfig, SatusehatConfig};
    use waras_db::Db;
    #[test]
    fn kunci_baru_format_wrs_plus_32_hex() {
        for _ in 0..100 {
            let k = generate_api_key();
            assert_eq!(k.len(), 36, "wrs_ (4) + 32 hex = 36, dapat {k}");
            assert!(k.starts_with("wrs_"), "harus berawalan wrs_: {k}");
            assert!(
                k[4..].chars().all(|c| c.is_ascii_hexdigit()),
                "32 karakter setelah prefix harus hex: {k}"
            );
            assert_eq!(key_prefix(&k).len(), 12);
            assert!(k.starts_with(key_prefix(&k)));
        }
        // Dua kunci tidak boleh sama.
        assert_ne!(generate_api_key(), generate_api_key());
    }

    #[test]
    fn hash_verify_round_trip_dan_vektor_sha256() {
        // Vektor SHA-256 yang dikenal: "abc".
        assert_eq!(
            sha256_hex_str("abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        let key = generate_api_key();
        let hash = sha256_hex_str(&key);
        assert_eq!(hash.len(), 64);
        assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
        // Verifikasi: hash(kunci yang sama) == hash tersimpan; kunci
        // lain tidak cocok.
        assert_eq!(sha256_hex_str(&key), hash);
        assert_ne!(sha256_hex_str(&generate_api_key()), hash);
    }

    #[test]
    fn scope_ditolak_bila_tidak_dikenal_atau_kosong() {
        assert!(validate_scopes(&["patients:read".to_string()]).is_ok());
        assert!(validate_scopes(
            &["patients:read".to_string(), "fhir:read".to_string()]
        )
        .is_ok());
        assert!(validate_scopes(&[]).is_err());
        assert!(validate_scopes(&["patients:write".to_string()]).is_err());
        assert!(validate_scopes(&["patients:read".to_string(), "admin".to_string()]).is_err());
        assert!(validate_scopes(&["".to_string()]).is_err());
    }

    fn disabled_config() -> BridgeConfig {
        BridgeConfig {
            satusehat: SatusehatConfig {
                enabled: false,
                base_url: "http://127.0.0.1:9/fhir-r4/v1".into(),
                auth_url: "http://127.0.0.1:9/oauth2/v1".into(),
                client_id: String::new(),
                client_secret: String::new(),
                org_id: String::new(),
            },
            bpjs: BpjsConfig {
                enabled: false,
                cons_id: String::new(),
                secret_key: String::new(),
                user_key: String::new(),
                vclaim_url: "http://127.0.0.1:9/vclaim-rest/".into(),
                pcare_url: "http://127.0.0.1:9/pcare-rest-v3.0/".into(),
                antrean_url: "http://127.0.0.1:9/antrean-rs/".into(),
                aplicares_url: "http://127.0.0.1:9/aplicaresws/".into(),
                kode_ppk: String::new(),
                eklaim_url: "http://127.0.0.1:9/eklaim/".into(),
            },
            worker_enabled: false,
            worker_interval_secs: 10,
            worker_batch: 10,
            outbox_max_attempts: 5,
        }
    }

    async fn key_app() -> (Router, Arc<Db>) {
        let db = Db::connect_for_tests().await.expect("buka DB test");
        db.migrate().await.expect("migrasi");
        assert!(db.seed_if_empty().await.expect("seed"), "seed harus jalan");
        let db = Arc::new(db);
        let app = Router::new()
            .route("/api/api-keys", get(list_api_keys).post(create_api_key))
            .route("/api/api-keys/{id}/revoke", post(revoke_api_key))
            .with_state(AppState {
                db: db.clone(),
                bridges: Arc::new(build_registry(&disabled_config())),
                bridge_config: disabled_config(),
            });
        (app, db)
    }

    async fn call(app: Router, req: Request<Body>) -> (StatusCode, Value) {
        let resp = app.oneshot(req).await.expect("oneshot");
        let status = resp.status();
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .expect("baca body");
        let value: Value = serde_json::from_slice(&bytes)
            .unwrap_or(Value::String(String::from_utf8_lossy(&bytes).into_owned()));
        (status, value)
    }

    fn authed(method: &str, uri: &str, token: &str, body: Option<Value>) -> Request<Body> {
        let mut b = Request::builder()
            .method(method)
            .uri(uri)
            .header("authorization", format!("Bearer {token}"));
        if let Some(v) = body {
            b = b.header("content-type", "application/json");
            b.body(Body::from(v.to_string())).expect("request")
        } else {
            b.body(Body::empty()).expect("request")
        }
    }

    /// Alur admin: buat kunci (kunci penuh HANYA SEKALI, tanpa hash
    /// di JSON) -> daftar (tanpa hash) -> cabut; dokter ditolak 403.
    #[tokio::test]
    async fn alur_admin_buat_daftar_cabut_kunci() {
        let (app, db) = key_app().await;
        let admin = db.create_session("U-006").await.expect("sesi admin");
        let dokter = db.create_session("U-001").await.expect("sesi dokter");

        // Tanpa token -> 401.
        let (status, _) = call(
            app.clone(),
            Request::builder()
                .method("GET")
                .uri("/api/api-keys")
                .body(Body::empty())
                .expect("request"),
        )
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        // Dokter (bukan Admin) -> 403 ManageApiKeys.
        let (status, _) = call(app.clone(), authed("GET", "/api/api-keys", &dokter, None)).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        // Scope tidak dikenal -> 400.
        let (status, _) = call(
            app.clone(),
            authed(
                "POST",
                "/api/api-keys",
                &admin,
                Some(serde_json::json!({"name": "X", "scopes": ["patients:write"]})),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        // Buat kunci -> 201, kunci penuh sekali, hash TIDAK ada di JSON.
        let (status, body) = call(
            app.clone(),
            authed(
                "POST",
                "/api/api-keys",
                &admin,
                Some(serde_json::json!({"name": "Aplikasi Mobile JKN", "scopes": ["patients:read", "queue:read"]})),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "{body}");
        let key = body["key"].as_str().expect("kunci penuh").to_string();
        assert_eq!(key.len(), 36);
        assert!(key.starts_with("wrs_"));
        assert!(body["api_key"]["key_hash"].is_null(), "hash tidak boleh bocor: {body}");
        assert_eq!(body["api_key"]["key_prefix"], &key[..12]);
        let id = body["api_key"]["id"].as_str().expect("id").to_string();
        // Daftar: kunci penuh TIDAK PERNAH muncul lagi.
        let (status, body) = call(app.clone(), authed("GET", "/api/api-keys", &admin, None)).await;
        assert_eq!(status, StatusCode::OK);
        assert!(!body.to_string().contains(&key), "kunci penuh bocor di daftar");
        assert_eq!(body.as_array().expect("array").len(), 1);
        // Cabut -> revoked.
        let (status, body) = call(
            app.clone(),
            authed("POST", &format!("/api/api-keys/{id}/revoke"), &admin, None),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["revoked"], true);
        // Cabut id tidak ada -> 404.
        let (status, _) = call(
            app,
            authed("POST", "/api/api-keys/AK-999999/revoke", &admin, None),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }
}
