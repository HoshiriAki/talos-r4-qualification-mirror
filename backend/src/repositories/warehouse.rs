use rusqlite::{OptionalExtension, params};
use serde::Serialize;

use crate::repositories::session::RepositorySession;
use crate::repositories::{RepositoryError, ScopedRepositories};

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WarehouseProjection {
    pub id: String,
    pub name: String,
    #[serde(rename = "type")]
    pub wh_type: String,
    pub enabled: bool,
    pub address: String,
    pub contact_name: String,
    pub contact_phone: String,
    pub notes: String,
    pub capacity: i32,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WarehouseStatsProjection {
    pub id: String,
    pub name: String,
    #[serde(rename = "type")]
    pub wh_type: String,
    pub enabled: bool,
    pub address: String,
    pub contact_name: String,
    pub contact_phone: String,
    pub notes: String,
    pub capacity: i32,
    pub total_devices: i64,
    pub available_devices: i64,
    pub rented_devices: i64,
    pub repairing_devices: i64,
    pub utilization_percent: i64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WarehouseDeviceProjection {
    pub id: String,
    pub serial_no: String,
    pub model_name: String,
    pub status: String,
    pub warehouse_id: String,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WarehouseDeviceDetailProjection {
    pub id: String,
    pub serial_no: String,
    pub rental_status: String,
    pub model_id: String,
    pub notes: String,
    pub current_warehouse_id: String,
    pub expected_warehouse_id: String,
    pub expected_available_date: String,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WarehouseDevicePage {
    pub data: Vec<WarehouseDeviceDetailProjection>,
    pub total: i64,
    pub page: i64,
    pub page_size: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WarehouseRegionRuleProjection {
    pub id: String,
    pub warehouse_id: String,
    pub province: String,
    pub shipping_days: i32,
    pub return_days: i32,
    pub is_primary: bool,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WarehouseRoutingRuleProjection {
    pub warehouse_id: String,
    pub warehouse_name: String,
    pub warehouse_type: String,
    pub shipping_days: i32,
    pub return_days: i32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WarehouseLowStockProjection {
    pub warehouse_id: String,
    pub warehouse_name: String,
    pub available_count: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct WarehouseAdvancedCapacityProjection {
    pub warehouse_id: String,
    pub warehouse_name: String,
    pub capacity: i64,
    pub total_devices: i64,
    pub available_devices: i64,
    pub rented_devices: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WarehouseDeviceMoveOutcome {
    Moved,
    DeviceNotInSource,
    TargetWarehouseNotFound,
}

#[derive(Debug, Clone)]
pub struct NewWarehouse {
    pub id: String,
    pub name: String,
    pub wh_type: String,
    pub enabled: bool,
    pub address: String,
    pub contact_name: String,
    pub contact_phone: String,
    pub notes: String,
    pub capacity: i32,
    pub now: String,
}

#[derive(Debug, Clone, Default)]
pub struct WarehousePatch {
    pub name: Option<String>,
    pub wh_type: Option<String>,
    pub enabled: Option<bool>,
    pub address: Option<String>,
    pub contact_name: Option<String>,
    pub contact_phone: Option<String>,
    pub notes: Option<String>,
    pub capacity: Option<i32>,
    pub now: String,
}

#[derive(Debug, Clone)]
pub struct UpsertWarehouseRegionRule {
    pub id: String,
    pub province: String,
    pub shipping_days: i32,
    pub return_days: i32,
    pub is_primary: bool,
    pub now: String,
}

#[derive(Debug, thiserror::Error)]
pub enum WarehouseMutationError {
    #[error("warehouse not found")]
    NotFound,
    #[error("warehouse name already exists")]
    DuplicateName,
    #[error("warehouse is referenced by {0} devices")]
    Referenced(u64),
    #[error("warehouse region rule not found")]
    RegionRuleNotFound,
    #[error(transparent)]
    Storage(#[from] RepositoryError),
}

pub(in crate::repositories) struct SqliteWarehouseRepository<'a> {
    session: &'a RepositorySession,
}

impl<'a> SqliteWarehouseRepository<'a> {
    pub(in crate::repositories) fn new(scoped: &'a ScopedRepositories) -> Self {
        Self {
            session: scoped.session(),
        }
    }

    pub(in crate::repositories) fn list(
        &self,
    ) -> Result<Vec<WarehouseProjection>, RepositoryError> {
        let tenant_id = self.tenant_id();
        self.session.read(move |connection| {
            let mut statement = connection.prepare(
                "SELECT id,name,type,enabled,address,contactName,contactPhone,notes,capacity,createdAt,updatedAt
                 FROM warehouses WHERE tenant_id=?1 ORDER BY name ASC",
            )?;
            statement
                .query_map([tenant_id], map_warehouse)?
                .collect::<Result<Vec<_>, _>>()
        })
    }

    pub(in crate::repositories) fn get(
        &self,
        id: &str,
    ) -> Result<Option<WarehouseProjection>, RepositoryError> {
        let tenant_id = self.tenant_id();
        let id = id.to_owned();
        self.session.read(move |connection| {
            connection
                .query_row(
                    "SELECT id,name,type,enabled,address,contactName,contactPhone,notes,capacity,createdAt,updatedAt
                     FROM warehouses WHERE tenant_id=?1 AND id=?2 LIMIT 1",
                    params![tenant_id, id],
                    map_warehouse,
                )
                .optional()
        })
    }

    pub(in crate::repositories) fn find_by_name(
        &self,
        name: &str,
    ) -> Result<Option<WarehouseProjection>, RepositoryError> {
        let tenant_id = self.tenant_id();
        let name = name.to_owned();
        self.session.read(move |connection| {
            connection
                .query_row(
                    "SELECT id,name,type,enabled,address,contactName,contactPhone,notes,capacity,createdAt,updatedAt
                     FROM warehouses WHERE tenant_id=?1 AND name=?2 LIMIT 1",
                    params![tenant_id, name],
                    map_warehouse,
                )
                .optional()
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
        self.session.write(move |tx| {
            tx.execute(
                "INSERT INTO warehouses
                 (id,name,type,enabled,address,contactName,contactPhone,notes,capacity,createdAt,updatedAt,tenant_id)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?10,?11)",
                params![
                    input.id,
                    input.name,
                    input.wh_type,
                    if input.enabled { 1 } else { 0 },
                    input.address,
                    input.contact_name,
                    input.contact_phone,
                    input.notes,
                    input.capacity,
                    input.now,
                    tenant_id,
                ],
            )?;
            Ok(())
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
                .write(move |tx| {
                    tx.execute(
                        "UPDATE warehouses SET
                        name=COALESCE(?1,name),
                        type=COALESCE(?2,type),
                        enabled=COALESCE(?3,enabled),
                        address=COALESCE(?4,address),
                        contactName=COALESCE(?5,contactName),
                        contactPhone=COALESCE(?6,contactPhone),
                        notes=COALESCE(?7,notes),
                        capacity=COALESCE(?8,capacity),
                        updatedAt=?9
                     WHERE tenant_id=?10 AND id=?11",
                        params![
                            patch.name,
                            patch.wh_type,
                            patch.enabled.map(|value| if value { 1 } else { 0 }),
                            patch.address,
                            patch.contact_name,
                            patch.contact_phone,
                            patch.notes,
                            patch.capacity,
                            patch.now,
                            tenant_id,
                            id,
                        ],
                    )?;
                    Ok(())
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
        let device_count: i64 = self.session.read({
            let tenant_id = tenant_id.clone();
            let id_owned = id_owned.clone();
            move |connection| {
                connection.query_row(
                    "SELECT COUNT(*) FROM devices
                     WHERE tenant_id=?1 AND (currentWarehouseId=?2 OR expectedWarehouseId=?2)",
                    params![tenant_id, id_owned],
                    |row| row.get(0),
                )
            }
        })?;
        if device_count > 0 {
            return Err(WarehouseMutationError::Referenced(
                u64::try_from(device_count).unwrap_or(u64::MAX),
            ));
        }
        self.session.write(move |tx| {
            tx.execute(
                "DELETE FROM warehouses WHERE tenant_id=?1 AND id=?2",
                params![tenant_id, id_owned],
            )?;
            Ok(())
        })?;
        Ok(())
    }

    pub(in crate::repositories) fn stats(
        &self,
    ) -> Result<Vec<WarehouseStatsProjection>, RepositoryError> {
        let tenant_id = self.tenant_id();
        self.session.read(move |connection| {
            let mut statement = connection.prepare(
                "SELECT
                    w.id,w.name,w.type,w.enabled,w.address,w.contactName,w.contactPhone,w.notes,w.capacity,
                    w.createdAt,w.updatedAt,
                    COUNT(d.id),
                    COALESCE(SUM(CASE WHEN d.rentalStatus='已入库' THEN 1 ELSE 0 END),0),
                    COALESCE(SUM(CASE WHEN d.rentalStatus='租赁中' THEN 1 ELSE 0 END),0),
                    COALESCE(SUM(CASE WHEN d.rentalStatus='返厂维修' THEN 1 ELSE 0 END),0)
                 FROM warehouses w
                 LEFT JOIN devices d
                   ON d.currentWarehouseId=w.id AND d.tenant_id=w.tenant_id
                 WHERE w.tenant_id=?1
                 GROUP BY w.id
                 ORDER BY w.name ASC",
            )?;
            statement
                .query_map([tenant_id], map_warehouse_stats)?
                .collect::<Result<Vec<_>, _>>()
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
            .read(move |connection| {
                let mut statement = connection.prepare(
                    "SELECT d.id,d.serialNo,COALESCE(dm.name,''),d.rentalStatus,
                            COALESCE(d.currentWarehouseId,''),d.createdAt
                     FROM devices d
                     LEFT JOIN device_models dm
                       ON dm.id=d.modelId AND dm.tenant_id=d.tenant_id
                     WHERE d.tenant_id=?1 AND d.currentWarehouseId=?2
                       AND (?3 IS NULL OR d.rentalStatus=?3)
                     ORDER BY d.createdAt DESC",
                )?;
                statement
                    .query_map(params![tenant_id, id, status], |row| {
                        Ok(WarehouseDeviceProjection {
                            id: row.get(0)?,
                            serial_no: row.get(1)?,
                            model_name: row.get(2)?,
                            status: row.get(3)?,
                            warehouse_id: row.get(4)?,
                            created_at: row.get(5)?,
                        })
                    })?
                    .collect::<Result<Vec<_>, _>>()
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
            .read(move |connection| {
                let total: i64 = connection.query_row(
                    "SELECT COUNT(*) FROM devices d
                     WHERE d.currentWarehouseId=?1 AND d.tenant_id=?2
                       AND (?3 IS NULL OR d.serialNo LIKE ?3 OR d.notes LIKE ?3)",
                    params![id, tenant_id, keyword],
                    |row| row.get(0),
                )?;
                let offset = (page - 1) * page_size;
                let mut statement = connection.prepare(
                    "SELECT d.id,d.serialNo,d.rentalStatus,d.modelId,d.notes,
                            d.currentWarehouseId,d.expectedWarehouseId,d.expectedAvailableDate,d.createdAt
                     FROM devices d
                     WHERE d.currentWarehouseId=?1 AND d.tenant_id=?2
                       AND (?3 IS NULL OR d.serialNo LIKE ?3 OR d.notes LIKE ?3)
                     ORDER BY d.createdAt DESC LIMIT ?4 OFFSET ?5",
                )?;
                let data = statement
                    .query_map(
                        params![id, tenant_id, keyword, page_size, offset],
                        map_warehouse_device_detail,
                    )?
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(WarehouseDevicePage {
                    data,
                    total,
                    page,
                    page_size,
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
        self.session.read(move |connection| {
            let mut statement = connection.prepare(
                "SELECT w.id,w.name,
                        COALESCE(SUM(CASE
                            WHEN d.rentalStatus IN ('available','已入库') THEN 1
                            ELSE 0
                        END),0) AS available_count
                 FROM warehouses w
                 LEFT JOIN devices d
                   ON d.currentWarehouseId=w.id AND d.tenant_id=w.tenant_id
                 WHERE w.tenant_id=?1
                   AND (?2 IS NULL OR w.id=?2)
                 GROUP BY w.id,w.name
                 HAVING COALESCE(SUM(CASE
                            WHEN d.rentalStatus IN ('available','已入库') THEN 1
                            ELSE 0
                        END),0) < ?3
                 ORDER BY w.name ASC",
            )?;
            statement
                .query_map(params![tenant_id, warehouse_id, threshold], |row| {
                    Ok(WarehouseLowStockProjection {
                        warehouse_id: row.get(0)?,
                        warehouse_name: row.get(1)?,
                        available_count: row.get(2)?,
                    })
                })?
                .collect::<Result<Vec<_>, _>>()
        })
    }

    pub(in crate::repositories) fn advanced_capacity_stats(
        &self,
    ) -> Result<Vec<WarehouseAdvancedCapacityProjection>, RepositoryError> {
        let tenant_id = self.tenant_id();
        self.session.read(move |connection| {
            let mut statement = connection.prepare(
                "SELECT w.id,w.name,w.capacity,
                        COUNT(d.id) AS total_devices,
                        COALESCE(SUM(CASE
                            WHEN d.rentalStatus IN ('available','已入库') THEN 1
                            ELSE 0
                        END),0) AS available_devices,
                        COALESCE(SUM(CASE
                            WHEN d.rentalStatus IN ('rented','租赁中') THEN 1
                            ELSE 0
                        END),0) AS rented_devices
                 FROM warehouses w
                 LEFT JOIN devices d
                   ON d.currentWarehouseId=w.id AND d.tenant_id=w.tenant_id
                 WHERE w.tenant_id=?1
                 GROUP BY w.id,w.name,w.capacity
                 ORDER BY total_devices DESC,w.name ASC",
            )?;
            statement
                .query_map([tenant_id], |row| {
                    Ok(WarehouseAdvancedCapacityProjection {
                        warehouse_id: row.get(0)?,
                        warehouse_name: row.get(1)?,
                        capacity: row.get(2)?,
                        total_devices: row.get(3)?,
                        available_devices: row.get(4)?,
                        rented_devices: row.get(5)?,
                    })
                })?
                .collect::<Result<Vec<_>, _>>()
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

        self.session.write_immediate(move |transaction| {
            let device_in_source: i64 = transaction
                .query_row(
                    "SELECT EXISTS(
                        SELECT 1
                        FROM devices d
                        JOIN warehouses w
                          ON w.id=d.currentWarehouseId AND w.tenant_id=d.tenant_id
                        WHERE d.tenant_id=?1 AND d.serialNo=?2
                          AND d.currentWarehouseId=?3
                    )",
                    params![tenant_id, serial_no, from_warehouse_id],
                    |row| row.get(0),
                )
                .map_err(sqlite_repository_error)?;
            if device_in_source == 0 {
                return Ok(WarehouseDeviceMoveOutcome::DeviceNotInSource);
            }

            let target_exists: i64 = transaction
                .query_row(
                    "SELECT EXISTS(
                        SELECT 1 FROM warehouses
                        WHERE tenant_id=?1 AND id=?2
                    )",
                    params![tenant_id, to_warehouse_id],
                    |row| row.get(0),
                )
                .map_err(sqlite_repository_error)?;
            if target_exists == 0 {
                return Ok(WarehouseDeviceMoveOutcome::TargetWarehouseNotFound);
            }

            let changed = transaction
                .execute(
                    "UPDATE devices
                     SET currentWarehouseId=?1
                     WHERE tenant_id=?2 AND serialNo=?3 AND currentWarehouseId=?4",
                    params![to_warehouse_id, tenant_id, serial_no, from_warehouse_id],
                )
                .map_err(sqlite_repository_error)?;
            if changed == 1 {
                Ok(WarehouseDeviceMoveOutcome::Moved)
            } else {
                Ok(WarehouseDeviceMoveOutcome::DeviceNotInSource)
            }
        })
    }

    pub(in crate::repositories) fn routing_rules_for_province(
        &self,
        province: &str,
    ) -> Result<Vec<WarehouseRoutingRuleProjection>, RepositoryError> {
        let tenant_id = self.tenant_id();
        let province = province.to_owned();
        self.session.read(move |connection| {
            let mut statement = connection.prepare(
                "SELECT r.warehouseId,w.name,w.type,r.shippingDays,r.returnDays
                 FROM warehouse_region_rules r
                 JOIN warehouses w ON w.id=r.warehouseId
                 WHERE w.tenant_id=?1 AND r.province=?2 AND w.enabled=1
                 ORDER BY r.shippingDays ASC,r.returnDays ASC,r.warehouseId ASC",
            )?;
            statement
                .query_map(params![tenant_id, province], map_routing_rule)?
                .collect::<Result<Vec<_>, _>>()
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
            .read(move |connection| {
                let mut statement = connection.prepare(
                    "SELECT r.id,r.warehouseId,r.province,r.shippingDays,r.returnDays,
                            r.isPrimary,r.createdAt,r.updatedAt
                     FROM warehouse_region_rules r
                     JOIN warehouses w ON w.id=r.warehouseId
                     WHERE r.warehouseId=?1 AND w.tenant_id=?2
                     ORDER BY r.province ASC",
                )?;
                statement
                    .query_map(params![id, tenant_id], map_region_rule)?
                    .collect::<Result<Vec<_>, _>>()
            })
            .map_err(Into::into)
    }

    pub(in crate::repositories) fn upsert_region_rule(
        &self,
        warehouse_id: &str,
        input: &UpsertWarehouseRegionRule,
    ) -> Result<Vec<WarehouseRegionRuleProjection>, WarehouseMutationError> {
        self.ensure_exists(warehouse_id)?;
        let tenant_id = self.tenant_id();
        let warehouse_id_owned = warehouse_id.to_owned();
        let input = input.clone();
        self.session.write(move |tx| {
            tx.execute(
                "INSERT INTO warehouse_region_rules
                 (id,warehouseId,province,shippingDays,returnDays,isPrimary,createdAt,updatedAt)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?7)
                 ON CONFLICT(warehouseId,province) DO UPDATE SET
                    shippingDays=excluded.shippingDays,
                    returnDays=excluded.returnDays,
                    isPrimary=excluded.isPrimary,
                    updatedAt=excluded.updatedAt
                 WHERE EXISTS (
                    SELECT 1 FROM warehouses w
                    WHERE w.id=excluded.warehouseId AND w.tenant_id=?8
                 )",
                params![
                    input.id,
                    warehouse_id_owned,
                    input.province,
                    input.shipping_days,
                    input.return_days,
                    if input.is_primary { 1 } else { 0 },
                    input.now,
                    tenant_id,
                ],
            )?;
            Ok(())
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
        let changed = self.session.write(move |tx| {
            tx.execute(
                "DELETE FROM warehouse_region_rules
                 WHERE warehouseId=?1 AND province=?2
                   AND EXISTS (
                     SELECT 1 FROM warehouses w
                     WHERE w.id=warehouse_region_rules.warehouseId AND w.tenant_id=?3
                   )",
                params![warehouse_id, province, tenant_id],
            )
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
        self.session.read(move |connection| match exclude_id {
            Some(exclude_id) => connection.query_row(
                "SELECT EXISTS(
                    SELECT 1 FROM warehouses
                    WHERE tenant_id=?1 AND name=?2 AND id<>?3
                 )",
                params![tenant_id, name, exclude_id],
                |row| row.get(0),
            ),
            None => connection.query_row(
                "SELECT EXISTS(
                    SELECT 1 FROM warehouses
                    WHERE tenant_id=?1 AND name=?2
                 )",
                params![tenant_id, name],
                |row| row.get(0),
            ),
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
        || error
            .to_string()
            .contains("warehouses.tenant_id, warehouses.name")
    {
        WarehouseMutationError::DuplicateName
    } else {
        WarehouseMutationError::Storage(error)
    }
}

fn sqlite_repository_error(error: rusqlite::Error) -> RepositoryError {
    RepositoryError::Sqlite(error.to_string())
}

fn map_warehouse(row: &rusqlite::Row<'_>) -> rusqlite::Result<WarehouseProjection> {
    Ok(WarehouseProjection {
        id: row.get(0)?,
        name: row.get(1)?,
        wh_type: row.get(2)?,
        enabled: row.get::<_, i64>(3)? != 0,
        address: row.get::<_, Option<String>>(4)?.unwrap_or_default(),
        contact_name: row.get::<_, Option<String>>(5)?.unwrap_or_default(),
        contact_phone: row.get::<_, Option<String>>(6)?.unwrap_or_default(),
        notes: row.get::<_, Option<String>>(7)?.unwrap_or_default(),
        capacity: row.get::<_, Option<i32>>(8)?.unwrap_or(0),
        created_at: row.get(9)?,
        updated_at: row.get(10)?,
    })
}

fn map_warehouse_stats(row: &rusqlite::Row<'_>) -> rusqlite::Result<WarehouseStatsProjection> {
    let capacity = row.get::<_, Option<i32>>(8)?.unwrap_or(0);
    let total_devices: i64 = row.get(11)?;
    let utilization_percent = if capacity > 0 {
        ((total_devices as f64 / capacity as f64) * 100.0).round() as i64
    } else {
        0
    };
    Ok(WarehouseStatsProjection {
        id: row.get(0)?,
        name: row.get(1)?,
        wh_type: row.get(2)?,
        enabled: row.get::<_, i64>(3)? != 0,
        address: row.get::<_, Option<String>>(4)?.unwrap_or_default(),
        contact_name: row.get::<_, Option<String>>(5)?.unwrap_or_default(),
        contact_phone: row.get::<_, Option<String>>(6)?.unwrap_or_default(),
        notes: row.get::<_, Option<String>>(7)?.unwrap_or_default(),
        capacity,
        total_devices,
        available_devices: row.get(12)?,
        rented_devices: row.get(13)?,
        repairing_devices: row.get(14)?,
        utilization_percent,
        created_at: row.get(9)?,
        updated_at: row.get(10)?,
    })
}

fn map_warehouse_device_detail(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<WarehouseDeviceDetailProjection> {
    Ok(WarehouseDeviceDetailProjection {
        id: row.get(0)?,
        serial_no: row.get(1)?,
        rental_status: row.get(2)?,
        model_id: row.get::<_, Option<String>>(3)?.unwrap_or_default(),
        notes: row.get::<_, Option<String>>(4)?.unwrap_or_default(),
        current_warehouse_id: row.get::<_, Option<String>>(5)?.unwrap_or_default(),
        expected_warehouse_id: row.get::<_, Option<String>>(6)?.unwrap_or_default(),
        expected_available_date: row.get::<_, Option<String>>(7)?.unwrap_or_default(),
        created_at: row.get(8)?,
    })
}

fn map_routing_rule(row: &rusqlite::Row<'_>) -> rusqlite::Result<WarehouseRoutingRuleProjection> {
    Ok(WarehouseRoutingRuleProjection {
        warehouse_id: row.get(0)?,
        warehouse_name: row.get(1)?,
        warehouse_type: row.get(2)?,
        shipping_days: row.get(3)?,
        return_days: row.get(4)?,
    })
}

fn map_region_rule(row: &rusqlite::Row<'_>) -> rusqlite::Result<WarehouseRegionRuleProjection> {
    Ok(WarehouseRegionRuleProjection {
        id: row.get(0)?,
        warehouse_id: row.get(1)?,
        province: row.get(2)?,
        shipping_days: row.get(3)?,
        return_days: row.get(4)?,
        is_primary: row.get::<_, i64>(5)? != 0,
        created_at: row.get(6)?,
        updated_at: row.get(7)?,
    })
}
