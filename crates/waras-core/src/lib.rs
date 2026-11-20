//! WARAS core: domain types (FHIR R4-shaped), medication-safety rules,
//! RBAC, error type and ID helpers. Pure Rust: no database, no HTTP, no I/O.
//!
//! Everything in this crate is shared by `waras-db` (persistence + seed),
//! `waras-api` (HTTP handlers) and the `waras` binary. If a type changes,
//! change it here first; the compiler will point at every place that drifts.

pub mod bridge;
pub mod casemix;
pub mod dosage;
pub mod error;
pub mod fhir;
pub mod ids;
pub mod nutrition;
pub mod rbac;
pub mod safety;
pub mod types;

pub use error::WarasError;
pub use types::*;
