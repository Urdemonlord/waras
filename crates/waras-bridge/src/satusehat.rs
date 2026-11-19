//! Adapter SATUSEHAT (FHIR R4).
//!
//! Implementasi pengiriman (OAuth2 client credentials, POST resource FHIR,
//! penanganan respons + validasi profil) dikerjakan di Fase 2 oleh agen
//! bridge. Sampai saat itu `send` mengembalikan `NotConfigured` supaya
//! tidak ada yang terkirim setengah jalan.

use waras_core::bridge::{BridgeHealth, OutboxDestination, OutboxJob};

use crate::{Bridge, BridgeError, SatusehatConfig, SendOutcome};

/// Adapter SATUSEHAT. `client` dipakai untuk token + FHIR POST.
pub struct SatusehatBridge {
    config: SatusehatConfig,
    client: reqwest::Client,
}

impl SatusehatBridge {
    pub fn new(config: SatusehatConfig, client: reqwest::Client) -> Self {
        Self { config, client }
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

    async fn send(&self, _job: &OutboxJob) -> Result<SendOutcome, BridgeError> {
        // TODO(Fase 2, agen bridge): OAuth2 + POST FHIR + respons.
        Err(BridgeError::NotConfigured(
            "adapter SATUSEHAT belum diimplementasikan".to_string(),
        ))
    }

    async fn health(&self) -> BridgeHealth {
        if !self.configured() {
            BridgeHealth::BelumDikonfigurasi
        } else {
            // TODO(Fase 2, agen bridge): probe ringan (mis. token / GET metadata).
            BridgeHealth::Offline
        }
    }
}
