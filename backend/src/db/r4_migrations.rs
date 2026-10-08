//! R4 production migration composition.
//!
//! The historical 001-066 registries remain intact because many focused tests
//! exercise them directly. Production startup goes through the `db::run_all_*`
//! wrappers, which always apply the legacy registry first and then R4 extension
//! migrations through the same `schema_migrations` evidence table.

use chrono::{SecondsFormat, Utc};

const MIGRATION_067_ID: &str = "067_r4_interconnect_fabric";
const MIGRATION_067_DESCRIPTION: &str =
    "Create R4 Interconnect Event, Work, State and Outbox durable semantics";
const MIGRATION_068_ID: &str = "068_r4_plugin_host_security";
const MIGRATION_068_DESCRIPTION: &str =
    "Create R4 plugin package identity, installation grant and isolation persistence";
const MIGRATION_069_ID: &str = "069_r4_platform_security_hardening";
const MIGRATION_069_DESCRIPTION: &str =
    "Create R4 durable authentication throttling security state";
const MIGRATION_070_ID: &str = "070_r4_session_revocation_invariant";
const MIGRATION_070_DESCRIPTION: &str =
    "Revoke all identity sessions atomically whenever password credentials rotate";
const MIGRATION_071_ID: &str = "071_r4_r3_terminal_blocker_trigger_fix";
const MIGRATION_071_DESCRIPTION: &str =
    "Repair R3 terminal-blocker trigger field access across heterogeneous PostgreSQL tables";
const MIGRATION_072_ID: &str = "072_r4_tenant_scoped_catalog_names";
const MIGRATION_072_DESCRIPTION: &str =
    "Scope device-model and warehouse catalog-name uniqueness by tenant";
const MIGRATION_073_ID: &str = "073_r4_tenant_scoped_device_serials";
const MIGRATION_073_DESCRIPTION: &str =
    "Scope device serial identity and legacy serial foreign keys by tenant";
const MIGRATION_074_ID: &str = "074_r4_tenant_scoped_asset_purchases";
const MIGRATION_074_DESCRIPTION: &str =
    "Scope procurement asset-purchase identity and tenant ownership";
const MIGRATION_075_ID: &str = "075_r4_depreciation_tenant_invariant";
const MIGRATION_075_DESCRIPTION: &str = "Enforce tenant ownership for depreciation-log persistence";
const MIGRATION_076_ID: &str = "076_r4_tenant_scoped_invoice_numbers";
const MIGRATION_076_DESCRIPTION: &str = "Scope invoice-number uniqueness and allocation by tenant";
const MIGRATION_077_ID: &str = "077_r4_tenant_scoped_tax_config";
const MIGRATION_077_DESCRIPTION: &str =
    "Enforce PostgreSQL tax configuration tenant ownership across search-path schemas";
const MIGRATION_078_ID: &str = "078_r4_credit_score_tenant_invariant";
const MIGRATION_078_DESCRIPTION: &str =
    "Enforce PostgreSQL credit-score tenant-local customer identity across search-path schemas";
const MIGRATION_079_ID: &str = "079_r4_overdue_tenant_invariant";
const MIGRATION_079_DESCRIPTION: &str =
    "Enforce tenant-local Overdue configuration and escalation identity";
const MIGRATION_080_ID: &str = "080_r4_contract_tenant_invariant";
const MIGRATION_080_DESCRIPTION: &str =
    "Normalize Contract order identity and tenant-local referential integrity";
const MIGRATION_081_ID: &str = "081_r4_optical_sop_tenant_invariant";
const MIGRATION_081_DESCRIPTION: &str =
    "Normalize Optical SOP identities and tenant-local referential integrity";
const MIGRATION_082_ID: &str = "082_r4_reservation_rule_tenant_invariant";
const MIGRATION_082_DESCRIPTION: &str =
    "Enforce Reservation rule tenant ownership across PostgreSQL search-path schemas";
const MIGRATION_083_ID: &str = "083_r4_reservation_rule_sequence_invariant";
const MIGRATION_083_DESCRIPTION: &str =
    "Align Reservation rule BIGSERIAL sequence with historical explicit seed ids";

fn applied_at() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

#[cfg(feature = "sqlite")]
fn sqlite_applied(conn: &rusqlite::Connection, id: &str) -> anyhow::Result<bool> {
    Ok(conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM schema_migrations WHERE id=?1)",
        [id],
        |row| row.get(0),
    )?)
}

#[cfg(feature = "sqlite")]
fn run_sqlite_extension(
    conn: &rusqlite::Connection,
    id: &str,
    description: &str,
    sql: &str,
) -> anyhow::Result<Vec<String>> {
    if sqlite_applied(conn, id)? {
        return Ok(Vec::new());
    }

    let tx = conn.unchecked_transaction()?;
    tx.execute_batch(sql)?;
    tx.execute(
        "INSERT INTO schema_migrations (id,description,appliedAt) VALUES (?1,?2,?3)",
        rusqlite::params![id, description, applied_at()],
    )?;
    tx.commit()?;
    Ok(vec![id.to_owned()])
}

#[cfg(feature = "sqlite")]
pub(super) fn run_sqlite_extension_067(conn: &rusqlite::Connection) -> anyhow::Result<Vec<String>> {
    run_sqlite_extension(
        conn,
        MIGRATION_067_ID,
        MIGRATION_067_DESCRIPTION,
        include_str!("migrations/067_r4_interconnect_fabric.sql"),
    )
}

#[cfg(feature = "sqlite")]
pub(super) fn run_sqlite_extension_068(conn: &rusqlite::Connection) -> anyhow::Result<Vec<String>> {
    run_sqlite_extension(
        conn,
        MIGRATION_068_ID,
        MIGRATION_068_DESCRIPTION,
        include_str!("migrations/068_r4_plugin_host_security.sql"),
    )
}

#[cfg(feature = "sqlite")]
pub(super) fn run_sqlite_extension_069(conn: &rusqlite::Connection) -> anyhow::Result<Vec<String>> {
    run_sqlite_extension(
        conn,
        MIGRATION_069_ID,
        MIGRATION_069_DESCRIPTION,
        include_str!("migrations/069_r4_platform_security_hardening.sql"),
    )
}

#[cfg(feature = "sqlite")]
pub(super) fn run_sqlite_extension_070(conn: &rusqlite::Connection) -> anyhow::Result<Vec<String>> {
    run_sqlite_extension(
        conn,
        MIGRATION_070_ID,
        MIGRATION_070_DESCRIPTION,
        include_str!("migrations/070_r4_session_revocation_invariant.sql"),
    )
}

#[cfg(feature = "sqlite")]
pub(super) fn run_sqlite_extension_072(conn: &rusqlite::Connection) -> anyhow::Result<Vec<String>> {
    if sqlite_applied(conn, MIGRATION_072_ID)? {
        return Ok(Vec::new());
    }

    let previous_foreign_keys: i64 =
        conn.pragma_query_value(None, "foreign_keys", |row| row.get(0))?;
    conn.pragma_update(None, "foreign_keys", 0_i64)?;

    let result = (|| -> anyhow::Result<Vec<String>> {
        let tx = conn.unchecked_transaction()?;
        tx.execute_batch(include_str!(
            "migrations/072_r4_tenant_scoped_catalog_names.sql"
        ))?;

        {
            let mut statement = tx.prepare("PRAGMA foreign_key_check")?;
            let mut rows = statement.query([])?;
            if let Some(row) = rows.next()? {
                let table: String = row.get(0)?;
                let rowid: Option<i64> = row.get(1)?;
                let parent: String = row.get(2)?;
                anyhow::bail!(
                    "072 foreign_key_check failed: table={table} rowid={rowid:?} parent={parent}"
                );
            }
        }

        tx.execute(
            "INSERT INTO schema_migrations (id,description,appliedAt) VALUES (?1,?2,?3)",
            rusqlite::params![MIGRATION_072_ID, MIGRATION_072_DESCRIPTION, applied_at()],
        )?;
        tx.commit()?;
        Ok(vec![MIGRATION_072_ID.to_owned()])
    })();

    let restore = conn.pragma_update(None, "foreign_keys", previous_foreign_keys);
    match (result, restore) {
        (Ok(executed), Ok(())) => Ok(executed),
        (Err(error), Ok(())) => Err(error),
        (Ok(_), Err(error)) => Err(error.into()),
        (Err(error), Err(restore_error)) => Err(anyhow::anyhow!(
            "{error:#}; additionally failed to restore PRAGMA foreign_keys: {restore_error}"
        )),
    }
}

#[cfg(feature = "sqlite")]
pub(super) fn run_sqlite_extension_073(conn: &rusqlite::Connection) -> anyhow::Result<Vec<String>> {
    if sqlite_applied(conn, MIGRATION_073_ID)? {
        return Ok(Vec::new());
    }

    let previous_foreign_keys: i64 =
        conn.pragma_query_value(None, "foreign_keys", |row| row.get(0))?;
    conn.pragma_update(None, "foreign_keys", 0_i64)?;

    let result = (|| -> anyhow::Result<Vec<String>> {
        let tx = conn.unchecked_transaction()?;
        tx.execute_batch(include_str!(
            "migrations/073_r4_tenant_scoped_device_serials.sql"
        ))?;

        {
            let mut statement = tx.prepare("PRAGMA foreign_key_check")?;
            let mut rows = statement.query([])?;
            if let Some(row) = rows.next()? {
                let table: String = row.get(0)?;
                let rowid: Option<i64> = row.get(1)?;
                let parent: String = row.get(2)?;
                anyhow::bail!(
                    "073 foreign_key_check failed: table={table} rowid={rowid:?} parent={parent}"
                );
            }
        }

        tx.execute(
            "INSERT INTO schema_migrations (id,description,appliedAt) VALUES (?1,?2,?3)",
            rusqlite::params![MIGRATION_073_ID, MIGRATION_073_DESCRIPTION, applied_at()],
        )?;
        tx.commit()?;
        Ok(vec![MIGRATION_073_ID.to_owned()])
    })();

    let restore = conn.pragma_update(None, "foreign_keys", previous_foreign_keys);
    match (result, restore) {
        (Ok(executed), Ok(())) => Ok(executed),
        (Err(error), Ok(())) => Err(error),
        (Ok(_), Err(error)) => Err(error.into()),
        (Err(error), Err(restore_error)) => Err(anyhow::anyhow!(
            "{error:#}; additionally failed to restore PRAGMA foreign_keys: {restore_error}"
        )),
    }
}

#[cfg(feature = "sqlite")]
pub(super) fn run_sqlite_extension_074(conn: &rusqlite::Connection) -> anyhow::Result<Vec<String>> {
    run_sqlite_extension(
        conn,
        MIGRATION_074_ID,
        MIGRATION_074_DESCRIPTION,
        include_str!("migrations/074_r4_tenant_scoped_asset_purchases.sql"),
    )
}

#[cfg(feature = "sqlite")]
pub(super) fn run_sqlite_extension_075(conn: &rusqlite::Connection) -> anyhow::Result<Vec<String>> {
    run_sqlite_extension(
        conn,
        MIGRATION_075_ID,
        MIGRATION_075_DESCRIPTION,
        include_str!("migrations/075_r4_depreciation_tenant_invariant.sql"),
    )
}

#[cfg(feature = "sqlite")]
pub(super) fn run_sqlite_extension_076(conn: &rusqlite::Connection) -> anyhow::Result<Vec<String>> {
    run_sqlite_extension(
        conn,
        MIGRATION_076_ID,
        MIGRATION_076_DESCRIPTION,
        include_str!("migrations/076_r4_tenant_scoped_invoice_numbers.sql"),
    )
}

#[cfg(feature = "sqlite")]
pub(super) fn run_sqlite_extension_079(conn: &rusqlite::Connection) -> anyhow::Result<Vec<String>> {
    run_sqlite_extension(
        conn,
        MIGRATION_079_ID,
        MIGRATION_079_DESCRIPTION,
        include_str!("migrations/079_r4_overdue_tenant_invariant.sql"),
    )
}

#[cfg(feature = "sqlite")]
pub(super) fn run_sqlite_extension_080(conn: &rusqlite::Connection) -> anyhow::Result<Vec<String>> {
    if sqlite_applied(conn, MIGRATION_080_ID)? {
        return Ok(Vec::new());
    }

    let previous_foreign_keys: i64 =
        conn.pragma_query_value(None, "foreign_keys", |row| row.get(0))?;
    conn.pragma_update(None, "foreign_keys", 0_i64)?;

    let result = (|| -> anyhow::Result<Vec<String>> {
        let tx = conn.unchecked_transaction()?;
        tx.execute_batch(include_str!(
            "migrations/080_r4_contract_tenant_invariant.sql"
        ))?;

        {
            let mut statement = tx.prepare("PRAGMA foreign_key_check")?;
            let mut rows = statement.query([])?;
            if let Some(row) = rows.next()? {
                let table: String = row.get(0)?;
                let rowid: Option<i64> = row.get(1)?;
                let parent: String = row.get(2)?;
                anyhow::bail!(
                    "080 foreign_key_check failed: table={table} rowid={rowid:?} parent={parent}"
                );
            }
        }

        tx.execute(
            "INSERT INTO schema_migrations (id,description,appliedAt) VALUES (?1,?2,?3)",
            rusqlite::params![MIGRATION_080_ID, MIGRATION_080_DESCRIPTION, applied_at()],
        )?;
        tx.commit()?;
        Ok(vec![MIGRATION_080_ID.to_owned()])
    })();

    let restore = conn.pragma_update(None, "foreign_keys", previous_foreign_keys);
    match (result, restore) {
        (Ok(executed), Ok(())) => Ok(executed),
        (Err(error), Ok(())) => Err(error),
        (Ok(_), Err(error)) => Err(error.into()),
        (Err(error), Err(restore_error)) => Err(anyhow::anyhow!(
            "{error:#}; additionally failed to restore PRAGMA foreign_keys: {restore_error}"
        )),
    }
}

#[cfg(feature = "sqlite")]
pub(super) fn run_sqlite_extension_081(conn: &rusqlite::Connection) -> anyhow::Result<Vec<String>> {
    if sqlite_applied(conn, MIGRATION_081_ID)? {
        return Ok(Vec::new());
    }

    let previous_foreign_keys: i64 =
        conn.pragma_query_value(None, "foreign_keys", |row| row.get(0))?;
    conn.pragma_update(None, "foreign_keys", 0_i64)?;

    let result = (|| -> anyhow::Result<Vec<String>> {
        let tx = conn.unchecked_transaction()?;
        tx.execute_batch(include_str!(
            "migrations/081_r4_optical_sop_tenant_invariant.sql"
        ))?;

        {
            let mut statement = tx.prepare("PRAGMA foreign_key_check")?;
            let mut rows = statement.query([])?;
            if let Some(row) = rows.next()? {
                let table: String = row.get(0)?;
                let rowid: Option<i64> = row.get(1)?;
                let parent: String = row.get(2)?;
                anyhow::bail!(
                    "081 foreign_key_check failed: table={table} rowid={rowid:?} parent={parent}"
                );
            }
        }

        tx.execute(
            "INSERT INTO schema_migrations (id,description,appliedAt) VALUES (?1,?2,?3)",
            rusqlite::params![MIGRATION_081_ID, MIGRATION_081_DESCRIPTION, applied_at()],
        )?;
        tx.commit()?;
        Ok(vec![MIGRATION_081_ID.to_owned()])
    })();

    let restore = conn.pragma_update(None, "foreign_keys", previous_foreign_keys);
    match (result, restore) {
        (Ok(executed), Ok(())) => Ok(executed),
        (Err(error), Ok(())) => Err(error),
        (Ok(_), Err(error)) => Err(error.into()),
        (Err(error), Err(restore_error)) => Err(anyhow::anyhow!(
            "{error:#}; additionally failed to restore PRAGMA foreign_keys: {restore_error}"
        )),
    }
}

#[cfg(feature = "postgres")]
async fn pg_applied(pool: &sqlx::PgPool, id: &str) -> anyhow::Result<bool> {
    Ok(
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM schema_migrations WHERE id=$1)")
            .bind(id)
            .fetch_one(pool)
            .await?,
    )
}

#[cfg(feature = "postgres")]
async fn run_pg_extension(
    pool: &sqlx::PgPool,
    id: &str,
    description: &str,
    sql: &str,
) -> anyhow::Result<Vec<String>> {
    if pg_applied(pool, id).await? {
        return Ok(Vec::new());
    }

    let mut tx = pool.begin().await?;
    sqlx::raw_sql(sql).execute(&mut *tx).await?;
    sqlx::query("INSERT INTO schema_migrations (id,description,applied_at) VALUES ($1,$2,$3)")
        .bind(id)
        .bind(description)
        .bind(applied_at())
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(vec![id.to_owned()])
}

#[cfg(feature = "postgres")]
pub(super) async fn run_pg_extension_067(pool: &sqlx::PgPool) -> anyhow::Result<Vec<String>> {
    run_pg_extension(
        pool,
        MIGRATION_067_ID,
        MIGRATION_067_DESCRIPTION,
        include_str!("migrations/postgres/067_r4_interconnect_fabric.sql"),
    )
    .await
}

#[cfg(feature = "postgres")]
pub(super) async fn run_pg_extension_068(pool: &sqlx::PgPool) -> anyhow::Result<Vec<String>> {
    run_pg_extension(
        pool,
        MIGRATION_068_ID,
        MIGRATION_068_DESCRIPTION,
        include_str!("migrations/postgres/068_r4_plugin_host_security.sql"),
    )
    .await
}

#[cfg(feature = "postgres")]
pub(super) async fn run_pg_extension_069(pool: &sqlx::PgPool) -> anyhow::Result<Vec<String>> {
    run_pg_extension(
        pool,
        MIGRATION_069_ID,
        MIGRATION_069_DESCRIPTION,
        include_str!("migrations/postgres/069_r4_platform_security_hardening.sql"),
    )
    .await
}

#[cfg(feature = "postgres")]
pub(super) async fn run_pg_extension_070(pool: &sqlx::PgPool) -> anyhow::Result<Vec<String>> {
    run_pg_extension(
        pool,
        MIGRATION_070_ID,
        MIGRATION_070_DESCRIPTION,
        include_str!("migrations/postgres/070_r4_session_revocation_invariant.sql"),
    )
    .await
}

#[cfg(feature = "postgres")]
pub(super) async fn run_pg_extension_071(pool: &sqlx::PgPool) -> anyhow::Result<Vec<String>> {
    run_pg_extension(
        pool,
        MIGRATION_071_ID,
        MIGRATION_071_DESCRIPTION,
        include_str!("migrations/postgres/071_r4_r3_terminal_blocker_trigger_fix.sql"),
    )
    .await
}

#[cfg(feature = "postgres")]
pub(super) async fn run_pg_extension_072(pool: &sqlx::PgPool) -> anyhow::Result<Vec<String>> {
    run_pg_extension(
        pool,
        MIGRATION_072_ID,
        MIGRATION_072_DESCRIPTION,
        include_str!("migrations/postgres/072_r4_tenant_scoped_catalog_names.sql"),
    )
    .await
}

#[cfg(feature = "postgres")]
pub(super) async fn run_pg_extension_073(pool: &sqlx::PgPool) -> anyhow::Result<Vec<String>> {
    run_pg_extension(
        pool,
        MIGRATION_073_ID,
        MIGRATION_073_DESCRIPTION,
        include_str!("migrations/postgres/073_r4_tenant_scoped_device_serials.sql"),
    )
    .await
}

#[cfg(feature = "postgres")]
pub(super) async fn run_pg_extension_074(pool: &sqlx::PgPool) -> anyhow::Result<Vec<String>> {
    run_pg_extension(
        pool,
        MIGRATION_074_ID,
        MIGRATION_074_DESCRIPTION,
        include_str!("migrations/postgres/074_r4_tenant_scoped_asset_purchases.sql"),
    )
    .await
}

#[cfg(feature = "postgres")]
pub(super) async fn run_pg_extension_075(pool: &sqlx::PgPool) -> anyhow::Result<Vec<String>> {
    run_pg_extension(
        pool,
        MIGRATION_075_ID,
        MIGRATION_075_DESCRIPTION,
        include_str!("migrations/postgres/075_r4_depreciation_tenant_invariant.sql"),
    )
    .await
}

#[cfg(feature = "postgres")]
pub(super) async fn run_pg_extension_076(pool: &sqlx::PgPool) -> anyhow::Result<Vec<String>> {
    run_pg_extension(
        pool,
        MIGRATION_076_ID,
        MIGRATION_076_DESCRIPTION,
        include_str!("migrations/postgres/076_r4_tenant_scoped_invoice_numbers.sql"),
    )
    .await
}

#[cfg(feature = "postgres")]
pub(super) async fn run_pg_extension_077(pool: &sqlx::PgPool) -> anyhow::Result<Vec<String>> {
    run_pg_extension(
        pool,
        MIGRATION_077_ID,
        MIGRATION_077_DESCRIPTION,
        include_str!("migrations/postgres/077_r4_tenant_scoped_tax_config.sql"),
    )
    .await
}

#[cfg(feature = "postgres")]
pub(super) async fn run_pg_extension_078(pool: &sqlx::PgPool) -> anyhow::Result<Vec<String>> {
    run_pg_extension(
        pool,
        MIGRATION_078_ID,
        MIGRATION_078_DESCRIPTION,
        include_str!("migrations/postgres/078_r4_credit_score_tenant_invariant.sql"),
    )
    .await
}

#[cfg(feature = "postgres")]
pub(super) async fn run_pg_extension_079(pool: &sqlx::PgPool) -> anyhow::Result<Vec<String>> {
    run_pg_extension(
        pool,
        MIGRATION_079_ID,
        MIGRATION_079_DESCRIPTION,
        include_str!("migrations/postgres/079_r4_overdue_tenant_invariant.sql"),
    )
    .await
}

#[cfg(feature = "postgres")]
pub(super) async fn run_pg_extension_080(pool: &sqlx::PgPool) -> anyhow::Result<Vec<String>> {
    run_pg_extension(
        pool,
        MIGRATION_080_ID,
        MIGRATION_080_DESCRIPTION,
        include_str!("migrations/postgres/080_r4_contract_tenant_invariant.sql"),
    )
    .await
}

#[cfg(feature = "postgres")]
pub(super) async fn run_pg_extension_081(pool: &sqlx::PgPool) -> anyhow::Result<Vec<String>> {
    run_pg_extension(
        pool,
        MIGRATION_081_ID,
        MIGRATION_081_DESCRIPTION,
        include_str!("migrations/postgres/081_r4_optical_sop_tenant_invariant.sql"),
    )
    .await
}

#[cfg(feature = "postgres")]
pub(super) async fn run_pg_extension_082(pool: &sqlx::PgPool) -> anyhow::Result<Vec<String>> {
    run_pg_extension(
        pool,
        MIGRATION_082_ID,
        MIGRATION_082_DESCRIPTION,
        include_str!("migrations/postgres/082_r4_reservation_rule_tenant_invariant.sql"),
    )
    .await
}

#[cfg(feature = "postgres")]
pub(super) async fn run_pg_extension_083(pool: &sqlx::PgPool) -> anyhow::Result<Vec<String>> {
    run_pg_extension(
        pool,
        MIGRATION_083_ID,
        MIGRATION_083_DESCRIPTION,
        include_str!("migrations/postgres/083_r4_reservation_rule_sequence_invariant.sql"),
    )
    .await
}

#[cfg(all(test, feature = "sqlite"))]
mod tests {
    use super::*;

    #[test]
    fn sqlite_r4_extensions_are_registered_once_in_order() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        crate::db::migrations::run_migrations(&conn).unwrap();

        assert_eq!(
            run_sqlite_extension_067(&conn).unwrap(),
            vec![MIGRATION_067_ID.to_owned()]
        );
        assert_eq!(
            run_sqlite_extension_068(&conn).unwrap(),
            vec![MIGRATION_068_ID.to_owned()]
        );
        assert_eq!(
            run_sqlite_extension_069(&conn).unwrap(),
            vec![MIGRATION_069_ID.to_owned()]
        );
        assert_eq!(
            run_sqlite_extension_070(&conn).unwrap(),
            vec![MIGRATION_070_ID.to_owned()]
        );
        assert_eq!(
            run_sqlite_extension_072(&conn).unwrap(),
            vec![MIGRATION_072_ID.to_owned()]
        );
        assert_eq!(
            run_sqlite_extension_073(&conn).unwrap(),
            vec![MIGRATION_073_ID.to_owned()]
        );
        assert_eq!(
            run_sqlite_extension_074(&conn).unwrap(),
            vec![MIGRATION_074_ID.to_owned()]
        );
        assert_eq!(
            run_sqlite_extension_075(&conn).unwrap(),
            vec![MIGRATION_075_ID.to_owned()]
        );
        assert_eq!(
            run_sqlite_extension_076(&conn).unwrap(),
            vec![MIGRATION_076_ID.to_owned()]
        );
        assert_eq!(
            run_sqlite_extension_079(&conn).unwrap(),
            vec![MIGRATION_079_ID.to_owned()]
        );
        assert_eq!(
            run_sqlite_extension_080(&conn).unwrap(),
            vec![MIGRATION_080_ID.to_owned()]
        );
        assert_eq!(
            run_sqlite_extension_081(&conn).unwrap(),
            vec![MIGRATION_081_ID.to_owned()]
        );
        assert!(run_sqlite_extension_067(&conn).unwrap().is_empty());
        assert!(run_sqlite_extension_068(&conn).unwrap().is_empty());
        assert!(run_sqlite_extension_069(&conn).unwrap().is_empty());
        assert!(run_sqlite_extension_070(&conn).unwrap().is_empty());
        assert!(run_sqlite_extension_072(&conn).unwrap().is_empty());
        assert!(run_sqlite_extension_073(&conn).unwrap().is_empty());
        assert!(run_sqlite_extension_074(&conn).unwrap().is_empty());
        assert!(run_sqlite_extension_075(&conn).unwrap().is_empty());
        assert!(run_sqlite_extension_076(&conn).unwrap().is_empty());
        assert!(run_sqlite_extension_079(&conn).unwrap().is_empty());
        assert!(run_sqlite_extension_080(&conn).unwrap().is_empty());
        assert!(run_sqlite_extension_081(&conn).unwrap().is_empty());

        conn.execute(
            "INSERT INTO tenants (id,name,slug,status,plan,created_at,updated_at)
             VALUES ('tenant-catalog-a','Catalog A','tenant-catalog-a','active','test','now','now'),
                    ('tenant-catalog-b','Catalog B','tenant-catalog-b','active','test','now','now')",
            [],
        )
        .unwrap();
        for (tenant, suffix) in [("tenant-catalog-a", "a"), ("tenant-catalog-b", "b")] {
            conn.execute(
                "INSERT INTO device_models
                 (id,name,category,prefix,enabled,createdAt,updatedAt,tenant_id)
                 VALUES (?1,'Shared Model Name','camera',?2,1,'now','now',?3)",
                rusqlite::params![format!("catalog-model-{suffix}"), suffix, tenant],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO warehouses
                 (id,name,type,enabled,createdAt,updatedAt,tenant_id)
                 VALUES (?1,'Shared Warehouse Name','owned',1,'now','now',?2)",
                rusqlite::params![format!("catalog-warehouse-{suffix}"), tenant],
            )
            .unwrap();
        }
        assert!(conn.execute(
            "INSERT INTO device_models
             (id,name,category,prefix,enabled,createdAt,updatedAt,tenant_id)
             VALUES ('catalog-model-a-dup','Shared Model Name','camera','dup',1,'now','now','tenant-catalog-a')",
            [],
        ).is_err());
        assert!(conn.execute(
            "INSERT INTO warehouses
             (id,name,type,enabled,createdAt,updatedAt,tenant_id)
             VALUES ('catalog-warehouse-a-dup','Shared Warehouse Name','owned',1,'now','now','tenant-catalog-a')",
            [],
        ).is_err());

        conn.execute(
            "INSERT INTO model_base_prices
             (modelId,weekdayPrice,weekendPrice,updatedBy,createdAt,updatedAt,tenant_id)
             VALUES ('catalog-model-a',100,120,'test','now','now','tenant-catalog-a')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO warehouse_region_rules
             (id,warehouseId,province,shippingDays,returnDays,isPrimary,createdAt,updatedAt)
             VALUES ('catalog-rule-a','catalog-warehouse-a','Shanghai',1,2,1,'now','now')",
            [],
        )
        .unwrap();
        let foreign_key_violations: i64 = conn
            .query_row("SELECT COUNT(*) FROM pragma_foreign_key_check", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(foreign_key_violations, 0);

        conn.execute_batch(
            "INSERT INTO devices
             (id,serialNo,rentalStatus,createdAt,tenant_id)
             VALUES
             ('catalog-device-a','SHARED-SERIAL','已入库','now','tenant-catalog-a'),
             ('catalog-device-b','SHARED-SERIAL','已入库','now','tenant-catalog-b'),
             ('catalog-device-a-only','ONLY-A','已入库','now','tenant-catalog-a');
             INSERT INTO orders
             (id,orderNo,startDate,endDate,deliveryDate,pickupMethods,createdAt,tenant_id)
             VALUES
             ('catalog-order-a','CATALOG-A','2026-09-01','2026-09-02','2026-09-01','[]','now','tenant-catalog-a'),
             ('catalog-order-b','CATALOG-B','2026-09-01','2026-09-02','2026-09-01','[]','now','tenant-catalog-b');
             INSERT INTO order_devices (id,orderId,serialNo,createdAt,tenant_id)
             VALUES ('catalog-link-a','catalog-order-a','SHARED-SERIAL','now','tenant-catalog-a'),
                    ('catalog-link-b','catalog-order-b','SHARED-SERIAL','now','tenant-catalog-b');
             INSERT INTO damage_reports
             (id,order_id,device_serial_no,reported_at,created_at,updated_at,tenant_id)
             VALUES ('catalog-damage-a','catalog-order-a','SHARED-SERIAL','now','now','now','tenant-catalog-a');
             INSERT INTO repair_orders
             (id,damage_report_id,device_serial_no,created_at,updated_at,tenant_id)
             VALUES ('catalog-repair-a','catalog-damage-a','SHARED-SERIAL','now','now','tenant-catalog-a');",
        )
        .unwrap();
        assert!(
            conn.execute(
                "INSERT INTO devices
                 (id,serialNo,rentalStatus,createdAt,tenant_id)
                 VALUES ('catalog-device-a-dup','SHARED-SERIAL','已入库','now','tenant-catalog-a')",
                [],
            )
            .is_err()
        );
        assert!(
            conn.execute(
                "INSERT INTO order_devices (id,orderId,serialNo,createdAt,tenant_id)
                 VALUES ('catalog-link-b-invalid','catalog-order-b','ONLY-A','now','tenant-catalog-b')",
                [],
            )
            .is_err()
        );
        let serial_fk_violations: i64 = conn
            .query_row("SELECT COUNT(*) FROM pragma_foreign_key_check", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(serial_fk_violations, 0);

        let restored_device_cross_table_triggers: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_schema
                 WHERE type='trigger'
                   AND name IN ('prevent_tenant_delete_with_data','trg_allocation_device_model_insert')",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(restored_device_cross_table_triggers, 2);
        assert!(
            conn.execute("DELETE FROM tenants WHERE id='tenant-catalog-a'", [],)
                .is_err(),
            "073 must preserve the tenant-delete guard that references devices",
        );

        conn.execute(
            "INSERT INTO asset_purchases
             (id,device_serial_no,purchase_price,purchase_date,vendor,invoice_no,
              replacement_value,notes,created_at,tenant_id)
             VALUES ('purchase-a','SHARED-SERIAL',100,'2026-09-01','','',100,'','now','tenant-catalog-a')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO asset_purchases
             (id,device_serial_no,purchase_price,purchase_date,vendor,invoice_no,
              replacement_value,notes,created_at,tenant_id)
             VALUES ('purchase-b','SHARED-SERIAL',120,'2026-09-01','','',120,'','now','tenant-catalog-b')",
            [],
        )
        .unwrap();
        assert!(
            conn.execute(
                "INSERT INTO asset_purchases
                 (id,device_serial_no,purchase_price,purchase_date,vendor,invoice_no,
                  replacement_value,notes,created_at,tenant_id)
                 VALUES ('purchase-a-dup','SHARED-SERIAL',130,'2026-09-01','','',130,'','now','tenant-catalog-a')",
                [],
            )
            .is_err(),
            "asset purchase serials must remain unique inside one tenant",
        );

        conn.execute(
            "INSERT INTO depreciation_log
             (id,device_serial_no,period,opening_value,depreciation_amount,closing_value,method,created_at,tenant_id)
             VALUES ('dep-a','SHARED-SERIAL','2026-09',100,10,90,'straight_line','now','tenant-catalog-a')",
            [],
        )
        .unwrap();
        assert!(
            conn.execute(
                "INSERT INTO depreciation_log
                 (id,device_serial_no,period,opening_value,depreciation_amount,closing_value,method,created_at,tenant_id)
                 VALUES ('dep-null','SHARED-SERIAL','2026-10',90,10,80,'straight_line','now',NULL)",
                [],
            )
            .is_err(),
            "depreciation rows must require tenant ownership",
        );
        assert!(
            conn.execute(
                "INSERT INTO depreciation_log
                 (id,device_serial_no,period,opening_value,depreciation_amount,closing_value,method,created_at,tenant_id)
                 VALUES ('dep-foreign','SHARED-SERIAL','2026-10',90,10,80,'straight_line','now','missing-tenant')",
                [],
            )
            .is_err(),
            "depreciation rows must reference an existing tenant",
        );

        let interconnect_heads: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_schema WHERE type='table' AND name='interconnect_state_heads'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(interconnect_heads, 1);

        let plugin_packages: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_schema WHERE type='table' AND name='plugin_packages'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let plugin_installations: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_schema WHERE type='table' AND name='plugin_installations'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(plugin_packages, 1);
        assert_eq!(plugin_installations, 1);

        let auth_rate_limit_state: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_schema WHERE type='table' AND name='auth_rate_limit_state'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(auth_rate_limit_state, 1);

        let session_trigger: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_schema WHERE type='trigger' AND name='trg_auth_password_change_revoke_sessions'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(session_trigger, 1);

        conn.execute_batch(
            "INSERT INTO identities
             (id,username,password_hash,display_name,status,created_at,updated_at)
             VALUES ('p7-trigger-identity','p7-trigger-user','old-hash','P7','active','now','now');
             INSERT INTO auth_sessions
             (id,token_hash,identity_id,auth_strength,created_at,last_seen_at,expires_at)
             VALUES ('p7-trigger-session','0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef',
                     'p7-trigger-identity','password','now','now','later');
             UPDATE identities SET password_hash='new-hash' WHERE id='p7-trigger-identity';",
        )
        .unwrap();
        let remaining_sessions: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM auth_sessions WHERE identity_id='p7-trigger-identity'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(remaining_sessions, 0);

        let pg_070 = include_str!("migrations/postgres/070_r4_session_revocation_invariant.sql");
        assert!(pg_070.contains("talos_revoke_sessions_on_password_change"));
        assert!(pg_070.contains("DELETE FROM auth_sessions WHERE identity_id = NEW.id"));
        assert!(pg_070.contains("AFTER UPDATE OF password_hash ON identities"));

        for id in [
            MIGRATION_067_ID,
            MIGRATION_068_ID,
            MIGRATION_069_ID,
            MIGRATION_070_ID,
            MIGRATION_072_ID,
            MIGRATION_073_ID,
            MIGRATION_074_ID,
            MIGRATION_075_ID,
            MIGRATION_076_ID,
            MIGRATION_079_ID,
        ] {
            let recorded: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM schema_migrations WHERE id=?1",
                    [id],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(recorded, 1, "migration {id} must be recorded exactly once");
        }
    }

    #[test]
    fn sqlite_invoice_number_scope_is_tenant_local() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE schema_migrations (
                id TEXT PRIMARY KEY,
                description TEXT NOT NULL,
                appliedAt TEXT NOT NULL
            );
            CREATE TABLE invoices (
                id TEXT PRIMARY KEY,
                order_id TEXT NOT NULL,
                invoice_no TEXT NOT NULL UNIQUE,
                type TEXT NOT NULL,
                amount REAL NOT NULL,
                tax_rate REAL NOT NULL,
                tax_amount REAL NOT NULL,
                status TEXT NOT NULL,
                tenant_id TEXT NOT NULL,
                issued_at TEXT NOT NULL,
                voided_at TEXT,
                created_at TEXT NOT NULL
            );
            INSERT INTO invoices
            VALUES ('invoice-a','order-a','INV-20260929-0001','普通发票',100,0.13,13,
                    'issued','tenant-a','now',NULL,'now');",
        )
        .unwrap();

        assert_eq!(
            run_sqlite_extension_076(&conn).unwrap(),
            vec![MIGRATION_076_ID.to_owned()]
        );

        conn.execute(
            "INSERT INTO invoices
             VALUES ('invoice-b','order-b','INV-20260929-0001','普通发票',100,0.13,13,
                     'issued','tenant-b','now',NULL,'now')",
            [],
        )
        .expect("same invoice number must be valid in a different tenant");
        assert!(
            conn.execute(
                "INSERT INTO invoices
                 VALUES ('invoice-a-dup','order-a-dup','INV-20260929-0001','普通发票',100,0.13,13,
                         'issued','tenant-a','now',NULL,'now')",
                [],
            )
            .is_err()
        );
    }
}
