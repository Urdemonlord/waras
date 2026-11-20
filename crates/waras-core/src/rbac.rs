//! RBAC: matriks peran x aksi. Satu tempat kebenaran; API menegakkannya di
//! setiap handler, UI menyembunyikan aksi yang tidak diizinkan.

use crate::types::Role;

/// Aksi yang dijaga. Nama aksi dipakai juga sebagai string di audit trail.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Action {
    ViewDashboard,
    CreatePatient,
    ViewPatient,
    TakeQueue,
    CallQueue,
    CreateEncounter,
    WriteSoap,
    RecordVitals,
    AddCondition,
    Prescribe,
    ConfirmPrescription,
    Dispense,
    ViewDrugs,
    CreateInvoice,
    ReceivePayment,
    ViewAudit,
    ManageUsers,
    // -- Fase 2 (FASE2.md §4.3) -------------------------------------------
    ViewBridges,
    ManageOutbox,
    CreateSep,
    CreateReferral,
    CreateClaim,
    SyncBridges,
    // -- Fase 3 (FASE3.md §3) -------------------------------------------
    ViewRumahSakit,
    TriaseIgd,
    KelolaRawatInap,
    KelolaOperasi,
    PesanLab,
    PesanRadiologi,
    HasilLab,
    LaporanRadiologi,
    GroupingCasemix,
    // -- Fase 4 (FASE4.md §3) -------------------------------------------
    ViewEkosistem,
    KelolaHemodialisa,
    KelolaTransfusi,
    KelolaGizi,
    KelolaGigi,
    KelolaInventori,
    KelolaKepegawaian,
    ViewLaporan,
    KelolaPengaturan,
    ManageApiKeys,
    ViewPortal,
}

impl Action {
    pub fn label(&self) -> &'static str {
        match self {
            Action::ViewDashboard => "lihat dasbor",
            Action::CreatePatient => "daftarkan pasien",
            Action::ViewPatient => "lihat pasien",
            Action::TakeQueue => "ambil nomor antrean",
            Action::CallQueue => "panggil antrean",
            Action::CreateEncounter => "buat encounter",
            Action::WriteSoap => "tulis SOAP",
            Action::RecordVitals => "catat tanda vital",
            Action::AddCondition => "tambah diagnosis",
            Action::Prescribe => "resepkan obat",
            Action::ConfirmPrescription => "konfirmasi resep",
            Action::Dispense => "serahkan obat (dispensing)",
            Action::ViewDrugs => "lihat stok obat",
            Action::CreateInvoice => "buat tagihan",
            Action::ReceivePayment => "terima pembayaran",
            Action::ViewAudit => "lihat audit trail",
            Action::ManageUsers => "kelola pengguna",
            Action::ViewBridges => "lihat status jembatan",
            Action::ManageOutbox => "kelola outbox",
            Action::CreateSep => "buat/batalkan SEP",
            Action::CreateReferral => "buat rujukan",
            Action::CreateClaim => "buat klaim",
            Action::SyncBridges => "sinkronisasi jembatan",
            Action::ViewRumahSakit => "lihat modul rumah sakit",
            Action::TriaseIgd => "triase dan ubah status IGD",
            Action::KelolaRawatInap => "kelola rawat inap",
            Action::KelolaOperasi => "kelola operasi dan persalinan",
            Action::PesanLab => "pesan pemeriksaan lab",
            Action::PesanRadiologi => "pesan pemeriksaan radiologi",
            Action::HasilLab => "isi hasil lab",
            Action::LaporanRadiologi => "tulis laporan radiologi",
            Action::GroupingCasemix => "grouping dan kelola casemix",
            Action::ViewEkosistem => "lihat modul ekosistem",
            Action::KelolaHemodialisa => "kelola hemodialisa",
            Action::KelolaTransfusi => "kelola transfusi darah",
            Action::KelolaGizi => "kelola gizi",
            Action::KelolaGigi => "kelola rekam medis gigi",
            Action::KelolaInventori => "kelola inventori",
            Action::KelolaKepegawaian => "kelola kepegawaian",
            Action::ViewLaporan => "lihat laporan dan SIRS",
            Action::KelolaPengaturan => "kelola pengaturan aplikasi",
            Action::ManageApiKeys => "kelola kunci API",
            Action::ViewPortal => "lihat portal pasien",
        }
    }
}

/// Apakah peran ini boleh melakukan aksi ini?
pub fn can(role: Role, action: Action) -> bool {
    use Action::*;
    match role {
        Role::Admin => true,
        Role::DokterUmum => matches!(
            action,
            ViewDashboard
                | CreatePatient
                | ViewPatient
                | TakeQueue
                | CallQueue
                | CreateEncounter
                | WriteSoap
                | RecordVitals
                | AddCondition
                | Prescribe
                | ConfirmPrescription
                | ViewDrugs
                | ViewAudit
                | ViewBridges
                | CreateSep
                | CreateReferral
                | ViewRumahSakit
                | TriaseIgd
                | KelolaRawatInap
                | KelolaOperasi
                | PesanLab
                | PesanRadiologi
                | HasilLab
                | LaporanRadiologi
                | GroupingCasemix
                | ViewEkosistem
                | KelolaHemodialisa
                | KelolaTransfusi
                | KelolaGizi
                | KelolaGigi
                | KelolaInventori
                | KelolaKepegawaian
                | ViewLaporan
        ),
        Role::Perawat => matches!(
            action,
            ViewDashboard
                | CreatePatient
                | ViewPatient
                | TakeQueue
                | CallQueue
                | RecordVitals
                | ViewDrugs
                | ViewBridges
                | CreateSep
                | ViewRumahSakit
                | TriaseIgd
                | KelolaRawatInap
                | PesanLab
                | PesanRadiologi
                | ViewEkosistem
                | KelolaHemodialisa
                | KelolaTransfusi
                | KelolaGizi
                | KelolaGigi
                | ViewLaporan
        ),
        Role::Apoteker => matches!(
            action,
            ViewDashboard
                | ViewPatient
                | Dispense
                | ViewDrugs
                | ViewBridges
                | ViewRumahSakit
                | ViewEkosistem
                | KelolaInventori
                | ViewLaporan
        ),
        Role::Kasir => matches!(
            action,
            ViewDashboard
                | ViewPatient
                | CreateInvoice
                | ReceivePayment
                | ViewBridges
                | CreateClaim
                | ViewRumahSakit
                | GroupingCasemix
                | ViewEkosistem
                | ViewLaporan
        ),
        // Pasien: HANYA portal (FASE4.md §3) — tidak ada aksi staf lain.
        Role::Pasien => matches!(action, ViewPortal),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Role;

    #[test]
    fn dokter_can_prescribe_perawat_cannot() {
        assert!(can(Role::DokterUmum, Action::Prescribe));
        assert!(!can(Role::Perawat, Action::Prescribe));
        assert!(!can(Role::Apoteker, Action::Prescribe));
        assert!(!can(Role::Kasir, Action::Prescribe));
    }

    #[test]
    fn apoteker_can_dispense_dokter_cannot() {
        assert!(can(Role::Apoteker, Action::Dispense));
        assert!(!can(Role::DokterUmum, Action::Dispense));
    }

    #[test]
    fn kasir_can_receive_payment_only() {
        assert!(can(Role::Kasir, Action::ReceivePayment));
        assert!(!can(Role::Kasir, Action::Prescribe));
        assert!(!can(Role::Kasir, Action::Dispense));
    }

    #[test]
    fn admin_can_do_everything() {
        for action in [
            Action::ViewDashboard,
            Action::CreatePatient,
            Action::Prescribe,
            Action::Dispense,
            Action::ReceivePayment,
            Action::ManageUsers,
            Action::ViewBridges,
            Action::ManageOutbox,
            Action::CreateSep,
            Action::CreateReferral,
            Action::CreateClaim,
            Action::SyncBridges,
            Action::ViewRumahSakit,
            Action::TriaseIgd,
            Action::KelolaRawatInap,
            Action::KelolaOperasi,
            Action::PesanLab,
            Action::PesanRadiologi,
            Action::HasilLab,
            Action::LaporanRadiologi,
            Action::GroupingCasemix,
            Action::ViewEkosistem,
            Action::KelolaHemodialisa,
            Action::KelolaTransfusi,
            Action::KelolaGizi,
            Action::KelolaGigi,
            Action::KelolaInventori,
            Action::KelolaKepegawaian,
            Action::ViewLaporan,
            Action::KelolaPengaturan,
            Action::ManageApiKeys,
            Action::ViewPortal,
        ] {
            assert!(can(Role::Admin, action));
        }
    }

    #[test]
    fn fase2_matrix() {
        // FASE2.md §4.3.
        assert!(can(Role::DokterUmum, Action::ViewBridges));
        assert!(can(Role::DokterUmum, Action::CreateSep));
        assert!(can(Role::DokterUmum, Action::CreateReferral));
        assert!(!can(Role::DokterUmum, Action::ManageOutbox));
        assert!(!can(Role::DokterUmum, Action::CreateClaim));
        assert!(!can(Role::DokterUmum, Action::SyncBridges));
        assert!(can(Role::Perawat, Action::ViewBridges));
        assert!(can(Role::Perawat, Action::CreateSep));
        assert!(!can(Role::Perawat, Action::CreateReferral));
        assert!(!can(Role::Perawat, Action::ManageOutbox));
        assert!(can(Role::Apoteker, Action::ViewBridges));
        assert!(!can(Role::Apoteker, Action::CreateSep));
        assert!(!can(Role::Apoteker, Action::CreateClaim));
        assert!(can(Role::Kasir, Action::ViewBridges));
        assert!(can(Role::Kasir, Action::CreateClaim));
        assert!(!can(Role::Kasir, Action::CreateSep));
        assert!(!can(Role::Kasir, Action::ManageOutbox));
        assert!(!can(Role::Kasir, Action::SyncBridges));
    }

    #[test]
    fn fase3_matrix() {
        // FASE3.md §3.
        let all9 = [
            Action::ViewRumahSakit,
            Action::TriaseIgd,
            Action::KelolaRawatInap,
            Action::KelolaOperasi,
            Action::PesanLab,
            Action::PesanRadiologi,
            Action::HasilLab,
            Action::LaporanRadiologi,
            Action::GroupingCasemix,
        ];
        // DokterUmum: semua 9.
        for a in all9 {
            assert!(can(Role::DokterUmum, a), "dokter harus bisa {a:?}");
        }
        // Perawat: lihat + triase + ranap + pesan lab/rad saja.
        for a in [
            Action::ViewRumahSakit,
            Action::TriaseIgd,
            Action::KelolaRawatInap,
            Action::PesanLab,
            Action::PesanRadiologi,
        ] {
            assert!(can(Role::Perawat, a), "perawat harus bisa {a:?}");
        }
        for a in [
            Action::KelolaOperasi,
            Action::HasilLab,
            Action::LaporanRadiologi,
            Action::GroupingCasemix,
        ] {
            assert!(!can(Role::Perawat, a), "perawat tidak boleh {a:?}");
        }
        // Apoteker: lihat saja.
        assert!(can(Role::Apoteker, Action::ViewRumahSakit));
        for a in all9.iter().filter(|a| **a != Action::ViewRumahSakit) {
            assert!(!can(Role::Apoteker, *a), "apoteker tidak boleh {a:?}");
        }
        // Kasir: lihat + grouping saja.
        assert!(can(Role::Kasir, Action::ViewRumahSakit));
        assert!(can(Role::Kasir, Action::GroupingCasemix));
        for a in [
            Action::TriaseIgd,
            Action::KelolaRawatInap,
            Action::KelolaOperasi,
            Action::PesanLab,
            Action::PesanRadiologi,
            Action::HasilLab,
            Action::LaporanRadiologi,
        ] {
            assert!(!can(Role::Kasir, a), "kasir tidak boleh {a:?}");
        }
    }

    #[test]
    fn fase4_matrix() {
        // FASE4.md §3.
        let dokter = [
            Action::ViewEkosistem,
            Action::KelolaHemodialisa,
            Action::KelolaTransfusi,
            Action::KelolaGizi,
            Action::KelolaGigi,
            Action::KelolaInventori,
            Action::KelolaKepegawaian,
            Action::ViewLaporan,
        ];
        for a in dokter {
            assert!(can(Role::DokterUmum, a), "dokter harus bisa {a:?}");
        }
        assert!(!can(Role::DokterUmum, Action::KelolaPengaturan));
        assert!(!can(Role::DokterUmum, Action::ManageApiKeys));
        assert!(!can(Role::DokterUmum, Action::ViewPortal));
        // Perawat: lihat + HD + transfusi + gizi + gigi + laporan.
        for a in [
            Action::ViewEkosistem,
            Action::KelolaHemodialisa,
            Action::KelolaTransfusi,
            Action::KelolaGizi,
            Action::KelolaGigi,
            Action::ViewLaporan,
        ] {
            assert!(can(Role::Perawat, a), "perawat harus bisa {a:?}");
        }
        for a in [
            Action::KelolaInventori,
            Action::KelolaKepegawaian,
            Action::KelolaPengaturan,
            Action::ManageApiKeys,
            Action::ViewPortal,
        ] {
            assert!(!can(Role::Perawat, a), "perawat tidak boleh {a:?}");
        }
        // Apoteker: lihat + inventori + laporan.
        for a in [
            Action::ViewEkosistem,
            Action::KelolaInventori,
            Action::ViewLaporan,
        ] {
            assert!(can(Role::Apoteker, a), "apoteker harus bisa {a:?}");
        }
        for a in [
            Action::KelolaHemodialisa,
            Action::KelolaTransfusi,
            Action::KelolaGizi,
            Action::KelolaGigi,
            Action::KelolaKepegawaian,
            Action::KelolaPengaturan,
            Action::ManageApiKeys,
            Action::ViewPortal,
        ] {
            assert!(!can(Role::Apoteker, a), "apoteker tidak boleh {a:?}");
        }
        // Kasir: lihat + laporan saja.
        assert!(can(Role::Kasir, Action::ViewEkosistem));
        assert!(can(Role::Kasir, Action::ViewLaporan));
        for a in [
            Action::KelolaHemodialisa,
            Action::KelolaTransfusi,
            Action::KelolaGizi,
            Action::KelolaGigi,
            Action::KelolaInventori,
            Action::KelolaKepegawaian,
            Action::KelolaPengaturan,
            Action::ManageApiKeys,
            Action::ViewPortal,
        ] {
            assert!(!can(Role::Kasir, a), "kasir tidak boleh {a:?}");
        }
        // Admin: semua, termasuk pengaturan + kunci API.
        assert!(can(Role::Admin, Action::KelolaPengaturan));
        assert!(can(Role::Admin, Action::ManageApiKeys));
        // Pasien: HANYA portal.
        assert!(can(Role::Pasien, Action::ViewPortal));
        for a in [
            Action::ViewDashboard,
            Action::ViewPatient,
            Action::ViewEkosistem,
            Action::KelolaHemodialisa,
            Action::KelolaTransfusi,
            Action::KelolaGizi,
            Action::KelolaGigi,
            Action::KelolaInventori,
            Action::KelolaKepegawaian,
            Action::ViewLaporan,
            Action::KelolaPengaturan,
            Action::ManageApiKeys,
        ] {
            assert!(!can(Role::Pasien, a), "pasien tidak boleh {a:?}");
        }
        // Label peran Pasien.
        assert_eq!(Role::Pasien.label(), "Pasien");
    }
}
