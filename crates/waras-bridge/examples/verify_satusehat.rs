//! Verifikasi live SATUSEHAT (read-only, manual). BUKAN bagian dari
//! `cargo test` dan TIDAK PERNAH berjalan di CI.
//!
//! Cara pakai di VPS (kredensial dibaca dari env, jangan ditempel di file):
//! ```sh
//! set -a; source /home/meowlabs/waras.env; set +a
//! WARAS_HTTP_PROXY=http://10.200.0.2:18080 \
//!   ./target/release/examples/verify_satusehat
//! ```
//! Hanya melakukan: minta token (1x, di-cache lalu dipakai ulang),
//! GET /metadata, GET Patient berdasarkan NIK dummy. TIDAK membuat,
//! mengubah, atau menghapus data apa pun di staging.
//! Output hanya status HTTP + ringkasan; token TIDAK PERNAH dicetak.
//! Endpoint token membatasi laju secara agresif: jalankan sekali, beri
//! jeda minimal 3 menit antar percobaan, jangan di-hammer.

use waras_bridge::satusehat::SatusehatBridge;
use waras_bridge::{Bridge, BridgeConfig, build_http_client};
use waras_core::bridge::BridgeHealth;

#[tokio::main]
async fn main() {
    let config = BridgeConfig::from_env();
    if !config.satusehat.configured() {
        eprintln!(
            "SATUSEHAT belum dikonfigurasi (butuh SATUSEHAT_ENABLED=true + CLIENT_ID + CLIENT_SECRET)"
        );
        std::process::exit(2);
    }
    let proxy = std::env::var("WARAS_HTTP_PROXY").unwrap_or_default();
    println!("auth_url: {}", config.satusehat.auth_url);
    println!("base_url: {}", config.satusehat.base_url);
    println!(
        "proxy: {}",
        if proxy.is_empty() {
            "langsung (tanpa proxy)".to_string()
        } else {
            proxy
        }
    );
    let bridge = SatusehatBridge::new(config.satusehat.clone(), build_http_client());

    let health = bridge.health().await;
    println!("health (minta token): {health:?}");
    if health != BridgeHealth::Online {
        eprintln!("token gagal — berhenti, jangan ulangi beruntun (rate limit)");
        std::process::exit(1);
    }

    match bridge.get_json("metadata").await {
        Ok(v) => println!(
            "GET metadata: 200 fhirVersion={}",
            v.get("fhirVersion")
                .and_then(|x| x.as_str())
                .unwrap_or("?")
        ),
        Err(e) => {
            eprintln!("GET metadata gagal: {e}");
            std::process::exit(1);
        }
    }

    match bridge
        .get_json("Patient?identifier=https://fhir.kemkes.go.id/id/nik|0000000000000000")
        .await
    {
        Ok(v) => println!(
            "GET Patient NIK dummy: 200 total={}",
            v.get("total").and_then(|x| x.as_i64()).unwrap_or(-1)
        ),
        Err(e) => {
            eprintln!("GET Patient gagal: {e}");
            std::process::exit(1);
        }
    }

    println!("VERIFIKASI LIVE OK");
}
