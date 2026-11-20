//! WARAS national bridges (Fase 2): SATUSEHAT (FHIR R4) + BPJS adapters
//! (VClaim, PCare, Antrean Online, Aplicares, E-Klaim).
//!
//! Satu adapter per tujuan (`OutboxDestination`), semua di belakang trait
//! [`Bridge`] yang sama, sehingga worker tidak tahu bedanya SATUSEHAT dan
//! BPJS. Kredensial HANYA dari env (lihat [`BridgeConfig::from_env`]);
//! tanpa kredensial, adapter berstatus `BelumDikonfigurasi` dan worker
//! membiarkan pekerjaannya Pending — tidak ada yang dikirim ke mana pun.
//!
//! Adapter nyata tinggal di `satusehat.rs` dan `bpjs.rs`; file ini hanya
//! kontrak (trait + registry + config + error) supaya crate lain
//! (`waras-api` worker, `waras-db` outbox) bisa dikompilasi tanpa menunggu
//! adapter selesai.

pub mod bpjs;
pub mod lzstring;
pub mod satusehat;

use std::collections::HashMap;
use std::sync::Arc;

use waras_core::bridge::{BridgeHealth, OutboxDestination, OutboxJob, OutboxStatus};

/// Hasil satu pengiriman adapter.
#[derive(Debug, Clone)]
pub struct SendOutcome {
    /// Id yang dikembalikan tujuan (IHS id, nomor SEP, dsb.).
    pub external_id: Option<String>,
    /// `Acked` bila tujuan mengakui; `Sent` bila HTTP 2xx tanpa id.
    pub status: OutboxStatus,
    pub note: Option<String>,
}

/// Kesalahan adapter. Pemetaan ke perilaku worker:
/// - `Validation` -> pekerjaan langsung `Dead` (tidak dicoba lagi).
/// - `NotConfigured` -> pekerjaan tetap `Pending` (tanpa menghabiskan percobaan).
/// - sisanya -> `Failed` + backoff + percobaan ulang.
#[derive(Debug, thiserror::Error)]
pub enum BridgeError {
    #[error("jembatan belum dikonfigurasi: {0}")]
    NotConfigured(String),

    #[error("HTTP: {0}")]
    Http(String),

    #[error("otentikasi gagal: {0}")]
    Auth(String),

    #[error("validasi ditolak: {0}")]
    Validation(String),

    #[error("tujuan menolak (HTTP {status}): {body}")]
    Rejected { status: u16, body: String },

    #[error("tak terduga: {0}")]
    Unexpected(String),
}

/// Satu adapter jembatan. Implementasi HARUS idempotent terhadap
/// `job.idempotency_key`: mengirim ulang job yang sama tidak boleh membuat
/// duplikat di tujuan (cek dulu bila tujuan menyediakan lookup).
#[async_trait::async_trait]
pub trait Bridge: Send + Sync {
    fn destination(&self) -> OutboxDestination;
    fn name(&self) -> &'static str;
    fn configured(&self) -> bool;
    async fn send(&self, job: &OutboxJob) -> Result<SendOutcome, BridgeError>;
    async fn health(&self) -> BridgeHealth {
        if !self.configured() {
            BridgeHealth::BelumDikonfigurasi
        } else {
            BridgeHealth::Offline
        }
    }
}

/// Adapter pengganti bila tujuan belum dikonfigurasi: tidak pernah mengirim.
pub struct DisabledBridge {
    destination: OutboxDestination,
}

impl DisabledBridge {
    pub fn new(destination: OutboxDestination) -> Self {
        Self { destination }
    }
}

#[async_trait::async_trait]
impl Bridge for DisabledBridge {
    fn destination(&self) -> OutboxDestination {
        self.destination
    }

    fn name(&self) -> &'static str {
        self.destination.label()
    }

    fn configured(&self) -> bool {
        false
    }

    async fn send(&self, _job: &OutboxJob) -> Result<SendOutcome, BridgeError> {
        Err(BridgeError::NotConfigured(format!(
            "{} belum dikonfigurasi (cek env di docs/FASE2.md)",
            self.destination.label()
        )))
    }

    async fn health(&self) -> BridgeHealth {
        BridgeHealth::BelumDikonfigurasi
    }
}

/// Konfigurasi SATUSEHAT (OAuth2 client credentials + FHIR base).
#[derive(Debug, Clone)]
pub struct SatusehatConfig {
    pub enabled: bool,
    pub base_url: String,
    pub auth_url: String,
    pub client_id: String,
    pub client_secret: String,
    pub org_id: String,
}

impl SatusehatConfig {
    pub fn configured(&self) -> bool {
        self.enabled && !self.client_id.is_empty() && !self.client_secret.is_empty()
    }
}

/// Konfigurasi BPJS (satu kredensial untuk semua layanan bridging).
#[derive(Debug, Clone)]
pub struct BpjsConfig {
    pub enabled: bool,
    pub cons_id: String,
    pub secret_key: String,
    pub user_key: String,
    pub vclaim_url: String,
    pub pcare_url: String,
    pub antrean_url: String,
    pub aplicares_url: String,
    /// Kode PPK faskes (dipakai Aplicares `bed/read/{kodeppk}/...`).
    pub kode_ppk: String,
    pub eklaim_url: String,
}

impl BpjsConfig {
    pub fn configured(&self) -> bool {
        self.enabled
            && !self.cons_id.is_empty()
            && !self.secret_key.is_empty()
            && !self.user_key.is_empty()
    }
}

/// Seluruh konfigurasi jembatan + worker, dibaca dari env.
#[derive(Debug, Clone)]
pub struct BridgeConfig {
    pub satusehat: SatusehatConfig,
    pub bpjs: BpjsConfig,
    pub worker_enabled: bool,
    pub worker_interval_secs: u64,
    pub worker_batch: usize,
    pub outbox_max_attempts: i64,
}

fn env_or(key: &str, default: &str) -> String {
    std::env::var(key)
        .map(|v| v.trim().to_string())
        .unwrap_or_else(|_| default.to_string())
}

fn env_bool(key: &str, default: bool) -> bool {
    std::env::var(key)
        .map(|v| v.trim().eq_ignore_ascii_case("true") || v.trim() == "1")
        .unwrap_or(default)
}

fn env_usize(key: &str, default: usize) -> usize {
    std::env::var(key)
        .ok()
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or(default)
}

impl BridgeConfig {
    /// Baca dari env. TIDAK ada kredensial default: string kosong berarti
    /// belum dikonfigurasi. Contoh nilai (PALSU) ada di README/FASE2.md.
    pub fn from_env() -> Self {
        Self {
            satusehat: SatusehatConfig {
                enabled: env_bool("SATUSEHAT_ENABLED", false),
                base_url: env_or(
                    "SATUSEHAT_BASE_URL",
                    "https://api-satusehat.kemkes.go.id/fhir-r4/v1",
                ),
                auth_url: env_or(
                    "SATUSEHAT_AUTH_URL",
                    "https://api-satusehat.kemkes.go.id/oauth2/v1",
                ),
                client_id: env_or("SATUSEHAT_CLIENT_ID", ""),
                client_secret: env_or("SATUSEHAT_CLIENT_SECRET", ""),
                org_id: env_or("SATUSEHAT_ORG_ID", ""),
            },
            bpjs: BpjsConfig {
                enabled: env_bool("BPJS_ENABLED", false),
                cons_id: env_or("BPJS_CONS_ID", ""),
                secret_key: env_or("BPJS_SECRET_KEY", ""),
                user_key: env_or("BPJS_USER_KEY", ""),
                vclaim_url: env_or(
                    "BPJS_VCLAIM_URL",
                    "https://apijkn.bpjs-kesehatan.go.id/vclaim-rest/",
                ),
                pcare_url: env_or(
                    "BPJS_PCARE_URL",
                    "https://apijkn.bpjs-kesehatan.go.id/pcare-rest-v3.0/",
                ),
                antrean_url: env_or(
                    "BPJS_ANTREAN_URL",
                    "https://apijkn.bpjs-kesehatan.go.id/antrean-rs/",
                ),
                aplicares_url: env_or(
                    "BPJS_APLICARES_URL",
                    "https://new-api.bpjs-kesehatan.go.id/aplicaresws/",
                ),
                kode_ppk: env_or("BPJS_KODE_PPK", ""),
                eklaim_url: env_or(
                    "BPJS_EKLAIM_URL",
                    "https://apijkn.bpjs-kesehatan.go.id/eklaim/",
                ),
            },
            worker_enabled: env_bool("WARAS_WORKER_ENABLED", true),
            worker_interval_secs: env_usize("WARAS_WORKER_INTERVAL_SECS", 10) as u64,
            worker_batch: env_usize("WARAS_WORKER_BATCH", 10),
            outbox_max_attempts: env_usize("WARAS_OUTBOX_MAX_ATTEMPTS", 5) as i64,
        }
    }
}

/// Kumpulan adapter yang dipakai worker. Tujuan yang belum dikonfigurasi
/// tetap terdaftar sebagai [`DisabledBridge`] supaya statusnya bisa
/// dilaporkan (`BelumDikonfigurasi`) dan pekerjaannya tetap Pending.
pub struct BridgeRegistry {
    bridges: HashMap<OutboxDestination, Arc<dyn Bridge>>,
}

impl BridgeRegistry {
    pub fn new() -> Self {
        Self {
            bridges: HashMap::new(),
        }
    }

    pub fn register(&mut self, bridge: Arc<dyn Bridge>) {
        self.bridges.insert(bridge.destination(), bridge);
    }

    pub fn get(&self, dest: OutboxDestination) -> Option<Arc<dyn Bridge>> {
        self.bridges.get(&dest).cloned()
    }

    /// Semua tujuan yang terdaftar.
    pub fn destinations(&self) -> Vec<OutboxDestination> {
        let mut v: Vec<_> = self.bridges.keys().copied().collect();
        v.sort_by_key(|d| *d as u8);
        v
    }

    /// Status kesehatan semua tujuan (termasuk yang belum dikonfigurasi).
    pub async fn health_all(&self) -> Vec<(OutboxDestination, BridgeHealth)> {
        let mut out = Vec::new();
        for dest in OutboxDestination::all() {
            let h = match self.get(dest) {
                Some(b) => b.health().await,
                None => BridgeHealth::BelumDikonfigurasi,
            };
            out.push((dest, h));
        }
        out
    }
}

/// Bangun registry dari config. Adapter nyata dibuat oleh
/// `satusehat::build` / `bpjs::build_*` bila `configured()`; selebihnya
/// `DisabledBridge`. Dipakai binary `waras` saat boot.
pub fn build_registry(config: &BridgeConfig) -> BridgeRegistry {
    let client = build_http_client();
    let mut reg = BridgeRegistry::new();
    if config.satusehat.configured() {
        reg.register(Arc::new(satusehat::SatusehatBridge::new(
            config.satusehat.clone(),
            client.clone(),
        )));
    } else {
        reg.register(Arc::new(DisabledBridge::new(
            OutboxDestination::Satusehat,
        )));
    }
    for dest in [
        OutboxDestination::VClaim,
        OutboxDestination::PCare,
        OutboxDestination::AntreanOnline,
        OutboxDestination::Aplicares,
        OutboxDestination::EKlaim,
    ] {
        if config.bpjs.configured() {
            reg.register(Arc::new(bpjs::BpjsBridge::new(
                dest,
                config.bpjs.clone(),
                client.clone(),
            )));
        } else {
            reg.register(Arc::new(DisabledBridge::new(dest)));
        }
    }
    reg
}

/// Klien HTTP bersama: rustls, timeout 30 detik, UA WARAS.
///
/// Proxy: bila env `WARAS_HTTP_PROXY` diisi (mis. `http://10.200.0.2:18080`),
/// semua request bridge lewat proxy itu (eksplisit mengalahkan proxy env
/// sistem). Bila kosong, reqwest memakai proxy dari env sistem
/// (`HTTPS_PROXY`/`HTTP_PROXY`/`NO_PROXY`) atau koneksi langsung.
/// Default: langsung (tanpa proxy). Proxy TIDAK pernah di-hardcode:
/// sebagian deployment (mis. VPS yang IP-nya diblokir edge SATUSEHAT)
/// butuh proxy, sebagian tidak.
pub fn build_http_client() -> reqwest::Client {
    let proxy = std::env::var("WARAS_HTTP_PROXY")
        .ok()
        .filter(|v| !v.trim().is_empty());
    build_http_client_with_proxy(proxy.as_deref())
}

/// Seperti [`build_http_client`], tapi proxy ditentukan eksplisit (dipakai
/// pengujian; `None` = perilaku default reqwest terhadap env sistem).
pub fn build_http_client_with_proxy(proxy_url: Option<&str>) -> reqwest::Client {
    let mut builder = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .user_agent(concat!("WARAS/", env!("CARGO_PKG_VERSION")));
    if let Some(url) = proxy_url {
        let proxy = reqwest::Proxy::all(url)
            .unwrap_or_else(|e| panic!("WARAS_HTTP_PROXY tidak valid ({url}): {e}"));
        builder = builder.proxy(proxy);
    }
    builder.build().expect("gagal membangun klien HTTP bridge")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn klien_http_tanpa_proxy_bisa_dibangun() {
        let _client = build_http_client_with_proxy(None);
    }

    #[test]
    fn klien_http_dengan_proxy_valid_bisa_dibangun() {
        let _client = build_http_client_with_proxy(Some("http://127.0.0.1:18080"));
    }

    #[test]
    #[should_panic(expected = "WARAS_HTTP_PROXY tidak valid")]
    fn klien_http_dengan_proxy_tidak_valid_panik() {
        let _client = build_http_client_with_proxy(Some("://bukan-url"));
    }
}
