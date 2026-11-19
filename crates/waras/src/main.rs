//! WARAS server binary: one binary, SQLite clinic mode.
//!
//! Startup: open the SQLite file (default `./waras.db`), run embedded
//! migrations, seed Fase 1 demo data when the database is empty, then serve
//! the API + UI on one port.
//!
//! Configuration (environment):
//! - `WARAS_HOST` (default `127.0.0.1`)
//! - `WARAS_PORT` (default `3000`)
//! - `WARAS_DB` (default `waras.db`, relative to the working directory)
//! - `WARAS_TODAY` (default `2026-11-19`, the clinic date used for "hari ini"
//!   in seed data and dashboard KPIs)
//! - `RUST_LOG` (default `waras=info,tower_http=info`)

use std::{net::SocketAddr, sync::Arc};

use waras_api::{router, worker, AppState};
use waras_bridge::{build_registry, BridgeConfig};
use waras_db::Db;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            std::env::var("RUST_LOG").unwrap_or_else(|_| "waras=info".to_string()),
        )
        .init();

    let host = std::env::var("WARAS_HOST").unwrap_or_else(|_| "127.0.0.1".to_string());
    let port: u16 = std::env::var("WARAS_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(3000);
    let db_path = std::env::var("WARAS_DB").unwrap_or_else(|_| "waras.db".to_string());

    let db = Db::connect(&db_path)
        .await
        .expect("gagal membuka basis data SQLite");
    db.migrate().await.expect("gagal menjalankan migrasi");
    db.seed_if_empty()
        .await
        .expect("gagal memuat data seed Fase 1");
    // Fase 2: DB Fase 1 yang sudah ada tidak menjalankan `seed()` lagi,
    // jadi seed tabel Fase 2 (SEP/antrean/bed/rujukan/PCare/klaim) bila
    // masih kosong.
    if db
        .seed_fase2_if_empty()
        .await
        .expect("gagal memuat data seed Fase 2")
    {
        tracing::info!("Seed Fase 2 dimuat (tabel seps masih kosong)");
    }

    // Jembatan nasional (FASE2.md §5/§7): config dari env, registry
    // berisi adapter nyata bila terkonfigurasi, `DisabledBridge` bila
    // belum. Worker memakai Db + registry yang SAMA dengan AppState.
    let bridge_config = BridgeConfig::from_env();
    let bridges = Arc::new(build_registry(&bridge_config));
    let db = Arc::new(db);
    if bridge_config.worker_enabled {
        let worker_db = db.clone();
        let worker_bridges = bridges.clone();
        let worker_config = bridge_config.clone();
        tokio::spawn(async move {
            worker::run_worker(worker_db, worker_bridges, worker_config).await;
        });
        tracing::info!(
            "Worker outbox aktif (interval {} dtk, batch {})",
            bridge_config.worker_interval_secs,
            bridge_config.worker_batch
        );
    } else {
        tracing::info!("Worker outbox nonaktif (WARAS_WORKER_ENABLED=false)");
    }

    let today = waras_db::clinic_today();
    let addr: SocketAddr = format!("{host}:{port}")
        .parse()
        .expect("WARAS_HOST/WARAS_PORT tidak valid");

    tracing::info!(
        "WARAS v{} siap — mode klinik (SQLite), tanggal klinik {}, basis data {}, dengarkan di http://{}",
        env!("CARGO_PKG_VERSION"),
        today,
        db_path,
        addr
    );
    tracing::info!("Login seed: NIP 198505122010011001 (dr. Ahmad Hidayat) / kata sandi waras123");

    let app = router(AppState {
        db,
        bridges,
        bridge_config,
    });
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .expect("gagal bind ke alamat");

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .expect("server berhenti dengan kesalahan");
}

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("gagal memasang handler Ctrl-C");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("gagal memasang handler SIGTERM")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
    tracing::info!("Mematikan WARAS…");
}
