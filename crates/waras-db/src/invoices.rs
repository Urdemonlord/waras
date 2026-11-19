//! Tagihan & pembayaran.

use crate::convert::*;
use crate::rows::{InvoiceRow, PaymentRow};
use crate::{clinic_today, db_err, fmt_dt, new_uuid, now_naive, Db};
use chrono::Datelike;
use serde_json::json;
use waras_core::{
    ids,
    types::{Invoice, InvoiceStatus, Payment, PaymentMethod},
    WarasError,
};

impl Db {
    /// Tagihan baru `INV-<tahun>-<seq>` (seq = maks tahun itu + 1),
    /// status Menunggu.
    pub async fn create_invoice(
        &self,
        patient_id: &str,
        encounter_id: Option<&str>,
        payer: waras_core::types::Insurance,
        service: &str,
        amount: i64,
        ina_cbg_code: Option<&str>,
    ) -> Result<Invoice, WarasError> {
        let year = clinic_today().year();
        let ids_year: Vec<String> = sqlx::query_scalar(
            "SELECT id FROM invoices WHERE id LIKE ?",
        )
        .bind(format!("INV-{year}-%"))
        .fetch_all(&self.pool)
        .await
        .map_err(db_err)?;
        let seq = crate::next_seq_from_ids(&ids_year, 1);
        let id = ids::invoice_id(year, seq);
        let created = fmt_dt(now_naive());
        sqlx::query(
            "INSERT INTO invoices (id, patient_id, encounter_id, payer, service, amount,
                 status, ina_cbg_code, created_at)
             VALUES (?, ?, ?, ?, ?, ?, 'menunggu', ?, ?)",
        )
        .bind(&id)
        .bind(patient_id)
        .bind(encounter_id)
        .bind(insurance_to_str(payer))
        .bind(service)
        .bind(amount)
        .bind(ina_cbg_code)
        .bind(&created)
        .execute(&self.pool)
        .await
        .map_err(db_err)?;
        self.get_invoice(&id)
            .await?
            .ok_or_else(|| WarasError::Database("tagihan baru tidak ditemukan".into()))
    }

    pub async fn get_invoice(&self, id: &str) -> Result<Option<Invoice>, WarasError> {
        let row: Option<InvoiceRow> =
            sqlx::query_as("SELECT * FROM invoices WHERE id = ? LIMIT 1")
                .bind(id)
                .fetch_optional(&self.pool)
                .await
                .map_err(db_err)?;
        row.map(|r| r.into_domain()).transpose()
    }

    pub async fn list_invoices(
        &self,
        status: Option<InvoiceStatus>,
    ) -> Result<Vec<Invoice>, WarasError> {
        let rows: Vec<InvoiceRow> = match status {
            Some(s) => {
                sqlx::query_as(
                    "SELECT * FROM invoices WHERE status = ? ORDER BY created_at DESC, id DESC",
                )
                .bind(invoice_status_to_str(s))
                .fetch_all(&self.pool)
                .await
                .map_err(db_err)?
            }
            None => {
                sqlx::query_as(
                    "SELECT * FROM invoices ORDER BY created_at DESC, id DESC",
                )
                .fetch_all(&self.pool)
                .await
                .map_err(db_err)?
            }
        };
        rows.into_iter().map(|r| r.into_domain()).collect()
    }

    pub async fn get_payments(&self, invoice_id: &str) -> Result<Vec<Payment>, WarasError> {
        let rows: Vec<PaymentRow> = sqlx::query_as(
            "SELECT * FROM payments WHERE invoice_id = ? ORDER BY paid_at ASC",
        )
        .bind(invoice_id)
        .fetch_all(&self.pool)
        .await
        .map_err(db_err)?;
        rows.into_iter().map(|r| r.into_domain()).collect()
    }

    /// Catat pembayaran. Bila total bayar >= jumlah tagihan -> Lunas;
    /// bila baru sebagian dan status masih Menunggu -> Cicilan.
    /// Audit `invoice.paid`.
    pub async fn pay_invoice(
        &self,
        id: &str,
        method: PaymentMethod,
        amount: i64,
        cashier: &str,
    ) -> Result<Invoice, WarasError> {
        let invoice = self
            .get_invoice(id)
            .await?
            .ok_or_else(|| WarasError::NotFound(format!("tagihan {id} tidak ditemukan")))?;
        if amount <= 0 {
            return Err(WarasError::Validation(
                "jumlah pembayaran harus lebih dari 0".into(),
            ));
        }
        let payment_id = format!("PAY-{}", new_uuid());
        let paid_at = fmt_dt(now_naive());
        sqlx::query(
            "INSERT INTO payments (id, invoice_id, method, amount, cashier, paid_at)
             VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(&payment_id)
        .bind(id)
        .bind(payment_method_to_str(method))
        .bind(amount)
        .bind(cashier)
        .bind(&paid_at)
        .execute(&self.pool)
        .await
        .map_err(db_err)?;
        let total_paid: i64 = sqlx::query_scalar(
            "SELECT COALESCE(SUM(amount), 0) FROM payments WHERE invoice_id = ?",
        )
        .bind(id)
        .fetch_one(&self.pool)
        .await
        .map_err(db_err)?;
        let new_status = if total_paid >= invoice.amount {
            InvoiceStatus::Lunas
        } else if invoice.status == InvoiceStatus::Menunggu {
            InvoiceStatus::Cicilan
        } else {
            invoice.status
        };
        if new_status != invoice.status {
            sqlx::query("UPDATE invoices SET status = ? WHERE id = ?")
                .bind(invoice_status_to_str(new_status))
                .bind(id)
                .execute(&self.pool)
                .await
                .map_err(db_err)?;
        }
        self.log_audit(
            cashier,
            "invoice.paid",
            "invoice",
            id,
            json!({"method": payment_method_to_str(method), "amount": amount, "total_paid": total_paid}),
        )
        .await?;
        self.get_invoice(id)
            .await?
            .ok_or_else(|| WarasError::Database("tagihan hilang setelah bayar".into()))
    }
}
