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

    // -- 5.2 Pasien (10 orang) ------------------------------------------------
    // (rm, nik, nama, lahir, gender, alamat, gol_darah, bpjs, penjamin)
    let patients: [(&str, &str, &str, NaiveDate, Gender, &str, &str, Option<&str>, Insurance); 10] = [
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
    ];
    for (rm, nik, name, birth, gender, address, blood, bpjs, insurance) in patients {
        sqlx::query(
            "INSERT INTO patients (id, nik, name, birth_date, gender, address, phone,
                 blood_type, bpjs_number, insurance, created_at)
             VALUES (?, ?, ?, ?, ?, ?, NULL, ?, ?, ?, ?)",
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
    let encounters: [(&str, &str, &str, Option<&str>, &str, NaiveDate, &str, EncounterStatus); 10] = [
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
             status, submitted_at, created_at)
         VALUES ('CLM-20261119-001', 'INV-2026-1181', 'SEP-20261119-001',
             'J-4-17-I', 185000, 'draft', NULL, ?)",
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
