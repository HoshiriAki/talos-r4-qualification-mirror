#![cfg(feature = "postgres")]

use sqlx::Row;

use crate::repositories::RepositoryError;
use crate::repositories::session::RepositorySession;
use crate::repositories::warehouse::{
    NewWarehouse, UpsertWarehouseRegionRule, WarehouseAdvancedCapacityProjection,
    WarehouseDeviceDetailProjection, WarehouseDeviceMoveOutcome, WarehouseDevicePage,
    WarehouseDeviceProjection, WarehouseLowStockProjection, WarehouseMutationError, WarehousePatch,
    WarehouseProjection, WarehouseRegionRuleProjection, WarehouseRoutingRuleProjection,
    WarehouseStatsProjection,
};

pub(in crate::repositories) struct PostgresWarehouseRepository<'a> {
    session: &'a RepositorySession,
}

impl<'a> PostgresWarehouseRepository<'a> {
    pub(in crate::repositories) fn new(session: &'a RepositorySession) -> Self {
        Self { session }
    }

    pub(in crate::repositories) fn list(
        &self,
    ) -> Result<Vec<WarehouseProjection>, RepositoryError> {
        let tenant_id = self.tenant_id();
        self.session.pg_read(move |connection| {
            Box::pin(async move {
                sqlx::query(
                    "SELECT id,name,type,enabled,address,contactname,contactphone,notes,capacity,createdat,updatedat
                     FROM warehouses WHERE tenant_id=$1 ORDER BY name ASC",
                )
                .bind(tenant_id)
                .fetch_all(&mut *connection)
                .await?
                .iter()
                .map(map_warehouse)
                .collect()
            })
        })
    }

    pub(in crate::repositories) fn get(
        &self,
        id: &str,
    ) -> Result<Option<WarehouseProjection>, RepositoryError> {
        let tenant_id = self.tenant_id();
        let id = id.to_owned();
        self.session.pg_read(move |connection| {
            Box::pin(async move {
                let row = sqlx::query(
                    "SELECT id,name,type,enabled,address,contactname,contactphone,notes,capacity,createdat,updatedat
                     FROM warehouses WHERE tenant_id=$1 AND id=$2 LIMIT 1",
                )
                .bind(tenant_id)
                .bind(id)
                .fetch_optional(&mut *connection)
                .await?;
                row.as_ref().map(map_warehouse).transpose()
            })
        })
    }

    pub(in crate::repositories) fn find_by_name(
        &self,
        name: &str,
    ) -> Result<Option<WarehouseProjection>, RepositoryError> {
        let tenant_id = self.tenant_id();
        let name = name.to_owned();
        self.session.pg_read(move |connection| {
            Box::pin(async move {
                let row = sqlx::query(
                    "SELECT id,name,type,enabled,address,contactname,contactphone,notes,capacity,createdat,updatedat
                     FROM warehouses WHERE tenant_id=$1 AND name=$2 LIMIT 1",
                )
                .bind(tenant_id)
                .bind(name)
                .fetch_optional(&mut *connection)
                .await?;
                row.as_ref().map(map_warehouse).transpose()
            })
        })
    }

    pub(in crate::repositories) fn create(
        &self,
        input: &NewWarehouse,
    ) -> Result<WarehouseProjection, WarehouseMutationError> {
        if self.name_exists(&input.name, None)? {
            return Err(WarehouseMutationError::DuplicateName);
        }
        let tenant_id = self.tenant_id();
        let input = input.clone();
        let reload_id = input.id.clone();
        self.session
            .pg_write(move |connection| {
                Box::pin(async move {
                    sqlx::query(
                        "INSERT INTO warehouses
                         (id,name,type,enabled,address,contactname,contactphone,notes,capacity,createdat,updatedat,tenant_id)
                         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$10,$11)",
                    )
                    .bind(&input.id)
                    .bind(&input.name)
                    .bind(&input.wh_type)
                    .bind(input.enabled)
                    .bind(&input.address)
                    .bind(&input.contact_name)
                    .bind(&input.contact_phone)
                    .bind(&input.notes)
                    .bind(input.capacity)
                    .bind(&input.now)
                    .bind(&tenant_id)
                    .execute(&mut *connection)
                    .await?;
                    Ok(())
                })
            })
            .map_err(map_duplicate_name)?;
        self.get(&reload_id)?.ok_or_else(|| {
            RepositoryError::ContractViolation("created warehouse could not be reloaded".into())
                .into()
        })
    }

    pub(in crate::repositories) fn update(
        &self,
        id: &str,
        patch: &WarehousePatch,
    ) -> Result<WarehouseProjection, WarehouseMutationError> {
        if self.get(id)?.is_none() {
            return Err(WarehouseMutationError::NotFound);
        }
        if let Some(name) = patch.name.as_deref()
            && self.name_exists(name, Some(id))?
        {
            return Err(WarehouseMutationError::DuplicateName);
        }
        let has_fields = patch.name.is_some()
            || patch.wh_type.is_some()
            || patch.enabled.is_some()
            || patch.address.is_some()
            || patch.contact_name.is_some()
            || patch.contact_phone.is_some()
            || patch.notes.is_some()
            || patch.capacity.is_some();

        if has_fields {
            let tenant_id = self.tenant_id();
            let id = id.to_owned();
            let patch = patch.clone();
            self.session
                .pg_write(move |connection| {
                    Box::pin(async move {
                        sqlx::query(
                            "UPDATE warehouses SET
                                name=COALESCE($1,name),
                                type=COALESCE($2,type),
                                enabled=COALESCE($3,enabled),
                                address=COALESCE($4,address),
                                contactname=COALESCE($5,contactname),
                                contactphone=COALESCE($6,contactphone),
                                notes=COALESCE($7,notes),
                                capacity=COALESCE($8,capacity),
                                updatedat=$9
                             WHERE tenant_id=$10 AND id=$11",
                        )
                        .bind(patch.name.as_deref())
                        .bind(patch.wh_type.as_deref())
                        .bind(patch.enabled)
                        .bind(patch.address.as_deref())
                        .bind(patch.contact_name.as_deref())
                        .bind(patch.contact_phone.as_deref())
                        .bind(patch.notes.as_deref())
                        .bind(patch.capacity)
                        .bind(&patch.now)
                        .bind(&tenant_id)
                        .bind(&id)
                        .execute(&mut *connection)
                        .await?;
                        Ok(())
                    })
                })
                .map_err(map_duplicate_name)?;
        }

        self.get(id)?.ok_or_else(|| {
            RepositoryError::ContractViolation("updated warehouse could not be reloaded".into())
                .into()
        })
    }

    pub(in crate::repositories) fn delete(&self, id: &str) -> Result<(), WarehouseMutationError> {
        if self.get(id)?.is_none() {
            return Err(WarehouseMutationError::NotFound);
        }
        let tenant_id = self.tenant_id();
        let id_owned = id.to_owned();
        let device_count: i64 = self.session.pg_read({
            let tenant_id = tenant_id.clone();
            let id_owned = id_owned.clone();
            move |connection| {
                Box::pin(async move {
                    sqlx::query_scalar::<_, i64>(
                        "SELECT COUNT(*)::bigint FROM devices
                         WHERE tenant_id=$1
                           AND (currentwarehouseid=$2 OR expectedwarehouseid=$2)",
                    )
                    .bind(tenant_id)
                    .bind(id_owned)
                    .fetch_one(&mut *connection)
                    .await
                })
            }
        })?;
        if device_count > 0 {
            return Err(WarehouseMutationError::Referenced(
                u64::try_from(device_count).unwrap_or(u64::MAX),
            ));
        }

        self.session.pg_write(move |connection| {
            Box::pin(async move {
                sqlx::query("DELETE FROM warehouses WHERE tenant_id=$1 AND id=$2")
                    .bind(tenant_id)
                    .bind(id_owned)
                    .execute(&mut *connection)
                    .await?;
                Ok(())
            })
        })?;
        Ok(())
    }

    pub(in crate::repositories) fn stats(
        &self,
    ) -> Result<Vec<WarehouseStatsProjection>, RepositoryError> {
        let tenant_id = self.tenant_id();
        self.session.pg_read(move |connection| {
            Box::pin(async move {
                sqlx::query(
                    "SELECT
                        w.id,w.name,w.type,w.enabled,w.address,w.contactname,w.contactphone,w.notes,w.capacity,
                        w.createdat,w.updatedat,
                        COUNT(d.id)::bigint AS total_devices,
                        COUNT(d.id) FILTER (WHERE d.rentalstatus='已入库')::bigint AS available_devices,
                        COUNT(d.id) FILTER (WHERE d.rentalstatus='租赁中')::bigint AS rented_devices,
                        COUNT(d.id) FILTER (WHERE d.rentalstatus='返厂维修')::bigint AS repairing_devices
                     FROM warehouses w
                     LEFT JOIN devices d
                       ON d.currentwarehouseid=w.id AND d.tenant_id=w.tenant_id
                     WHERE w.tenant_id=$1
                     GROUP BY w.id
                     ORDER BY w.name ASC",
                )
                .bind(tenant_id)
                .fetch_all(&mut *connection)
                .await?
                .iter()
                .map(map_warehouse_stats)
                .collect()
            })
        })
    }

    pub(in crate::repositories) fn devices(
        &self,
        id: &str,
        status: Option<&str>,
    ) -> Result<Vec<WarehouseDeviceProjection>, WarehouseMutationError> {
        self.ensure_exists(id)?;
        let tenant_id = self.tenant_id();
        let id = id.to_owned();
        let status = status.filter(|value| !value.is_empty()).map(str::to_owned);
        self.session
            .pg_read(move |connection| {
                Box::pin(async move {
                    sqlx::query(
                        "SELECT d.id,d.serialno,COALESCE(dm.name,'') AS model_name,d.rentalstatus,
                                COALESCE(d.currentwarehouseid,'') AS warehouse_id,d.createdat
                         FROM devices d
                         LEFT JOIN device_models dm
                           ON dm.id=d.modelid AND dm.tenant_id=d.tenant_id
                         WHERE d.tenant_id=$1 AND d.currentwarehouseid=$2
                           AND ($3::text IS NULL OR d.rentalstatus=$3)
                         ORDER BY d.createdat DESC",
                    )
                    .bind(tenant_id)
                    .bind(id)
                    .bind(status)
                    .fetch_all(&mut *connection)
                    .await?
                    .iter()
                    .map(|row| {
                        Ok(WarehouseDeviceProjection {
                            id: row.try_get("id")?,
                            serial_no: row.try_get("serialno")?,
                            model_name: row.try_get("model_name")?,
                            status: row.try_get("rentalstatus")?,
                            warehouse_id: row.try_get("warehouse_id")?,
                            created_at: row.try_get("createdat")?,
                        })
                    })
                    .collect()
                })
            })
            .map_err(Into::into)
    }

    pub(in crate::repositories) fn devices_paged(
        &self,
        id: &str,
        page: i64,
        page_size: i64,
        keyword: Option<&str>,
    ) -> Result<WarehouseDevicePage, WarehouseMutationError> {
        self.ensure_exists(id)?;
        let page = page.max(1);
        let page_size = page_size.clamp(1, 100);
        let keyword = keyword
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|value| format!("%{value}%"));
        let tenant_id = self.tenant_id();
        let id = id.to_owned();
        self.session
            .pg_read(move |connection| {
                Box::pin(async move {
                    let total = sqlx::query_scalar::<_, i64>(
                        "SELECT COUNT(*)::bigint FROM devices d
                         WHERE d.currentwarehouseid=$1 AND d.tenant_id=$2
                           AND ($3::text IS NULL OR d.serialno ILIKE $3 OR COALESCE(d.notes,'') ILIKE $3)",
                    )
                    .bind(&id)
                    .bind(&tenant_id)
                    .bind(keyword.as_deref())
                    .fetch_one(&mut *connection)
                    .await?;
                    let offset = (page - 1) * page_size;
                    let data = sqlx::query(
                        "SELECT d.id,d.serialno,d.rentalstatus,d.modelid,d.notes,
                                d.currentwarehouseid,d.expectedwarehouseid,d.expectedavailabledate,d.createdat
                         FROM devices d
                         WHERE d.currentwarehouseid=$1 AND d.tenant_id=$2
                           AND ($3::text IS NULL OR d.serialno ILIKE $3 OR COALESCE(d.notes,'') ILIKE $3)
                         ORDER BY d.createdat DESC LIMIT $4 OFFSET $5",
                    )
                    .bind(&id)
                    .bind(&tenant_id)
                    .bind(keyword.as_deref())
                    .bind(page_size)
                    .bind(offset)
                    .fetch_all(&mut *connection)
                    .await?
                    .iter()
                    .map(map_warehouse_device_detail)
                    .collect::<Result<Vec<_>, _>>()?;
                    Ok(WarehouseDevicePage {
                        data,
                        total,
                        page,
                        page_size,
                    })
                })
            })
            .map_err(Into::into)
    }

    pub(in crate::repositories) fn advanced_low_stock(
        &self,
        warehouse_id: Option<&str>,
        threshold: i64,
    ) -> Result<Vec<WarehouseLowStockProjection>, RepositoryError> {
        let tenant_id = self.tenant_id();
        let warehouse_id = warehouse_id
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned);
        self.session.pg_read(move |connection| {
            Box::pin(async move {
                sqlx::query(
                    "SELECT w.id,w.name,
                            COUNT(d.id) FILTER (
                                WHERE d.rentalstatus IN ('available','已入库')
                            )::bigint AS available_count
                     FROM warehouses w
                     LEFT JOIN devices d
                       ON d.currentwarehouseid=w.id AND d.tenant_id=w.tenant_id
                     WHERE w.tenant_id=$1
                       AND ($2::text IS NULL OR w.id=$2)
                     GROUP BY w.id,w.name
                     HAVING COUNT(d.id) FILTER (
                                WHERE d.rentalstatus IN ('available','已入库')
                            ) < $3
                     ORDER BY w.name ASC",
                )
                .bind(tenant_id)
                .bind(warehouse_id)
                .bind(threshold)
                .fetch_all(&mut *connection)
                .await?
                .iter()
                .map(|row| {
                    Ok(WarehouseLowStockProjection {
                        warehouse_id: row.try_get("id")?,
                        warehouse_name: row.try_get("name")?,
                        available_count: row.try_get("available_count")?,
                    })
                })
                .collect()
            })
        })
    }

    pub(in crate::repositories) fn advanced_capacity_stats(
        &self,
    ) -> Result<Vec<WarehouseAdvancedCapacityProjection>, RepositoryError> {
        let tenant_id = self.tenant_id();
        self.session.pg_read(move |connection| {
            Box::pin(async move {
                sqlx::query(
                    "SELECT w.id,w.name,w.capacity,
                            COUNT(d.id)::bigint AS total_devices,
                            COUNT(d.id) FILTER (
                                WHERE d.rentalstatus IN ('available','已入库')
                            )::bigint AS available_devices,
                            COUNT(d.id) FILTER (
                                WHERE d.rentalstatus IN ('rented','租赁中')
                            )::bigint AS rented_devices
                     FROM warehouses w
                     LEFT JOIN devices d
                       ON d.currentwarehouseid=w.id AND d.tenant_id=w.tenant_id
                     WHERE w.tenant_id=$1
                     GROUP BY w.id,w.name,w.capacity
                     ORDER BY total_devices DESC,w.name ASC",
                )
                .bind(tenant_id)
                .fetch_all(&mut *connection)
                .await?
                .iter()
                .map(|row| {
                    Ok(WarehouseAdvancedCapacityProjection {
                        warehouse_id: row.try_get("id")?,
                        warehouse_name: row.try_get("name")?,
                        capacity: row.try_get::<i32, _>("capacity").map(i64::from)?,
                        total_devices: row.try_get("total_devices")?,
                        available_devices: row.try_get("available_devices")?,
                        rented_devices: row.try_get("rented_devices")?,
                    })
                })
                .collect()
            })
        })
    }

    pub(in crate::repositories) fn move_device_between_warehouses(
        &self,
        serial_no: &str,
        from_warehouse_id: &str,
        to_warehouse_id: &str,
    ) -> Result<WarehouseDeviceMoveOutcome, RepositoryError> {
        let tenant_id = self.tenant_id();
        let serial_no = serial_no.to_owned();
        let from_warehouse_id = from_warehouse_id.to_owned();
        let to_warehouse_id = to_warehouse_id.to_owned();

        self.session.pg_write_serializable(move |connection| {
            Box::pin(async move {
                let device_in_source = sqlx::query_scalar::<_, bool>(
                    "SELECT EXISTS(
                        SELECT 1
                        FROM devices d
                        JOIN warehouses w
                          ON w.id=d.currentwarehouseid AND w.tenant_id=d.tenant_id
                        WHERE d.tenant_id=$1 AND d.serialno=$2
                          AND d.currentwarehouseid=$3
                    )",
                )
                .bind(&tenant_id)
                .bind(&serial_no)
                .bind(&from_warehouse_id)
                .fetch_one(&mut *connection)
                .await?;
                if !device_in_source {
                    return Ok(WarehouseDeviceMoveOutcome::DeviceNotInSource);
                }

                let target_exists = sqlx::query_scalar::<_, bool>(
                    "SELECT EXISTS(
                        SELECT 1 FROM warehouses
                        WHERE tenant_id=$1 AND id=$2
                    )",
                )
                .bind(&tenant_id)
                .bind(&to_warehouse_id)
                .fetch_one(&mut *connection)
                .await?;
                if !target_exists {
                    return Ok(WarehouseDeviceMoveOutcome::TargetWarehouseNotFound);
                }

                let changed = sqlx::query(
                    "UPDATE devices
                     SET currentwarehouseid=$1
                     WHERE tenant_id=$2 AND serialno=$3 AND currentwarehouseid=$4",
                )
                .bind(&to_warehouse_id)
                .bind(&tenant_id)
                .bind(&serial_no)
                .bind(&from_warehouse_id)
                .execute(&mut *connection)
                .await?
                .rows_affected();
                if changed == 1 {
                    Ok(WarehouseDeviceMoveOutcome::Moved)
                } else {
                    Ok(WarehouseDeviceMoveOutcome::DeviceNotInSource)
                }
            })
        })
    }

    pub(in crate::repositories) fn routing_rules_for_province(
        &self,
        province: &str,
    ) -> Result<Vec<WarehouseRoutingRuleProjection>, RepositoryError> {
        let tenant_id = self.tenant_id();
        let province = province.to_owned();
        self.session.pg_read(move |connection| {
            Box::pin(async move {
                sqlx::query(
                    "SELECT r.warehouseid,w.name,w.type,r.shippingdays,r.returndays
                     FROM warehouse_region_rules r
                     JOIN warehouses w ON w.id=r.warehouseid
                     WHERE w.tenant_id=$1 AND r.province=$2 AND w.enabled=true
                     ORDER BY r.shippingdays ASC,r.returndays ASC,r.warehouseid ASC",
                )
                .bind(tenant_id)
                .bind(province)
                .fetch_all(&mut *connection)
                .await?
                .iter()
                .map(map_routing_rule)
                .collect()
            })
        })
    }

    pub(in crate::repositories) fn region_rules(
        &self,
        id: &str,
    ) -> Result<Vec<WarehouseRegionRuleProjection>, WarehouseMutationError> {
        self.ensure_exists(id)?;
        let tenant_id = self.tenant_id();
        let id = id.to_owned();
        self.session
            .pg_read(move |connection| {
                Box::pin(async move {
                    sqlx::query(
                        "SELECT r.id,r.warehouseid,r.province,r.shippingdays,r.returndays,
                                r.isprimary,r.createdat,r.updatedat
                         FROM warehouse_region_rules r
                         JOIN warehouses w ON w.id=r.warehouseid
                         WHERE r.warehouseid=$1 AND w.tenant_id=$2
                         ORDER BY r.province ASC",
                    )
                    .bind(id)
                    .bind(tenant_id)
                    .fetch_all(&mut *connection)
                    .await?
                    .iter()
                    .map(map_region_rule)
                    .collect()
                })
            })
            .map_err(Into::into)
    }

    pub(in crate::repositories) fn upsert_region_rule(
        &self,
        warehouse_id: &str,
        input: &UpsertWarehouseRegionRule,
    ) -> Result<Vec<WarehouseRegionRuleProjection>, WarehouseMutationError> {
        self.ensure_exists(warehouse_id)?;
        let warehouse_id_owned = warehouse_id.to_owned();
        let input = input.clone();
        self.session.pg_write(move |connection| {
            Box::pin(async move {
                sqlx::query(
                    "INSERT INTO warehouse_region_rules
                     (id,warehouseid,province,shippingdays,returndays,isprimary,createdat,updatedat)
                     VALUES ($1,$2,$3,$4,$5,$6,$7,$7)
                     ON CONFLICT (warehouseid,province) DO UPDATE SET
                        shippingdays=EXCLUDED.shippingdays,
                        returndays=EXCLUDED.returndays,
                        isprimary=EXCLUDED.isprimary,
                        updatedat=EXCLUDED.updatedat",
                )
                .bind(input.id)
                .bind(warehouse_id_owned)
                .bind(input.province)
                .bind(input.shipping_days)
                .bind(input.return_days)
                .bind(input.is_primary)
                .bind(input.now)
                .execute(&mut *connection)
                .await?;
                Ok(())
            })
        })?;
        self.region_rules(warehouse_id)
    }

    pub(in crate::repositories) fn delete_region_rule(
        &self,
        warehouse_id: &str,
        province: &str,
    ) -> Result<(), WarehouseMutationError> {
        self.ensure_exists(warehouse_id)?;
        let tenant_id = self.tenant_id();
        let warehouse_id = warehouse_id.to_owned();
        let province = province.to_owned();
        let changed = self.session.pg_write(move |connection| {
            Box::pin(async move {
                let result = sqlx::query(
                    "DELETE FROM warehouse_region_rules r
                     USING warehouses w
                     WHERE r.warehouseid=$1 AND r.province=$2
                       AND w.id=r.warehouseid AND w.tenant_id=$3",
                )
                .bind(warehouse_id)
                .bind(province)
                .bind(tenant_id)
                .execute(&mut *connection)
                .await?;
                Ok(result.rows_affected())
            })
        })?;
        if changed == 0 {
            return Err(WarehouseMutationError::RegionRuleNotFound);
        }
        Ok(())
    }

    fn ensure_exists(&self, id: &str) -> Result<(), WarehouseMutationError> {
        if self.get(id)?.is_none() {
            Err(WarehouseMutationError::NotFound)
        } else {
            Ok(())
        }
    }

    fn name_exists(&self, name: &str, exclude_id: Option<&str>) -> Result<bool, RepositoryError> {
        let tenant_id = self.tenant_id();
        let name = name.to_owned();
        let exclude_id = exclude_id.map(str::to_owned);
        self.session.pg_read(move |connection| {
            Box::pin(async move {
                match exclude_id {
                    Some(exclude_id) => {
                        sqlx::query_scalar::<_, bool>(
                            "SELECT EXISTS(
                                SELECT 1 FROM warehouses
                                WHERE tenant_id=$1 AND name=$2 AND id<>$3
                             )",
                        )
                        .bind(tenant_id)
                        .bind(name)
                        .bind(exclude_id)
                        .fetch_one(&mut *connection)
                        .await
                    }
                    None => {
                        sqlx::query_scalar::<_, bool>(
                            "SELECT EXISTS(
                                SELECT 1 FROM warehouses
                                WHERE tenant_id=$1 AND name=$2
                             )",
                        )
                        .bind(tenant_id)
                        .bind(name)
                        .fetch_one(&mut *connection)
                        .await
                    }
                }
            })
        })
    }

    fn tenant_id(&self) -> String {
        self.session.binding().tenant_id().as_str().to_owned()
    }
}

fn map_duplicate_name(error: RepositoryError) -> WarehouseMutationError {
    if error
        .to_string()
        .contains("idx_warehouses_tenant_name_unique")
    {
        WarehouseMutationError::DuplicateName
    } else {
        WarehouseMutationError::Storage(error)
    }
}

fn map_warehouse(row: &sqlx::postgres::PgRow) -> Result<WarehouseProjection, sqlx::Error> {
    Ok(WarehouseProjection {
        id: row.try_get("id")?,
        name: row.try_get("name")?,
        wh_type: row.try_get("type")?,
        enabled: row.try_get("enabled")?,
        address: row
            .try_get::<Option<String>, _>("address")?
            .unwrap_or_default(),
        contact_name: row
            .try_get::<Option<String>, _>("contactname")?
            .unwrap_or_default(),
        contact_phone: row
            .try_get::<Option<String>, _>("contactphone")?
            .unwrap_or_default(),
        notes: row
            .try_get::<Option<String>, _>("notes")?
            .unwrap_or_default(),
        capacity: row.try_get::<Option<i32>, _>("capacity")?.unwrap_or(0),
        created_at: row.try_get("createdat")?,
        updated_at: row.try_get("updatedat")?,
    })
}

fn map_warehouse_stats(
    row: &sqlx::postgres::PgRow,
) -> Result<WarehouseStatsProjection, sqlx::Error> {
    let capacity = row.try_get::<Option<i32>, _>("capacity")?.unwrap_or(0);
    let total_devices: i64 = row.try_get("total_devices")?;
    let utilization_percent = if capacity > 0 {
        ((total_devices as f64 / capacity as f64) * 100.0).round() as i64
    } else {
        0
    };
    Ok(WarehouseStatsProjection {
        id: row.try_get("id")?,
        name: row.try_get("name")?,
        wh_type: row.try_get("type")?,
        enabled: row.try_get("enabled")?,
        address: row
            .try_get::<Option<String>, _>("address")?
            .unwrap_or_default(),
        contact_name: row
            .try_get::<Option<String>, _>("contactname")?
            .unwrap_or_default(),
        contact_phone: row
            .try_get::<Option<String>, _>("contactphone")?
            .unwrap_or_default(),
        notes: row
            .try_get::<Option<String>, _>("notes")?
            .unwrap_or_default(),
        capacity,
        total_devices,
        available_devices: row.try_get("available_devices")?,
        rented_devices: row.try_get("rented_devices")?,
        repairing_devices: row.try_get("repairing_devices")?,
        utilization_percent,
        created_at: row.try_get("createdat")?,
        updated_at: row.try_get("updatedat")?,
    })
}

fn map_warehouse_device_detail(
    row: &sqlx::postgres::PgRow,
) -> Result<WarehouseDeviceDetailProjection, sqlx::Error> {
    Ok(WarehouseDeviceDetailProjection {
        id: row.try_get("id")?,
        serial_no: row.try_get("serialno")?,
        rental_status: row.try_get("rentalstatus")?,
        model_id: row
            .try_get::<Option<String>, _>("modelid")?
            .unwrap_or_default(),
        notes: row
            .try_get::<Option<String>, _>("notes")?
            .unwrap_or_default(),
        current_warehouse_id: row
            .try_get::<Option<String>, _>("currentwarehouseid")?
            .unwrap_or_default(),
        expected_warehouse_id: row
            .try_get::<Option<String>, _>("expectedwarehouseid")?
            .unwrap_or_default(),
        expected_available_date: row
            .try_get::<Option<String>, _>("expectedavailabledate")?
            .unwrap_or_default(),
        created_at: row.try_get("createdat")?,
    })
}

fn map_routing_rule(
    row: &sqlx::postgres::PgRow,
) -> Result<WarehouseRoutingRuleProjection, sqlx::Error> {
    Ok(WarehouseRoutingRuleProjection {
        warehouse_id: row.try_get("warehouseid")?,
        warehouse_name: row.try_get("name")?,
        warehouse_type: row.try_get("type")?,
        shipping_days: row.try_get("shippingdays")?,
        return_days: row.try_get("returndays")?,
    })
}

fn map_region_rule(
    row: &sqlx::postgres::PgRow,
) -> Result<WarehouseRegionRuleProjection, sqlx::Error> {
    Ok(WarehouseRegionRuleProjection {
        id: row.try_get("id")?,
        warehouse_id: row.try_get("warehouseid")?,
        province: row.try_get("province")?,
        shipping_days: row.try_get("shippingdays")?,
        return_days: row.try_get("returndays")?,
        is_primary: row.try_get("isprimary")?,
        created_at: row.try_get("createdat")?,
        updated_at: row.try_get("updatedat")?,
    })
}
