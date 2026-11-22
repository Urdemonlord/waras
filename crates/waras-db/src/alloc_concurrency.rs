//! Uji konkurensi alokasi id (follow-up Fase A item 3), gaya uji beban A2:
//! 20 task x 10 ronde tulis bersamaan atas semua jalur yang baru dikunci
//! `Db::alloc_lock` + retry 5x. Tiap ronde memakai pasien + encounter baru
//! sendiri, supaya tidak ada perebutan kunci asing (NIK, username, bed,
//! mesin, ruang) yang mengaburkan hasil: yang diuji di sini hanya
//! tabrakan alokasi id. Tanpa kunci, 20 penulis bersamaan menghitung id
//! yang sama dan yang kedua gagal pelanggaran PK (terbukti di A2 §2.3
//! untuk antrean/resep/outbox: 59-61/100 gagal 500).

#[cfg(test)]
mod tests {
    use crate::lab::NewLabResult;
    use crate::{clinic_today, Db, NewPatient};
    use chrono::{NaiveDate, NaiveDateTime};
    use waras_core::types::{
        Claim, ClaimStatus, ConditionGroup, ConditionStatus, Gender, Insurance, PcareVisit,
        Referral, ReferralStatus, Role, Sep, SepStatus, TriageLevel,
    };
    use waras_core::WarasError;

    async fn seeded_db() -> Db {
        let db = Db::connect_for_tests().await.expect("buka DB test");
        db.migrate().await.expect("migrasi");
        assert!(db.seed_if_empty().await.expect("seed"));
        db
    }

    fn now() -> NaiveDateTime {
        NaiveDateTime::parse_from_str("2026-11-19 08:00:00", "%Y-%m-%d %H:%M:%S").unwrap()
    }

    fn new_patient(n: usize) -> NewPatient {
        NewPatient {
            nik: format!("337401010190{n:04}"),
            name: format!("Pasien Uji {n}"),
            birth_date: NaiveDate::from_ymd_opt(1990, 1, 1).unwrap(),
            gender: Gender::LakiLaki,
            address: "Jl. Uji No. 1".into(),
            phone: None,
            blood_type: None,
            bpjs_number: None,
            insurance: Insurance::Umum,
        }
    }

    /// Satu ronde tulis atas 19 jalur yang baru dikunci. `n` unik per
    /// ronde supaya NIK/username/nama tidak bertabrakan antar ronde.
    /// Tiap langkah dibungkus label supaya kegagalan di bawah konkurensi
    /// langsung menunjuk jalurnya (bukan sekadar error DB mentah).
    async fn one_round(db: &Db, n: usize) -> Result<(), WarasError> {
        async fn step<T>(
            label: &str,
            fut: impl std::future::Future<Output = Result<T, WarasError>>,
        ) -> Result<T, WarasError> {
            fut.await
                .map_err(|e| WarasError::Database(format!("langkah {label}: {e}")))
        }
        let p = step("create_patient", db.create_patient(new_patient(n))).await?;
        let enc = step(
            "create_encounter",
            db.create_encounter(&p.id, "Poli Umum", "dr. Ahmad Hidayat", Some("U-001")),
        )
        .await?;
        let inv = step(
            "create_invoice",
            db.create_invoice(
                &p.id,
                Some(&enc.id),
                Insurance::Umum,
                "Poli Umum",
                100_000,
                None,
            ),
        )
        .await?;
        // Klaim langsung (id dihitung) + klaim via grouping (id CLM-5518 dst).
        step(
            "create_claim",
            db.create_claim(&Claim {
                id: String::new(),
                invoice_id: Some(inv.id.clone()),
                patient_id: Some(p.id.clone()),
                sep_id: None,
                ina_cbg_code: "J-4-12-I".into(),
                amount: 100_000,
                status: ClaimStatus::Draft,
                submitted_at: None,
                created_at: now(),
                grouping_note: None,
            }),
        )
        .await?;
        let lab = step(
            "create_lab_order",
            db.create_lab_order(
                &p.id,
                Some(&enc.id),
                "Gula Darah",
                "1558-6",
                "Poli Umum",
                "dr. Ahmad Hidayat",
            ),
        )
        .await?;
        step(
            "add_lab_results",
            db.add_lab_results(
                &lab.id,
                &[NewLabResult {
                    analyte: "Gula Darah".into(),
                    loinc: Some("1558-6".into()),
                    value_text: "120".into(),
                    unit: "mg/dL".into(),
                    ref_text: "70-140".into(),
                    ref_low: Some(70.0),
                    ref_high: Some(140.0),
                    critical_high: Some(300.0),
                    critical_low: Some(50.0),
                }],
                "dr. Ahmad Hidayat",
            ),
        )
        .await?;
        let rad = step(
            "create_rad_order",
            db.create_rad_order(
                &p.id,
                Some(&enc.id),
                "X-Ray",
                "Thorax",
                "Poli Umum",
                "dr. Ahmad Hidayat",
            ),
        )
        .await?;
        step(
            "save_rad_report",
            db.save_rad_report(&rad.id, "Paru bersih.", "dr. Rina, Sp.PD"),
        )
        .await?;
        let igd = step(
            "create_igd_triage",
            db.create_igd_triage(
                &p.id,
                "Sesak napas",
                TriageLevel::Hijau,
                "dr. Ahmad Hidayat",
                "08.00",
                None,
            ),
        )
        .await?;
        step(
            "add_igd_observation",
            db.add_igd_observation(&igd.id, "Observasi 15 menit.", "Ns. Sari Wijaya"),
        )
        .await?;
        let req = step(
            "create_blood_request",
            db.create_blood_request(
                &p.id,
                Some(&enc.id),
                "O+",
                "PRC",
                1,
                "Anemia",
                "dr. Ahmad Hidayat",
            ),
        )
        .await?;
        step(
            "add_crossmatch",
            db.add_crossmatch(&req.id, "BAG-UJI-1", "Tidak Cocok", "apt. Dina Puspita"),
        )
        .await?;
        step(
            "create_dental_record",
            db.create_dental_record(
                &p.id,
                Some(&enc.id),
                "11",
                "Karies",
                "K02.9",
                "Tambal",
                "89.01",
                "drg. Maya K.",
            ),
        )
        .await?;
        step(
            "create_nutrition_assessment",
            db.create_nutrition_assessment(
                &p.id,
                Some(&enc.id),
                70.0,
                170.0,
                "Rendah",
                "Biasa",
                "",
                "Ns. Sari Wijaya",
            ),
        )
        .await?;
        step(
            "create_nutrition_diet",
            db.create_nutrition_diet(&p.id, "Biasa", 2000, "", "2026-11-19"),
        )
        .await?;
        let item = step(
            "create_inventory_item",
            db.create_inventory_item(
                &format!("Item Uji {n}"),
                "Obat",
                "Tablet",
                10,
                1,
                "Rak A-01",
                "2027-01-31",
            ),
        )
        .await?;
        step(
            "add_inventory_move",
            db.add_inventory_move(&item.id, "Masuk", 5, "tes", "Admin Sistem"),
        )
        .await?;
        let staff = step(
            "create_staff",
            db.create_staff(
                &format!("Staf Uji {n}"),
                &format!("19900101000000{n:04}"),
                "Perawat",
                "Poli Umum",
                "081200000000",
            ),
        )
        .await?;
        step(
            "create_shift",
            db.create_shift(&staff.id, "2026-11-19", "Pagi", "Poli Umum"),
        )
        .await?;
        step(
            "create_api_key",
            db.create_api_key(
                &format!("kunci uji {n}"),
                &format!("uji{n:04}"),
                &format!("hash-uji-{n:04}"),
                "patients:read",
                "admin",
            ),
        )
        .await?;
        step(
            "create_sep",
            db.create_sep(&Sep {
                id: String::new(),
                sep_number: String::new(),
                bpjs_sep_number: None,
                patient_id: p.id.clone(),
                encounter_id: Some(enc.id.clone()),
                bpjs_number: "0001234567890".into(),
                jenis: "RawatJalan".into(),
                poli: "Poli Umum".into(),
                status: SepStatus::Aktif,
                created_by: "admin".into(),
                created_at: now(),
            }),
        )
        .await?;
        step(
            "create_referral",
            db.create_referral(&Referral {
                id: String::new(),
                patient_id: p.id.clone(),
                from_poli: "Poli Umum".into(),
                to_facility: "RSUD Dr. Kariadi".into(),
                reason: "Rujuk".into(),
                status: ReferralStatus::Menunggu,
                created_at: now(),
            }),
        )
        .await?;
        step(
            "create_pcare_visit",
            db.create_pcare_visit(&PcareVisit {
                id: String::new(),
                patient_id: p.id.clone(),
                visit_date: clinic_today(),
                poli: "Poli Umum".into(),
                status: "Tercatat".into(),
                created_at: now(),
            }),
        )
        .await?;
        step(
            "create_surgery",
            db.create_surgery(
                &p.id,
                Some(&enc.id),
                "OK 1",
                "Appendektomi",
                "47.09",
                "dr. Budi, Sp.B",
                "dr. Ahmad Hidayat",
                "2026-11-19 10:00",
            ),
        )
        .await?;
        // Grouping casemix: J18.9 terpetakan (A2 §1: J18.9 -> J-4-12-I).
        step(
            "add_condition",
            db.add_condition(
                &p.id,
                Some(&enc.id),
                ConditionGroup::Utama,
                "J18.9",
                "Pneumonia",
                ConditionStatus::Aktif,
                "dr. Ahmad Hidayat",
            ),
        )
        .await?;
        step(
            "group_claim_for_encounter",
            db.group_claim_for_encounter(&enc.id, "admin"),
        )
        .await?;
        Ok(())
    }

    /// Jumlah baris vs jumlah id unik satu tabel (harus sama: tidak ada
    /// id ganda, tidak ada baris yang hilang karena gagal diam-diam).
    async fn assert_ids_unique(db: &Db, table: &str) {
        let (total, distinct): (i64, i64) =
            sqlx::query_as(&format!("SELECT COUNT(*), COUNT(DISTINCT id) FROM {table}"))
                .fetch_one(&db.pool)
                .await
                .unwrap();
        assert_eq!(
            total, distinct,
            "id ganda di {table}: {total} baris, {distinct} id unik"
        );
    }

    #[tokio::test]
    async fn concurrent_allocations_are_unique() {
        let db = seeded_db().await;
        let mut handles = Vec::new();
        for t in 0..20 {
            let db = db.clone();
            handles.push(tokio::spawn(async move {
                for r in 0..10 {
                    one_round(&db, t * 10 + r).await?;
                }
                Ok::<(), WarasError>(())
            }));
        }
        for h in handles {
            h.await.expect("task panik").expect("ronde gagal");
        }
        // 200 pasien baru di atas 14 seed (RM-000812..000825).
        let n_patients: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM patients")
            .fetch_one(&db.pool)
            .await
            .unwrap();
        assert_eq!(n_patients, 214);
        for table in [
            "patients",
            "encounters",
            "invoices",
            "claims",
            "lab_orders",
            "lab_results",
            "rad_orders",
            "rad_reports",
            "igd_visits",
            "igd_observations",
            "blood_requests",
            "blood_crossmatches",
            "dental_records",
            "nutrition_assessments",
            "nutrition_diets",
            "inventory_items",
            "inventory_moves",
            "staff",
            "shift_schedules",
            "api_keys",
            "seps",
            "referrals",
            "pcare_visits",
            "surgeries",
        ] {
            assert_ids_unique(&db, table).await;
        }
    }

    #[tokio::test]
    async fn concurrent_scarce_resource_allocations() {
        let db = seeded_db().await;
        // Admisi: 10 task x 2 ronde, tiap ronde satu bed Kosong sendiri
        // (seed: 34 Kosong dari 120 bed).
        let beds = db.list_ward_beds(None).await.unwrap();
        let mut kosong: Vec<String> = beds
            .iter()
            .filter(|b| b.status == waras_core::types::BedStatus::Kosong)
            .map(|b| b.id.clone())
            .collect();
        kosong.truncate(20);
        assert_eq!(kosong.len(), 20, "seed harus punya 20 bed Kosong");
        let mut handles = Vec::new();
        for t in 0..10 {
            let db = db.clone();
            let beds = vec![kosong[t * 2].clone(), kosong[t * 2 + 1].clone()];
            handles.push(tokio::spawn(async move {
                for bed in beds {
                    let adm = db
                        .create_admission(
                            "RM-000812",
                            &bed,
                            None,
                            "J18.9",
                            "Pneumonia",
                            "dr. Ahmad Hidayat",
                            false,
                        )
                        .await?;
                    db.add_care_note(&adm.id, "Observasi.", "Ns. Sari Wijaya")
                        .await?;
                }
                Ok::<(), WarasError>(())
            }));
        }
        // Persalinan + HD: seed hanya punya 2 ruang Kosong + 1 mesin Siap,
        // jadi buka semuanya dulu (setup tes, bukan perilaku aplikasi),
        // lalu 4 task x 1 tulis per jenis dengan sumber daya sendiri.
        sqlx::query("UPDATE delivery_rooms SET status = 'kosong'")
            .execute(&db.pool)
            .await
            .unwrap();
        sqlx::query("UPDATE hd_machines SET status = 'siap'")
            .execute(&db.pool)
            .await
            .unwrap();
        let rooms = db.list_delivery_rooms().await.unwrap();
        let machines = db.list_hd_machines().await.unwrap();
        assert_eq!(rooms.len(), 4);
        assert_eq!(machines.len(), 4);
        for i in 0..4 {
            let db_d = db.clone();
            let room = rooms[i].id.clone();
            handles.push(tokio::spawn(async move {
                db_d.create_delivery(
                    "RM-000812",
                    28,
                    "G2P1",
                    5,
                    140,
                    "Bd. Sari",
                    "dr. Rina, Sp.PD",
                    "Normal",
                    &room,
                )
                .await?;
                Ok::<(), WarasError>(())
            }));
            let db_h = db.clone();
            let machine = machines[i].id.clone();
            handles.push(tokio::spawn(async move {
                db_h.create_hd_session(
                    "RM-000812",
                    &machine,
                    None,
                    "FX8",
                    240,
                    300,
                    500,
                    2000,
                    "2026-11-19 10:00:00",
                    "",
                )
                .await?;
                Ok::<(), WarasError>(())
            }));
        }
        for h in handles {
            h.await.expect("task panik").expect("tulis gagal");
        }
        for table in ["admissions", "care_notes", "deliveries", "hd_sessions"] {
            assert_ids_unique(&db, table).await;
        }
        let n_adm: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM admissions")
            .fetch_one(&db.pool)
            .await
            .unwrap();
        // Seed 7 admisi + 20 baru (A2 §1: seed Fase 3 = 7 admisi).
        assert_eq!(n_adm, 27);
    }

    #[tokio::test]
    async fn concurrent_user_creation_is_unique() {
        // Argon2 mahal: 10 task x 2 ronde saja (20 user), bukan 20x10.
        let db = seeded_db().await;
        let mut handles = Vec::new();
        for t in 0..10 {
            let db = db.clone();
            handles.push(tokio::spawn(async move {
                for r in 0..2 {
                    let n = t * 2 + r;
                    db.create_user(
                        &format!("19900101000000{n:04}"),
                        &format!("uji{n:04}"),
                        &format!("User Uji {n}"),
                        Role::Perawat,
                        "waras123",
                    )
                    .await?;
                }
                Ok::<(), WarasError>(())
            }));
        }
        for h in handles {
            h.await.expect("task panik").expect("buat user gagal");
        }
        assert_ids_unique(&db, "users").await;
    }

    #[tokio::test]
    async fn concurrent_same_bed_admission_only_one_wins() {
        // Efek samping kunci: cek-bed + INSERT admisi atomik dalam satu
        // proses, jadi dua admisi bersamaan ke bed yang sama tidak bisa
        // lolos bersama (satu 422, bukan double-booking).
        let db = seeded_db().await;
        let beds = db.list_ward_beds(None).await.unwrap();
        let bed = beds
            .iter()
            .find(|b| b.status == waras_core::types::BedStatus::Kosong)
            .expect("seed harus punya bed Kosong")
            .id
            .clone();
        let db2 = db.clone();
        let bed2 = bed.clone();
        let h1 = tokio::spawn(async move {
            db.create_admission(
                "RM-000812",
                &bed,
                None,
                "J18.9",
                "Pneumonia",
                "dr. Ahmad Hidayat",
                false,
            )
            .await
        });
        let h2 = tokio::spawn(async move {
            db2.create_admission(
                "RM-000813",
                &bed2,
                None,
                "J18.9",
                "Pneumonia",
                "dr. Ahmad Hidayat",
                false,
            )
            .await
        });
        let (r1, r2) = (h1.await.unwrap(), h2.await.unwrap());
        assert!(
            r1.is_ok() ^ r2.is_ok(),
            "tepat satu admisi harus lolos: {r1:?} vs {r2:?}"
        );
        let err = r1.err().or(r2.err()).unwrap();
        assert!(
            matches!(err, WarasError::Unprocessable(_)),
            "yang kalah harus 422, bukan {err:?}"
        );
    }

    #[tokio::test]
    async fn concurrent_duplicate_shift_only_one_wins() {
        // Efek samping kunci: cek-duplikat + INSERT shift atomik dalam
        // satu proses, jadi dua shift bersamaan untuk staf + tanggal yang
        // sama tidak bisa lolos bersama.
        let db = seeded_db().await;
        let staff = db
            .create_staff(
                "Staf Uji Shift",
                "1990010100009999",
                "Perawat",
                "Poli Umum",
                "0812",
            )
            .await
            .unwrap();
        let db2 = db.clone();
        let s2 = staff.id.clone();
        let h1 = tokio::spawn(async move {
            db.create_shift(&staff.id, "2026-11-19", "Pagi", "Poli Umum")
                .await
        });
        let h2 = tokio::spawn(async move {
            db2.create_shift(&s2, "2026-11-19", "Pagi", "Poli Umum")
                .await
        });
        let (r1, r2) = (h1.await.unwrap(), h2.await.unwrap());
        assert!(
            r1.is_ok() ^ r2.is_ok(),
            "tepat satu shift harus lolos: {r1:?} vs {r2:?}"
        );
        let err = r1.err().or(r2.err()).unwrap();
        assert!(
            matches!(err, WarasError::Unprocessable(_)),
            "yang kalah harus 422, bukan {err:?}"
        );
    }
}
