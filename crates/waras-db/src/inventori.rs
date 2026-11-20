//! Inventori (FASE4.md §4): item + mutasi stok.
//! Masuk: stok += qty; Keluar: stok -= qty (gagal 422 bila stok tidak
//! cukup); Opname: stok = qty. Mutasi + perubahan stok dalam satu transaksi.

use crate::rows::{clinic_now_string, InventoryItemRow, InventoryMoveRow};
use crate::{db_err, Db};
use waras_core::{
    types::{InventoryItem, InventoryMove},
    WarasError,
};

impl Db {
    pub async fn list_inventory_items(&self) -> Result<Vec<InventoryItem>, WarasError> {
        let rows: Vec<InventoryItemRow> =
            sqlx::query_as("SELECT * FROM inventory_items ORDER BY name ASC")
                .fetch_all(&self.pool)
                .await
                .map_err(db_err)?;
        rows.into_iter().map(|r| r.into_domain()).collect()
    }

    pub async fn get_inventory_item(
        &self,
        id: &str,
    ) -> Result<Option<InventoryItem>, WarasError> {
        let row: Option<InventoryItemRow> =
            sqlx::query_as("SELECT * FROM inventory_items WHERE id = ? LIMIT 1")
                .bind(id)
                .fetch_optional(&self.pool)
                .await
                .map_err(db_err)?;
        row.map(|r| r.into_domain()).transpose()
    }

    /// Item baru: id `INV-ITM-<seq>`.
    #[allow(clippy::too_many_arguments)]
    pub async fn create_inventory_item(
        &self,
        name: &str,
        category: &str,
        unit: &str,
        stock: i64,
        min_stock: i64,
        location: &str,
        expiry: &str,
    ) -> Result<InventoryItem, WarasError> {
        if stock < 0 {
            return Err(WarasError::Validation(
                "stok awal tidak boleh negatif".into(),
            ));
        }
        let ids: Vec<String> = sqlx::query_scalar("SELECT id FROM inventory_items")
            .fetch_all(&self.pool)
            .await
            .map_err(db_err)?;
        let seq = crate::next_seq_from_ids(&ids, 1);
        let id = format!("INV-ITM-{seq:03}");
        sqlx::query(
            "INSERT INTO inventory_items (id, name, category, unit, stock,
                 min_stock, location, expiry, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(name)
        .bind(category)
        .bind(unit)
        .bind(stock)
        .bind(min_stock)
        .bind(location)
        .bind(expiry)
        .bind(clinic_now_string())
        .execute(&self.pool)
        .await
        .map_err(db_err)?;
        self.get_inventory_item(&id)
            .await?
            .ok_or_else(|| WarasError::Database("item inventori baru tidak ditemukan".into()))
    }

    pub async fn list_inventory_moves(
        &self,
        item_id: Option<&str>,
    ) -> Result<Vec<InventoryMove>, WarasError> {
        let rows: Vec<InventoryMoveRow> = match item_id {
            Some(item) => {
                sqlx::query_as(
                    "SELECT * FROM inventory_moves WHERE item_id = ?
                     ORDER BY created_at DESC, id DESC",
                )
                .bind(item)
                .fetch_all(&self.pool)
                .await
                .map_err(db_err)?
            }
            None => {
                sqlx::query_as(
                    "SELECT * FROM inventory_moves ORDER BY created_at DESC, id DESC",
                )
                .fetch_all(&self.pool)
                .await
                .map_err(db_err)?
            }
        };
        rows.into_iter().map(|r| r.into_domain()).collect()
    }

    /// Mutasi stok: id `INM-<seq>`. Keluar gagal 422 bila stok tidak cukup.
    pub async fn add_inventory_move(
        &self,
        item_id: &str,
        kind: &str,
        qty: i64,
        note: &str,
        by_name: &str,
    ) -> Result<InventoryMove, WarasError> {
        if kind != "Masuk" && kind != "Keluar" && kind != "Opname" {
            return Err(WarasError::Validation(format!(
                "jenis mutasi harus 'Masuk', 'Keluar', atau 'Opname', bukan {kind:?}"
            )));
        }
        if qty < 0 {
            return Err(WarasError::Validation(
                "jumlah mutasi tidak boleh negatif".into(),
            ));
        }
        if kind != "Opname" && qty == 0 {
            return Err(WarasError::Validation(
                "jumlah mutasi Masuk/Keluar harus lebih dari 0".into(),
            ));
        }
        let mut tx = self.pool.begin().await.map_err(db_err)?;
        let item: Option<InventoryItemRow> =
            sqlx::query_as("SELECT * FROM inventory_items WHERE id = ? LIMIT 1")
                .bind(item_id)
                .fetch_optional(&mut tx)
                .await
                .map_err(db_err)?;
        let item = match item {
            Some(i) => i.into_domain()?,
            None => {
                return Err(WarasError::NotFound(format!(
                    "item inventori {item_id} tidak ditemukan"
                )))
            }
        };
        let new_stock = match kind {
            "Masuk" => item.stock + qty,
            "Keluar" => {
                if qty > item.stock {
                    return Err(WarasError::Unprocessable(format!(
                        "stok {} tidak cukup (sisa {}, keluar {})",
                        item.name, item.stock, qty
                    )));
                }
                item.stock - qty
            }
            "Opname" => qty,
            _ => unreachable!(),
        };
        let ids: Vec<String> = sqlx::query_scalar("SELECT id FROM inventory_moves")
            .fetch_all(&mut tx)
            .await
            .map_err(db_err)?;
        let seq = crate::next_seq_from_ids(&ids, 1);
        let id = format!("INM-{seq:06}");
        sqlx::query(
            "INSERT INTO inventory_moves (id, item_id, kind, qty, note, by_name, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(item_id)
        .bind(kind)
        .bind(qty)
        .bind(note)
        .bind(by_name)
        .bind(clinic_now_string())
        .execute(&mut tx)
        .await
        .map_err(db_err)?;
        sqlx::query("UPDATE inventory_items SET stock = ? WHERE id = ?")
            .bind(new_stock)
            .bind(item_id)
            .execute(&mut tx)
            .await
            .map_err(db_err)?;
        tx.commit().await.map_err(db_err)?;
        let row: Option<InventoryMoveRow> =
            sqlx::query_as("SELECT * FROM inventory_moves WHERE id = ? LIMIT 1")
                .bind(&id)
                .fetch_optional(&self.pool)
                .await
                .map_err(db_err)?;
        row.map(|r| r.into_domain())
            .transpose()?
            .ok_or_else(|| WarasError::Database("mutasi inventori baru tidak ditemukan".into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use waras_core::types::StockStatus;

    async fn seeded_db() -> Db {
        let db = Db::connect_for_tests().await.expect("buka DB test");
        db.migrate().await.expect("migrasi");
        assert!(db.seed_if_empty().await.expect("seed"));
        db
    }

    #[tokio::test]
    async fn seed_inventory_items_and_moves() {
        let db = seeded_db().await;
        let items = db.list_inventory_items().await.unwrap();
        assert_eq!(items.len(), 10);
        let by_id = |id: &str| items.iter().find(|i| i.id == id).unwrap();
        // Stok AKHIR setelah mutasi seed (INV-ITM-001 140+100, 003 360-40,
        // 009 4+4) sama dengan angka kontrak §4.2.
        assert_eq!(by_id("INV-ITM-001").stock, 240);
        assert_eq!(by_id("INV-ITM-001").name, "Infus set");
        assert_eq!(by_id("INV-ITM-003").stock, 320);
        assert_eq!(by_id("INV-ITM-009").stock, 8);
        assert_eq!(by_id("INV-ITM-009").name, "Oksigen tabung");
        // Status stok ambang Farmasi (12/30), FASE4.md §2.
        assert_eq!(
            waras_core::types::inventory_stock_status(by_id("INV-ITM-001").stock),
            StockStatus::Aman
        );
        assert_eq!(
            waras_core::types::inventory_stock_status(by_id("INV-ITM-007").stock),
            StockStatus::Segera
        );
        assert_eq!(
            waras_core::types::inventory_stock_status(by_id("INV-ITM-009").stock),
            StockStatus::Kritis
        );
        assert_eq!(
            waras_core::types::inventory_stock_status(by_id("INV-ITM-010").stock),
            StockStatus::Kritis
        );
        let moves = db.list_inventory_moves(None).await.unwrap();
        assert_eq!(moves.len(), 3);
        assert_eq!(
            db.list_inventory_moves(Some("INV-ITM-001"))
                .await
                .unwrap()
                .len(),
            1
        );
        assert_eq!(moves[0].kind, "Masuk");
    }

    #[tokio::test]
    async fn inventory_move_updates_stock() {
        let db = seeded_db().await;
        // Masuk: 240 -> 290.
        let m = db
            .add_inventory_move("INV-ITM-001", "Masuk", 50, "Tambahan", "apt. Dina Puspita")
            .await
            .unwrap();
        assert_eq!(m.id, "INM-000004");
        assert_eq!(
            db.get_inventory_item("INV-ITM-001").await.unwrap().unwrap().stock,
            290
        );
        // Keluar melebihi stok -> 422, stok tidak berubah.
        let err = db
            .add_inventory_move("INV-ITM-009", "Keluar", 9, "Pakai", "apt. Dina Puspita")
            .await;
        assert!(matches!(err, Err(WarasError::Unprocessable(_))));
        assert_eq!(err.unwrap_err().http_status(), 422);
        assert_eq!(
            db.get_inventory_item("INV-ITM-009").await.unwrap().unwrap().stock,
            8
        );
        // Keluar pas stok -> 0.
        db.add_inventory_move("INV-ITM-009", "Keluar", 8, "Pakai habis", "apt. Dina Puspita")
            .await
            .unwrap();
        assert_eq!(
            db.get_inventory_item("INV-ITM-009").await.unwrap().unwrap().stock,
            0
        );
        // Opname menetapkan stok = qty.
        db.add_inventory_move("INV-ITM-009", "Opname", 12, "Hitung ulang", "apt. Dina Puspita")
            .await
            .unwrap();
        assert_eq!(
            db.get_inventory_item("INV-ITM-009").await.unwrap().unwrap().stock,
            12
        );
        assert!(matches!(
            db.add_inventory_move("INV-ITM-001", "Pindah", 1, "", "x").await,
            Err(WarasError::Validation(_))
        ));
        assert!(matches!(
            db.add_inventory_move("INV-ITM-001", "Masuk", 0, "", "x").await,
            Err(WarasError::Validation(_))
        ));
        assert!(matches!(
            db.add_inventory_move("INV-X-001", "Masuk", 1, "", "x").await,
            Err(WarasError::NotFound(_))
        ));
        let item = db
            .create_inventory_item("Perban elastis", "Alkes", "pcs", 64, 20, "Gudang A", "")
            .await
            .unwrap();
        assert_eq!(item.id, "INV-ITM-011");
    }
}
