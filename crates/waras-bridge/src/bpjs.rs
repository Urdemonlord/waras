//! Adapter BPJS Kesehatan: satu struct untuk lima layanan bridging
//! (VClaim, PCare, Antrean Online, Aplicares, E-Klaim), dibedakan oleh
//! `destination`. Header tanda tangan BPJS (X-cons-id, X-timestamp,
//! X-signature HMAC-SHA256, user_key) dibangun di sini, dipakai semua
//! layanan.
//!
//! Implementasi pemanggilan tiap layanan dikerjakan di Fase 2 oleh agen
//! bridge. Sampai saat itu `send` mengembalikan `NotConfigured` supaya
//! tidak ada yang terkirim setengah jalan.

use hmac::{Hmac, Mac};
use sha2::Sha256;

use waras_core::bridge::{BridgeHealth, OutboxDestination, OutboxJob};

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

    async fn send(&self, _job: &OutboxJob) -> Result<SendOutcome, BridgeError> {
        // TODO(Fase 2, agen bridge): dispatch per resource_type ke endpoint
        // layanan ini (SEP, peserta, kunjungan, antrean, bed, klaim).
        Err(BridgeError::NotConfigured(format!(
            "adapter {} belum diimplementasikan",
            self.destination.label()
        )))
    }

    async fn health(&self) -> BridgeHealth {
        if !self.configured() {
            BridgeHealth::BelumDikonfigurasi
        } else {
            // TODO(Fase 2, agen bridge): probe ringan bila layanan menyediakan.
            BridgeHealth::Offline
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
