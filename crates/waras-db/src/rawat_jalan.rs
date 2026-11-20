//! Rawat jalan (FASE4.md §4): agregat tanpa tabel baru.
//! Gabung antrean + encounter + janji hari itu, satu baris per
//! pasien+poli; bila pasien punya lebih dari satu sumber, baris Antrean
//! yang dipakai (lalu Encounter, lalu Janji).

use crate::rows::EncounterRow;
use crate::{db_err, parse_date, Db};
use std::collections::HashMap;
use waras_core::{
    types::{AppointmentStatus, OutpatientVisit},
    WarasError,
};

fn appointment_status_label(s: AppointmentStatus) -> &'static str {
    match s {
        AppointmentStatus::Terkonfirmasi => "Terkonfirmasi",
        AppointmentStatus::Menunggu => "Menunggu",
        AppointmentStatus::Selesai => "Selesai",
        AppointmentStatus::Dibatalkan => "Dibatalkan",
    }
}

impl Db {
    pub async fn list_outpatient_visits(
        &self,
        date: &str,
    ) -> Result<Vec<OutpatientVisit>, WarasError> {
        let day = parse_date(date)?;
        // Nama pasien (RM = id pasien itu sendiri).
        let names: HashMap<String, String> = self
            .list_patients(None)
            .await?
            .into_iter()
            .map(|p| (p.id.clone(), p.name))
            .collect();
        let mut by_key: HashMap<(String, String), OutpatientVisit> = HashMap::new();
        let mut put = |v: OutpatientVisit, names: &HashMap<String, String>| {
            if names.contains_key(&v.patient_id) {
                by_key.insert((v.patient_id.clone(), v.poli.clone()), v);
            }
        };

        // 1. Janji (prioritas terendah).
        for apt in self.list_appointments(Some(day)).await? {
            let Some(name) = names.get(&apt.patient_id) else {
                continue;
            };
            put(
                OutpatientVisit {
                    patient_id: apt.patient_id.clone(),
                    patient_name: name.clone(),
                    rm: apt.patient_id.clone(),
                    poli: apt.poli.clone(),
                    doctor: String::new(),
                    time: apt.consultation_time.clone(),
                    source: "Janji".to_string(),
                    status: appointment_status_label(apt.status).to_string(),
                },
                &names,
            );
        }
        // 2. Encounter (menimpa Janji bila pasien+poli sama).
        let enc_rows: Vec<EncounterRow> = sqlx::query_as(
            "SELECT * FROM encounters WHERE date = ? ORDER BY time ASC",
        )
        .bind(date)
        .fetch_all(&self.pool)
        .await
        .map_err(db_err)?;
        for row in enc_rows {
            let enc = row.into_domain()?;
            let Some(name) = names.get(&enc.patient_id) else {
                continue;
            };
            put(
                OutpatientVisit {
                    patient_id: enc.patient_id.clone(),
                    patient_name: name.clone(),
                    rm: enc.patient_id.clone(),
                    poli: enc.poli.clone(),
                    doctor: enc.doctor_name.clone(),
                    time: enc.time.clone(),
                    source: "Encounter".to_string(),
                    status: enc.status.label().to_string(),
                },
                &names,
            );
        }
        // 3. Antrean (prioritas tertinggi).
        for q in self.list_queue(day).await? {
            let Some(name) = names.get(&q.patient_id) else {
                continue;
            };
            put(
                OutpatientVisit {
                    patient_id: q.patient_id.clone(),
                    patient_name: name.clone(),
                    rm: q.patient_id.clone(),
                    poli: q.poli.clone(),
                    doctor: q.doctor.clone(),
                    time: q.time.clone(),
                    source: "Antrean".to_string(),
                    status: q.status.label().to_string(),
                },
                &names,
            );
        }

        let mut visits: Vec<OutpatientVisit> = by_key.into_values().collect();
        visits.sort_by(|a, b| a.time.cmp(&b.time).then(a.patient_name.cmp(&b.patient_name)));
        Ok(visits)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn seeded_db() -> Db {
        let db = Db::connect("sqlite::memory:").await.expect("buka in-memory");
        db.migrate().await.expect("migrasi");
        assert!(db.seed_if_empty().await.expect("seed"));
        db
    }

    #[tokio::test]
    async fn outpatient_visits_dedup_prefers_queue() {
        let db = seeded_db().await;
        let date = crate::clinic_today().to_string();
        let visits = db.list_outpatient_visits(&date).await.unwrap();
        // Antrean 19 Nov: 44 baris, 10 pasien x Poli Umum (satu baris per
        // pasien+poli) — semua pasien seed sudah punya antrean Poli Umum,
        // jadi encounter/janji poli lain mereka tertimpa HANYA bila
        // poli sama; poli lain tetap jadi baris sendiri.
        // Siti: antrean Poli Umum (Dipanggil) + janji Poli Umum + encounter
        // Poli Umum -> 1 baris Antrean. Budi: antrean Poli Umum + janji
        // Poli Anak + encounter Poli Anak -> 2 baris (Umum Antrean, Anak
        // Encounter — encounter menimpa janji).
        let siti: Vec<_> = visits
            .iter()
            .filter(|v| v.patient_id == "RM-000812")
            .collect();
        assert_eq!(siti.len(), 1);
        assert_eq!(siti[0].source, "Antrean");
        assert_eq!(siti[0].poli, "Poli Umum");
        assert_eq!(siti[0].status, "Dipanggil");
        assert_eq!(siti[0].rm, "RM-000812");
        let budi: Vec<_> = visits
            .iter()
            .filter(|v| v.patient_id == "RM-000813")
            .collect();
        assert_eq!(budi.len(), 2);
        let anak = budi.iter().find(|v| v.poli == "Poli Anak").unwrap();
        assert_eq!(anak.source, "Encounter");
        assert_eq!(anak.doctor, "dr. Sari Dewi");
        // Urut jam naik.
        let times: Vec<&str> = visits.iter().map(|v| v.time.as_str()).collect();
        let mut sorted = times.clone();
        sorted.sort();
        assert_eq!(times, sorted);
        // Tanggal tanpa kunjungan -> kosong; tanggal tidak valid -> error.
        // (Janji kontrol ulang hanya sampai 26 Nov.)
        assert!(db
            .list_outpatient_visits("2026-11-27")
            .await
            .unwrap()
            .is_empty());
        assert!(db.list_outpatient_visits("bukan-tanggal").await.is_err());
    }
}
