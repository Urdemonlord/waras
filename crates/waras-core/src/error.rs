use crate::safety::MedicationBlocked;
use thiserror::Error;

/// Single error type for the whole workspace. Messages are in Indonesian
/// where they can reach a user; the API maps each variant to an HTTP status.
#[derive(Debug, Error)]
pub enum WarasError {
    #[error("tidak ditemukan: {0}")]
    NotFound(String),

    #[error("validasi gagal: {0}")]
    Validation(String),

    #[error("resep diblokir: {0}")]
    Blocked(#[from] MedicationBlocked),

    #[error("belum masuk (login diperlukan)")]
    Unauthorized,

    #[error("tidak diizinkan: {0}")]
    Forbidden(String),

    #[error("konflik: {0}")]
    Conflict(String),

    #[error("pasien salah: {0}")]
    WrongPatient(String),

    #[error("tidak dapat diproses: {0}")]
    Unprocessable(String),

    #[error("basis data: {0}")]
    Database(String),

    #[error("kesalahan internal: {0}")]
    Internal(String),
}

impl WarasError {
    /// HTTP status code for the API layer.
    pub fn http_status(&self) -> u16 {
        match self {
            WarasError::NotFound(_) => 404,
            WarasError::Validation(_) => 400,
            WarasError::Blocked(_) => 422,
            WarasError::WrongPatient(_) => 422,
            WarasError::Unprocessable(_) => 422,
            WarasError::Unauthorized => 401,
            WarasError::Forbidden(_) => 403,
            WarasError::Conflict(_) => 409,
            WarasError::Database(_) | WarasError::Internal(_) => 500,
        }
    }
}
