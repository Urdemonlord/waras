//! WARAS server binary: one binary, SQLite clinic mode by default,
//! PostgreSQL hospital mode when `WARAS_DATABASE_URL` is set.
//!
//! Startup: open the database (SQLite file default `./waras.db`, or
//! Postgres via `WARAS_DATABASE_URL`), run embedded migrations, seed
//! demo data when the database is empty, then serve the API + UI on
//! one port.
//!
//! Configuration (environment):
//! - `WARAS_HOST` (default `127.0.0.1`)
//! - `WARAS_PORT` (default `3000`)
//! - `WARAS_DB` (default `waras.db`, relative to the working directory)
//! - `WARAS_DATABASE_URL` (bila berisi `postgres://...` -> mode
//!   PostgreSQL; bila kosong -> SQLite via `WARAS_DB`)
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

    // Backend database (FASE3.md §10): `WARAS_DATABASE_URL` berisi
    // `postgres://...` (atau `postgresql://...`) -> mode PostgreSQL
    // (rumah sakit); kosong / tidak ada -> SQLite via `WARAS_DB`.
    // Nilai terisi tapi bukan postgres://: peringatan + tetap SQLite
    // (jangan gagal boot karena salah ketik env di klinik). URL TIDAK
    // pernah di-log (bisa berisi kata sandi).
    let db_url = std::env::var("WARAS_DATABASE_URL")
        .ok()
        .map(|u| u.trim().to_string())
        .filter(|u| !u.is_empty());
    let db = match db_url.as_deref() {
        Some(url) if url.starts_with("postgres://") || url.starts_with("postgresql://") => {
            Db::connect_postgres(url)
                .await
                .expect("gagal membuka basis data PostgreSQL")
        }
        Some(_) => {
            tracing::warn!(
                "WARAS_DATABASE_URL diisi tapi bukan postgres://...; memakai SQLite"
            );
            Db::connect(&db_path)
                .await
                .expect("gagal membuka basis data SQLite")
        }
        None => Db::connect(&db_path)
            .await
            .expect("gagal membuka basis data SQLite"),
    };
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
    // Fase 3: seed tabel rumah sakit (IGD/ranap/OK/lab/radiologi/casemix)
    // bila masih kosong (DB Fase 1/2 yang sudah ada tidak menjalankan
    // `seed()` lagi).
    if db
        .seed_fase3_if_empty()
        .await
        .expect("gagal memuat data seed Fase 3")
    {
        tracing::info!("Seed Fase 3 dimuat (tabel wards masih kosong)");
    }
    // Fase 4: seed tabel ekosistem (hemodialisa/transfusi/gizi/gigi/
    // inventori/kepegawaian/pengaturan + 2 pasien baru) bila masih kosong
    // (DB Fase 1–3 yang sudah ada tidak menjalankan `seed()` lagi).
    if db
        .seed_fase4_if_empty()
        .await
        .expect("gagal memuat data seed Fase 4")
    {
        tracing::info!("Seed Fase 4 dimuat (tabel hd_machines masih kosong)");
    }
    // Fase A/A4: DB lama yang ter-seed sebelum migrasi 0007 punya
    // `patients.portal_password_hash` NULL — beri kata sandi portal
    // demo `waras123` (idempotent; DB baru sudah terisi dari seed).
    let backfilled = db
        .backfill_portal_passwords()
        .await
        .expect("gagal backfill kata sandi portal");
    if backfilled > 0 {
        tracing::info!(
            "Portal: {backfilled} pasien diberi kata sandi demo waras123 (backfill migrasi 0007)"
        );
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

    // Label basis data untuk log: URL Postgres tidak pernah dicetak
    // (bisa berisi kata sandi).
    let db_label = if db.mode() == "postgres" {
        "PostgreSQL (WARAS_DATABASE_URL)".to_string()
    } else {
        db_path.clone()
    };
    tracing::info!(
        "WARAS v{} siap — mode {}, tanggal klinik {}, basis data {}, dengarkan di http://{}",
        env!("CARGO_PKG_VERSION"),
        if db.mode() == "postgres" {
            "rumah sakit (PostgreSQL)"
        } else {
            "klinik (SQLite)"
        },
        today,
        db_label,
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

    // `into_make_service_with_connect_info`: supaya handler login
    // bisa membaca IP peer (`ConnectInfo<SocketAddr>`) untuk kunci
    // brute-force (Fase A/A4). Tanpa ini extractor ConnectInfo gagal.
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
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
