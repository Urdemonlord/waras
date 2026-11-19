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
        ),
        Role::Apoteker => matches!(
            action,
            ViewDashboard | ViewPatient | Dispense | ViewDrugs | ViewBridges
        ),
        Role::Kasir => matches!(
            action,
            ViewDashboard | ViewPatient | CreateInvoice | ReceivePayment | ViewBridges | CreateClaim
        ),
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
}
