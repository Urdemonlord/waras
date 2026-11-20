//! Seed data — KONTRAK §5, persis. Semua data sintetis (didokumentasikan
//! di kontrak). Password semua user: `waras123` (di-hash Argon2 saat seed).
//!
//! Penyimpangan yang didokumentasikan di kontrak (§5.8) dipakai di sini:
//! 44 entri antrean (bukan 42), sehingga KPI antrean total 44 / aktif 32.
//!
//! Pilihan seed yang tidak ditentukan kontrak (dicatat agar bisa diaudit):
//! - jam check-in antrean A-041..A-044 diambil dari kontrak (07.12/07.18/
//!   07.24/07.31); `called_at` A-041 = 09.10 (dari audit seed §5.13);
//! - dokter antrean A-001..A-040 = dr. Ahmad Hidayat (tidak disebut kontrak);
//! - jam encounter seed yang tidak disebut kontrak memakai jam janji
//!   konsultasi (§5.9) atau 09.00/08.30/10.00 untuk encounter lama Siti;
//! - kondisi riwayat Siti dicatat 2024-03-01 (Asma) dan 2025-03-01
//!   (nyeri punggung) — kontrak hanya menyebut tahunnya;
//! - pembayaran seed: INV-1181 Tunai, INV-1182 QRIS, INV-1184 Tunai,
//!   INV-1188 QRIS (kontrak: "lunas tunai/QRIS"), INV-1185 Tunai 2 jt;
//! - janji kontrol ulang 25x: tanggal 20..26 Nov bergiliran, poli
//!   bergiliran (Umum/Anak/Penyakit Dalam/Gigi/Mata), jam konsultasi
//!   08.00/10.00/13.00/14.00, semua Terkonfirmasi.

use crate::convert::*;
use crate::queue::queue_seed_id;
use crate::{clinic_today, db_err, fmt_dt, Db};
use chrono::{NaiveDate, NaiveDateTime};
use serde_json::json;
use waras_core::nutrition::{bmi, bmi_category};
use waras_core::types::{
    AppointmentStatus, ConditionGroup, ConditionStatus, EncounterStatus, Gender,
    Insurance, InvoiceStatus, NotificationKind, PaymentMethod, PrescriptionStatus,
    QueueStatus, Role, VitalCode,
};

fn dt(date: NaiveDate, hh: u32, mm: u32) -> NaiveDateTime {
    date.and_hms_opt(hh, mm, 0).expect("jam seed valid")
}

fn d(y: i32, m: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, day).expect("tanggal seed valid")
}

/// Jalankan seed. Dipanggil `Db::seed_if_empty` hanya bila `users` kosong.
pub async fn seed(db: &Db) -> Result<(), waras_core::WarasError> {
    let today = clinic_today();
    debug_assert_eq!(today, d(2026, 11, 19), "seed memakai tanggal klinik kontrak");
    let t = |hh: u32, mm: u32| fmt_dt(dt(today, hh, mm));

    // -- 5.1 Pengguna ---------------------------------------------------------
    let users: [(&str, &str, &str, &str, Role); 6] = [
        ("U-001", "198505122010011001", "ahmad", "dr. Ahmad Hidayat", Role::DokterUmum),
        ("U-002", "199001152015032002", "sari", "Ns. Sari Wijaya", Role::Perawat),
        ("U-003", "198208102008122003", "rina", "dr. Rina, Sp.PD", Role::DokterUmum),
        ("U-004", "199303252019032004", "dina", "apt. Dina Puspita", Role::Apoteker),
        ("U-005", "199507082021012005", "sinta", "Sinta Maharani", Role::Kasir),
        ("U-006", "199001012015011006", "admin", "Admin Sistem", Role::Admin),
    ];
    for (id, nip, username, name, role) in users {
        let hash = Db::hash_password("waras123")?;
        sqlx::query(
            "INSERT INTO users (id, nip, username, name, role, active, password_hash, created_at)
             VALUES (?, ?, ?, ?, ?, 1, ?, ?)",
        )
        .bind(id)
        .bind(nip)
        .bind(username)
        .bind(name)
        .bind(role_to_str(role))
        .bind(&hash)
        .bind(t(7, 0))
        .execute(&db.pool)
        .await
        .map_err(db_err)?;
    }

    // -- 5.2 Pasien (12 orang: 10 Fase 1 + 2 Fase 3) -------------------------
    // Kata sandi portal demo semua pasien seed: `waras123` (sama seperti
    // staf; hash Argon2, satu hash dipakai semua pasien seed — kata
    // sandi demo, bukan rahasia per pasien; lihat README + FASE-A-A4.md).
    let portal_hash = Db::hash_password("waras123")?;
    // (rm, nik, nama, lahir, gender, alamat, gol_darah, bpjs, penjamin)
    let patients: [(&str, &str, &str, NaiveDate, Gender, &str, &str, Option<&str>, Insurance); 12] = [
        ("RM-000812", "3374021205850001", "Siti Rahmawati", d(1985, 5, 12), Gender::Perempuan,
         "Jl. Pandanaran No. 12, Semarang", "O+", Some("0001234567890"), Insurance::Bpjs),
        ("RM-000813", "3374011503900002", "Budi Santoso", d(1990, 3, 15), Gender::LakiLaki,
         "Jl. Gajah Mada No. 45, Semarang", "A+", Some("0001234567891"), Insurance::Bpjs),
        ("RM-000814", "3374032207900003", "Dewi Lestari", d(1979, 7, 22), Gender::Perempuan,
         "Jl. Pemuda No. 88, Semarang", "B+", None, Insurance::Umum),
        ("RM-000815", "3374020310880004", "Agus Prasetyo", d(1988, 10, 3), Gender::LakiLaki,
         "Jl. Siliwangi No. 21, Semarang", "O+", Some("0001234567892"), Insurance::Bpjs),
        ("RM-000816", "3374051402920005", "Rina Marlina", d(1992, 2, 14), Gender::Perempuan,
         "Jl. Ahmad Yani No. 7, Semarang", "AB+", Some("0001234567893"), Insurance::Bpjs),
        ("RM-000817", "3374022505810006", "Joko Susilo", d(1981, 5, 25), Gender::LakiLaki,
         "Jl. Diponegoro No. 33, Semarang", "A+", None, Insurance::Umum),
        ("RM-000818", "3374031103890007", "Ahmad Fauzi", d(1989, 11, 3), Gender::LakiLaki,
         "Jl. Kartini No. 19, Semarang", "B+", Some("0001234567894"), Insurance::Bpjs),
        ("RM-000819", "3374052807940008", "Putri Ayu", d(1994, 7, 28), Gender::Perempuan,
         "Jl. Sudirman No. 56, Semarang", "O+", Some("0001234567895"), Insurance::Bpjs),
        ("RM-000820", "3374011806750009", "Bambang Riyanto", d(1975, 6, 18), Gender::LakiLaki,
         "Jl. Veteran No. 64, Semarang", "A+", None, Insurance::Umum),
        ("RM-000821", "3374033009800010", "Sri Wahyuni", d(1980, 9, 30), Gender::Perempuan,
         "Jl. Pahlawan No. 102, Semarang", "B+", Some("0001234567896"), Insurance::Bpjs),
        // Fase 3 (FASE3.md §4.2).
        ("RM-000822", "3374031505930011", "Maya Kusuma", d(1993, 5, 15), Gender::Perempuan,
         "Jl. Majapahit No. 9, Semarang", "B+", Some("0001234567897"), Insurance::Bpjs),
        ("RM-000823", "3374022204880012", "Andi Wijaya", d(1988, 4, 22), Gender::LakiLaki,
         "Jl. Hayam Wuruk No. 27, Semarang", "O+", None, Insurance::Umum),
    ];
    for (rm, nik, name, birth, gender, address, blood, bpjs, insurance) in patients {
        sqlx::query(
            "INSERT INTO patients (id, nik, name, birth_date, gender, address, phone,
                 blood_type, bpjs_number, insurance, created_at, portal_password_hash)
             VALUES (?, ?, ?, ?, ?, ?, NULL, ?, ?, ?, ?, ?)",
        )
        .bind(rm)
        .bind(nik)
        .bind(name)
        .bind(birth.to_string())
        .bind(gender_to_str(gender))
        .bind(address)
        .bind(blood)
        .bind(bpjs)
        .bind(insurance_to_str(insurance))
        .bind(t(7, 0))
        .bind(&portal_hash)
        .execute(&db.pool)
        .await
        .map_err(db_err)?;
    }

    // Alergi Siti: Penisilin, kelas penicillin, kritis Tinggi.
    sqlx::query(
        "INSERT INTO allergies (id, patient_id, substance, substance_class,
             criticality, note, recorded_at)
         VALUES ('ALG-001', 'RM-000812', 'Penisilin', 'penicillin', 'tinggi', NULL, ?)",
    )
    .bind(t(7, 12))
    .execute(&db.pool)
    .await
    .map_err(db_err)?;

    // -- 5.3 Encounter --------------------------------------------------------
    // (id, patient, poli, doctor_id, doctor_name, date, time, status)
    let encounters: [(&str, &str, &str, Option<&str>, &str, NaiveDate, &str, EncounterStatus); 12] = [
        ("ENC-2026-11-19-014", "RM-000812", "Poli Umum", Some("U-001"), "dr. Ahmad Hidayat",
         d(2026, 11, 19), "09.15", EncounterStatus::DalamPemeriksaan),
        ("ENC-2026-11-12-008", "RM-000812", "Penyakit Dalam", Some("U-003"), "dr. Rina, Sp.PD",
         d(2026, 11, 12), "09.00", EncounterStatus::Selesai),
        ("ENC-2026-10-28-005", "RM-000812", "Poli Umum", Some("U-001"), "dr. Ahmad Hidayat",
         d(2026, 10, 28), "08.30", EncounterStatus::Selesai),
        ("ENC-2026-09-15-003", "RM-000812", "Poli Umum", Some("U-001"), "dr. Ahmad Hidayat",
         d(2026, 9, 15), "10.00", EncounterStatus::Selesai),
        ("ENC-2026-11-19-015", "RM-000813", "Poli Anak", None, "dr. Sari Dewi",
         d(2026, 11, 19), "10.15", EncounterStatus::Selesai),
        ("ENC-2026-11-19-016", "RM-000814", "Penyakit Dalam", Some("U-003"), "dr. Rina, Sp.PD",
         d(2026, 11, 19), "11.00", EncounterStatus::Selesai),
        ("ENC-2026-11-19-017", "RM-000815", "Gigi", None, "drg. Maya K.",
         d(2026, 11, 19), "12.00", EncounterStatus::DalamPemeriksaan),
        ("ENC-2026-11-19-018", "RM-000816", "Poli Umum", Some("U-001"), "dr. Ahmad Hidayat",
         d(2026, 11, 19), "13.30", EncounterStatus::Selesai),
        ("ENC-2026-11-19-019", "RM-000817", "Mata", None, "dr. Budi, Sp.B",
         d(2026, 11, 19), "14.00", EncounterStatus::Selesai),
        ("ENC-2026-11-19-020", "RM-000821", "Poli Umum", Some("U-003"), "dr. Rina, Sp.PD",
         d(2026, 11, 19), "15.00", EncounterStatus::Selesai),
        // Fase 3: encounter fast-track IGD (FASE3.md §4.2).
        ("ENC-2026-11-19-021", "RM-000820", "IGD", None, "dr. Andi Wijaya",
         d(2026, 11, 19), "08.02", EncounterStatus::DalamPemeriksaan),
        ("ENC-2026-11-19-022", "RM-000817", "IGD", None, "dr. Sari Dewi",
         d(2026, 11, 19), "08.15", EncounterStatus::DalamPemeriksaan),
    ];
    for (id, patient, poli, doctor_id, doctor_name, date, time, status) in encounters {
        sqlx::query(
            "INSERT INTO encounters (id, patient_id, poli, doctor_id, doctor_name,
                 date, time, status, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(id)
        .bind(patient)
        .bind(poli)
        .bind(doctor_id)
        .bind(doctor_name)
        .bind(date.to_string())
        .bind(time)
        .bind(encounter_status_to_str(status))
        .bind(fmt_dt(dt(date, 7, 0)))
        .execute(&db.pool)
        .await
        .map_err(db_err)?;
    }

    // -- 5.2 (lanjutan) Kondisi ----------------------------------------------
    // (id, patient, encounter, group, code, display, status, clinician, recorded_at)
    let conditions: [(&str, &str, Option<&str>, ConditionGroup, &str, &str, ConditionStatus, &str, NaiveDateTime); 10] = [
        ("COND-001", "RM-000812", Some("ENC-2026-11-19-014"), ConditionGroup::Utama,
         "J20.9", "Bronkitis akut", ConditionStatus::Aktif, "dr. Ahmad Hidayat", dt(today, 9, 21)),
        ("COND-002", "RM-000812", Some("ENC-2026-11-19-014"), ConditionGroup::Sekunder,
         "E11.9", "Diabetes melitus tipe 2 tanpa komplikasi", ConditionStatus::Kronis,
         "dr. Ahmad Hidayat", dt(today, 9, 21)),
        ("COND-003", "RM-000812", None, ConditionGroup::Kronis,
         "E11.9", "Diabetes melitus tipe 2", ConditionStatus::Kronis,
         "dr. Rina, Sp.PD", dt(d(2026, 11, 12), 9, 0)),
        ("COND-004", "RM-000812", None, ConditionGroup::Kronis,
         "I10", "Hipertensi esensial", ConditionStatus::Kronis,
         "dr. Rina, Sp.PD", dt(d(2026, 11, 12), 9, 0)),
        ("COND-005", "RM-000812", None, ConditionGroup::Riwayat,
         "J45.9", "Asma", ConditionStatus::Selesai,
         "dr. Ahmad Hidayat", dt(d(2024, 3, 1), 9, 0)),
        ("COND-006", "RM-000812", None, ConditionGroup::Riwayat,
         "M54.5", "Nyeri punggung bawah", ConditionStatus::Selesai,
         "dr. Ahmad Hidayat", dt(d(2025, 3, 1), 9, 0)),
        ("COND-007", "RM-000812", Some("ENC-2026-11-19-014"), ConditionGroup::TindakanMedis,
         "89.52", "Elektrokardigram (EKG)", ConditionStatus::Selesai,
         "dr. Ahmad Hidayat", dt(today, 9, 21)),
        ("COND-008", "RM-000813", Some("ENC-2026-11-19-015"), ConditionGroup::Riwayat,
         "J06.9", "ISPA atas", ConditionStatus::Selesai,
         "dr. Sari Dewi", dt(today, 10, 15)),
        ("COND-009", "RM-000818", None, ConditionGroup::Riwayat,
         "J06.9", "ISPA atas", ConditionStatus::Selesai,
         "dr. Ahmad Hidayat", dt(today, 9, 0)),
        ("COND-010", "RM-000819", None, ConditionGroup::Riwayat,
         "J06.9", "ISPA atas", ConditionStatus::Selesai,
         "dr. Ahmad Hidayat", dt(today, 9, 0)),
    ];
    for (id, patient, encounter, group, code, display, status, clinician, recorded) in conditions {
        sqlx::query(
            "INSERT INTO conditions (id, patient_id, encounter_id, condition_group,
                 code, display, status, clinician, recorded_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(id)
        .bind(patient)
        .bind(encounter)
        .bind(condition_group_to_str(group))
        .bind(code)
        .bind(display)
        .bind(condition_status_to_str(status))
        .bind(clinician)
        .bind(fmt_dt(recorded))
        .execute(&db.pool)
        .await
        .map_err(db_err)?;
    }

    // -- 5.4 SOAP Siti (versi 3, Draft) ---------------------------------------
    let s = "Batuk berdahak 3 hari. Demam malam hari. Nyeri dada ringan saat batuk.";
    let o = "Paru: ronki basal kanan. THT: faring hiperemis. Suhu 37,2 °C, SpO2 98%.";
    let a = "Bronkitis akut (J20.9). DM tipe 2 terkontrol (E11.9).";
    let p = "Azithromycin 500 mg 1x1, 3 hari. Kontrol 3 hari lagi. Edukasi hidrasi & istirahat. Belum lengkap — resep menunggu konfirmasi.";
    sqlx::query(
        "INSERT INTO soap_notes (id, encounter_id, subjective, objective, assessment,
             plan, author, status, version, created_at, updated_at, draft_saved_at)
         VALUES ('SOAP-ENC-2026-11-19-014', 'ENC-2026-11-19-014', ?, ?, ?, ?, ?, 'draft', 3, ?, ?, ?)",
    )
    .bind(s)
    .bind(o)
    .bind(a)
    .bind(p)
    .bind("dr. Ahmad Hidayat")
    .bind(t(9, 18))
    .bind(t(9, 21))
    .bind(t(9, 22))
    .execute(&db.pool)
    .await
    .map_err(db_err)?;
    // Riwayat versi: v1 09.18 S saja, v2 09.20 S+O, v3 09.21 lengkap.
    let v1_at = t(9, 18);
    let v2_at = t(9, 20);
    let v3_at = t(9, 21);
    let versions: [(u32, &str, &str, &str, &str, &str); 3] = [
        (1, s, "", "", "", v1_at.as_str()),
        (2, s, o, "", "", v2_at.as_str()),
        (3, s, o, a, p, v3_at.as_str()),
    ];
    for (v, vs, vo, va, vp, saved_at) in versions {
        sqlx::query(
            "INSERT INTO soap_versions (id, soap_id, version, subjective, objective,
                 assessment, plan, author, saved_at)
             VALUES (?, 'SOAP-ENC-2026-11-19-014', ?, ?, ?, ?, ?, 'dr. Ahmad Hidayat', ?)",
        )
        .bind(format!("SV-014-v{v}"))
        .bind(v as i64)
        .bind(vs)
        .bind(vo)
        .bind(va)
        .bind(vp)
        .bind(saved_at)
        .execute(&db.pool)
        .await
        .map_err(db_err)?;
    }

    // -- 5.5 Tanda vital Siti --------------------------------------------------
    // (code, value, previous_value)
    let vitals: [(VitalCode, &str, &str); 5] = [
        (VitalCode::TekananDarah, "130/85", "128/82"),
        (VitalCode::Nadi, "82", "80"),
        (VitalCode::Suhu, "37,2", "37,0"),
        (VitalCode::SpO2, "98", "99"),
        (VitalCode::GulaDarah, "142", "138"),
    ];
    for (i, (code, value, prev)) in vitals.iter().enumerate() {
        sqlx::query(
            "INSERT INTO observations (id, encounter_id, patient_id, code, value, unit,
                 measured_at, measured_by, previous_value, previous_date)
             VALUES (?, 'ENC-2026-11-19-014', 'RM-000812', ?, ?, ?, ?, 'Ns. Sari Wijaya', ?, ?)",
        )
        .bind(format!("OBS-014-{}", i + 1))
        .bind(vital_code_to_str(*code))
        .bind(value)
        .bind(code.unit())
        .bind(t(9, 15))
        .bind(prev)
        .bind(d(2026, 11, 12).to_string())
        .execute(&db.pool)
        .await
        .map_err(db_err)?;
    }

    // -- 5.7 Obat & stok KFA (diisi dulu supaya resep merujuk stok yang benar) --
    let drugs: [(&str, &str, &str, &str, i64, &str, NaiveDate); 9] = [
        ("KFA-9201", "Amoxicillin 500 mg", "penicillin", "Tablet", 12, "Rak A-02", d(2027, 3, 31)),
        ("KFA-9202", "Paracetamol 500 mg", "analgesic", "Tablet", 1240, "Rak A-01", d(2027, 11, 30)),
        ("KFA-9203", "Omeprazole 20 mg", "ppi", "Kapsul", 15, "Rak B-03", d(2027, 1, 31)),
        ("KFA-9204", "Salbutamol 100 mcg", "bronchodilator", "Inhaler", 3, "Rak C-01", d(2027, 6, 30)),
        ("KFA-9205", "Metformin 500 mg", "antidiabetic", "Tablet", 860, "Rak A-04", d(2027, 9, 30)),
        ("KFA-9206", "Ibuprofen 400 mg", "analgesic", "Tablet", 540, "Rak A-02", d(2027, 8, 31)),
        ("KFA-9207", "Cefixime 200 mg", "cephalosporin", "Kapsul", 22, "Rak B-01", d(2027, 5, 31)),
        ("KFA-9208", "Azithromycin 500 mg", "macrolide", "Tablet", 320, "Rak A-03", d(2027, 8, 31)),
        ("KFA-9209", "Amlodipine 5 mg", "ccb", "Tablet", 145, "Rak B-02", d(2027, 5, 31)),
    ];
    for (kfa, name, class, form, stock, location, expiry) in drugs {
        sqlx::query(
            "INSERT INTO drugs (kfa_code, name, drug_class, form, stock, location, expiry)
             VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(kfa)
        .bind(name)
        .bind(class)
        .bind(form)
        .bind(stock)
        .bind(location)
        .bind(expiry.to_string())
        .execute(&db.pool)
        .await
        .map_err(db_err)?;
    }

    // -- 5.6 Resep -------------------------------------------------------------
    // (id, encounter, patient, drug, class, dosage, duration, status, doctor,
    //  blocked_reason, replaced_by, created_at)
    let prescriptions: [(&str, &str, &str, &str, &str, &str, &str, PrescriptionStatus, &str, Option<&str>, Option<&str>, NaiveDateTime); 8] = [
        ("RX-1040", "ENC-2026-11-19-014", "RM-000812", "Amoxicillin 500 mg", "penicillin",
         "1x1", "3 hari", PrescriptionStatus::Diblokir, "dr. Ahmad Hidayat",
         Some("Alergi Penisilin"), Some("RX-1041"), dt(today, 9, 21)),
        ("RX-1041", "ENC-2026-11-19-014", "RM-000812", "Azithromycin 500 mg", "macrolide",
         "1x1", "3 hari", PrescriptionStatus::MenungguKonfirmasi, "dr. Ahmad Hidayat",
         None, None, dt(today, 9, 21)),
        ("RX-1042", "ENC-2026-11-19-015", "RM-000813", "Paracetamol 500 mg", "analgesic",
         "3x1", "3 hari", PrescriptionStatus::Aktif, "dr. Sari Dewi",
         None, None, dt(today, 10, 15)),
        ("RX-1043", "ENC-2026-11-19-016", "RM-000814", "Amlodipine 5 mg", "ccb",
         "1x1", "30 hari", PrescriptionStatus::Aktif, "dr. Rina, Sp.PD",
         None, None, dt(today, 11, 0)),
        ("RX-1044", "ENC-2026-11-19-017", "RM-000815", "Cefixime 200 mg", "cephalosporin",
         "2x1", "5 hari", PrescriptionStatus::Aktif, "drg. Maya K.",
         None, None, dt(today, 12, 0)),
        ("RX-1045", "ENC-2026-11-19-018", "RM-000816", "Metformin 500 mg", "antidiabetic",
         "3x1", "30 hari", PrescriptionStatus::Aktif, "dr. Ahmad Hidayat",
         None, None, dt(today, 13, 30)),
        ("RX-1046", "ENC-2026-11-19-019", "RM-000817", "Ibuprofen 400 mg", "nsaid",
         "3x1", "3 hari", PrescriptionStatus::Aktif, "dr. Budi, Sp.B",
         None, None, dt(today, 14, 0)),
        ("RX-1047", "ENC-2026-11-19-020", "RM-000821", "Omeprazole 20 mg", "ppi",
         "1x1", "14 hari", PrescriptionStatus::Aktif, "dr. Rina, Sp.PD",
         None, None, dt(today, 15, 0)),
    ];
    for (id, enc, patient, drug, class, dosage, duration, status, doctor, blocked, replaced, created) in prescriptions {
        sqlx::query(
            "INSERT INTO medication_requests (id, encounter_id, patient_id, drug_name,
                 drug_class, dosage, duration, status, doctor, blocked_reason,
                 replaced_by, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(id)
        .bind(enc)
        .bind(patient)
        .bind(drug)
        .bind(class)
        .bind(dosage)
        .bind(duration)
        .bind(prescription_status_to_str(status))
        .bind(doctor)
        .bind(blocked)
        .bind(replaced)
        .bind(fmt_dt(created))
        .execute(&db.pool)
        .await
        .map_err(db_err)?;
    }

    // -- 5.8 Antrean 44 entri --------------------------------------------------
    let patient_cycle = [
        "RM-000812", "RM-000813", "RM-000814", "RM-000815", "RM-000816",
        "RM-000817", "RM-000818", "RM-000819", "RM-000820", "RM-000821",
    ];
    for n in 1..=44u32 {
        let number = format!("A-{n:03}");
        let patient = patient_cycle[((n - 1) % 10) as usize];
        let (status, time, called_at): (QueueStatus, String, Option<String>) = match n {
            1..=10 => (QueueStatus::Selesai, checkin_time(n), None),
            11..=12 => (QueueStatus::TidakHadir, checkin_time(n), None),
            13..=27 => (QueueStatus::DalamKonsultasi, checkin_time(n), None),
            28..=40 => (QueueStatus::Menunggu, checkin_time(n), None),
            41 => (QueueStatus::Dipanggil, "07.12".into(), Some(t(9, 10))),
            42 => (QueueStatus::Menunggu, "07.18".into(), None),
            43 => (QueueStatus::Menunggu, "07.24".into(), None),
            44 => (QueueStatus::DalamKonsultasi, "07.31".into(), None),
            _ => unreachable!(),
        };
        sqlx::query(
            "INSERT INTO queue_entries (id, number, patient_id, poli, doctor, date,
                 time, status, called_at)
             VALUES (?, ?, ?, 'Poli Umum', 'dr. Ahmad Hidayat', ?, ?, ?, ?)",
        )
        .bind(queue_seed_id(today, &number))
        .bind(&number)
        .bind(patient)
        .bind(today.to_string())
        .bind(&time)
        .bind(queue_status_to_str(status))
        .bind(called_at)
        .execute(&db.pool)
        .await
        .map_err(db_err)?;
    }

    // -- 5.9 Janji (31 total) --------------------------------------------------
    // 6 hari ini.
    let today_apts: [(&str, &str, &str, &str, &str, AppointmentStatus); 6] = [
        ("APT-001", "RM-000812", "Poli Umum", "07.30", "09.30", AppointmentStatus::Terkonfirmasi),
        ("APT-002", "RM-000813", "Poli Anak", "08.00", "10.15", AppointmentStatus::Terkonfirmasi),
        ("APT-003", "RM-000814", "Penyakit Dalam", "08.30", "11.00", AppointmentStatus::Menunggu),
        ("APT-004", "RM-000815", "Gigi", "09.00", "12.00", AppointmentStatus::Terkonfirmasi),
        ("APT-005", "RM-000816", "Poli Umum", "09.15", "13.30", AppointmentStatus::Menunggu),
        ("APT-006", "RM-000817", "Mata", "09.45", "14.00", AppointmentStatus::Menunggu),
    ];
    for (id, patient, poli, appt, consult, status) in today_apts {
        insert_appointment(db, id, patient, poli, today, appt, consult, status).await?;
    }
    // 25 kontrol ulang 20..26 Nov.
    let polis = ["Poli Umum", "Poli Anak", "Penyakit Dalam", "Gigi", "Mata"];
    let consult_times = ["08.00", "10.00", "13.00", "14.00"];
    let appt_times = ["07.30", "09.30", "12.30", "13.30"];
    for i in 0..25usize {
        let date = today + chrono::Duration::days(1 + (i % 7) as i64);
        let slot = i / 7;
        insert_appointment(
            db,
            &format!("APT-{:03}", i + 7),
            patient_cycle[i % 10],
            polis[i % 5],
            date,
            appt_times[slot],
            consult_times[slot],
            AppointmentStatus::Terkonfirmasi,
        )
        .await?;
    }

    // -- 5.10 Tagihan (8) + pembayaran -----------------------------------------
    // (id, patient, encounter, payer, service, amount, status)
    let invoices: [(&str, &str, Option<&str>, Insurance, &str, i64, InvoiceStatus); 8] = [
        ("INV-2026-1181", "RM-000812", Some("ENC-2026-11-19-014"), Insurance::Bpjs,
         "Poli Umum + Lab", 185_000, InvoiceStatus::Lunas),
        ("INV-2026-1182", "RM-000813", Some("ENC-2026-11-19-015"), Insurance::Bpjs,
         "Poli Anak", 120_000, InvoiceStatus::Lunas),
        ("INV-2026-1183", "RM-000814", Some("ENC-2026-11-19-016"), Insurance::Umum,
         "Penyakit Dalam + EKG", 450_000, InvoiceStatus::Menunggu),
        ("INV-2026-1184", "RM-000815", Some("ENC-2026-11-19-017"), Insurance::Bpjs,
         "Gigi + tambal", 275_000, InvoiceStatus::Lunas),
        ("INV-2026-1185", "RM-000820", None, Insurance::Umum,
         "Rawat inap 3 hari", 4_850_000, InvoiceStatus::Cicilan),
        ("INV-2026-1186", "RM-000821", Some("ENC-2026-11-19-020"), Insurance::Bpjs,
         "Radiologi X-Ray", 310_000, InvoiceStatus::Menunggu),
        ("INV-2026-1187", "RM-000818", None, Insurance::Umum,
         "Poli Umum + Lab", 265_000, InvoiceStatus::Menunggu),
        ("INV-2026-1188", "RM-000819", None, Insurance::Bpjs,
         "Poli Anak", 95_000, InvoiceStatus::Lunas),
    ];
    for (id, patient, encounter, payer, service, amount, status) in invoices {
        sqlx::query(
            "INSERT INTO invoices (id, patient_id, encounter_id, payer, service, amount,
                 status, ina_cbg_code, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, NULL, ?)",
        )
        .bind(id)
        .bind(patient)
        .bind(encounter)
        .bind(insurance_to_str(payer))
        .bind(service)
        .bind(amount)
        .bind(invoice_status_to_str(status))
        .bind(t(7, 30))
        .execute(&db.pool)
        .await
        .map_err(db_err)?;
    }
    // Pembayaran: satu per tagihan lunas + cicilan 2 jt untuk INV-1185.
    let payments: [(&str, &str, PaymentMethod, i64, &str, NaiveDateTime); 5] = [
        ("PAY-1181", "INV-2026-1181", PaymentMethod::Tunai, 185_000, "Sinta Maharani", dt(today, 10, 5)),
        ("PAY-1182", "INV-2026-1182", PaymentMethod::Qris, 120_000, "Sinta Maharani", dt(today, 10, 40)),
        ("PAY-1184", "INV-2026-1184", PaymentMethod::Tunai, 275_000, "Sinta Maharani", dt(today, 12, 20)),
        ("PAY-1185", "INV-2026-1185", PaymentMethod::Tunai, 2_000_000, "Sinta Maharani", dt(today, 11, 15)),
        ("PAY-1188", "INV-2026-1188", PaymentMethod::Qris, 95_000, "Sinta Maharani", dt(today, 9, 50)),
    ];
    for (id, invoice, method, amount, cashier, paid_at) in payments {
        sqlx::query(
            "INSERT INTO payments (id, invoice_id, method, amount, cashier, paid_at)
             VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(id)
        .bind(invoice)
        .bind(payment_method_to_str(method))
        .bind(amount)
        .bind(cashier)
        .bind(fmt_dt(paid_at))
        .execute(&db.pool)
        .await
        .map_err(db_err)?;
    }

    // -- 5.11 Daily stats (7 hari) ----------------------------------------------
    let stats: [(NaiveDate, i64, i64, i64, i64, i64, i64, i64, i64, i64); 7] = [
        (d(2026, 11, 13), 231, 162, 16_100_000, 128, 21, 1, 2, 81, 120),
        (d(2026, 11, 14), 240, 174, 17_200_000, 135, 24, 1, 1, 83, 120),
        (d(2026, 11, 15), 219, 168, 15_800_000, 121, 19, 0, 2, 79, 120),
        (d(2026, 11, 16), 252, 182, 18_900_000, 148, 27, 2, 4, 88, 120),
        (d(2026, 11, 17), 244, 176, 17_600_000, 139, 23, 1, 2, 84, 120),
        (d(2026, 11, 18), 238, 174, 17_900_000, 141, 25, 2, 3, 85, 120),
        (d(2026, 11, 19), 248, 210, 18_400_000, 142, 26, 2, 3, 86, 120),
    ];
    for (date, reg, visits, revenue, tx, lab_p, lab_c, igd, beds_o, beds_t) in stats {
        sqlx::query(
            "INSERT INTO daily_stats (date, registrations, visits, revenue_idr,
                 transactions, lab_pending, lab_critical, emergency_red,
                 beds_occupied, beds_total)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(date.to_string())
        .bind(reg)
        .bind(visits)
        .bind(revenue)
        .bind(tx)
        .bind(lab_p)
        .bind(lab_c)
        .bind(igd)
        .bind(beds_o)
        .bind(beds_t)
        .execute(&db.pool)
        .await
        .map_err(db_err)?;
    }

    // -- 5.12 Notifikasi ---------------------------------------------------------
    let notifs: [(&str, NotificationKind, &str, &str); 3] = [
        ("NOTIF-001", NotificationKind::Info, "Hasil lab Siti Rahmawati siap.", "09.15"),
        ("NOTIF-002", NotificationKind::Info, "Antrean baru dari JKN-881206.", "08.50"),
        ("NOTIF-003", NotificationKind::Kritis, "Stok Amoxicillin kritis: 12 tablet.", "08.40"),
    ];
    for (id, kind, message, time) in notifs {
        sqlx::query(
            "INSERT INTO notifications (id, kind, message, time, date, read)
             VALUES (?, ?, ?, ?, ?, 0)",
        )
        .bind(id)
        .bind(notification_kind_to_str(kind))
        .bind(message)
        .bind(time)
        .bind(today.to_string())
        .execute(&db.pool)
        .await
        .map_err(db_err)?;
    }

    // -- 5.13 Audit seed (6 entri, berantai hash) -------------------------------
    // insert_audit_at menghitung prev_hash/entry_hash dari rantai yang ada.
    let audits: [(NaiveDateTime, &str, &str, &str, &str, serde_json::Value); 6] = [
        (dt(today, 7, 0), "dr. Ahmad Hidayat", "auth.login", "user", "U-001",
         json!({"username": "ahmad"})),
        (dt(today, 7, 12), "Ns. Sari Wijaya", "patient.registered", "patient", "RM-000812",
         json!({"name": "Siti Rahmawati"})),
        (dt(today, 9, 10), "dr. Ahmad Hidayat", "queue.called", "queue_entry", "A-041",
         json!({"number": "A-041", "patient_id": "RM-000812"})),
        (dt(today, 9, 21), "dr. Ahmad Hidayat", "prescription.blocked", "medication_request", "RX-1040",
         json!({"drug": "Amoxicillin 500 mg", "reason": "Alergi Penisilin"})),
        (dt(today, 9, 21), "dr. Ahmad Hidayat", "prescription.created", "medication_request", "RX-1041",
         json!({"drug": "Azithromycin 500 mg", "replaces": "RX-1040"})),
        (dt(today, 9, 22), "dr. Ahmad Hidayat", "soap.saved", "encounter", "ENC-2026-11-19-014",
         json!({"version": 3})),
    ];
    for (ts, actor, action, rtype, rid, details) in audits {
        db.insert_audit_at(ts, actor, action, rtype, rid, details).await?;
    }

    // -- Fase 2 (FASE2.md §2.2): SEP, antrean online, bed, rujukan, PCare,
    // klaim. Outbox sengaja KOSONG saat seed (tidak ada kiriman palsu).
    seed_fase2(db).await?;

    // -- Fase 3 (FASE3.md §4.2): IGD, rawat inap, operasi & bersalin,
    // lab, radiologi, casemix.
    seed_fase3(db).await?;

    // -- Fase 4 (FASE4.md §4.2): hemodialisa, transfusi, gizi, gigi,
    // inventori, kepegawaian, pengaturan + 2 pasien baru.
    seed_fase4(db).await?;

    Ok(())
}

/// Seed Fase 2 (FASE2.md §2.2). Idempotent: hanya jalan bila tabel `seps`
/// kosong. Data dari layar desain v16, tanggal klinik 2026-11-19.
///
/// Catatan yang tidak ditentukan kontrak (dicatat agar bisa diaudit):
/// - nomor kartu BPJS di baris SEP diambil VERBATIM dari FASE2.md §2.2
///   (0001234567890..97 berurutan). Nomor ini TIDAK cocok dengan
///   `bpjs_number` pasien Fase 1 untuk Agus/Bambang/Dewi/Rina/Ahmad/Putri
///   (Fase 1: Agus 92, Dewi tanpa BPJS, Rina 93, Ahmad 94, Putri 95,
///   Bambang tanpa BPJS); kolom SEP hanya membawa nomor yang ditulis
///   kontrak, bukan FK ke pasien.
/// - `online_queue` JKN-881202 berstatus Hadir tanpa `local_queue_id`:
///   kontrak hanya menyebut local A-041 untuk JKN-881201, jadi yang lain
///   dibiarkan NULL (tidak ditebak).
/// - `bed_availability` dibaca sebagai tersedia/total (FASE2.md §6:
///   panel Aplicares menampilkan "tersedia/total"), mis. Kelas 1 = 12
///   tersedia dari 16.
/// - `referrals.reason` = "Bedah Saraf" (tanda kurung di §2.2) dengan
///   `to_facility` = "RSUD Dr. Kariadi"; `from_poli` mengikuti encounter
///   pasien hari itu (Rina Poli Umum, Budi Poli Anak).
/// - `pcare_visits.status` = "Tercatat"; `claims.ina_cbg_code` = "J-4-17-I"
///   adalah kode INA-CBG CONTOH, bukan grouping sungguhan (FASE2.md §2.2).
async fn seed_fase2(db: &Db) -> Result<(), waras_core::WarasError> {
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM seps")
        .fetch_one(&db.pool)
        .await
        .map_err(db_err)?;
    if n > 0 {
        return Ok(());
    }
    let today = clinic_today();
    let t = |hh: u32, mm: u32| fmt_dt(dt(today, hh, mm));

    // -- SEP (8): (id, patient, encounter, bpjs, jenis, poli, status) -------
    // encounter_id: SEP rawat jalan hari itu menaut ke encounter pasien.
    let seps: [(&str, &str, Option<&str>, &str, &str, &str, &str); 8] = [
        ("SEP-20261119-001", "RM-000812", Some("ENC-2026-11-19-014"), "0001234567890",
         "RawatJalan", "Poli Umum", "aktif"),
        ("SEP-20261119-002", "RM-000813", Some("ENC-2026-11-19-015"), "0001234567891",
         "RawatJalan", "Poli Anak", "aktif"),
        ("SEP-20261119-003", "RM-000820", None, "0001234567892",
         "RawatInap", "Melati", "aktif"),
        ("SEP-20261119-004", "RM-000814", Some("ENC-2026-11-19-016"), "0001234567893",
         "RawatJalan", "Penyakit Dalam", "aktif"),
        ("SEP-20261119-005", "RM-000815", Some("ENC-2026-11-19-017"), "0001234567894",
         "RawatJalan", "Gigi", "gagal"),
        ("SEP-20261119-006", "RM-000816", Some("ENC-2026-11-19-018"), "0001234567895",
         "RawatInap", "Cempaka", "aktif"),
        ("SEP-20261119-007", "RM-000818", None, "0001234567896",
         "RawatJalan", "Poli Umum", "aktif"),
        ("SEP-20261119-008", "RM-000819", None, "0001234567897",
         "RawatJalan", "Poli Anak", "aktif"),
    ];
    for (id, patient, encounter, bpjs, jenis, poli, status) in seps {
        sqlx::query(
            "INSERT INTO seps (id, sep_number, patient_id, encounter_id, bpjs_number,
                 jenis, poli, status, created_by, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, 'dr. Ahmad Hidayat', ?)",
        )
        .bind(id)
        .bind(id)
        .bind(patient)
        .bind(encounter)
        .bind(bpjs)
        .bind(jenis)
        .bind(poli)
        .bind(status)
        .bind(t(7, 30))
        .execute(&db.pool)
        .await
        .map_err(db_err)?;
    }

    // Tautkan tagihan ke SEP lewat encounter (mengisi invoices.sep_id).
    for (id, _patient, encounter, _bpjs, _jenis, _poli, _status) in seps {
        if let Some(enc) = encounter {
            sqlx::query("UPDATE invoices SET sep_id = ? WHERE encounter_id = ?")
                .bind(id)
                .bind(enc)
                .execute(&db.pool)
                .await
                .map_err(db_err)?;
        }
    }

    // -- Antrean online (7): (booking, patient, poli, estimasi, status, local)
    let queue: [(&str, &str, &str, &str, &str, Option<&str>); 7] = [
        ("JKN-881201", "RM-000812", "Poli Umum", "07.30", "Hadir", Some("A-041")),
        ("JKN-881202", "RM-000813", "Poli Anak", "08.00", "Hadir", None),
        ("JKN-881203", "RM-000814", "Penyakit Dalam", "08.30", "Menunggu", None),
        ("JKN-881204", "RM-000815", "Gigi", "09.00", "Menunggu", None),
        ("JKN-881205", "RM-000816", "Poli Umum", "09.15", "Menunggu", None),
        ("JKN-881206", "RM-000818", "Poli Umum", "09.30", "Menunggu", None),
        ("JKN-881207", "RM-000819", "Poli Anak", "09.45", "Menunggu", None),
    ];
    for (code, patient, poli, estimasi, status, local) in queue {
        sqlx::query(
            "INSERT INTO online_queue (id, booking_code, patient_id, poli, estimasi,
                 status, local_queue_id, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(code)
        .bind(code)
        .bind(patient)
        .bind(poli)
        .bind(estimasi)
        .bind(status)
        .bind(local)
        .bind(t(7, 30))
        .execute(&db.pool)
        .await
        .map_err(db_err)?;
    }

    // -- Bed (5): (class, available, total) -----------------------------------
    let beds: [(&str, i64, i64); 5] = [
        ("Kelas 1", 12, 16),
        ("Kelas 2", 24, 32),
        ("Kelas 3", 38, 48),
        ("VIP", 6, 8),
        ("ICU", 0, 12),
    ];
    for (class, available, total) in beds {
        sqlx::query(
            "INSERT INTO bed_availability (class, total, available, updated_at)
             VALUES (?, ?, ?, ?)",
        )
        .bind(class)
        .bind(total)
        .bind(available)
        .bind(t(7, 30))
        .execute(&db.pool)
        .await
        .map_err(db_err)?;
    }

    // -- Rujukan (2) -----------------------------------------------------------
    let referrals: [(&str, &str, &str, &str); 2] = [
        ("RUJ-20261119-001", "RM-000816", "Poli Umum", "RSUD Dr. Kariadi"),
        ("RUJ-20261119-002", "RM-000813", "Poli Anak", "RSUD Dr. Kariadi"),
    ];
    for (id, patient, from_poli, to_facility) in referrals {
        sqlx::query(
            "INSERT INTO referrals (id, patient_id, from_poli, to_facility, reason,
                 status, created_at)
             VALUES (?, ?, ?, ?, 'Bedah Saraf', 'menunggu', ?)",
        )
        .bind(id)
        .bind(patient)
        .bind(from_poli)
        .bind(to_facility)
        .bind(t(7, 30))
        .execute(&db.pool)
        .await
        .map_err(db_err)?;
    }

    // -- Kunjungan PCare (3): Siti, Budi, Dewi, poli masing-masing ------------
    let visits: [(&str, &str, &str); 3] = [
        ("PCV-20261119-001", "RM-000812", "Poli Umum"),
        ("PCV-20261119-002", "RM-000813", "Poli Anak"),
        ("PCV-20261119-003", "RM-000814", "Penyakit Dalam"),
    ];
    for (id, patient, poli) in visits {
        sqlx::query(
            "INSERT INTO pcare_visits (id, patient_id, visit_date, poli, status, created_at)
             VALUES (?, ?, ?, ?, 'Tercatat', ?)",
        )
        .bind(id)
        .bind(patient)
        .bind(today.to_string())
        .bind(poli)
        .bind(t(7, 30))
        .execute(&db.pool)
        .await
        .map_err(db_err)?;
    }

    // -- Klaim (1): draft contoh; kode INA-CBG BUKAN grouping sungguhan ------
    sqlx::query(
        "INSERT INTO claims (id, invoice_id, sep_id, ina_cbg_code, amount,
             status, submitted_at, created_at, grouping_note)
         VALUES ('CLM-20261119-001', 'INV-2026-1181', 'SEP-20261119-001',
             'J-4-17-I', 185000, 'draft', NULL, ?, NULL)",
    )
    .bind(t(7, 30))
    .execute(&db.pool)
    .await
    .map_err(db_err)?;

    // Outbox: KOSONG saat seed (tidak ada kiriman palsu).
    Ok(())
}

/// Jam check-in antrean A-001..A-040: 07.00 + 2 menit per nomor.
fn checkin_time(n: u32) -> String {
    let mins = 7 * 60 + 2 * (n - 1);
    format!("{:02}.{:02}", mins / 60, mins % 60)
}

async fn insert_appointment(
    db: &Db,
    id: &str,
    patient_id: &str,
    poli: &str,
    date: NaiveDate,
    appointment_time: &str,
    consultation_time: &str,
    status: AppointmentStatus,
) -> Result<(), waras_core::WarasError> {
    sqlx::query(
        "INSERT INTO appointments (id, patient_id, poli, date, appointment_time,
             consultation_time, status)
         VALUES (?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(id)
    .bind(patient_id)
    .bind(poli)
    .bind(date.to_string())
    .bind(appointment_time)
    .bind(consultation_time)
    .bind(crate::convert::appointment_status_to_str(status))
    .execute(&db.pool)
    .await
    .map_err(db_err)?;
    Ok(())
}

impl Db {
    /// Seed Fase 2 bila tabel `seps` masih kosong (FASE2.md §2.2).
    ///
    /// Alasan: `seed()` hanya dipanggil `seed_if_empty` bila `users`
    /// kosong. DB Fase 1 yang sudah ada lalu mendapat migrasi 0002
    /// tidak pernah menjalankan `seed()` lagi, jadi tabel Fase 2-nya
    /// tetap kosong. Dipanggil binary sekali saat boot, tepat setelah
    /// `seed_if_empty()`. Mengembalikan `true` bila seed dijalankan.
    pub async fn seed_fase2_if_empty(&self) -> Result<bool, waras_core::WarasError> {
        let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM seps")
            .fetch_one(&self.pool)
            .await
            .map_err(db_err)?;
        if n > 0 {
            return Ok(false);
        }
        seed_fase2(self).await?;
        Ok(true)
    }
}

/// Seed Fase 3 (FASE3.md §4.2). Idempotent: hanya jalan bila tabel
/// `wards` kosong. Data dari layar desain v16, tanggal klinik 2026-11-19.
/// Mengembalikan `true` bila seed dijalankan.
///
/// Catatan yang tidak ditentukan kontrak (dicatat agar bisa diaudit):
/// - `ordered_by` lab: nama dokter penanggung jawab tiap order
///   (kontrak hanya memberi `ordered_from`); `ordered_by`/`ordered_from`
///   radiologi: mengikuti poli/ruang yang merujuk (IGD/Cempaka/Mawar/
///   Penyakit Dalam/Poli Umum/Poli Anak).
/// - Jam `created_at` lab/rad: disebar 08.05–09.15 (kontrak tidak memberi
///   jam); jam observasi IGD: 08.30 (Bambang) dan 08.45 (Joko).
/// - Peta bed: 3A persis kontrak; bed lain diisi Terisi sampai kuota
///   hunian per ward tercapai (urut nomor), sisanya Kosong, 2 bed per
///   ward Dibersihkan (3A-05/11, 2A-13/2B-11, 1C-10/1D-10, 4A-10/4B-10,
///   ICU-01/02) — ICU 0 terisi.
/// - `encounter_id` admisi: encounter pasien hari itu bila ada
///   (Bambang 021, Sri 020, Joko 022, Dewi 016, Rina 018); Ahmad Fauzi
///   dan Putri Ayu tidak punya encounter 19 Nov, jadi NULL.
/// - `encounter_id` operasi: encounter pasien hari itu bila ada.
pub async fn seed_fase3(db: &Db) -> Result<bool, waras_core::WarasError> {
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM wards")
        .fetch_one(&db.pool)
        .await
        .map_err(db_err)?;
    if n > 0 {
        return Ok(false);
    }
    let today = clinic_today();
    let t = |hh: u32, mm: u32| fmt_dt(dt(today, hh, mm));

    // -- Diagnosis utama encounter rawat inap (casemix) -----------------------
    // Seed Fase 1 hanya memberi diagnosis 'utama' ke ENC-...-014 (J20.9,
    // tidak terpetakan grouper) dan ENC-...-015 (J06.9 tapi grup Riwayat).
    // Tanpa baris ini, grouping casemix dari data seed tidak bisa
    // didemokan untuk satu pun diagnosis terpetakan (FASE3.md §8.11).
    // Empat encounter selesai di bawah ini adalah pemilik klaim seed
    // CLM-5510..5512 dan CLM-5514; diagnosisnya sesuai cerita desain.
    // (id, patient, encounter, code, display, clinician, recorded_at)
    let f3_conditions: [(&str, &str, &str, &str, &str, &str, String); 4] = [
        ("COND-011", "RM-000814", "ENC-2026-11-19-016", "J18.9",
         "Pneumonia", "dr. Rina, Sp.PD", t(11, 0)),
        ("COND-012", "RM-000816", "ENC-2026-11-19-018", "I20.9",
         "Angina pektoris", "dr. Ahmad Hidayat", t(13, 30)),
        ("COND-013", "RM-000817", "ENC-2026-11-19-019", "K35.8",
         "Apendisitis akut", "dr. Budi, Sp.B", t(14, 0)),
        ("COND-014", "RM-000821", "ENC-2026-11-19-020", "O80",
         "Persalinan normal", "dr. Rina, Sp.PD", t(15, 0)),
    ];
    for (id, patient, encounter, code, display, clinician, recorded) in f3_conditions {
        sqlx::query(
            "INSERT INTO conditions (id, patient_id, encounter_id, condition_group,
                 code, display, status, clinician, recorded_at)
             VALUES (?, ?, ?, 'utama', ?, ?, 'aktif', ?, ?)",
        )
        .bind(id)
        .bind(patient)
        .bind(encounter)
        .bind(code)
        .bind(display)
        .bind(clinician)
        .bind(recorded)
        .execute(&db.pool)
        .await
        .map_err(db_err)?;
    }

    // -- Bangsal + bed --------------------------------------------------------
    // (id, kind, floor, [(room, jumlah bed)])
    let wards: [(&str, &str, &str, &[(&str, u32)]); 5] = [
        ("Melati", "Interna", "Lantai 2", &[("3A", 12), ("3B", 10), ("3C", 10)]),
        ("Mawar", "Bedah", "Lantai 2", &[("2A", 14), ("2B", 14)]),
        ("Anggrek", "Anak", "Lantai 1", &[("1C", 12), ("1D", 12)]),
        ("Cempaka", "Obstetri", "Lantai 4", &[("4A", 12), ("4B", 12)]),
        ("ICU", "ICU", "Lantai 3", &[("ICU", 12)]),
    ];
    for (id, kind, floor, _rooms) in &wards {
        sqlx::query("INSERT INTO wards (id, name, kind, floor) VALUES (?, ?, ?, ?)")
            .bind(*id)
            .bind(*id)
            .bind(*kind)
            .bind(*floor)
            .execute(&db.pool)
            .await
            .map_err(db_err)?;
    }
    // Status bed: 3A persis kontrak; sisanya aturan isi-sampai-kuota.
    // (ward, room, bed_no) -> status; default dihitung di bawah.
    let bed_status = |ward: &str, room: &str, no: u32| -> &'static str {
        match (ward, room, no) {
            ("Melati", "3A", 1) => "terisi",
            ("Melati", "3A", 2) => "terisi",
            ("Melati", "3A", 3) => "terisi",
            ("Melati", "3A", 4) => "terisi",
            ("Melati", "3A", 5) => "dibersihkan",
            ("Melati", "3A", 6) => "terisi",
            ("Melati", "3A", 7) => "kosong",
            ("Melati", "3A", 8) => "terisi",
            ("Melati", "3A", 9) => "terisi",
            ("Melati", "3A", 10) => "kosong",
            ("Melati", "3A", 11) => "dibersihkan",
            ("Melati", "3A", 12) => "terisi",
            ("Melati", _, _) => "terisi",
            ("Mawar", "2A", 13) => "dibersihkan",
            ("Mawar", "2A", 14) => "kosong",
            ("Mawar", "2A", _) => "terisi",
            ("Mawar", "2B", 11) => "dibersihkan",
            ("Mawar", "2B", 12..=14) => "kosong",
            ("Mawar", "2B", _) => "terisi",
            ("Anggrek", _, 10) => "dibersihkan",
            ("Anggrek", _, 11..=12) => "kosong",
            ("Anggrek", _, _) => "terisi",
            ("Cempaka", _, 10) => "dibersihkan",
            ("Cempaka", _, 11..=12) => "kosong",
            ("Cempaka", _, _) => "terisi",
            ("ICU", _, 1..=2) => "dibersihkan",
            ("ICU", _, _) => "kosong",
            _ => "kosong",
        }
    };
    for (ward, _kind, _floor, rooms) in &wards {
        for (room, count) in rooms.iter() {
            for no in 1..=*count {
                let id = format!("{room}-{no:02}");
                sqlx::query(
                    "INSERT INTO ward_beds (id, ward_id, room, bed_no, status, updated_at)
                     VALUES (?, ?, ?, ?, ?, ?)",
                )
                .bind(&id)
                .bind(*ward)
                .bind(*room)
                .bind(format!("{no:02}"))
                .bind(bed_status(ward, room, no))
                .bind(t(7, 0))
                .execute(&db.pool)
                .await
                .map_err(db_err)?;
            }
        }
    }

    // -- Admisi (7) -----------------------------------------------------------
    // (id, patient, bed, encounter, diag, display, admitted, dpjp, fall)
    let admissions: [(&str, &str, &str, Option<&str>, &str, &str, NaiveDate, &str, i64); 7] = [
        ("ADM-20261119-001", "RM-000820", "3A-01", Some("ENC-2026-11-19-021"),
         "I20.9", "Angina pektoris", d(2026, 11, 17), "dr. Rina, Sp.PD", 1),
        ("ADM-20261119-002", "RM-000821", "2B-04", Some("ENC-2026-11-19-020"),
         "K35.8", "Apendisitis akut", d(2026, 11, 18), "dr. Budi, Sp.B", 0),
        ("ADM-20261119-003", "RM-000817", "2B-07", Some("ENC-2026-11-19-022"),
         "S52.5", "Fraktur radius", d(2026, 11, 19), "dr. Budi, Sp.B", 0),
        ("ADM-20261119-004", "RM-000814", "1C-02", Some("ENC-2026-11-19-016"),
         "J18.9", "Pneumonia", d(2026, 11, 18), "dr. Sari Dewi, Sp.A", 0),
        ("ADM-20261119-005", "RM-000816", "4A-03", Some("ENC-2026-11-19-018"),
         "O80", "Persalinan normal", d(2026, 11, 19), "dr. Maya, Sp.OG", 1),
        ("ADM-20261119-006", "RM-000818", "3A-03", None,
         "J18.9", "Pneumonia", d(2026, 11, 19), "dr. Rina, Sp.PD", 0),
        ("ADM-20261119-007", "RM-000819", "1C-05", None,
         "J06.9", "ISPA atas", d(2026, 11, 19), "dr. Sari Dewi, Sp.A", 0),
    ];
    for (id, patient, bed, encounter, code, display, admitted, dpjp, fall) in admissions {
        sqlx::query(
            "INSERT INTO admissions (id, patient_id, bed_id, encounter_id,
                 diagnosis_code, diagnosis_display, admitted_date, dpjp,
                 fall_risk, status, discharged_at, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, 'dirawat', NULL, ?)",
        )
        .bind(id)
        .bind(patient)
        .bind(bed)
        .bind(encounter)
        .bind(code)
        .bind(display)
        .bind(admitted.to_string())
        .bind(dpjp)
        .bind(fall)
        .bind(fmt_dt(dt(admitted, 8, 0)))
        .execute(&db.pool)
        .await
        .map_err(db_err)?;
    }
    // Catatan perawatan seed.
    let notes: [(&str, &str, &str, &str, &str); 2] = [
        ("CRN-000001", "ADM-20261119-001",
         "Nyeri dada berkurang, TD 145/90, observasi ketat.",
         "Ns. Sari Wijaya", "2026-11-19 08:00:00"),
        ("CRN-000002", "ADM-20261119-005",
         "Pembukaan 6 cm, DJJ 140, observasi VK.",
         "Bidan Sari", "2026-11-19 09:00:00"),
    ];
    for (id, adm, note, by, at) in notes {
        sqlx::query(
            "INSERT INTO care_notes (id, admission_id, note, recorded_by, recorded_at)
             VALUES (?, ?, ?, ?, ?)",
        )
        .bind(id)
        .bind(adm)
        .bind(note)
        .bind(by)
        .bind(at)
        .execute(&db.pool)
        .await
        .map_err(db_err)?;
    }

    // -- IGD (8) ---------------------------------------------------------------
    // (id, patient, encounter, complaint, triage, doctor, status, arrived)
    let igd: [(&str, &str, Option<&str>, &str, &str, &str, &str, &str); 8] = [
        ("IGD-20261119-001", "RM-000820", Some("ENC-2026-11-19-021"),
         "Nyeri dada, sesak", "merah", "dr. Andi Wijaya", "resusitasi", "08.02"),
        ("IGD-20261119-002", "RM-000817", Some("ENC-2026-11-19-022"),
         "Patah lengan kanan", "kuning", "dr. Sari Dewi", "observasi", "08.15"),
        ("IGD-20261119-003", "RM-000822", None,
         "Demam 39,5 °C", "kuning", "dr. Ahmad Hidayat", "observasi", "08.21"),
        ("IGD-20261119-004", "RM-000821", None,
         "Luka robek dahi", "hijau", "dr. Ahmad Hidayat", "tindakan", "08.33"),
        ("IGD-20261119-005", "RM-000823", None,
         "Mual, muntah", "hijau", "dr. Sari Dewi", "menunggu", "08.47"),
        ("IGD-20261119-006", "RM-000816", None,
         "Sesak ringan", "kuning", "dr. Andi Wijaya", "observasi", "09.05"),
        ("IGD-20261119-007", "RM-000818", None,
         "Luka sayat tangan", "hijau", "dr. Sari Dewi", "tindakan", "09.18"),
        ("IGD-20261119-008", "RM-000819", None,
         "Demam 38,8 °C", "hijau", "dr. Ahmad Hidayat", "menunggu", "09.26"),
    ];
    for (id, patient, encounter, complaint, triage, doctor, status, arrived) in igd {
        let (hh, mm): (u32, u32) = {
            let mut parts = arrived.split('.');
            (
                parts.next().unwrap_or("8").parse().unwrap_or(8),
                parts.next().unwrap_or("0").parse().unwrap_or(0),
            )
        };
        sqlx::query(
            "INSERT INTO igd_visits (id, patient_id, encounter_id, complaint,
                 triage, doctor, status, arrived_at, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(id)
        .bind(patient)
        .bind(encounter)
        .bind(complaint)
        .bind(triage)
        .bind(doctor)
        .bind(status)
        .bind(arrived)
        .bind(fmt_dt(dt(today, hh, mm)))
        .execute(&db.pool)
        .await
        .map_err(db_err)?;
    }
    // Observasi seed (Ruang 2).
    let igd_obs: [(&str, &str, &str, &str, &str); 2] = [
        ("IGO-000001", "IGD-20261119-001",
         "TD 150/95 · Nadi 104 · SpO2 96% · EKG: sinus takikardi. Rencana: rontgen thorax, troponin, observasi 4 jam.",
         "dr. Andi Wijaya", "2026-11-19 08:30:00"),
        ("IGO-000002", "IGD-20261119-002",
         "TD 120/80 · Nadi 88 · SpO2 99% · X-Ray: fraktur radius distal. Rencana: gips, kontrol ortopedi 1 minggu.",
         "dr. Sari Dewi", "2026-11-19 08:45:00"),
    ];
    for (id, igd_id, note, by, at) in igd_obs {
        sqlx::query(
            "INSERT INTO igd_observations (id, igd_id, note, recorded_by, recorded_at)
             VALUES (?, ?, ?, ?, ?)",
        )
        .bind(id)
        .bind(igd_id)
        .bind(note)
        .bind(by)
        .bind(at)
        .execute(&db.pool)
        .await
        .map_err(db_err)?;
    }

    // -- Operasi (6) + bersalin ------------------------------------------------
    // (id, patient, encounter, room, procedure, code, operator, team, jam, status)
    let team_ok1 = "dr. Budi, Sp.B · dr. Ani, Sp.An · 2 perawat";
    let team_ok2 = "dr. Rina, Sp.JP · dr. Ani, Sp.An · 2 perawat";
    let team_ok3 = "dr. Maya, Sp.OG · dr. Ani, Sp.An · 1 bidan";
    let surgeries: [(&str, &str, Option<&str>, &str, &str, &str, &str, &str, (u32, u32), &str); 6] = [
        ("OK-20261119-001", "RM-000821", Some("ENC-2026-11-19-020"), "OK 1",
         "Apendektomi", "47.0", "dr. Budi, Sp.B", team_ok1, (8, 0), "selesai"),
        ("OK-20261119-002", "RM-000820", Some("ENC-2026-11-19-021"), "OK 2",
         "Kateterisasi jantung", "37.22", "dr. Rina, Sp.JP", team_ok2, (9, 30), "berjalan"),
        ("OK-20261119-003", "RM-000815", Some("ENC-2026-11-19-017"), "OK 1",
         "Hernioplasti", "53.0", "dr. Budi, Sp.B", team_ok1, (11, 0), "siap"),
        ("OK-20261119-004", "RM-000816", Some("ENC-2026-11-19-018"), "OK 3",
         "Seksio sesarea", "74.1", "dr. Maya, Sp.OG", team_ok3, (13, 0), "siap"),
        ("OK-20261119-005", "RM-000817", Some("ENC-2026-11-19-022"), "OK 2",
         "ORIF radius", "79.32", "dr. Budi, Sp.B", team_ok2, (14, 30), "menunggu"),
        ("OK-20261119-006", "RM-000821", Some("ENC-2026-11-19-020"), "OK 3",
         "Histerektomi", "68.4", "dr. Maya, Sp.OG", team_ok3, (16, 0), "terjadwal"),
    ];
    for (id, patient, encounter, room, proc, code, operator, team, (hh, mm), status) in surgeries {
        sqlx::query(
            "INSERT INTO surgeries (id, patient_id, encounter_id, room, procedure,
                 procedure_code, operator, team, scheduled_at, status, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(id)
        .bind(patient)
        .bind(encounter)
        .bind(room)
        .bind(proc)
        .bind(code)
        .bind(operator)
        .bind(team)
        .bind(fmt_dt(dt(today, hh, mm)))
        .bind(status)
        .bind(t(7, 30))
        .execute(&db.pool)
        .await
        .map_err(db_err)?;
    }
    // Ruang bersalin VK 1..4: VK 1 + VK 2 Terisi, VK 3 + VK 4 Kosong.
    for (id, status) in [("VK 1", "terisi"), ("VK 2", "terisi"), ("VK 3", "kosong"), ("VK 4", "kosong")] {
        sqlx::query("INSERT INTO delivery_rooms (id, status) VALUES (?, ?)")
            .bind(id)
            .bind(status)
            .execute(&db.pool)
            .await
            .map_err(db_err)?;
    }
    sqlx::query(
        "INSERT INTO deliveries (id, patient_id, age_years, gravida_para,
             dilation_cm, fetal_heart_rate, midwife, doctor, plan, room,
             status, created_at)
         VALUES ('VK-20261119-001', 'RM-000816', 34, 'G2P1', 6, 140,
             'Bidan Sari', 'dr. Maya, Sp.OG',
             'Persalinan pervaginam, siaga seksio sesarea', 'VK 2', 'Aktif', ?)",
    )
    .bind(t(9, 0))
    .execute(&db.pool)
    .await
    .map_err(db_err)?;

    // -- Lab (8 order + hasil) --------------------------------------------------
    // (id, patient, encounter, test, loinc, from, by, status, jam)
    let lab_orders: [(&str, &str, Option<&str>, &str, &str, &str, &str, &str, (u32, u32)); 8] = [
        ("LAB-2210", "RM-000812", Some("ENC-2026-11-19-014"), "Darah lengkap", "58410-2",
         "Poli Umum", "dr. Ahmad Hidayat", "selesai", (8, 5)),
        ("LAB-2211", "RM-000820", Some("ENC-2026-11-19-021"), "Troponin I", "10839-9",
         "IGD", "dr. Andi Wijaya", "kritis", (8, 10)),
        ("LAB-2212", "RM-000814", Some("ENC-2026-11-19-016"), "HbA1c", "4548-4",
         "Penyakit Dalam", "dr. Rina, Sp.PD", "selesai", (8, 20)),
        ("LAB-2213", "RM-000816", Some("ENC-2026-11-19-018"), "Urinalisis lengkap", "24357-6",
         "Cempaka", "dr. Maya, Sp.OG", "proses", (8, 30)),
        ("LAB-2214", "RM-000817", Some("ENC-2026-11-19-022"), "Golongan darah", "883-9",
         "IGD", "dr. Sari Dewi", "selesai", (8, 40)),
        ("LAB-2215", "RM-000821", Some("ENC-2026-11-19-020"), "Elektrolit (Na/K)", "2951-2",
         "Mawar", "dr. Budi, Sp.B", "proses", (8, 50)),
        ("LAB-2216", "RM-000818", None, "Gula darah puasa", "1558-6",
         "Poli Umum", "dr. Ahmad Hidayat", "proses", (9, 0)),
        ("LAB-2217", "RM-000819", None, "Darah lengkap", "58410-2",
         "Poli Anak", "dr. Sari Dewi", "selesai", (9, 10)),
    ];
    for (id, patient, encounter, test, loinc, from, by, status, (hh, mm)) in lab_orders {
        sqlx::query(
            "INSERT INTO lab_orders (id, patient_id, encounter_id, test_name,
                 loinc, ordered_from, ordered_by, status, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(id)
        .bind(patient)
        .bind(encounter)
        .bind(test)
        .bind(loinc)
        .bind(from)
        .bind(by)
        .bind(status)
        .bind(fmt_dt(dt(today, hh, mm)))
        .execute(&db.pool)
        .await
        .map_err(db_err)?;
    }
    // Hasil LAB-2211 (persis desain) + LAB-2210 (seed, nilai normal).
    // (id, order, analyte, loinc, value, unit, ref_text, low, high, crit_high, crit_low, flag, jam)
    let lab_results: [(&str, &str, &str, Option<&str>, &str, &str, &str, Option<f64>, Option<f64>, Option<f64>, Option<f64>, &str, (u32, u32)); 8] = [
        ("LBR-000001", "LAB-2211", "Troponin I", Some("10839-9"), "0,42", "ng/mL",
         "< 0,04 ng/mL", None, Some(0.04), Some(0.04), None, "kritis", (9, 5)),
        ("LBR-000002", "LAB-2211", "Hemoglobin", Some("718-7"), "13,8", "g/dL",
         "13,0–17,0", Some(13.0), Some(17.0), None, None, "normal", (9, 5)),
        ("LBR-000003", "LAB-2211", "Leukosit", Some("6690-2"), "11.200", "/µL",
         "4.000–10.000", Some(4_000.0), Some(10_000.0), None, None, "tinggi", (9, 5)),
        ("LBR-000004", "LAB-2211", "Trombosit", Some("777-3"), "245.000", "/µL",
         "150.000–400.000", Some(150_000.0), Some(400_000.0), None, None, "normal", (9, 5)),
        ("LBR-000005", "LAB-2211", "Kreatinin", Some("2160-0"), "1,1", "mg/dL",
         "0,7–1,3", Some(0.7), Some(1.3), None, None, "normal", (9, 5)),
        ("LBR-000006", "LAB-2210", "Hemoglobin", Some("718-7"), "14,2", "g/dL",
         "13,0–17,0", Some(13.0), Some(17.0), None, None, "normal", (8, 35)),
        ("LBR-000007", "LAB-2210", "Leukosit", Some("6690-2"), "8.900", "/µL",
         "4.000–10.000", Some(4_000.0), Some(10_000.0), None, None, "normal", (8, 35)),
        ("LBR-000008", "LAB-2210", "Trombosit", Some("777-3"), "280.000", "/µL",
         "150.000–400.000", Some(150_000.0), Some(400_000.0), None, None, "normal", (8, 35)),
    ];
    for (id, order, analyte, loinc, value, unit, ref_text, low, high, crit_high, crit_low, flag, (hh, mm)) in lab_results {
        sqlx::query(
            "INSERT INTO lab_results (id, order_id, analyte, loinc, value_text,
                 unit, ref_text, ref_low, ref_high, critical_high, critical_low,
                 flag, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(id)
        .bind(order)
        .bind(analyte)
        .bind(loinc)
        .bind(value)
        .bind(unit)
        .bind(ref_text)
        .bind(low)
        .bind(high)
        .bind(crit_high)
        .bind(crit_low)
        .bind(flag)
        .bind(fmt_dt(dt(today, hh, mm)))
        .execute(&db.pool)
        .await
        .map_err(db_err)?;
    }

    // -- Radiologi (7 order + 3 laporan) ----------------------------------------
    // (id, patient, encounter, modality, area, from, by, status, jam)
    let rad_orders: [(&str, &str, Option<&str>, &str, &str, &str, &str, &str, (u32, u32)); 7] = [
        ("RAD-0911", "RM-000820", Some("ENC-2026-11-19-021"), "X-Ray", "Thorax AP",
         "IGD", "dr. Andi Wijaya", "selesai", (8, 15)),
        ("RAD-0912", "RM-000817", Some("ENC-2026-11-19-022"), "X-Ray", "Antebrachii dextra",
         "IGD", "dr. Sari Dewi", "selesai", (8, 25)),
        ("RAD-0913", "RM-000816", Some("ENC-2026-11-19-018"), "USG", "Obstetri trimester 3",
         "Cempaka", "dr. Maya, Sp.OG", "selesai", (8, 35)),
        ("RAD-0914", "RM-000814", Some("ENC-2026-11-19-016"), "CT Scan", "Abdomen kontras",
         "Penyakit Dalam", "dr. Rina, Sp.PD", "menunggu", (8, 45)),
        ("RAD-0915", "RM-000821", Some("ENC-2026-11-19-020"), "X-Ray", "Thorax post-op",
         "Mawar", "dr. Budi, Sp.B", "proses", (8, 55)),
        ("RAD-0916", "RM-000818", None, "USG", "Abdomen",
         "Poli Umum", "dr. Ahmad Hidayat", "menunggu", (9, 5)),
        ("RAD-0917", "RM-000819", None, "X-Ray", "Thorax AP",
         "Poli Anak", "dr. Sari Dewi", "menunggu", (9, 15)),
    ];
    for (id, patient, encounter, modality, area, from, by, status, (hh, mm)) in rad_orders {
        sqlx::query(
            "INSERT INTO rad_orders (id, patient_id, encounter_id, modality, area,
                 ordered_from, ordered_by, status, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(id)
        .bind(patient)
        .bind(encounter)
        .bind(modality)
        .bind(area)
        .bind(from)
        .bind(by)
        .bind(status)
        .bind(fmt_dt(dt(today, hh, mm)))
        .execute(&db.pool)
        .await
        .map_err(db_err)?;
    }
    // Laporan: 0912 persis desain; 0911/0913 seed.
    let rad_reports: [(&str, &str, &str, &str, (u32, u32)); 3] = [
        ("RDR-000001", "RAD-0912",
         "Fraktur radius distal, tampak garis fraktur transversal.",
         "dr. Andi Wijaya, Sp.Rad", (9, 5)),
        ("RDR-000002", "RAD-0911",
         "Tidak tampak infiltrat; cor dalam batas normal.",
         "dr. Andi Wijaya, Sp.Rad", (8, 50)),
        ("RDR-000003", "RAD-0913",
         "Janin tunggal hidup, presentasi kepala, taksiran berat 2.900 g.",
         "dr. Maya, Sp.OG", (9, 10)),
    ];
    for (id, order, findings, radiologist, (hh, mm)) in rad_reports {
        sqlx::query(
            "INSERT INTO rad_reports (id, order_id, findings, radiologist, reported_at)
             VALUES (?, ?, ?, ?, ?)",
        )
        .bind(id)
        .bind(order)
        .bind(findings)
        .bind(radiologist)
        .bind(fmt_dt(dt(today, hh, mm)))
        .execute(&db.pool)
        .await
        .map_err(db_err)?;
    }

    // -- Casemix (8 klaim; invoice_id NULL) ---------------------------------------
    // (id, patient, diag, display, code, amount, status, note)
    let claims: [(&str, &str, &str, &str, i64, &str, &str); 8] = [
        ("CLM-5510", "RM-000820", "I20.9 Angina pektoris", "I-4-13-I", 4_210_000, "dibayar",
         "Diagnosis utama: I20.9 Angina pektoris. Grouping lokal (perkiraan): I-4-13-I."),
        ("CLM-5511", "RM-000821", "K35.8 Apendisitis akut", "G-4-11-II", 6_840_000, "terverifikasi",
         "Diagnosis utama: K35.8 Apendisitis akut. Grouping lokal (perkiraan): G-4-11-II."),
        ("CLM-5512", "RM-000814", "J18.9 Pneumonia", "J-4-12-I", 3_150_000, "diajukan",
         "Diagnosis utama: J18.9 Pneumonia. Grouping lokal (perkiraan): J-4-12-I."),
        ("CLM-5513", "RM-000817", "S52.5 Fraktur radius", "H-4-15-I", 5_420_000, "pending",
         "Kode prosedur ORIF (79.32) belum terpetakan ke grouping. Perbaiki pengkodean, lalu kirim ulang."),
        ("CLM-5514", "RM-000816", "O80 Persalinan normal", "O-4-10-I", 2_980_000, "diajukan",
         "Diagnosis utama: O80 Persalinan normal. Grouping lokal (perkiraan): O-4-10-I."),
        ("CLM-5515", "RM-000814", "J06.9 ISPA atas", "J-4-10-0", 210_000, "dibayar",
         "Diagnosis utama: J06.9 ISPA atas. Grouping lokal (perkiraan): J-4-10-0."),
        ("CLM-5516", "RM-000818", "J06.9 ISPA atas", "J-4-10-0", 210_000, "diajukan",
         "Diagnosis utama: J06.9 ISPA atas. Grouping lokal (perkiraan): J-4-10-0."),
        ("CLM-5517", "RM-000819", "J03.9 Tonsilitis", "J-4-10-0", 185_000, "diajukan",
         "Diagnosis utama: J03.9 Tonsilitis. Grouping lokal (perkiraan): J-4-10-0."),
    ];
    for (id, patient, _diag, code, amount, status, note) in claims {
        sqlx::query(
            "INSERT INTO claims (id, invoice_id, patient_id, sep_id, ina_cbg_code, amount,
                 status, submitted_at, created_at, grouping_note)
             VALUES (?, NULL, ?, NULL, ?, ?, ?, NULL, ?, ?)",
        )
        .bind(id)
        .bind(patient)
        .bind(code)
        .bind(amount)
        .bind(status)
        .bind(t(7, 30))
        .bind(note)
        .execute(&db.pool)
        .await
        .map_err(db_err)?;
    }

    Ok(true)
}

impl Db {
    /// Seed Fase 3 bila tabel `wards` masih kosong (FASE3.md §4.2).
    ///
    /// Alasan sama seperti Fase 2: `seed()` hanya dipanggil
    /// `seed_if_empty` bila `users` kosong. DB lama yang mendapat
    /// migrasi 0004 tidak pernah menjalankan `seed()` lagi. Dipanggil
    /// binary sekali saat boot, tepat setelah `seed_fase2_if_empty()`.
    /// Mengembalikan `true` bila seed dijalankan.
    pub async fn seed_fase3_if_empty(&self) -> Result<bool, waras_core::WarasError> {
        let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM wards")
            .fetch_one(&self.pool)
            .await
            .map_err(db_err)?;
        if n > 0 {
            return Ok(false);
        }
        seed_fase3(self).await?;
        Ok(true)
    }
}

/// Seed Fase 4 (FASE4.md §4.2). Idempotent: hanya jalan bila tabel
/// `hd_machines` kosong. Data dari layar desain v16 — bila layar dan
/// kontrak berbeda, KONTRAK yang menang (penyimpangan dicatat di bawah).
/// Mengembalikan `true` bila seed dijalankan.
///
/// Catatan yang tidak ditentukan kontrak (dicatat agar bisa diaudit):
/// - Pasien baru dimasukkan DI SINI (bukan di array pasien `seed()`),
///   supaya `seed_fase4_if_empty` pada DB lama (Fase 1–3) juga
///   menambahkan RM-000824/825 — pola Fase 3 (pasien di array `seed()`)
///   tidak menambahkan pasiennya ke DB lama. Total pasien jadi 14;
///   `create_patient` berikutnya jadi RM-000826.
/// - Sesi HD tidak menaut encounter (tidak ada encounter HD di seed);
///   `encounter_id` NULL. Dewi Selesai tapi HD-2 tetap Terpakai —
///   persis kontrak §4.2 (mesin belum di-reset), walau aturan repo
///   (`set_hd_session_status` Selesai -> Siap) akan membebaskannya
///   bila status diubah lewat API.
/// - `requested_by` permintaan darah: Bambang "dr. Rina, Sp.JP"
///   (operator kateterisasi seed Fase 3), Sri "dr. Budi, Sp.B"
///   (operator apendektomi seed Fase 3). Jam crossmatch 09.30.
/// - `assessed_by` asesmen gizi: "Ns. Sari Wijaya" (tidak ada
///   dietisian di pengguna seed). `notes` diet kosong.
/// - `dentist` rekam gigi: "drg. Maya K." (nama di encounter Gigi
///   seed Fase 1); Agus menaut encounter Gigi ENC-2026-11-19-017,
///   yang lain NULL (tidak punya encounter Gigi).
/// - Inventori: kategori semua "Alkes", lokasi "Gudang A",
///   `min_stock` 30, `expiry` kosong (kontrak tidak memberi nilai);
///   stok yang di-insert adalah stok SEBELUM mutasi seed, lalu 3
///   mutasi diterapkan lewat `add_inventory_move` (aturan repo) —
///   stok akhir sama dengan angka kontrak (240 / 320 / 8).
///   Mutasi oleh "apt. Dina Puspita".
/// - Staf: 6 pengguna seed + drg. Maya + Bidan Sari + dr. Andi Wijaya
///   (STF-009) — kontrak §4.2 menulis "Staf 8", tapi shift ke-8
///   (Andi Wijaya, Malam IGD) butuh baris staf; tanpa STF-009,
///   `staff_id` shift itu menggantung. Total staf 9, shift 8.
/// - Layar vs kontrak (kontrak menang): HD layar 6 mesin/8 klaim
///   + pasien Bambang/Sri/Joko/Dewi/Agus/Rina + dialyzer FX 80/60
///   (kontrak: 4 mesin, 3 sesi Slamet/Dewi/Agus, F6/F8); transfusi
///   layar 18/4/14/3/6/2/22/5 kantong + 7 permintaan TRF-0331..337
///   (kontrak: 2/2/2/2/1/2/2/0 + 2 permintaan); gizi layar IMT Dewi
///   27,4 + 6 diet berbeda (kontrak: BMI 27,1 dari 71 kg/162 cm +
///   3 diet); gigi layar nomor 19/20/47–62 bukan FDI + gigi 16/23/
///   32/38 K02.1/K04.0 (kontrak: 5 rekam FDI); inventori layar
///   INV-M001..M007/U001 stok ribuan (kontrak: 10 item); kepegawaian
///   layar 142 pegawai (kontrak: 8+1 staf); laporan layar agregat
///   ilustrasi BOR 71,7% dkk. (kontrak: agregat dari tabel nyata);
///   rawat jalan layar 9 kunjungan + diagnosis H52.1/K02.1
///   (kontrak: agregat antrean+encounter+janji, tanpa seed baris).
pub async fn seed_fase4(db: &Db) -> Result<bool, waras_core::WarasError> {
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM hd_machines")
        .fetch_one(&db.pool)
        .await
        .map_err(db_err)?;
    if n > 0 {
        return Ok(false);
    }
    let today = clinic_today();
    let t = |hh: u32, mm: u32| fmt_dt(dt(today, hh, mm));

    // -- Pasien baru (RM-000824/825) + kondisi kronik Slamet -------------
    // Kata sandi portal demo: `waras123`, sama seperti pasien seed lain.
    let portal_hash = Db::hash_password("waras123")?;
    let new_patients: [(&str, &str, &str, NaiveDate, Gender, &str, &str, Option<&str>, Insurance); 2] = [
        ("RM-000824", "3374031005720013", "Slamet Riyadi", d(1972, 5, 10), Gender::LakiLaki,
         "Jl. Tlogosari No. 18, Semarang", "O+", Some("0001234567898"), Insurance::Bpjs),
        ("RM-000825", "3374042505850014", "Ratna Sari", d(1985, 5, 25), Gender::Perempuan,
         "Jl. Lamper Tengah No. 5, Semarang", "A+", None, Insurance::Umum),
    ];
    for (rm, nik, name, birth, gender, address, blood, bpjs, insurance) in new_patients {
        sqlx::query(
            "INSERT INTO patients (id, nik, name, birth_date, gender, address, phone,
                 blood_type, bpjs_number, insurance, created_at, portal_password_hash)
             VALUES (?, ?, ?, ?, ?, ?, NULL, ?, ?, ?, ?, ?)",
        )
        .bind(rm)
        .bind(nik)
        .bind(name)
        .bind(birth.to_string())
        .bind(gender_to_str(gender))
        .bind(address)
        .bind(blood)
        .bind(bpjs)
        .bind(insurance_to_str(insurance))
        .bind(t(7, 0))
        .bind(&portal_hash)
        .execute(&db.pool)
        .await
        .map_err(db_err)?;
    }
    // Hanya Slamet yang diberi kondisi kronik N18.9 (FASE4.md §8.3).
    sqlx::query(
        "INSERT INTO conditions (id, patient_id, encounter_id, condition_group,
             code, display, status, clinician, recorded_at)
         VALUES ('COND-015', 'RM-000824', NULL, 'kronis',
             'N18.9', 'Gagal ginjal kronik', 'kronis', 'dr. Rina, Sp.PD', ?)",
    )
    .bind(t(7, 0))
    .execute(&db.pool)
    .await
    .map_err(db_err)?;

    // -- Hemodialisa: 4 mesin + 3 sesi --------------------------------------
    let machines: [(&str, &str, &str); 4] = [
        ("HD-1", "Mesin HD 1", "terpakai"),
        ("HD-2", "Mesin HD 2", "terpakai"),
        ("HD-3", "Mesin HD 3", "siap"),
        ("HD-4", "Mesin HD 4", "perawatan"),
    ];
    for (id, name, status) in machines {
        sqlx::query("INSERT INTO hd_machines (id, name, status) VALUES (?, ?, ?)")
            .bind(id)
            .bind(name)
            .bind(status)
            .execute(&db.pool)
            .await
            .map_err(db_err)?;
    }
    // (id, patient, machine, dialyzer, status, jadwal, mulai, selesai)
    let hd_sessions: [(&str, &str, &str, &str, &str, &str, Option<&str>, Option<&str>); 3] = [
        ("HD-20261119-001", "RM-000824", "HD-1", "F6", "berjalan",
         "2026-11-19 07:00", Some("2026-11-19 07:00"), None),
        ("HD-20261119-002", "RM-000814", "HD-2", "F8", "selesai",
         "2026-11-19 06:00", Some("2026-11-19 06:00"), Some("2026-11-19 10:00")),
        ("HD-20261119-003", "RM-000815", "HD-3", "F6", "terjadwal",
         "2026-11-19 13:00", None, None),
    ];
    for (id, patient, machine, dialyzer, status, scheduled, started, finished) in hd_sessions {
        sqlx::query(
            "INSERT INTO hd_sessions (id, patient_id, machine_id, encounter_id,
                 dialyzer, duration_minutes, blood_flow, dialysate_flow,
                 uf_goal_ml, status, scheduled_at, started_at, finished_at,
                 notes, created_at)
             VALUES (?, ?, ?, NULL, ?, 240, 300, 500, 2000, ?, ?, ?, ?, '', ?)",
        )
        .bind(id)
        .bind(patient)
        .bind(machine)
        .bind(dialyzer)
        .bind(status)
        .bind(scheduled)
        .bind(started)
        .bind(finished)
        .bind(t(7, 0))
        .execute(&db.pool)
        .await
        .map_err(db_err)?;
    }

    // -- Transfusi: 8 stok PRC + 2 permintaan + 1 crossmatch -----------------
    // (id, golongan, kantong, status)
    let blood: [(&str, &str, i64, &str); 8] = [
        ("BL-A-POS-001", "A+", 2, "Tersedia"),
        ("BL-A-NEG-001", "A-", 2, "Tersedia"),
        ("BL-B-POS-001", "B+", 2, "Tersedia"),
        ("BL-B-NEG-001", "B-", 2, "Tersedia"),
        ("BL-O-POS-001", "O+", 1, "Menipis"),
        ("BL-O-NEG-001", "O-", 2, "Tersedia"),
        ("BL-AB-POS-001", "AB+", 2, "Tersedia"),
        ("BL-AB-NEG-001", "AB-", 0, "Habis"),
    ];
    for (id, blood_type, units, status) in blood {
        sqlx::query(
            "INSERT INTO blood_stock (id, blood_type, component, units, expires_at, status)
             VALUES (?, ?, 'PRC', ?, '2026-12-19', ?)",
        )
        .bind(id)
        .bind(blood_type)
        .bind(units)
        .bind(status)
        .execute(&db.pool)
        .await
        .map_err(db_err)?;
    }
    // (id, patient, golongan, kantong, alasan, status, diminta_oleh, jam)
    let requests: [(&str, &str, &str, i64, &str, &str, &str, (u32, u32)); 2] = [
        ("TR-20261119-001", "RM-000820", "O+", 2, "Pasca kateterisasi jantung",
         "menunggu", "dr. Rina, Sp.JP", (8, 30)),
        ("TR-20261119-002", "RM-000821", "B+", 1, "Pasca apendektomi",
         "crossmatch", "dr. Budi, Sp.B", (9, 15)),
    ];
    for (id, patient, blood_type, units, reason, status, by, (hh, mm)) in requests {
        sqlx::query(
            "INSERT INTO blood_requests (id, patient_id, encounter_id, blood_type,
                 component, units, reason, status, requested_by, created_at)
             VALUES (?, ?, NULL, ?, 'PRC', ?, ?, ?, ?, ?)",
        )
        .bind(id)
        .bind(patient)
        .bind(blood_type)
        .bind(units)
        .bind(reason)
        .bind(status)
        .bind(by)
        .bind(fmt_dt(dt(today, hh, mm)))
        .execute(&db.pool)
        .await
        .map_err(db_err)?;
    }
    sqlx::query(
        "INSERT INTO blood_crossmatches (id, request_id, bag_id, result, tested_by, tested_at)
         VALUES ('XM-000001', 'TR-20261119-002', 'BL-B-POS-001', 'Cocok', 'Ns. Sari Wijaya', ?)",
    )
    .bind(t(9, 30))
    .execute(&db.pool)
    .await
    .map_err(db_err)?;

    // -- Gizi: 4 asesmen (BMI dihitung core) + 3 diet -------------------------
    // (id, patient, berat, tinggi, risiko, diet, jam)
    let assessments: [(&str, &str, f64, f64, &str, &str, (u32, u32)); 4] = [
        ("GZ-000001", "RM-000812", 62.0, 160.0, "Rendah", "DM 1800 kkal", (8, 0)),
        ("GZ-000002", "RM-000820", 78.0, 170.0, "Sedang", "Jantung 2000 kkal", (8, 30)),
        ("GZ-000003", "RM-000816", 58.0, 158.0, "Rendah", "Nifas 2200 kkal", (9, 0)),
        ("GZ-000004", "RM-000814", 71.0, 162.0, "Sedang", "DM 1700 kkal", (9, 30)),
    ];
    for (id, patient, weight, height, risk, diet, (hh, mm)) in assessments {
        let bmi_value = (bmi(weight, height) * 10.0).round() / 10.0;
        let category = bmi_category(bmi_value);
        sqlx::query(
            "INSERT INTO nutrition_assessments (id, patient_id, encounter_id,
                 weight_kg, height_cm, bmi, imt_category, risk, diet, notes,
                 assessed_by, created_at)
             VALUES (?, ?, NULL, ?, ?, ?, ?, ?, ?, '', 'Ns. Sari Wijaya', ?)",
        )
        .bind(id)
        .bind(patient)
        .bind(weight)
        .bind(height)
        .bind(bmi_value)
        .bind(category)
        .bind(risk)
        .bind(diet)
        .bind(fmt_dt(dt(today, hh, mm)))
        .execute(&db.pool)
        .await
        .map_err(db_err)?;
    }
    // (id, patient, tipe, kalori)
    let diets: [(&str, &str, &str, i64); 3] = [
        ("DT-000001", "RM-000812", "DM", 1800),
        ("DT-000002", "RM-000820", "Jantung", 2000),
        ("DT-000003", "RM-000814", "DM", 1700),
    ];
    for (id, patient, diet_type, calories) in diets {
        sqlx::query(
            "INSERT INTO nutrition_diets (id, patient_id, diet_type, calories,
                 notes, start_date, status)
             VALUES (?, ?, ?, ?, '', ?, 'Aktif')",
        )
        .bind(id)
        .bind(patient)
        .bind(diet_type)
        .bind(calories)
        .bind(today.to_string())
        .execute(&db.pool)
        .await
        .map_err(db_err)?;
    }

    // -- Rekam medis gigi: 5 rekam ---------------------------------------------
    // (id, patient, encounter, gigi, diagnosis, kode, tindakan, kode, status, jam)
    let dental: [(&str, &str, Option<&str>, &str, &str, &str, &str, &str, &str, (u32, u32)); 5] = [
        ("GG-000001", "RM-000815", Some("ENC-2026-11-19-017"), "36",
         "Karies dentin", "K02.9", "Tambal komposit", "23.2", "Selesai", (8, 0)),
        ("GG-000002", "RM-000812", None, "11",
         "Atrisi email", "K03.0", "Scaling", "89.42", "Selesai", (9, 0)),
        ("GG-000003", "RM-000813", None, "46",
         "Karies dentin", "K02.9", "Tambal komposit", "23.2", "Rencana", (10, 0)),
        ("GG-000004", "RM-000819", None, "21",
         "Gigi supernumerary", "K00.6", "Observasi", "", "Selesai", (11, 0)),
        ("GG-000005", "RM-000817", None, "16",
         "Karies dentin", "K02.9", "Pencabutan", "23.1", "Selesai", (13, 0)),
    ];
    for (id, patient, encounter, tooth, diagnosis, code, treatment, tcode, status, (hh, mm)) in dental {
        sqlx::query(
            "INSERT INTO dental_records (id, patient_id, encounter_id, tooth_number,
                 diagnosis, diagnosis_code, treatment, treatment_code, dentist,
                 status, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, 'drg. Maya K.', ?, ?)",
        )
        .bind(id)
        .bind(patient)
        .bind(encounter)
        .bind(tooth)
        .bind(diagnosis)
        .bind(code)
        .bind(treatment)
        .bind(tcode)
        .bind(status)
        .bind(fmt_dt(dt(today, hh, mm)))
        .execute(&db.pool)
        .await
        .map_err(db_err)?;
    }

    // -- Inventori: 10 item (stok SEBELUM mutasi) + 3 mutasi -------------------
    // (id, nama, satuan, stok_awal)
    let items: [(&str, &str, &str, i64); 10] = [
        ("INV-ITM-001", "Infus set", "pcs", 140),
        ("INV-ITM-002", "Sarung tangan", "box", 180),
        ("INV-ITM-003", "Masker bedah", "box", 360),
        ("INV-ITM-004", "Alkohol 70%", "botol", 45),
        ("INV-ITM-005", "Kasa steril", "pcs", 90),
        ("INV-ITM-006", "Spuit 5 mL", "pcs", 150),
        ("INV-ITM-007", "Benang jahit 3-0", "pcs", 28),
        ("INV-ITM-008", "Kateter urine", "pcs", 15),
        ("INV-ITM-009", "Oksigen tabung", "tabung", 4),
        ("INV-ITM-010", "Handscoon steril", "box", 5),
    ];
    for (id, name, unit, stock) in items {
        sqlx::query(
            "INSERT INTO inventory_items (id, name, category, unit, stock,
                 min_stock, location, expiry, created_at)
             VALUES (?, ?, 'Alkes', ?, ?, 30, 'Gudang A', '', ?)",
        )
        .bind(id)
        .bind(name)
        .bind(unit)
        .bind(stock)
        .bind(t(7, 0))
        .execute(&db.pool)
        .await
        .map_err(db_err)?;
    }
    // Mutasi lewat aturan repo (stok akhir = angka kontrak §4.2).
    db.add_inventory_move("INV-ITM-001", "Masuk", 100, "Pengadaan November", "apt. Dina Puspita")
        .await?;
    db.add_inventory_move("INV-ITM-003", "Keluar", 40, "Distribusi poli", "apt. Dina Puspita")
        .await?;
    db.add_inventory_move("INV-ITM-009", "Masuk", 4, "Isi ulang", "apt. Dina Puspita")
        .await?;

    // -- Kepegawaian: 9 staf + 8 shift ------------------------------------------
    // (id, nama, nip, peran, poli)
    let staff: [(&str, &str, &str, &str, &str); 9] = [
        ("STF-001", "dr. Ahmad Hidayat", "198505122010011001", "Dokter Umum", "Poli Umum"),
        ("STF-002", "Ns. Sari Wijaya", "199001152015032002", "Perawat", "Poli Umum"),
        ("STF-003", "dr. Rina, Sp.PD", "198208102008122003", "Dokter Umum", "Penyakit Dalam"),
        ("STF-004", "apt. Dina Puspita", "199303252019032004", "Apoteker", "Farmasi"),
        ("STF-005", "Sinta Maharani", "199507082021012005", "Kasir", "Kasir"),
        ("STF-006", "Admin Sistem", "199001012015011006", "Admin", ""),
        ("STF-007", "drg. Maya Kusuma", "", "Dokter Gigi", "Gigi"),
        ("STF-008", "Bidan Sari Handayani", "", "Bidan", "Cempaka"),
        ("STF-009", "dr. Andi Wijaya", "", "Dokter Umum", "IGD"),
    ];
    for (id, name, nip, role, poli) in staff {
        sqlx::query(
            "INSERT INTO staff (id, name, nip, role, poli, phone, active)
             VALUES (?, ?, ?, ?, ?, '', 1)",
        )
        .bind(id)
        .bind(name)
        .bind(nip)
        .bind(role)
        .bind(poli)
        .execute(&db.pool)
        .await
        .map_err(db_err)?;
    }
    // (id, staff, shift, poli)
    let shifts: [(&str, &str, &str, &str); 8] = [
        ("SHF-20261119-001", "STF-001", "Pagi", "Poli Umum"),
        ("SHF-20261119-002", "STF-002", "Pagi", "Poli Umum"),
        ("SHF-20261119-003", "STF-003", "Siang", "Penyakit Dalam"),
        ("SHF-20261119-004", "STF-007", "Pagi", "Gigi"),
        ("SHF-20261119-005", "STF-008", "Pagi", "Cempaka"),
        ("SHF-20261119-006", "STF-004", "Pagi", "Farmasi"),
        ("SHF-20261119-007", "STF-005", "Pagi", "Kasir"),
        ("SHF-20261119-008", "STF-009", "Malam", "IGD"),
    ];
    for (id, staff_id, shift, poli) in shifts {
        sqlx::query(
            "INSERT INTO shift_schedules (id, staff_id, date, shift, poli, created_at)
             VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(id)
        .bind(staff_id)
        .bind(today.to_string())
        .bind(shift)
        .bind(poli)
        .bind(t(7, 0))
        .execute(&db.pool)
        .await
        .map_err(db_err)?;
    }

    // -- Pengaturan seed ---------------------------------------------------------
    let settings: [(&str, &str); 4] = [
        ("backup_otomatis", "on"),
        ("sinkron_satusehat", "on"),
        ("mode_offline", "off"),
        ("retensi_audit_hari", "365"),
    ];
    for (key, value) in settings {
        sqlx::query(
            "INSERT INTO app_settings (key, value, updated_at) VALUES (?, ?, ?)",
        )
        .bind(key)
        .bind(value)
        .bind(t(7, 30))
        .execute(&db.pool)
        .await
        .map_err(db_err)?;
    }

    // API keys: TIDAK ADA seed (kunci dibuat admin saat runtime, FASE4.md §4.2).

    Ok(true)
}

impl Db {
    /// Seed Fase 4 bila tabel `hd_machines` masih kosong (FASE4.md §4.2).
    ///
    /// Alasan sama seperti Fase 2/3: `seed()` hanya dipanggil
    /// `seed_if_empty` bila `users` kosong. DB lama yang mendapat
    /// migrasi 0006 tidak pernah menjalankan `seed()` lagi. Dipanggil
    /// binary sekali saat boot, tepat setelah `seed_fase3_if_empty()`.
    /// Mengembalikan `true` bila seed dijalankan.
    pub async fn seed_fase4_if_empty(&self) -> Result<bool, waras_core::WarasError> {
        let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM hd_machines")
            .fetch_one(&self.pool)
            .await
            .map_err(db_err)?;
        if n > 0 {
            return Ok(false);
        }
        seed_fase4(self).await?;
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn seeded_db() -> Db {
        let db = Db::connect_for_tests().await.expect("buka DB test");
        db.migrate().await.expect("migrasi");
        assert!(db.seed_if_empty().await.expect("seed"));
        db
    }

    #[tokio::test]
    async fn seed_fase3_counts() {
        let db = seeded_db().await;
        // Pasien baru Fase 3 + Fase 4 (seed() menjalankan keduanya).
        assert_eq!(db.list_patients(None).await.unwrap().len(), 14);
        assert!(db.get_patient("RM-000822").await.unwrap().is_some());
        assert!(db.get_patient("RM-000823").await.unwrap().is_some());
        // Encounter fast-track IGD.
        let enc21 = db.get_encounter("ENC-2026-11-19-021").await.unwrap().unwrap();
        assert_eq!(enc21.patient_id, "RM-000820");
        assert_eq!(enc21.poli, "IGD");
        // Tabel Fase 3.
        assert_eq!(db.list_wards().await.unwrap().len(), 5);
        assert_eq!(db.list_ward_beds(None).await.unwrap().len(), 120);
        assert_eq!(db.list_admissions(None).await.unwrap().len(), 7);
        assert_eq!(
            db.list_igd(&clinic_today().to_string()).await.unwrap().len(),
            8
        );
        assert_eq!(
            db.list_surgeries(&clinic_today().to_string())
                .await
                .unwrap()
                .len(),
            6
        );
        assert_eq!(db.list_delivery_rooms().await.unwrap().len(), 4);
        assert_eq!(db.list_deliveries().await.unwrap().len(), 1);
        assert_eq!(
            db.list_lab_orders(&clinic_today().to_string(), None)
                .await
                .unwrap()
                .len(),
            8
        );
        assert_eq!(db.list_lab_results("LAB-2211").await.unwrap().len(), 5);
        assert_eq!(
            db.list_rad_orders(&clinic_today().to_string(), None)
                .await
                .unwrap()
                .len(),
            7
        );
        assert_eq!(db.list_casemix().await.unwrap().len(), 9);
        // Idempotent: panggilan kedua tidak menambah apa-apa.
        assert!(!seed_fase3(&db).await.unwrap());
        assert!(!db.seed_fase3_if_empty().await.unwrap());
        assert_eq!(db.list_ward_beds(None).await.unwrap().len(), 120);
    }

    #[tokio::test]
    async fn seed_fase4_counts() {
        let db = seeded_db().await;
        // Pasien baru Fase 4; pasien berikutnya jadi RM-000826.
        assert_eq!(db.list_patients(None).await.unwrap().len(), 14);
        assert!(db.get_patient("RM-000824").await.unwrap().is_some());
        assert!(db.get_patient("RM-000825").await.unwrap().is_some());
        let slamet = db.get_patient("RM-000824").await.unwrap().unwrap();
        assert_eq!(slamet.name, "Slamet Riyadi");
        assert_eq!(slamet.nik, "3374031005720013");
        let conds = db.get_conditions("RM-000824").await.unwrap();
        assert_eq!(conds.len(), 1);
        assert_eq!(conds[0].code, "N18.9");
        let next = db
            .create_patient(crate::NewPatient {
                nik: "3374010101900001".into(),
                name: "Test Pasien".into(),
                birth_date: NaiveDate::from_ymd_opt(1990, 1, 1).unwrap(),
                gender: Gender::LakiLaki,
                address: "Jl. Test No. 1".into(),
                phone: None,
                blood_type: None,
                bpjs_number: None,
                insurance: Insurance::Umum,
            })
            .await
            .unwrap();
        assert_eq!(next.id, "RM-000826");
        // Tabel Fase 4.
        assert_eq!(db.list_hd_machines().await.unwrap().len(), 4);
        assert_eq!(
            db.list_hd_sessions(&clinic_today().to_string())
                .await
                .unwrap()
                .len(),
            3
        );
        assert_eq!(db.list_blood_stock().await.unwrap().len(), 8);
        assert_eq!(db.list_blood_requests(None).await.unwrap().len(), 2);
        assert_eq!(
            db.list_crossmatches("TR-20261119-002").await.unwrap().len(),
            1
        );
        assert_eq!(db.list_nutrition_assessments(None).await.unwrap().len(), 4);
        assert_eq!(db.list_nutrition_diets(None).await.unwrap().len(), 3);
        assert_eq!(db.list_dental_records(None).await.unwrap().len(), 5);
        assert_eq!(db.list_inventory_items().await.unwrap().len(), 10);
        assert_eq!(db.list_inventory_moves(None).await.unwrap().len(), 3);
        assert_eq!(db.list_staff().await.unwrap().len(), 9);
        assert_eq!(
            db.list_shifts(&clinic_today().to_string())
                .await
                .unwrap()
                .len(),
            8
        );
        assert_eq!(db.list_settings().await.unwrap().len(), 4);
        // Sesi pasien (portal): token tidak bisa dipakai sebagai staf.
        let token = db
            .create_patient_session("RM-000824")
            .await
            .unwrap();
        assert_eq!(token.len(), 64);
        let owner = db.find_patient_by_token(&token).await.unwrap().unwrap();
        assert_eq!(owner.id, "RM-000824");
        assert!(db.find_user_by_token(&token).await.unwrap().is_none());
        assert_eq!(
            db.find_patient_by_nik("3374031005720013")
                .await
                .unwrap()
                .unwrap()
                .id,
            "RM-000824"
        );
        assert!(db
            .find_patient_by_nik("0000000000000000")
            .await
            .unwrap()
            .is_none());
        // Idempotent: panggilan kedua tidak menambah apa-apa.
        assert!(!seed_fase4(&db).await.unwrap());
        assert!(!db.seed_fase4_if_empty().await.unwrap());
        assert_eq!(db.list_patients(None).await.unwrap().len(), 15);
    }
}
