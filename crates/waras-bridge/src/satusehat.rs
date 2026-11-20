//! Adapter SATUSEHAT (FHIR R4).
//!
//! OAuth2 client credentials: POST `{auth_url}/accesstoken?grant_type=client_credentials`
//! dengan `client_id` + `client_secret` di body form-urlencoded.
//! JANGAN pakai HTTP Basic auth: API SATUSEHAT menolaknya dengan 401
//! "client_id or client_secret combination is unacceptable" (terbukti
//! live lawan staging 2026-11-20; bentuk query-grant + body ini yang
//! menghasilkan token 200). Token di-cache di struct sampai kedaluwarsa
//! (leeway 30 detik) dan di-refresh sekali setelah 401. Resource dikirim
//! POST `{base_url}/{ResourceType}` dengan body = `job.payload` (JSON FHIR
//! dari `waras-core::fhir`); `Bundle` dikirim ke `{base_url}` (root).
//! Validasi profil berjalan SEBELUM HTTP apa pun, supaya payload tidak
//! lengkap tidak pernah sampai ke server.
//!
//! Pemetaan error: 401 -> `Auth` (setelah satu refresh); 400/422 ->
//! `Validation` (body dipotong 500 karakter); 429/5xx/transport -> `Http`
//! (retryable); lainnya -> `Rejected{status, body}`.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde_json::Value;
use waras_core::bridge::{BridgeHealth, OutboxDestination, OutboxJob};

use crate::{Bridge, BridgeError, SatusehatConfig, SendOutcome};

/// Token yang di-cache + kapan dianggap kedaluwarsa (sudah dipotong leeway).
struct CachedToken {
    access_token: String,
    expires_at: Instant,
}

/// Adapter SATUSEHAT. `client` dipakai untuk token + FHIR POST.
pub struct SatusehatBridge {
    config: SatusehatConfig,
    client: reqwest::Client,
    token: Mutex<Option<CachedToken>>,
}

impl SatusehatBridge {
    pub fn new(config: SatusehatConfig, client: reqwest::Client) -> Self {
        Self {
            config,
            client,
            token: Mutex::new(None),
        }
    }

    /// URL FHIR base dari config.
    pub fn base_url(&self) -> &str {
        &self.config.base_url
    }

    /// URL otentikasi dari config.
    pub fn auth_url(&self) -> &str {
        &self.config.auth_url
    }

    /// Klien HTTP bersama.
    pub fn http_client(&self) -> &reqwest::Client {
        &self.client
    }

    /// Config (untuk token cache / pengujian).
    pub fn config(&self) -> &SatusehatConfig {
        &self.config
    }

    /// Ambil token akses yang masih berlaku; minta yang baru bila belum
    /// ada, kedaluwarsa, atau `force_refresh`. Token baru disimpan di cache.
    async fn ensure_token(&self, force_refresh: bool) -> Result<String, BridgeError> {
        if !force_refresh {
            if let Ok(guard) = self.token.lock() {
                if let Some(cached) = guard.as_ref() {
                    if cached.expires_at > Instant::now() {
                        return Ok(cached.access_token.clone());
                    }
                }
            }
        }
        // Bentuk resmi (playbook Kemenkes; terbukti live 2026-11-20):
        // grant di query, kredensial di body form. Basic auth DITOLAK
        // API asli (401), jadi jangan pernah dikirim.
        let url = format!(
            "{}/accesstoken?grant_type=client_credentials",
            self.config.auth_url.trim_end_matches('/')
        );
        let resp = self
            .client
            .post(&url)
            .form(&[
                ("client_id", self.config.client_id.as_str()),
                ("client_secret", self.config.client_secret.as_str()),
            ])
            .send()
            .await
            .map_err(|e| BridgeError::Http(format!("gagal meminta token SATUSEHAT: {e}")))?;
        let status = resp.status();
        if status == reqwest::StatusCode::UNAUTHORIZED {
            return Err(BridgeError::Auth(
                "client_id/client_secret SATUSEHAT ditolak (401)".to_string(),
            ));
        }
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            let body = truncate(&body, 500);
            return Err(map_token_error(status, &body));
        }
        let body: Value = resp
            .json()
            .await
            .map_err(|e| BridgeError::Unexpected(format!("respons token SATUSEHAT bukan JSON: {e}")))?;
        let access_token = body["access_token"]
            .as_str()
            .ok_or_else(|| {
                BridgeError::Unexpected("respons token SATUSEHAT tanpa access_token".to_string())
            })?
            .to_string();
        // `expires_in` dalam detik; potong 30 detik supaya token tidak
        // dipakai di detik-detik terakhir masa berlakunya.
        let expires_in = body["expires_in"].as_u64().unwrap_or(3600);
        let expires_at = Instant::now()
            + Duration::from_secs(expires_in.saturating_sub(30).max(1));
        if let Ok(mut guard) = self.token.lock() {
            *guard = Some(CachedToken {
                access_token: access_token.clone(),
                expires_at,
            });
        }
        Ok(access_token)
    }

    /// Satu POST resource FHIR dengan Bearer token.
    async fn post_resource(
        &self,
        url: &str,
        token: &str,
        payload: &Value,
    ) -> Result<reqwest::Response, BridgeError> {
        self.client
            .post(url)
            .bearer_auth(token)
            .json(payload)
            .send()
            .await
            .map_err(|e| BridgeError::Http(format!("gagal POST ke SATUSEHAT: {e}")))
    }

    /// GET baca-saja `{base_url}/{path}` dengan Bearer token (dipakai
    /// verifikasi live, mis. `metadata` dan pencarian `Patient?identifier=…`).
    /// TIDAK membuat, mengubah, atau menghapus data apa pun.
    pub async fn get_json(&self, path: &str) -> Result<Value, BridgeError> {
        if !self.configured() {
            return Err(BridgeError::NotConfigured(
                "SATUSEHAT belum dikonfigurasi (cek env di docs/FASE2.md)".to_string(),
            ));
        }
        let url = format!(
            "{}/{}",
            self.config.base_url.trim_end_matches('/'),
            path.trim_start_matches('/')
        );
        let token = self.ensure_token(false).await?;
        let mut resp = self
            .client
            .get(&url)
            .bearer_auth(&token)
            .send()
            .await
            .map_err(|e| BridgeError::Http(format!("gagal GET ke SATUSEHAT: {e}")))?;
        if resp.status() == reqwest::StatusCode::UNAUTHORIZED {
            // Token mungkin kedaluwarsa di sisi server: refresh sekali, coba ulang.
            let token = self.ensure_token(true).await?;
            resp = self
                .client
                .get(&url)
                .bearer_auth(&token)
                .send()
                .await
                .map_err(|e| BridgeError::Http(format!("gagal GET ke SATUSEHAT: {e}")))?;
        }
        let status = resp.status();
        if !status.is_success() {
            let body = truncate(&resp.text().await.unwrap_or_default(), 500);
            let code = status.as_u16();
            return Err(match code {
                401 => BridgeError::Auth(format!("SATUSEHAT menolak token (401): {body}")),
                429 => BridgeError::Http(format!("SATUSEHAT membatasi laju (429): {body}")),
                _ if status.is_server_error() => {
                    BridgeError::Http(format!("SATUSEHAT gagal (HTTP {code}): {body}"))
                }
                _ => BridgeError::Rejected { status: code, body },
            });
        }
        resp.json()
            .await
            .map_err(|e| BridgeError::Unexpected(format!("respons SATUSEHAT bukan JSON: {e}")))
    }
}

/// Potong string ke `max` karakter (bukan byte) supaya pesan error aman.
fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        s.chars().take(max).collect()
    }
}

/// Pemetaan error endpoint token: 4xx = kredensial salah (Auth),
/// 429/5xx = sementara (Http), sisanya Rejected.
fn map_token_error(status: reqwest::StatusCode, body: &str) -> BridgeError {
    if status.is_client_error() {
        BridgeError::Auth(format!("token SATUSEHAT ditolak (HTTP {}): {}", status.as_u16(), body))
    } else if status.as_u16() == 429 || status.is_server_error() {
        BridgeError::Http(format!("token SATUSEHAT gagal (HTTP {}): {}", status.as_u16(), body))
    } else {
        BridgeError::Rejected {
            status: status.as_u16(),
            body: body.to_string(),
        }
    }
}

/// Validasi profil SATUSEHAT SEBELUM kirim. Mengembalikan
/// `BridgeError::Validation` yang menyebut NAMA FIELD yang bermasalah.
fn validate_payload(resource_type: &str, payload: &Value) -> Result<(), BridgeError> {
    let missing = |field: &str| {
        BridgeError::Validation(format!("{resource_type}: field '{field}' wajib diisi"))
    };
    let text = |v: &Value| v.as_str().map(|s| !s.trim().is_empty()).unwrap_or(false);
    // Referensi FHIR, mis. `Patient/812` di `subject.reference` / `patient.reference`.
    let has_ref = |v: &Value| text(&v["reference"]);
    match resource_type {
        "Patient" => {
            // NIK: identifier dengan system .../nik, atau field `nik` langsung.
            let nik = payload["identifier"]
                .as_array()
                .and_then(|ids| {
                    ids.iter().find(|i| {
                        i["system"]
                            .as_str()
                            .map(|s| s.contains("/nik"))
                            .unwrap_or(false)
                    })
                })
                .map(|i| text(&i["value"]))
                .unwrap_or(false)
                || text(&payload["nik"]);
            if !nik {
                return Err(missing("nik"));
            }
            if !text(&payload["name"][0]["text"]) && !text(&payload["nama"]) {
                return Err(missing("nama"));
            }
        }
        "Encounter" => {
            if !has_ref(&payload["subject"]) {
                return Err(missing("subject"));
            }
        }
        "Observation" => {
            if !text(&payload["code"]["text"])
                && !text(&payload["code"]["coding"][0]["code"])
            {
                return Err(missing("code"));
            }
            if !has_ref(&payload["subject"]) {
                return Err(missing("subject"));
            }
        }
        "Condition" => {
            if !text(&payload["code"]["coding"][0]["code"]) && !text(&payload["code"]["text"]) {
                return Err(missing("code"));
            }
        }
        "MedicationRequest" => {
            if !text(&payload["medicationCodeableConcept"]["text"]) {
                return Err(missing("obat"));
            }
            if !has_ref(&payload["subject"]) {
                return Err(missing("subject"));
            }
        }
        "AllergyIntolerance" => {
            if !text(&payload["code"]["text"]) {
                return Err(missing("substance"));
            }
            if !has_ref(&payload["patient"]) {
                return Err(missing("patient"));
            }
        }
        "Bundle" => {
            if payload["resourceType"].as_str() != Some("Bundle")
                || !payload["entry"].as_array().map(|e| !e.is_empty()).unwrap_or(false)
            {
                return Err(missing("entry"));
            }
        }
        other => {
            return Err(BridgeError::Validation(format!(
                "resource_type '{other}' tidak didukung SATUSEHAT"
            )));
        }
    }
    Ok(())
}

/// Pemetaan respons FHIR POST ke `SendOutcome` / `BridgeError`.
async fn map_response(resp: reqwest::Response) -> Result<SendOutcome, BridgeError> {
    let status = resp.status();
    if status.is_success() {
        let body: Value = resp.json().await.unwrap_or(Value::Null);
        let external_id = body["id"].as_str().map(|s| s.to_string());
        return Ok(SendOutcome {
            status: if external_id.is_some() {
                waras_core::bridge::OutboxStatus::Acked
            } else {
                waras_core::bridge::OutboxStatus::Sent
            },
            external_id,
            note: None,
        });
    }
    let body = resp.text().await.unwrap_or_default();
    let body = truncate(&body, 500);
    let code = status.as_u16();
    match code {
        401 => Err(BridgeError::Auth(format!(
            "SATUSEHAT menolak token (401): {body}"
        ))),
        400 | 422 => Err(BridgeError::Validation(format!(
            "SATUSEHAT menolak payload (HTTP {code}): {body}"
        ))),
        429 => Err(BridgeError::Http(format!(
            "SATUSEHAT membatasi laju (429): {body}"
        ))),
        _ if status.is_server_error() => Err(BridgeError::Http(format!(
            "SATUSEHAT gagal (HTTP {code}): {body}"
        ))),
        _ => Err(BridgeError::Rejected { status: code, body }),
    }
}

#[async_trait::async_trait]
impl Bridge for SatusehatBridge {
    fn destination(&self) -> OutboxDestination {
        OutboxDestination::Satusehat
    }

    fn name(&self) -> &'static str {
        "SATUSEHAT"
    }

    fn configured(&self) -> bool {
        self.config.configured()
    }

    async fn send(&self, job: &OutboxJob) -> Result<SendOutcome, BridgeError> {
        if !self.configured() {
            return Err(BridgeError::NotConfigured(
                "SATUSEHAT belum dikonfigurasi (cek env di docs/FASE2.md)".to_string(),
            ));
        }
        validate_payload(&job.resource_type, &job.payload)?;
        let base = self.config.base_url.trim_end_matches('/');
        let url = if job.resource_type == "Bundle" {
            base.to_string()
        } else {
            format!("{base}/{}", job.resource_type)
        };
        let token = self.ensure_token(false).await?;
        let mut resp = self.post_resource(&url, &token, &job.payload).await?;
        if resp.status() == reqwest::StatusCode::UNAUTHORIZED {
            // Token mungkin kedaluwarsa di sisi server: refresh sekali, coba ulang.
            let token = self.ensure_token(true).await?;
            resp = self.post_resource(&url, &token, &job.payload).await?;
        }
        map_response(resp).await
    }

    async fn health(&self) -> BridgeHealth {
        if !self.configured() {
            BridgeHealth::BelumDikonfigurasi
        } else {
            // Probe ringan: minta token (di-cache, jadi murah setelah pertama).
            match self.ensure_token(false).await {
                Ok(_) => BridgeHealth::Online,
                Err(_) => BridgeHealth::Offline,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::extract::State;
    use axum::http::{HeaderMap, StatusCode};
    use axum::routing::post;
    use axum::{Json, Router};
    use serde_json::json;
    use std::sync::Arc;
    use tokio::net::TcpListener;
    use waras_core::bridge::OutboxStatus;
    use waras_core::fhir;
    use waras_core::types::{
        AllergyIntolerance, Condition, ConditionGroup, ConditionStatus, Criticality,
        Encounter, EncounterStatus, Gender, Insurance, MedicationRequest, Observation,
        Patient, PrescriptionStatus, VitalCode,
    };
    use chrono::{NaiveDate, NaiveDateTime};

    // -- Stub SATUSEHAT -----------------------------------------------------

    #[derive(Clone, Default)]
    struct StubState {
        inner: Arc<Mutex<StubInner>>,
    }

    #[derive(Default)]
    struct StubInner {
        token_hits: usize,
        token_requests: Vec<TokenRecorded>,
        fhir_hits: usize,
        fhir_requests: Vec<Recorded>,
        /// Kembalikan 401 pada POST FHIR pertama, lalu sukses.
        unauthorized_once: bool,
        /// Selalu kembalikan 401 pada POST FHIR.
        always_401: bool,
        /// Kembalikan status ini sebanyak `fail_times` panggilan FHIR pertama.
        fail_status: Option<u16>,
        fail_times: usize,
    }

    #[derive(Clone, Debug)]
    struct TokenRecorded {
        grant_in_query: bool,
        has_basic_auth: bool,
        client_id: Option<String>,
        client_secret: Option<String>,
    }

    #[derive(Clone, Debug)]
    struct Recorded {
        path: String,
        auth: Option<String>,
        body: Value,
    }

    impl StubState {
        fn token_hits(&self) -> usize {
            self.inner.lock().unwrap().token_hits
        }
        fn token_requests(&self) -> Vec<TokenRecorded> {
            self.inner.lock().unwrap().token_requests.clone()
        }
        fn fhir_hits(&self) -> usize {
            self.inner.lock().unwrap().fhir_hits
        }
        fn requests(&self) -> Vec<Recorded> {
            self.inner.lock().unwrap().fhir_requests.clone()
        }
    }

    /// Parse `a=b&c=d` (query atau body form) dengan percent-decode minimal.
    /// Cukup untuk nilai ASCII seperti client id/secret di stub ini.
    fn parse_form(s: &str) -> std::collections::HashMap<String, String> {
        let mut out = std::collections::HashMap::new();
        for pair in s.split('&') {
            if pair.is_empty() {
                continue;
            }
            let mut it = pair.splitn(2, '=');
            let k = it.next().unwrap_or("");
            let v = it.next().unwrap_or("");
            out.insert(pct_decode(k), pct_decode(v));
        }
        out
    }

    fn pct_decode(s: &str) -> String {
        let mut out = String::with_capacity(s.len());
        let mut chars = s.chars();
        while let Some(c) = chars.next() {
            if c == '%' {
                let h1 = chars.next().unwrap_or('0');
                let h2 = chars.next().unwrap_or('0');
                let b = u8::from_str_radix(&format!("{h1}{h2}"), 16).unwrap_or(b'?');
                out.push(b as char);
            } else if c == '+' {
                out.push(' ');
            } else {
                out.push(c);
            }
        }
        out
    }

    /// Stub token KETAT: meniru API SATUSEHAT asli (terbukti live 2026-11-20).
    /// Bentuk resmi: `grant_type=client_credentials` di QUERY + `client_id`
    /// + `client_secret` di body form. Basic auth DITOLAK (401) seperti API
    /// asli — stub lama yang menerima apa pun menyembunyikan bug ini.
    async fn token_handler(
        State(state): State<StubState>,
        uri: axum::http::Uri,
        headers: HeaderMap,
        body: String,
    ) -> (StatusCode, Json<Value>) {
        let query = parse_form(uri.query().unwrap_or(""));
        let form = parse_form(&body);
        let has_basic = headers
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_ascii_lowercase().starts_with("basic "))
            .unwrap_or(false);
        let mut inner = state.inner.lock().unwrap();
        inner.token_hits += 1;
        let n = inner.token_hits;
        inner.token_requests.push(TokenRecorded {
            grant_in_query: query.get("grant_type").map(|s| s.as_str())
                == Some("client_credentials"),
            has_basic_auth: has_basic,
            client_id: form.get("client_id").cloned(),
            client_secret: form.get("client_secret").cloned(),
        });
        if has_basic {
            return (
                StatusCode::UNAUTHORIZED,
                Json(json!({"resourceType": "OperationOutcome", "issue": [{"code": "value", "details": {"text": "Basic auth tidak diterima"}}]})),
            );
        }
        if query.get("grant_type").map(|s| s.as_str()) != Some("client_credentials")
            || form.get("client_id").map(|s| s.as_str()) != Some("CONTOH-CLIENT-ID")
            || form.get("client_secret").map(|s| s.as_str()) != Some("CONTOH-CLIENT-SECRET")
        {
            return (
                StatusCode::UNAUTHORIZED,
                Json(json!({"resourceType": "OperationOutcome", "issue": [{"code": "value", "details": {"text": "client_id or client_secret combination is unacceptable"}}]})),
            );
        }
        (
            StatusCode::OK,
            Json(json!({
                "access_token": format!("TOKEN-CONTOH-{n}"),
                "expires_in": 3600,
                "token_type": "Bearer"
            })),
        )
    }

    /// Bearer stub harus diawali `Bearer TOKEN-CONTOH-` (token hasil stub).
    fn bearer_ok(headers: &HeaderMap) -> bool {
        headers
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.starts_with("Bearer TOKEN-CONTOH-"))
            .unwrap_or(false)
    }

    async fn metadata_handler(headers: HeaderMap) -> (StatusCode, Json<Value>) {
        if !bearer_ok(&headers) {
            return (
                StatusCode::UNAUTHORIZED,
                Json(json!({"resourceType": "OperationOutcome", "issue": [{"code": "invalid-access-token", "details": {"text": "Invalid Access Token."}}]})),
            );
        }
        (
            StatusCode::OK,
            Json(json!({"resourceType": "CapabilityStatement", "fhirVersion": "4.0.1"})),
        )
    }

    async fn patient_search_handler(headers: HeaderMap) -> (StatusCode, Json<Value>) {
        if !bearer_ok(&headers) {
            return (
                StatusCode::UNAUTHORIZED,
                Json(json!({"resourceType": "OperationOutcome", "issue": [{"code": "invalid-access-token", "details": {"text": "Invalid Access Token."}}]})),
            );
        }
        (
            StatusCode::OK,
            Json(json!({"resourceType": "Bundle", "type": "searchset", "total": 0, "entry": []})),
        )
    }

    async fn fhir_handler(
        State(state): State<StubState>,
        axum::extract::Path(resource): axum::extract::Path<String>,
        headers: HeaderMap,
        Json(body): Json<Value>,
    ) -> (StatusCode, Json<Value>) {
        fhir_post(&state, &resource, &headers, body).await
    }

    /// POST Patient di rute statik (axum mengutamakan rute statik
    /// `/fhir-r4/v1/Patient` di atas `/{resource}`; tanpa ini POST Patient
    /// jadi 405 karena rute itu hanya punya GET pencarian).
    async fn patient_post_handler(
        State(state): State<StubState>,
        headers: HeaderMap,
        Json(body): Json<Value>,
    ) -> (StatusCode, Json<Value>) {
        fhir_post(&state, "Patient", &headers, body).await
    }

    async fn fhir_post(
        state: &StubState,
        resource: &str,
        headers: &HeaderMap,
        body: Value,
    ) -> (StatusCode, Json<Value>) {
        let mut inner = state.inner.lock().unwrap();
        inner.fhir_hits += 1;
        inner.fhir_requests.push(Recorded {
            path: format!("/fhir-r4/v1/{resource}"),
            auth: headers
                .get("authorization")
                .and_then(|v| v.to_str().ok())
                .map(|s| s.to_string()),
            body: body.clone(),
        });
        if inner.always_401 {
            return (StatusCode::UNAUTHORIZED, Json(json!({"error": "invalid_token"})));
        }
        if inner.unauthorized_once {
            inner.unauthorized_once = false;
            return (StatusCode::UNAUTHORIZED, Json(json!({"error": "invalid_token"})));
        }
        if inner.fail_times > 0 {
            inner.fail_times -= 1;
            let status = inner.fail_status.unwrap_or(500);
            return (
                StatusCode::from_u16(status).unwrap(),
                Json(json!({"error": format!("stub gagal dengan HTTP {status}")})),
            );
        }
        let id = if resource == "Bundle" { "BND-CONTOH-001" } else { "IHS-CONTOH-001" };
        (
            StatusCode::CREATED,
            Json(json!({"resourceType": resource, "id": id})),
        )
    }

    async fn bundle_root_handler(
        State(state): State<StubState>,
        headers: HeaderMap,
        Json(body): Json<Value>,
    ) -> (StatusCode, Json<Value>) {
        let mut inner = state.inner.lock().unwrap();
        inner.fhir_hits += 1;
        inner.fhir_requests.push(Recorded {
            path: "/fhir-r4/v1".to_string(),
            auth: headers
                .get("authorization")
                .and_then(|v| v.to_str().ok())
                .map(|s| s.to_string()),
            body: body.clone(),
        });
        if inner.fail_times > 0 {
            inner.fail_times -= 1;
            let status = inner.fail_status.unwrap_or(500);
            return (
                StatusCode::from_u16(status).unwrap(),
                Json(json!({"error": format!("stub gagal dengan HTTP {status}")})),
            );
        }
        (
            StatusCode::CREATED,
            Json(json!({"resourceType": "Bundle", "id": "BND-CONTOH-001"})),
        )
    }

    /// Jalankan stub di port ephemeral. Mengembalikan (state, auth_url, base_url).
    async fn spawn_stub() -> (StubState, String, String) {
        let state = StubState::default();
        let app = Router::new()
            .route("/oauth2/v1/accesstoken", post(token_handler))
            .route("/fhir-r4/v1/metadata", axum::routing::get(metadata_handler))
            .route(
                "/fhir-r4/v1/Patient",
                axum::routing::get(patient_search_handler).post(patient_post_handler),
            )
            .route("/fhir-r4/v1/{resource}", post(fhir_handler))
            .route("/fhir-r4/v1", post(bundle_root_handler))
            .with_state(state.clone());
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        (
            state,
            format!("http://{addr}/oauth2/v1"),
            format!("http://{addr}/fhir-r4/v1"),
        )
    }

    fn make_bridge(auth_url: &str, base_url: &str) -> SatusehatBridge {
        SatusehatBridge::new(
            SatusehatConfig {
                enabled: true,
                base_url: base_url.to_string(),
                auth_url: auth_url.to_string(),
                client_id: "CONTOH-CLIENT-ID".to_string(),
                client_secret: "CONTOH-CLIENT-SECRET".to_string(),
                org_id: "CONTOH-ORG-ID".to_string(),
            },
            crate::build_http_client(),
        )
    }

    fn job(resource_type: &str, payload: Value) -> OutboxJob {
        OutboxJob {
            id: "OBX-000001".to_string(),
            destination: OutboxDestination::Satusehat,
            resource_type: resource_type.to_string(),
            resource_id: "RM-000812".to_string(),
            idempotency_key: format!("satusehat:{resource_type}:RM-000812:created"),
            payload,
            status: OutboxStatus::Pending,
            attempts: 0,
            max_attempts: 5,
            next_retry_at: None,
            last_error: None,
            external_id: None,
            created_at: "2026-11-19T09:15:00".to_string(),
            updated_at: "2026-11-19T09:15:00".to_string(),
        }
    }

    fn seed_patient() -> Patient {
        Patient {
            id: "RM-000812".to_string(),
            nik: "3374021205850001".to_string(),
            name: "Siti Rahmawati".to_string(),
            birth_date: NaiveDate::from_ymd_opt(1985, 5, 12).unwrap(),
            gender: Gender::Perempuan,
            address: "Jl. Pandanaran No. 12, Semarang".to_string(),
            phone: None,
            blood_type: Some("O+".to_string()),
            bpjs_number: Some("0001234567890".to_string()),
            insurance: Insurance::Bpjs,
            created_at: NaiveDateTime::default(),
        }
    }

    fn seed_encounter(patient: &Patient) -> Encounter {
        Encounter {
            id: "ENC-2026-11-19-014".to_string(),
            patient_id: patient.id.clone(),
            poli: "Poli Umum".to_string(),
            doctor_id: Some("U-001".to_string()),
            doctor_name: "dr. Ahmad Hidayat".to_string(),
            date: NaiveDate::from_ymd_opt(2026, 11, 19).unwrap(),
            time: "09.15".to_string(),
            status: EncounterStatus::DalamPemeriksaan,
            created_at: NaiveDateTime::default(),
        }
    }

    #[tokio::test]
    async fn kirim_patient_sukses_dan_token_dipakai_ulang() {
        let (state, auth_url, base_url) = spawn_stub().await;
        let bridge = make_bridge(&auth_url, &base_url);
        let payload = fhir::patient_resource(&seed_patient());
        let out = bridge.send(&job("Patient", payload)).await.unwrap();
        assert_eq!(out.status, OutboxStatus::Acked);
        assert_eq!(out.external_id.as_deref(), Some("IHS-CONTOH-001"));
        // Kirim kedua: token tidak diminta lagi.
        let encounter = seed_encounter(&seed_patient());
        let out2 = bridge
            .send(&job("Encounter", fhir::encounter_resource(&encounter)))
            .await
            .unwrap();
        assert_eq!(out2.status, OutboxStatus::Acked);
        assert_eq!(state.token_hits(), 1, "token harus dipakai ulang");
        assert_eq!(state.fhir_hits(), 2);
        let reqs = state.requests();
        assert_eq!(reqs[0].path, "/fhir-r4/v1/Patient");
        assert_eq!(reqs[0].auth.as_deref(), Some("Bearer TOKEN-CONTOH-1"));
        assert_eq!(reqs[0].body["resourceType"], "Patient");
        assert_eq!(reqs[1].path, "/fhir-r4/v1/Encounter");
    }

    #[tokio::test]
    async fn refresh_sekali_setelah_401() {
        let (state, auth_url, base_url) = spawn_stub().await;
        state.inner.lock().unwrap().unauthorized_once = true;
        let bridge = make_bridge(&auth_url, &base_url);
        let out = bridge
            .send(&job("Patient", fhir::patient_resource(&seed_patient())))
            .await
            .unwrap();
        assert_eq!(out.status, OutboxStatus::Acked);
        assert_eq!(state.token_hits(), 2, "token harus di-refresh sekali");
        assert_eq!(state.fhir_hits(), 2, "POST diulang sekali setelah 401");
        assert_eq!(
            state.requests()[1].auth.as_deref(),
            Some("Bearer TOKEN-CONTOH-2")
        );
    }

    #[tokio::test]
    async fn gagal_401_terus_menerus_jadi_auth() {
        let (state, auth_url, base_url) = spawn_stub().await;
        state.inner.lock().unwrap().always_401 = true;
        let bridge = make_bridge(&auth_url, &base_url);
        let err = bridge
            .send(&job("Patient", fhir::patient_resource(&seed_patient())))
            .await
            .unwrap_err();
        assert!(matches!(err, BridgeError::Auth(_)), "diharapkan Auth, dapat {err:?}");
        assert_eq!(state.token_hits(), 2, "refresh dicoba sekali lalu menyerah");
    }

    #[tokio::test]
    async fn bundle_dikirim_ke_root_base_url() {
        let (state, auth_url, base_url) = spawn_stub().await;
        let bridge = make_bridge(&auth_url, &base_url);
        let patient = seed_patient();
        let encounter = seed_encounter(&patient);
        let bundle = fhir::encounter_bundle(&patient, &encounter, &[], &[], &[]);
        let out = bridge.send(&job("Bundle", bundle)).await.unwrap();
        assert_eq!(out.status, OutboxStatus::Acked);
        assert_eq!(out.external_id.as_deref(), Some("BND-CONTOH-001"));
        assert_eq!(state.requests()[0].path, "/fhir-r4/v1");
    }

    #[tokio::test]
    async fn validasi_menyebut_field_yang_hilang_tanpa_http() {
        let (state, auth_url, base_url) = spawn_stub().await;
        let bridge = make_bridge(&auth_url, &base_url);
        let patient = seed_patient();
        let encounter = seed_encounter(&patient);

        let cases: Vec<(&str, Value, &str)> = vec![
            ("Patient", json!({"resourceType": "Patient", "name": [{"text": "Siti"}]}), "nik"),
            (
                "Patient",
                json!({"resourceType": "Patient", "identifier": [{"system": "https://fhir.kemkes.go.id/id/nik", "value": "3374021205850001"}]}),
                "nama",
            ),
            ("Encounter", json!({"resourceType": "Encounter", "id": "ENC-1"}), "subject"),
            (
                "Observation",
                json!({"resourceType": "Observation", "subject": {"reference": "Patient/812"}}),
                "code",
            ),
            (
                "Observation",
                json!({"resourceType": "Observation", "code": {"text": "Tekanan darah"}}),
                "subject",
            ),
            ("Condition", json!({"resourceType": "Condition", "subject": {"reference": "Patient/812"}}), "code"),
            (
                "MedicationRequest",
                json!({"resourceType": "MedicationRequest", "subject": {"reference": "Patient/812"}}),
                "obat",
            ),
            (
                "AllergyIntolerance",
                json!({"resourceType": "AllergyIntolerance", "patient": {"reference": "Patient/812"}}),
                "substance",
            ),
            (
                "AllergyIntolerance",
                json!({"resourceType": "AllergyIntolerance", "code": {"text": "Penisilin"}}),
                "patient",
            ),
        ];
        for (rt, payload, field) in cases {
            let err = bridge.send(&job(rt, payload)).await.unwrap_err();
            match &err {
                BridgeError::Validation(msg) => {
                    assert!(msg.contains(field), "{rt}: pesan '{msg}' harus menyebut '{field}'")
                }
                other => panic!("{rt}: diharapkan Validation, dapat {other:?}"),
            }
        }
        assert_eq!(state.token_hits(), 0, "validasi gagal sebelum HTTP apa pun");
        assert_eq!(state.fhir_hits(), 0);
        // Payload lengkap dari builder lolos validasi.
        let _ = encounter;
    }

    #[tokio::test]
    async fn semua_builder_fhir_lolos_validasi_dan_terkirim() {
        let (state, auth_url, base_url) = spawn_stub().await;
        let bridge = make_bridge(&auth_url, &base_url);
        let patient = seed_patient();
        let encounter = seed_encounter(&patient);
        let obs = Observation {
            id: "OBS-0001".to_string(),
            encounter_id: encounter.id.clone(),
            patient_id: patient.id.clone(),
            code: VitalCode::TekananDarah,
            value: "130/85".to_string(),
            unit: "mmHg".to_string(),
            measured_at: NaiveDateTime::default(),
            measured_by: "Ns. Sari Wijaya".to_string(),
            previous_value: None,
            previous_date: None,
        };
        let cond = Condition {
            id: "COND-0001".to_string(),
            patient_id: patient.id.clone(),
            encounter_id: Some(encounter.id.clone()),
            group: ConditionGroup::Utama,
            code: "J20.9".to_string(),
            display: "Bronkitis akut".to_string(),
            status: ConditionStatus::Aktif,
            clinician: "dr. Ahmad Hidayat".to_string(),
            recorded_at: NaiveDateTime::default(),
        };
        let rx = MedicationRequest {
            id: "RX-1041".to_string(),
            encounter_id: encounter.id.clone(),
            patient_id: patient.id.clone(),
            drug_name: "Azithromycin 500 mg".to_string(),
            drug_class: "macrolide".to_string(),
            dosage: "1x1".to_string(),
            duration: "3 hari".to_string(),
            status: PrescriptionStatus::MenungguKonfirmasi,
            doctor: "dr. Ahmad Hidayat".to_string(),
            blocked_reason: None,
            replaced_by: None,
            created_at: NaiveDateTime::default(),
        };
        let allergy = AllergyIntolerance {
            id: "ALG-000812-01".to_string(),
            patient_id: patient.id.clone(),
            substance: "Penisilin".to_string(),
            substance_class: Some("penicillin".to_string()),
            criticality: Criticality::Tinggi,
            note: None,
            recorded_at: NaiveDateTime::default(),
        };
        let cases = vec![
            ("Patient", fhir::patient_resource(&patient)),
            ("Encounter", fhir::encounter_resource(&encounter)),
            ("Observation", fhir::observation_resource(&obs)),
            ("Condition", fhir::condition_resource(&cond)),
            ("MedicationRequest", fhir::medication_request_resource(&rx)),
            ("AllergyIntolerance", fhir::allergy_intolerance_resource(&allergy)),
        ];
        for (rt, payload) in cases {
            let out = bridge.send(&job(rt, payload)).await.unwrap();
            assert_eq!(out.status, OutboxStatus::Acked, "{rt} harus Acked");
            assert_eq!(out.external_id.as_deref(), Some("IHS-CONTOH-001"));
        }
        assert_eq!(state.fhir_hits(), 6);
        assert_eq!(state.token_hits(), 1);
    }

    #[tokio::test]
    async fn pemetaan_error_500_400_dan_429() {
        // 500 -> Http (retryable di worker).
        let (state, auth_url, base_url) = spawn_stub().await;
        {
            let mut inner = state.inner.lock().unwrap();
            inner.fail_status = Some(500);
            inner.fail_times = 1;
        }
        let bridge = make_bridge(&auth_url, &base_url);
        let err = bridge
            .send(&job("Patient", fhir::patient_resource(&seed_patient())))
            .await
            .unwrap_err();
        assert!(matches!(err, BridgeError::Http(_)), "500 -> Http, dapat {err:?}");

        // 400 -> Validation.
        let (state, auth_url, base_url) = spawn_stub().await;
        {
            let mut inner = state.inner.lock().unwrap();
            inner.fail_status = Some(400);
            inner.fail_times = 1;
        }
        let bridge = make_bridge(&auth_url, &base_url);
        let err = bridge
            .send(&job("Patient", fhir::patient_resource(&seed_patient())))
            .await
            .unwrap_err();
        assert!(
            matches!(err, BridgeError::Validation(_)),
            "400 -> Validation, dapat {err:?}"
        );

        // 429 -> Http.
        let (state, auth_url, base_url) = spawn_stub().await;
        {
            let mut inner = state.inner.lock().unwrap();
            inner.fail_status = Some(429);
            inner.fail_times = 1;
        }
        let bridge = make_bridge(&auth_url, &base_url);
        let err = bridge
            .send(&job("Patient", fhir::patient_resource(&seed_patient())))
            .await
            .unwrap_err();
        assert!(matches!(err, BridgeError::Http(_)), "429 -> Http, dapat {err:?}");
    }

    #[tokio::test]
    async fn belum_dikonfigurasi_tidak_mengirim() {
        let bridge = SatusehatBridge::new(
            SatusehatConfig {
                enabled: false,
                base_url: "http://127.0.0.1:9/fhir-r4/v1".to_string(),
                auth_url: "http://127.0.0.1:9/oauth2/v1".to_string(),
                client_id: String::new(),
                client_secret: String::new(),
                org_id: String::new(),
            },
            crate::build_http_client(),
        );
        let err = bridge
            .send(&job("Patient", fhir::patient_resource(&seed_patient())))
            .await
            .unwrap_err();
        assert!(matches!(err, BridgeError::NotConfigured(_)));
        assert_eq!(bridge.health().await, BridgeHealth::BelumDikonfigurasi);
    }

    #[tokio::test]
    async fn health_online_bila_token_didapat() {
        let (_state, auth_url, base_url) = spawn_stub().await;
        let bridge = make_bridge(&auth_url, &base_url);
        assert_eq!(bridge.health().await, BridgeHealth::Online);
    }

    #[tokio::test]
    async fn token_memakai_bentuk_resmi_bukan_basic() {
        let (state, auth_url, base_url) = spawn_stub().await;
        let bridge = make_bridge(&auth_url, &base_url);
        assert_eq!(bridge.health().await, BridgeHealth::Online);
        let reqs = state.token_requests();
        assert_eq!(reqs.len(), 1, "token diminta sekali");
        assert!(reqs[0].grant_in_query, "grant_type harus di query");
        assert!(!reqs[0].has_basic_auth, "Basic auth tidak boleh dikirim");
        assert_eq!(reqs[0].client_id.as_deref(), Some("CONTOH-CLIENT-ID"));
        assert_eq!(
            reqs[0].client_secret.as_deref(),
            Some("CONTOH-CLIENT-SECRET")
        );
    }

    #[tokio::test]
    async fn kredensial_salah_token_401_jadi_auth() {
        let (state, auth_url, base_url) = spawn_stub().await;
        let bridge = SatusehatBridge::new(
            SatusehatConfig {
                enabled: true,
                base_url: base_url.clone(),
                auth_url: auth_url.clone(),
                client_id: "CONTOH-CLIENT-ID".to_string(),
                client_secret: "SALAH".to_string(),
                org_id: "CONTOH-ORG-ID".to_string(),
            },
            crate::build_http_client(),
        );
        let err = bridge
            .send(&job("Patient", fhir::patient_resource(&seed_patient())))
            .await
            .unwrap_err();
        assert!(
            matches!(err, BridgeError::Auth(_)),
            "kredensial salah -> Auth, dapat {err:?}"
        );
        assert_eq!(state.token_hits(), 1, "token 401 tidak dicoba ulang");
    }

    #[tokio::test]
    async fn stub_menolak_basic_auth() {
        let (state, auth_url, _base_url) = spawn_stub().await;
        let client = reqwest::Client::new();
        let resp = client
            .post(format!("{auth_url}/accesstoken"))
            .basic_auth("CONTOH-CLIENT-ID", Some("CONTOH-CLIENT-SECRET"))
            .form(&[("grant_type", "client_credentials")])
            .send()
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            reqwest::StatusCode::UNAUTHORIZED,
            "Basic auth harus ditolak seperti API asli"
        );
        assert_eq!(state.token_hits(), 1);
    }

    #[tokio::test]
    async fn get_json_metadata_dan_cari_pasien() {
        let (state, auth_url, base_url) = spawn_stub().await;
        let bridge = make_bridge(&auth_url, &base_url);
        let meta = bridge.get_json("metadata").await.unwrap();
        assert_eq!(meta["fhirVersion"], "4.0.1");
        let found = bridge
            .get_json("Patient?identifier=https://fhir.kemkes.go.id/id/nik|0000000000000000")
            .await
            .unwrap();
        assert_eq!(found["resourceType"], "Bundle");
        assert_eq!(found["total"], 0);
        // Token yang sama dipakai ulang untuk kedua GET.
        assert_eq!(state.token_hits(), 1);
    }
}
