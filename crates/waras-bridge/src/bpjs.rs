//! Adapter BPJS Kesehatan: satu struct untuk lima layanan bridging
//! (VClaim, PCare, Antrean Online, Aplicares, E-Klaim), dibedakan oleh
//! `destination`. Header tanda tangan BPJS (X-cons-id, X-timestamp,
//! X-signature HMAC-SHA256, user_key) dibangun di sini, dipakai semua
//! layanan.
//!
//! **Status verifikasi per layanan (2026-11-20, melawan dokumen bridging
//! resmi — lihat docs/FASE2.md §9):** tanda tangan + dekripsi AES-256-CBC
//! + LZString terverifikasi; path VClaim `SEP/2.0/insert` + envelope
//! `{"request": {"t_sep": ...}}` terverifikasi; path Aplicares
//! `rest/bed/read/{kodeppk}/{start}/{limit}` di host
//! `new-api.bpjs-kesehatan.go.id` terverifikasi. Path `SEP/delete`,
//! `Rujukan/insert`, dan seluruh PCare/Antrean masih pola umum yang
//! belum terverifikasi ke dokumen resmi. **E-Klaim: endpoint
//! `klaim/submit` di host BPJS TIDAK ADA** — integrasi E-Klaim asli
//! memakai `ws.php` lokal atau alur FHIR Claim SATUSEHAT; adapter ini
//! belum siap live (lihat FASE2.md §9).
//!
//! Respons BPJS dibungkus envelope `{ metaData: {code, message},
//! response }`; `code == "200"` = sukses. Bila `response` berupa string,
//! itu ciphertext AES-256-CBC yang didekripsi dengan
//! [`decrypt_response`] (kunci = SHA-256 mentah dari
//! `cons_id + secret_key + timestamp`, IV = 16 byte pertama kunci),
//! lalu plaintext-nya masih dikompresi LZString
//! (`decompressFromEncodedURIComponent`, lihat [`crate::lzstring`]).
//! Bila `response` sudah berupa objek JSON (mis. di stub pengujian),
//! dipakai langsung.

use aes::Aes256;
use cbc::Decryptor;
use cipher::{block_padding::Pkcs7, BlockModeDecrypt, KeyIvInit};
use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};

use reqwest::Method;
use serde_json::Value;
use waras_core::bridge::{BridgeHealth, OutboxDestination, OutboxJob, OutboxStatus};

use crate::{BpjsConfig, Bridge, BridgeError, SendOutcome};

type HmacSha256 = Hmac<Sha256>;

/// Adapter BPJS untuk satu layanan (`destination`).
pub struct BpjsBridge {
    destination: OutboxDestination,
    config: BpjsConfig,
    client: reqwest::Client,
}

impl BpjsBridge {
    pub fn new(
        destination: OutboxDestination,
        config: BpjsConfig,
        client: reqwest::Client,
    ) -> Self {
        Self {
            destination,
            config,
            client,
        }
    }

    /// Base URL layanan ini dari config.
    pub fn base_url(&self) -> &str {
        match self.destination {
            OutboxDestination::VClaim => &self.config.vclaim_url,
            OutboxDestination::PCare => &self.config.pcare_url,
            OutboxDestination::AntreanOnline => &self.config.antrean_url,
            OutboxDestination::Aplicares => &self.config.aplicares_url,
            OutboxDestination::EKlaim => &self.config.eklaim_url,
            OutboxDestination::Satusehat => unreachable!("BpjsBridge tidak melayani SATUSEHAT"),
        }
    }

    /// Klien HTTP bersama.
    pub fn http_client(&self) -> &reqwest::Client {
        &self.client
    }

    /// Config (untuk pengujian).
    pub fn config(&self) -> &BpjsConfig {
        &self.config
    }

    /// Satu panggilan bridging: bangun header tanda tangan dengan timestamp
    /// baru, kirim, parse envelope (dekripsi bila `response` terenkripsi).
    /// Mengembalikan isi `response` yang sudah terdekripsi (atau `None`
    /// bila `response` null).
    async fn call(
        &self,
        method: Method,
        path: &str,
        body: Option<&Value>,
    ) -> Result<Option<Value>, BridgeError> {
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let (cons_id, ts, signature, user_key) = bpjs_headers(
            &self.config.cons_id,
            &self.config.secret_key,
            &self.config.user_key,
            timestamp,
        );
        let url = format!(
            "{}/{}",
            self.base_url().trim_end_matches('/'),
            path.trim_start_matches('/')
        );
        let mut req = self
            .client
            .request(method, &url)
            .header("X-cons-id", cons_id)
            .header("X-timestamp", ts)
            .header("X-signature", signature)
            .header("user_key", user_key);
        if let Some(body) = body {
            req = req.json(body);
        }
        let resp = req
            .send()
            .await
            .map_err(|e| BridgeError::Http(format!("gagal memanggil BPJS {url}: {e}")))?;
        let status = resp.status();
        let text = resp
            .text()
            .await
            .map_err(|e| BridgeError::Http(format!("gagal membaca respons BPJS: {e}")))?;
        let parsed: Value = match serde_json::from_str(&text) {
            Ok(v) => v,
            Err(_) => {
                // Bukan JSON: petakan dari status HTTP saja.
                return Err(map_http_error(status, &text));
            }
        };
        // Bila badannya envelope, petakan lewat metaData; bila bukan
        // envelope (mis. layanan mengembalikan JSON polos), anggap sukses
        // bila HTTP 2xx.
        if parsed.get("metaData").is_some() {
            let response = envelope_response(&parsed)?;
            Ok(decrypt_if_needed(response, &self.config, timestamp)?)
        } else if status.is_success() {
            Ok(Some(parsed))
        } else {
            Err(map_http_error(status, &text))
        }
    }
}

/// Header bridging BPJS: `X-cons-id`, `X-timestamp` (detik Unix UTC),
/// `X-signature` (base64 HMAC-SHA256 dari `cons_id&timestamp` dengan
/// `secret_key`), dan `user_key`. Murni fungsi — bisa diuji tanpa HTTP.
pub fn bpjs_headers(
    cons_id: &str,
    secret_key: &str,
    user_key: &str,
    timestamp_secs: u64,
) -> (String, String, String, String) {
    let timestamp = timestamp_secs.to_string();
    let message = format!("{cons_id}&{timestamp}");
    let mut mac = HmacSha256::new_from_slice(secret_key.as_bytes())
        .expect("secret_key BPJS selalu bisa jadi kunci HMAC");
    mac.update(message.as_bytes());
    let signature = base64::Engine::encode(
        &base64::engine::general_purpose::STANDARD,
        mac.finalize().into_bytes(),
    );
    (cons_id.to_string(), timestamp, signature, user_key.to_string())
}

/// Dekripsi respons BPJS terenkripsi (AES-256-CBC, PKCS7).
///
/// Kunci = SHA-256 mentah (32 byte) dari string
/// `cons_id + secret_key + timestamp`; IV = 16 byte pertama kunci;
/// ciphertext = base64. Plaintext hasil dekripsi masih dikompresi
/// LZString (`compressToEncodedURIComponent`) di server BPJS asli, jadi
/// setelah AES: coba parse JSON langsung dulu (kompatibel dengan stub
/// pengujian), bila gagal coba dekompresi LZString dulu baru parse.
/// Mengembalikan JSON hasil dekripsi.
pub fn decrypt_response(
    ciphertext_b64: &str,
    cons_id: &str,
    secret_key: &str,
    timestamp_secs: u64,
) -> Result<Value, BridgeError> {
    let key_material = format!("{cons_id}{secret_key}{timestamp_secs}");
    let digest = Sha256::digest(key_material.as_bytes());
    let key: [u8; 32] = digest.as_slice().try_into().expect("SHA-256 selalu 32 byte");
    let iv: [u8; 16] = digest[..16].try_into().expect("16 byte pertama kunci");
    let cipher_bytes = base64::Engine::decode(
        &base64::engine::general_purpose::STANDARD,
        ciphertext_b64.trim(),
    )
    .map_err(|_| {
        BridgeError::Unexpected("respons BPJS terenkripsi bukan base64 yang valid".to_string())
    })?;
    let plaintext = Decryptor::<Aes256>::new(&key.into(), &iv.into())
        .decrypt_padded_vec::<Pkcs7>(&cipher_bytes)
        .map_err(|_| {
            BridgeError::Unexpected(
                "gagal mendekripsi respons BPJS (kunci/timestamp tidak cocok)".to_string(),
            )
        })?;
    let text = String::from_utf8(plaintext).map_err(|_| {
        BridgeError::Unexpected("plaintext respons BPJS bukan UTF-8".to_string())
    })?;
    if let Ok(v) = serde_json::from_str::<Value>(&text) {
        return Ok(v);
    }
    // Server BPJS asli mengembalikan plaintext terkompresi LZString.
    if let Some(decompressed) = crate::lzstring::decompress_from_encoded_uri_component(&text) {
        if let Ok(v) = serde_json::from_str::<Value>(&decompressed) {
            return Ok(v);
        }
    }
    Err(BridgeError::Unexpected(
        "plaintext respons BPJS bukan JSON".to_string(),
    ))
}

/// Bila `response` envelope berupa string, dekripsi dulu; bila sudah
/// objek/array/null, kembalikan apa adanya.
fn decrypt_if_needed(
    response: Option<Value>,
    config: &BpjsConfig,
    timestamp_secs: u64,
) -> Result<Option<Value>, BridgeError> {
    match response {
        Some(Value::String(ciphertext)) => Ok(Some(decrypt_response(
            &ciphertext,
            &config.cons_id,
            &config.secret_key,
            timestamp_secs,
        )?)),
        other => Ok(other),
    }
}

/// Petakan envelope BPJS ke isi `response` atau error.
/// `code == "200"` = sukses. Selain itu: "tidak ditemukan" (atau 404) ->
/// `Validation`; 4xx lain -> `Rejected`; 5xx -> `Http`.
fn envelope_response(body: &Value) -> Result<Option<Value>, BridgeError> {
    let meta = body
        .get("metaData")
        .ok_or_else(|| BridgeError::Unexpected("respons BPJS tanpa metaData".to_string()))?;
    let code = meta.get("code").and_then(|c| c.as_str()).unwrap_or("");
    let message = meta
        .get("message")
        .and_then(|m| m.as_str())
        .unwrap_or("")
        .to_string();
    if code == "200" {
        return Ok(body.get("response").cloned());
    }
    let lower = message.to_lowercase();
    if code == "404" || lower.contains("tidak ditemukan") || lower.contains("not found") {
        return Err(BridgeError::Validation(if message.is_empty() {
            format!("BPJS: data tidak ditemukan (code {code})")
        } else {
            message
        }));
    }
    if code.starts_with('4') {
        return Err(BridgeError::Rejected {
            status: code.parse().unwrap_or(400),
            body: message,
        });
    }
    if code.starts_with('5') {
        return Err(BridgeError::Http(format!("BPJS gagal (code {code}): {message}")));
    }
    Err(BridgeError::Rejected {
        status: code.parse().unwrap_or(500),
        body: message,
    })
}

/// Pemetaan error HTTP non-envelope: 4xx -> Rejected, 5xx/429 -> Http.
fn map_http_error(status: reqwest::StatusCode, body: &str) -> BridgeError {
    let body = if body.chars().count() > 500 {
        body.chars().take(500).collect()
    } else {
        body.to_string()
    };
    let code = status.as_u16();
    if status.is_client_error() {
        BridgeError::Rejected { status: code, body }
    } else {
        BridgeError::Http(format!("BPJS gagal (HTTP {code}): {body}"))
    }
}

/// Ambil string dari `response` mengikuti salah satu jalur bertitik,
/// mis. `sep.noSep`. Mengembalikan `None` bila tidak ada yang cocok.
fn pick_str(response: &Value, paths: &[&str]) -> Option<String> {
    for path in paths {
        let mut cur = response;
        let mut ok = true;
        for part in path.split('.') {
            match cur.get(part) {
                Some(v) => cur = v,
                None => {
                    ok = false;
                    break;
                }
            }
        }
        if ok {
            if let Some(s) = cur.as_str() {
                if !s.trim().is_empty() {
                    return Some(s.to_string());
                }
            }
        }
    }
    None
}

/// Bungkus payload VClaim dalam envelope resmi:
/// `{"request": {"t_sep": {...}}}` atau `{"request": {"t_rujukan": {...}}}`.
/// Payload internal (outbox + handler) tetap datar; pembungkusan hanya
/// terjadi di sini, saat request dibangun. Sumber: dokumen bridging
/// VClaim 2.0 / README flask-bpjs.
fn vclaim_envelope(kind: &str, payload: &Value) -> Value {
    serde_json::json!({ "request": { kind: payload } })
}

#[async_trait::async_trait]
impl Bridge for BpjsBridge {
    fn destination(&self) -> OutboxDestination {
        self.destination
    }

    fn name(&self) -> &'static str {
        self.destination.label()
    }

    fn configured(&self) -> bool {
        self.config.configured()
    }

    async fn send(&self, job: &OutboxJob) -> Result<SendOutcome, BridgeError> {
        if !self.configured() {
            return Err(BridgeError::NotConfigured(format!(
                "{} belum dikonfigurasi (cek env di docs/FASE2.md)",
                self.destination.label()
            )));
        }
        // Dispatch (destination, resource_type). Path di bawah adalah pola
        // bridging yang umum; verifikasi ulang ke dokumen bridging resmi
        // saat kredensial live tersedia (lihat docs modul).
        match (self.destination, job.resource_type.as_str()) {
            (OutboxDestination::VClaim, "SEP") => {
                let body = vclaim_envelope("t_sep", &job.payload);
                let resp = self
                    .call(Method::POST, "SEP/2.0/insert", Some(&body))
                    .await?;
                let response = resp.unwrap_or(Value::Null);
                Ok(SendOutcome {
                    external_id: pick_str(&response, &["sep.noSep", "noSep"]),
                    status: OutboxStatus::Acked,
                    note: None,
                })
            }
            (OutboxDestination::VClaim, "SEP Batal") => {
                let no_sep = job.payload["noSep"]
                    .as_str()
                    .or_else(|| job.payload["sep"]["noSep"].as_str())
                    .map(|s| s.to_string());
                let body = vclaim_envelope("t_sep", &job.payload);
                self.call(Method::DELETE, "SEP/delete", Some(&body))
                    .await?;
                Ok(SendOutcome {
                    external_id: no_sep,
                    status: OutboxStatus::Acked,
                    note: None,
                })
            }
            (OutboxDestination::VClaim, "Peserta") => {
                let nik = job.payload["nik"]
                    .as_str()
                    .or_else(|| job.payload["noKartu"].as_str())
                    .ok_or_else(|| {
                        BridgeError::Validation("Peserta: field 'nik' wajib diisi".to_string())
                    })?;
                let tanggal = job.payload["tanggal"]
                    .as_str()
                    .unwrap_or("2026-11-19")
                    .to_string();
                let resp = self
                    .call(
                        Method::GET,
                        &format!("Peserta/nik/{nik}/tglSEP/{tanggal}"),
                        None,
                    )
                    .await?;
                let response = resp.unwrap_or(Value::Null);
                Ok(SendOutcome {
                    external_id: pick_str(&response, &["peserta.noKartu", "noKartu"]),
                    status: OutboxStatus::Acked,
                    note: None,
                })
            }
            (OutboxDestination::VClaim, "Rujukan") => {
                let body = vclaim_envelope("t_rujukan", &job.payload);
                let resp = self
                    .call(Method::POST, "Rujukan/insert", Some(&body))
                    .await?;
                let response = resp.unwrap_or(Value::Null);
                Ok(SendOutcome {
                    external_id: pick_str(
                        &response,
                        &["rujukan.noKunjungan", "rujukan.noRujukan", "noKunjungan"],
                    ),
                    status: OutboxStatus::Acked,
                    note: None,
                })
            }
            (OutboxDestination::PCare, "Kunjungan") => {
                let resp = self
                    .call(Method::POST, "kunjungan", Some(&job.payload))
                    .await?;
                let response = resp.unwrap_or(Value::Null);
                Ok(SendOutcome {
                    external_id: pick_str(&response, &["noKunjungan"]),
                    status: OutboxStatus::Acked,
                    note: None,
                })
            }
            (OutboxDestination::PCare, "Peserta") => {
                let no_kartu = job.payload["noKartu"]
                    .as_str()
                    .or_else(|| job.payload["nik"].as_str())
                    .ok_or_else(|| {
                        BridgeError::Validation("Peserta: field 'noKartu' wajib diisi".to_string())
                    })?;
                let resp = self
                    .call(Method::GET, &format!("peserta/{no_kartu}"), None)
                    .await?;
                let response = resp.unwrap_or(Value::Null);
                Ok(SendOutcome {
                    external_id: pick_str(&response, &["noKartu"])
                        .or_else(|| Some(no_kartu.to_string())),
                    status: OutboxStatus::Acked,
                    note: None,
                })
            }
            (OutboxDestination::AntreanOnline, "Antrean") => {
                let resp = self
                    .call(Method::POST, "antrean/add", Some(&job.payload))
                    .await?;
                let response = resp.unwrap_or(Value::Null);
                Ok(SendOutcome {
                    external_id: pick_str(&response, &["kodebooking"]),
                    status: OutboxStatus::Acked,
                    note: None,
                })
            }
            (OutboxDestination::AntreanOnline, "Antrean Batal") => {
                let kode = job.payload["kodebooking"]
                    .as_str()
                    .map(|s| s.to_string());
                let resp = self
                    .call(Method::POST, "antrean/batal", Some(&job.payload))
                    .await?;
                let response = resp.unwrap_or(Value::Null);
                Ok(SendOutcome {
                    external_id: pick_str(&response, &["kodebooking"]).or(kode),
                    status: OutboxStatus::Acked,
                    note: None,
                })
            }
            (OutboxDestination::Aplicares, "Bed") => {
                // Refresh cache ketersediaan bed: GET tanpa efek samping.
                // Endpoint resmi: GET aplicaresws/rest/bed/read/{kodeppk}/
                // {start}/{limit} di host new-api.bpjs-kesehatan.go.id.
                // Daftar bed dikembalikan di `note` untuk disimpan worker.
                let kode_ppk = self.config.kode_ppk.trim();
                if kode_ppk.is_empty() {
                    return Err(BridgeError::Validation(
                        "Aplicares: BPJS_KODE_PPK belum diisi (kode PPK faskes)".to_string(),
                    ));
                }
                let resp = self
                    .call(
                        Method::GET,
                        &format!("rest/bed/read/{kode_ppk}/1/100"),
                        None,
                    )
                    .await?;
                let response = resp.unwrap_or(Value::Null);
                Ok(SendOutcome {
                    external_id: None,
                    status: OutboxStatus::Sent,
                    note: Some(response.to_string()),
                })
            }
            (OutboxDestination::EKlaim, "Klaim") => {
                let resp = self
                    .call(Method::POST, "klaim/submit", Some(&job.payload))
                    .await?;
                let response = resp.unwrap_or(Value::Null);
                Ok(SendOutcome {
                    external_id: pick_str(&response, &["claim_id", "noKlaim", "id"]),
                    status: OutboxStatus::Acked,
                    note: None,
                })
            }
            (dest, rt) => Err(BridgeError::Validation(format!(
                "{}: resource_type '{rt}' tidak didukung",
                dest.label()
            ))),
        }
    }

    async fn health(&self) -> BridgeHealth {
        if !self.configured() {
            BridgeHealth::BelumDikonfigurasi
        } else {
            // Tidak ada endpoint probe tanpa efek samping yang dijamin ada
            // di semua layanan bridging; status live dinilai dari hasil
            // kirim outbox (agen C), bukan dari sini.
            BridgeHealth::Offline
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::extract::State;
    use axum::http::{HeaderMap, Method as HttpMethod, StatusCode};
    use axum::routing::any;
    use axum::{Json, Router};
    use cbc::Encryptor;
    use cipher::{block_padding::Pkcs7, BlockModeEncrypt, KeyIvInit};
    use serde_json::json;
    use std::sync::Arc;
    use tokio::net::TcpListener;
    use waras_core::bridge::OutboxStatus;

    const CONS_ID: &str = "12345";
    const SECRET: &str = "CONTOH-SECRET-KEY";
    const USER_KEY: &str = "CONTOH-USER-KEY";

    // -- Stub BPJS ----------------------------------------------------------

    #[derive(Clone, Default)]
    struct StubState {
        inner: Arc<std::sync::Mutex<StubInner>>,
    }

    #[derive(Default)]
    struct StubInner {
        requests: Vec<Recorded>,
        /// Kembalikan HTTP status ini sebanyak `fail_times` panggilan pertama.
        http_fail_status: Option<u16>,
        fail_times: usize,
        /// Timpa envelope sukses dengan (code, message) ini.
        envelope_override: Option<(String, String)>,
        /// Enkripsi isi `response` dengan timestamp dari header request.
        encrypt_responses: bool,
    }

    #[derive(Clone, Debug)]
    struct Recorded {
        method: String,
        path: String,
        headers: Vec<(String, String)>,
        body: Value,
    }

    impl StubState {
        fn requests(&self) -> Vec<Recorded> {
            self.inner.lock().unwrap().requests.clone()
        }
    }

    /// Enkripsi untuk stub/pengujian: AES-256-CBC, kunci = SHA-256 mentah
    /// dari `cons_id + secret_key + timestamp`, IV = 16 byte pertama kunci.
    fn encrypt_for_test(plaintext: &str, cons_id: &str, secret_key: &str, timestamp: u64) -> String {
        let digest = Sha256::digest(format!("{cons_id}{secret_key}{timestamp}").as_bytes());
        let key: [u8; 32] = digest.as_slice().try_into().unwrap();
        let iv: [u8; 16] = digest[..16].try_into().unwrap();
        let ct = Encryptor::<Aes256>::new(&key.into(), &iv.into())
            .encrypt_padded_vec::<Pkcs7>(plaintext.as_bytes());
        base64::Engine::encode(&base64::engine::general_purpose::STANDARD, ct)
    }

    /// Respons kaleng per path (bentuk `response` envelope BPJS).
    fn canned_response(path: &str, body: &Value) -> Value {
        if path.contains("SEP/2.0/insert") {
            json!({"sep": {"noSep": "SEP-CONTOH-001"}})
        } else if path.contains("SEP/delete") {
            json!({"noSep": body["request"]["t_sep"]["noSep"].as_str().unwrap_or("SEP-CONTOH-001")})
        } else if path.contains("Peserta/nik/") {
            json!({"peserta": {"noKartu": "0001234567890", "nama": "Siti Rahmawati"}})
        } else if path.contains("Rujukan/insert") {
            json!({"rujukan": {"noKunjungan": "0301R0010126B000001"}})
        } else if path.contains("kunjungan") {
            json!({"noKunjungan": "KUNJ-CONTOH-001"})
        } else if path.contains("peserta/") {
            json!({"noKartu": "0001234567890", "nama": "Siti Rahmawati"})
        } else if path.contains("antrean/add") || path.contains("antrean/batal") {
            json!({"kodebooking": "JKN-CONTOH-001"})
        } else if path.contains("rest/bed/read/") {
            // Bentuk respons Aplicares asli: field huruf kecil, kapasitas
            // = total tempat tidur kelas itu.
            json!({"list": [
                {"kodekelas": "1", "koderuang": "R001", "namaruang": "Mawar", "kapasitas": 16, "tersedia": 12, "tersediapria": 6, "tersediawanita": 6, "tersediapriawanita": 0},
                {"kodekelas": "2", "koderuang": "R002", "namaruang": "Melati", "kapasitas": 32, "tersedia": 24, "tersediapria": 12, "tersediawanita": 12, "tersediapriawanita": 0}
            ]})
        } else if path.contains("klaim/submit") {
            json!({"claim_id": "CLM-CONTOH-001"})
        } else {
            json!({})
        }
    }

    async fn bpjs_handler(
        State(state): State<StubState>,
        method: HttpMethod,
        axum::extract::Path(path): axum::extract::Path<String>,
        headers: HeaderMap,
        body: axum::body::Bytes,
    ) -> (StatusCode, Json<Value>) {
        let body_json: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
        let recorded_headers: Vec<(String, String)> = headers
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_str().unwrap_or("").to_string()))
            .collect();
        let mut inner = state.inner.lock().unwrap();
        inner.requests.push(Recorded {
            method: method.to_string(),
            path: path.clone(),
            headers: recorded_headers.clone(),
            body: body_json.clone(),
        });
        if inner.fail_times > 0 {
            inner.fail_times -= 1;
            let status = inner.http_fail_status.unwrap_or(500);
            return (
                StatusCode::from_u16(status).unwrap(),
                Json(json!({"metaData": {"code": status.to_string(), "message": format!("stub gagal {status}")}, "response": null})),
            );
        }
        let (code, message) = inner
            .envelope_override
            .clone()
            .unwrap_or_else(|| ("200".to_string(), "OK".to_string()));
        let response = canned_response(&path, &body_json);
        let response = if inner.encrypt_responses {
            let ts: u64 = headers
                .get("x-timestamp")
                .and_then(|v| v.to_str().ok())
                .and_then(|s| s.parse().ok())
                .unwrap_or(0);
            let ct = encrypt_for_test(&response.to_string(), CONS_ID, SECRET, ts);
            Value::String(ct)
        } else {
            response
        };
        (
            StatusCode::OK,
            Json(json!({"metaData": {"code": code, "message": message}, "response": response})),
        )
    }

    /// Jalankan stub di port ephemeral; semua base URL layanan menunjuk ke
    /// server yang sama dengan prefix berbeda. Mengembalikan (state, config).
    async fn spawn_stub() -> (StubState, BpjsConfig) {
        let state = StubState::default();
        let app = Router::new()
            .route("/{*path}", any(bpjs_handler))
            .with_state(state.clone());
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let config = BpjsConfig {
            enabled: true,
            cons_id: CONS_ID.to_string(),
            secret_key: SECRET.to_string(),
            user_key: USER_KEY.to_string(),
            vclaim_url: format!("http://{addr}/vclaim-rest/"),
            pcare_url: format!("http://{addr}/pcare-rest-v3.0/"),
            antrean_url: format!("http://{addr}/antrean-rs/"),
            aplicares_url: format!("http://{addr}/aplicaresws/"),
            kode_ppk: "1234567".to_string(),
            eklaim_url: format!("http://{addr}/eklaim/"),
        };
        (state, config)
    }

    fn bridge(dest: OutboxDestination, config: &BpjsConfig) -> BpjsBridge {
        BpjsBridge::new(dest, config.clone(), crate::build_http_client())
    }

    fn job(dest: OutboxDestination, resource_type: &str, payload: Value) -> OutboxJob {
        OutboxJob {
            id: "OBX-000002".to_string(),
            destination: dest,
            resource_type: resource_type.to_string(),
            resource_id: "SEP-20261119-001".to_string(),
            idempotency_key: format!("vclaim:{resource_type}:SEP-20261119-001:created"),
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

    fn header<'a>(req: &'a Recorded, name: &str) -> Option<&'a str> {
        req.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    #[test]
    fn signature_bisa_dihitung_ulang() {
        let (cons, ts, sig, user) = bpjs_headers("12345", "SECRET_CONTOH", "USER_CONTOH", 1700000000);
        assert_eq!(cons, "12345");
        assert_eq!(ts, "1700000000");
        assert_eq!(user, "USER_CONTOH");
        // Verifikasi independen: HMAC-SHA256("12345&1700000000", "SECRET_CONTOH").
        let mut mac = HmacSha256::new_from_slice(b"SECRET_CONTOH").unwrap();
        mac.update(b"12345&1700000000");
        let expected = base64::Engine::encode(
            &base64::engine::general_purpose::STANDARD,
            mac.finalize().into_bytes(),
        );
        assert_eq!(sig, expected);
    }

    #[test]
    fn dekripsi_round_trip_aes256cbc() {
        let plaintext = json!({"sep": {"noSep": "SEP-CONTOH-001"}}).to_string();
        let ct = encrypt_for_test(&plaintext, CONS_ID, SECRET, 1700000000);
        let back = decrypt_response(&ct, CONS_ID, SECRET, 1700000000).unwrap();
        assert_eq!(back["sep"]["noSep"], "SEP-CONTOH-001");
        // Timestamp lain -> kunci lain -> dekripsi gagal.
        assert!(decrypt_response(&ct, CONS_ID, SECRET, 1700000001).is_err());
        // Secret salah -> gagal.
        assert!(decrypt_response(&ct, CONS_ID, "SECRET-SALAH", 1700000000).is_err());
    }

    #[test]
    fn dekripsi_lzstring_setelah_aes() {
        // Plaintext hasil dekripsi AES di server BPJS asli masih
        // terkompresi LZString; vektor ini dihasilkan paket lz-string
        // 1.5.0 asli (lihat crates/waras-bridge/src/lzstring.rs).
        let compressed = "N4IgzgpgDiBcoDsD2BlacQoKIAUC0AwgPIByAKkQBJ4AMNAjCAL5NA";
        let ct = encrypt_for_test(compressed, CONS_ID, SECRET, 1700000000);
        let back = decrypt_response(&ct, CONS_ID, SECRET, 1700000000).unwrap();
        assert_eq!(back["sep"]["noSep"], "SEP-CONTOH-001");
    }

    #[test]
    fn envelope_200_ok_non200_dipetakan() {
        let ok = json!({"metaData": {"code": "200", "message": "OK"}, "response": {"a": 1}});
        assert_eq!(envelope_response(&ok).unwrap().unwrap()["a"], 1);
        let not_found =
            json!({"metaData": {"code": "404", "message": "NIK tidak ditemukan"}, "response": null});
        assert!(matches!(
            envelope_response(&not_found).unwrap_err(),
            BridgeError::Validation(_)
        ));
        let bad = json!({"metaData": {"code": "400", "message": "Data tidak valid"}, "response": null});
        match envelope_response(&bad).unwrap_err() {
            BridgeError::Rejected { status, .. } => assert_eq!(status, 400),
            other => panic!("400 -> Rejected, dapat {other:?}"),
        }
        let server =
            json!({"metaData": {"code": "500", "message": "Internal error"}, "response": null});
        assert!(matches!(
            envelope_response(&server).unwrap_err(),
            BridgeError::Http(_)
        ));
    }

    #[tokio::test]
    async fn sep_insert_header_tanda_tangan_dan_external_id() {
        let (state, config) = spawn_stub().await;
        let b = bridge(OutboxDestination::VClaim, &config);
        let out = b
            .send(&job(
                OutboxDestination::VClaim,
                "SEP",
                json!({"noKartu": "0001234567890", "tglSep": "2026-11-19"}),
            ))
            .await
            .unwrap();
        assert_eq!(out.status, OutboxStatus::Acked);
        assert_eq!(out.external_id.as_deref(), Some("SEP-CONTOH-001"));
        let req = &state.requests()[0];
        assert_eq!(req.method, "POST");
        assert!(req.path.contains("SEP/2.0/insert"), "path: {}", req.path);
        assert_eq!(req.body["request"]["t_sep"]["noKartu"], "0001234567890", "body SEP ikut terekam dalam envelope t_sep");
        assert_eq!(header(req, "x-cons-id"), Some(CONS_ID));
        assert_eq!(header(req, "user_key"), Some(USER_KEY));
        // Signature bisa dihitung ulang dari timestamp yang tercatat.
        let ts: u64 = header(req, "x-timestamp").unwrap().parse().unwrap();
        let (_, _, expected_sig, _) = bpjs_headers(CONS_ID, SECRET, USER_KEY, ts);
        assert_eq!(header(req, "x-signature"), Some(expected_sig.as_str()));
    }

    #[tokio::test]
    async fn peserta_eligibilitas_get_tanpa_body() {
        let (state, config) = spawn_stub().await;
        let b = bridge(OutboxDestination::VClaim, &config);
        let out = b
            .send(&job(
                OutboxDestination::VClaim,
                "Peserta",
                json!({"nik": "3374021205850001", "tanggal": "2026-11-19"}),
            ))
            .await
            .unwrap();
        assert_eq!(out.status, OutboxStatus::Acked);
        assert_eq!(out.external_id.as_deref(), Some("0001234567890"));
        let req = &state.requests()[0];
        assert_eq!(req.method, "GET");
        assert!(
            req.path.contains("Peserta/nik/3374021205850001/tglSEP/2026-11-19"),
            "path: {}",
            req.path
        );
    }

    #[tokio::test]
    async fn dispatch_semua_layanan() {
        let (_state, config) = spawn_stub().await;
        let cases = vec![
            (OutboxDestination::VClaim, "SEP Batal", json!({"noSep": "SEP-CONTOH-001"}), Some("SEP-CONTOH-001")),
            (OutboxDestination::VClaim, "Rujukan", json!({"noKartu": "0001234567890"}), Some("0301R0010126B000001")),
            (OutboxDestination::PCare, "Kunjungan", json!({"noKartu": "0001234567890"}), Some("KUNJ-CONTOH-001")),
            (OutboxDestination::PCare, "Peserta", json!({"noKartu": "0001234567890"}), Some("0001234567890")),
            (OutboxDestination::AntreanOnline, "Antrean", json!({"kodebooking": "JKN-881201"}), Some("JKN-CONTOH-001")),
            (OutboxDestination::AntreanOnline, "Antrean Batal", json!({"kodebooking": "JKN-881201"}), Some("JKN-CONTOH-001")),
            (OutboxDestination::EKlaim, "Klaim", json!({"invoice_id": "INV-2026-1181"}), Some("CLM-CONTOH-001")),
        ];
        for (dest, rt, payload, expected_id) in cases {
            let b = bridge(dest, &config);
            let out = b.send(&job(dest, rt, payload)).await.unwrap();
            assert_eq!(out.status, OutboxStatus::Acked, "{rt} harus Acked");
            assert_eq!(out.external_id.as_deref(), expected_id, "{rt} external_id");
        }
    }

    #[tokio::test]
    async fn bed_refresh_mengembalikan_daftar_di_note() {
        let (state, config) = spawn_stub().await;
        let b = bridge(OutboxDestination::Aplicares, &config);
        let out = b
            .send(&job(OutboxDestination::Aplicares, "Bed", json!({})))
            .await
            .unwrap();
        assert_eq!(out.status, OutboxStatus::Sent);
        assert!(out.external_id.is_none());
        let req = &state.requests()[0];
        assert_eq!(req.method, "GET");
        assert!(
            req.path.contains("rest/bed/read/1234567/1/100"),
            "path: {}",
            req.path
        );
        let note = out.note.expect("note harus berisi daftar bed");
        let beds: Value = serde_json::from_str(&note).unwrap();
        assert_eq!(beds["list"][0]["kodekelas"], "1");
        assert_eq!(beds["list"][0]["kapasitas"], 16);
        assert_eq!(beds["list"][0]["tersedia"], 12);
    }

    #[tokio::test]
    async fn bed_tanpa_kode_ppk_ditolak_validasi() {
        let (_state, config) = spawn_stub().await;
        let mut config = config.clone();
        config.kode_ppk = String::new();
        let b = bridge(OutboxDestination::Aplicares, &config);
        let err = b
            .send(&job(OutboxDestination::Aplicares, "Bed", json!({})))
            .await
            .unwrap_err();
        assert!(
            matches!(err, BridgeError::Validation(_)),
            "dapat {err:?}"
        );
    }

    #[tokio::test]
    async fn respons_terenkripsi_didekripsi_end_to_end() {
        let (state, config) = spawn_stub().await;
        state.inner.lock().unwrap().encrypt_responses = true;
        let b = bridge(OutboxDestination::VClaim, &config);
        let out = b
            .send(&job(
                OutboxDestination::VClaim,
                "SEP",
                json!({"noKartu": "0001234567890"}),
            ))
            .await
            .unwrap();
        assert_eq!(out.status, OutboxStatus::Acked);
        assert_eq!(out.external_id.as_deref(), Some("SEP-CONTOH-001"));
    }

    #[tokio::test]
    async fn envelope_non200_dan_http_500_dipetakan() {
        // Envelope 404 "NIK tidak ditemukan" -> Validation.
        let (state, config) = spawn_stub().await;
        state.inner.lock().unwrap().envelope_override =
            Some(("404".to_string(), "NIK tidak ditemukan".to_string()));
        let b = bridge(OutboxDestination::VClaim, &config);
        let err = b
            .send(&job(
                OutboxDestination::VClaim,
                "Peserta",
                json!({"nik": "3374021205850001", "tanggal": "2026-11-19"}),
            ))
            .await
            .unwrap_err();
        assert!(matches!(err, BridgeError::Validation(_)), "dapat {err:?}");

        // Envelope 400 -> Rejected.
        let (state, config) = spawn_stub().await;
        state.inner.lock().unwrap().envelope_override =
            Some(("400".to_string(), "Data tidak valid".to_string()));
        let b = bridge(OutboxDestination::VClaim, &config);
        let err = b
            .send(&job(OutboxDestination::VClaim, "SEP", json!({})))
            .await
            .unwrap_err();
        match err {
            BridgeError::Rejected { status, .. } => assert_eq!(status, 400),
            other => panic!("400 -> Rejected, dapat {other:?}"),
        }

        // HTTP 500 -> Http.
        let (state, config) = spawn_stub().await;
        {
            let mut inner = state.inner.lock().unwrap();
            inner.http_fail_status = Some(500);
            inner.fail_times = 1;
        }
        let b = bridge(OutboxDestination::VClaim, &config);
        let err = b
            .send(&job(OutboxDestination::VClaim, "SEP", json!({})))
            .await
            .unwrap_err();
        assert!(matches!(err, BridgeError::Http(_)), "500 -> Http, dapat {err:?}");
    }

    #[tokio::test]
    async fn resource_type_tidak_didukung_jadi_validation() {
        let (_state, config) = spawn_stub().await;
        let b = bridge(OutboxDestination::VClaim, &config);
        let err = b
            .send(&job(OutboxDestination::VClaim, "Bed", json!({})))
            .await
            .unwrap_err();
        assert!(matches!(err, BridgeError::Validation(_)), "dapat {err:?}");
    }

    #[tokio::test]
    async fn belum_dikonfigurasi_tidak_mengirim() {
        let mut config = spawn_stub().await.1;
        config.enabled = false;
        config.cons_id = String::new();
        let b = bridge(OutboxDestination::VClaim, &config);
        let err = b
            .send(&job(OutboxDestination::VClaim, "SEP", json!({})))
            .await
            .unwrap_err();
        assert!(matches!(err, BridgeError::NotConfigured(_)));
        assert_eq!(b.health().await, BridgeHealth::BelumDikonfigurasi);
    }
}
