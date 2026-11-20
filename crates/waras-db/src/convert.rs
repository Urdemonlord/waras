//! Enum <-> TEXT (snake_case) untuk kolom SQLite.
//! Satu tempat kebenaran; nilai yang tidak dikenal jadi WarasError::Database
//! supaya data rusak ketahuan, bukan ditebak diam-diam.

use waras_core::{
    types::{
        AppointmentStatus, ConditionGroup, ConditionStatus, Criticality, EncounterStatus,
        Gender, Insurance, InvoiceStatus, NotificationKind, PaymentMethod,
        PrescriptionStatus, QueueStatus, Role, SoapStatus, VitalCode,
    },
    WarasError,
};

fn bad(what: &str, v: &str) -> WarasError {
    WarasError::Database(format!("nilai {what} tidak dikenal di DB: {v}"))
}

// --- Role ---
// CATATAN Fase 4: `Role::Pasien` (FASE4.md §2) hanya peran portal —
// bukan pengguna staf dan tidak pernah tersimpan di tabel `users`.
// `role_to_str`/`role_from_str` tetap memetakannya ("pasien") supaya
// penambahan varian tidak merusak kompilasi; tidak ada baris user
// ber-peran pasien.
pub fn role_to_str(r: Role) -> &'static str {
    match r {
        Role::DokterUmum => "dokter_umum",
        Role::Perawat => "perawat",
        Role::Apoteker => "apoteker",
        Role::Kasir => "kasir",
        Role::Admin => "admin",
        Role::Pasien => "pasien",
    }
}
pub fn role_from_str(s: &str) -> Result<Role, WarasError> {
    Ok(match s {
        "dokter_umum" => Role::DokterUmum,
        "perawat" => Role::Perawat,
        "apoteker" => Role::Apoteker,
        "kasir" => Role::Kasir,
        "admin" => Role::Admin,
        "pasien" => Role::Pasien,
        _ => return Err(bad("role", s)),
    })
}

// --- Gender ---
pub fn gender_to_str(g: Gender) -> &'static str {
    match g {
        Gender::LakiLaki => "laki_laki",
        Gender::Perempuan => "perempuan",
    }
}
pub fn gender_from_str(s: &str) -> Result<Gender, WarasError> {
    Ok(match s {
        "laki_laki" => Gender::LakiLaki,
        "perempuan" => Gender::Perempuan,
        _ => return Err(bad("gender", s)),
    })
}

// --- Insurance ---
pub fn insurance_to_str(i: Insurance) -> &'static str {
    match i {
        Insurance::Bpjs => "bpjs",
        Insurance::Umum => "umum",
    }
}
pub fn insurance_from_str(s: &str) -> Result<Insurance, WarasError> {
    Ok(match s {
        "bpjs" => Insurance::Bpjs,
        "umum" => Insurance::Umum,
        _ => return Err(bad("insurance", s)),
    })
}

// --- Criticality ---
pub fn criticality_to_str(c: Criticality) -> &'static str {
    match c {
        Criticality::Tinggi => "tinggi",
        Criticality::Sedang => "sedang",
        Criticality::Rendah => "rendah",
    }
}
pub fn criticality_from_str(s: &str) -> Result<Criticality, WarasError> {
    Ok(match s {
        "tinggi" => Criticality::Tinggi,
        "sedang" => Criticality::Sedang,
        "rendah" => Criticality::Rendah,
        _ => return Err(bad("criticality", s)),
    })
}

// --- EncounterStatus ---
pub fn encounter_status_to_str(s: EncounterStatus) -> &'static str {
    match s {
        EncounterStatus::Aktif => "aktif",
        EncounterStatus::DalamPemeriksaan => "dalam_pemeriksaan",
        EncounterStatus::Selesai => "selesai",
        EncounterStatus::Dibatalkan => "dibatalkan",
    }
}
pub fn encounter_status_from_str(s: &str) -> Result<EncounterStatus, WarasError> {
    Ok(match s {
        "aktif" => EncounterStatus::Aktif,
        "dalam_pemeriksaan" => EncounterStatus::DalamPemeriksaan,
        "selesai" => EncounterStatus::Selesai,
        "dibatalkan" => EncounterStatus::Dibatalkan,
        _ => return Err(bad("encounter_status", s)),
    })
}

// --- VitalCode ---
pub fn vital_code_to_str(c: VitalCode) -> &'static str {
    match c {
        VitalCode::TekananDarah => "tekanan_darah",
        VitalCode::Nadi => "nadi",
        VitalCode::Suhu => "suhu",
        VitalCode::SpO2 => "sp_o2",
        VitalCode::GulaDarah => "gula_darah",
    }
}
pub fn vital_code_from_str(s: &str) -> Result<VitalCode, WarasError> {
    Ok(match s {
        "tekanan_darah" => VitalCode::TekananDarah,
        "nadi" => VitalCode::Nadi,
        "suhu" => VitalCode::Suhu,
        "sp_o2" => VitalCode::SpO2,
        "gula_darah" => VitalCode::GulaDarah,
        _ => return Err(bad("vital_code", s)),
    })
}

// --- SoapStatus ---
pub fn soap_status_to_str(s: SoapStatus) -> &'static str {
    match s {
        SoapStatus::Draft => "draft",
        SoapStatus::Lengkap => "lengkap",
    }
}
pub fn soap_status_from_str(s: &str) -> Result<SoapStatus, WarasError> {
    Ok(match s {
        "draft" => SoapStatus::Draft,
        "lengkap" => SoapStatus::Lengkap,
        _ => return Err(bad("soap_status", s)),
    })
}

// --- ConditionGroup ---
pub fn condition_group_to_str(g: ConditionGroup) -> &'static str {
    match g {
        ConditionGroup::Utama => "utama",
        ConditionGroup::Sekunder => "sekunder",
        ConditionGroup::Kronis => "kronis",
        ConditionGroup::Riwayat => "riwayat",
        ConditionGroup::TindakanMedis => "tindakan_medis",
    }
}
pub fn condition_group_from_str(s: &str) -> Result<ConditionGroup, WarasError> {
    Ok(match s {
        "utama" => ConditionGroup::Utama,
        "sekunder" => ConditionGroup::Sekunder,
        "kronis" => ConditionGroup::Kronis,
        "riwayat" => ConditionGroup::Riwayat,
        "tindakan_medis" => ConditionGroup::TindakanMedis,
        _ => return Err(bad("condition_group", s)),
    })
}

// --- ConditionStatus ---
pub fn condition_status_to_str(s: ConditionStatus) -> &'static str {
    match s {
        ConditionStatus::Aktif => "aktif",
        ConditionStatus::Kronis => "kronis",
        ConditionStatus::Selesai => "selesai",
    }
}
pub fn condition_status_from_str(s: &str) -> Result<ConditionStatus, WarasError> {
    Ok(match s {
        "aktif" => ConditionStatus::Aktif,
        "kronis" => ConditionStatus::Kronis,
        "selesai" => ConditionStatus::Selesai,
        _ => return Err(bad("condition_status", s)),
    })
}

// --- PrescriptionStatus ---
pub fn prescription_status_to_str(s: PrescriptionStatus) -> &'static str {
    match s {
        PrescriptionStatus::MenungguKonfirmasi => "menunggu_konfirmasi",
        PrescriptionStatus::Aktif => "aktif",
        PrescriptionStatus::Diblokir => "diblokir",
        PrescriptionStatus::Diberikan => "diberikan",
        PrescriptionStatus::Dibatalkan => "dibatalkan",
    }
}
pub fn prescription_status_from_str(s: &str) -> Result<PrescriptionStatus, WarasError> {
    Ok(match s {
        "menunggu_konfirmasi" => PrescriptionStatus::MenungguKonfirmasi,
        "aktif" => PrescriptionStatus::Aktif,
        "diblokir" => PrescriptionStatus::Diblokir,
        "diberikan" => PrescriptionStatus::Diberikan,
        "dibatalkan" => PrescriptionStatus::Dibatalkan,
        _ => return Err(bad("prescription_status", s)),
    })
}

// --- QueueStatus ---
pub fn queue_status_to_str(s: QueueStatus) -> &'static str {
    match s {
        QueueStatus::Menunggu => "menunggu",
        QueueStatus::Dipanggil => "dipanggil",
        QueueStatus::DalamKonsultasi => "dalam_konsultasi",
        QueueStatus::Selesai => "selesai",
        QueueStatus::TidakHadir => "tidak_hadir",
    }
}
pub fn queue_status_from_str(s: &str) -> Result<QueueStatus, WarasError> {
    Ok(match s {
        "menunggu" => QueueStatus::Menunggu,
        "dipanggil" => QueueStatus::Dipanggil,
        "dalam_konsultasi" => QueueStatus::DalamKonsultasi,
        "selesai" => QueueStatus::Selesai,
        "tidak_hadir" => QueueStatus::TidakHadir,
        _ => return Err(bad("queue_status", s)),
    })
}

// --- AppointmentStatus ---
pub fn appointment_status_to_str(s: AppointmentStatus) -> &'static str {
    match s {
        AppointmentStatus::Terkonfirmasi => "terkonfirmasi",
        AppointmentStatus::Menunggu => "menunggu",
        AppointmentStatus::Selesai => "selesai",
        AppointmentStatus::Dibatalkan => "dibatalkan",
    }
}
pub fn appointment_status_from_str(s: &str) -> Result<AppointmentStatus, WarasError> {
    Ok(match s {
        "terkonfirmasi" => AppointmentStatus::Terkonfirmasi,
        "menunggu" => AppointmentStatus::Menunggu,
        "selesai" => AppointmentStatus::Selesai,
        "dibatalkan" => AppointmentStatus::Dibatalkan,
        _ => return Err(bad("appointment_status", s)),
    })
}

// --- InvoiceStatus ---
pub fn invoice_status_to_str(s: InvoiceStatus) -> &'static str {
    match s {
        InvoiceStatus::Lunas => "lunas",
        InvoiceStatus::Menunggu => "menunggu",
        InvoiceStatus::Cicilan => "cicilan",
    }
}
pub fn invoice_status_from_str(s: &str) -> Result<InvoiceStatus, WarasError> {
    Ok(match s {
        "lunas" => InvoiceStatus::Lunas,
        "menunggu" => InvoiceStatus::Menunggu,
        "cicilan" => InvoiceStatus::Cicilan,
        _ => return Err(bad("invoice_status", s)),
    })
}

// --- PaymentMethod ---
pub fn payment_method_to_str(m: PaymentMethod) -> &'static str {
    match m {
        PaymentMethod::Tunai => "tunai",
        PaymentMethod::Qris => "qris",
        PaymentMethod::Transfer => "transfer",
        PaymentMethod::Deposit => "deposit",
    }
}
pub fn payment_method_from_str(s: &str) -> Result<PaymentMethod, WarasError> {
    Ok(match s {
        "tunai" => PaymentMethod::Tunai,
        "qris" => PaymentMethod::Qris,
        "transfer" => PaymentMethod::Transfer,
        "deposit" => PaymentMethod::Deposit,
        _ => return Err(bad("payment_method", s)),
    })
}

// --- NotificationKind ---
pub fn notification_kind_to_str(k: NotificationKind) -> &'static str {
    match k {
        NotificationKind::Info => "info",
        NotificationKind::Kritis => "kritis",
    }
}
pub fn notification_kind_from_str(s: &str) -> Result<NotificationKind, WarasError> {
    Ok(match s {
        "info" => NotificationKind::Info,
        "kritis" => NotificationKind::Kritis,
        _ => return Err(bad("notification_kind", s)),
    })
}
