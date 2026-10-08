use crate::repositories::warehouse::{
    NewWarehouse, SqliteWarehouseRepository, UpsertWarehouseRegionRule,
    WarehouseAdvancedCapacityProjection, WarehouseDeviceMoveOutcome, WarehouseDevicePage,
    WarehouseDeviceProjection, WarehouseLowStockProjection, WarehouseMutationError, WarehousePatch,
    WarehouseProjection, WarehouseRegionRuleProjection, WarehouseRoutingRuleProjection,
    WarehouseStatsProjection,
};
use crate::repositories::{RepositoryError, ScopedRepositories};

#[cfg(feature = "postgres")]
use crate::repositories::warehouse_postgres::PostgresWarehouseRepository;

pub struct ScopedWarehouseRepository<'a> {
    scoped: &'a ScopedRepositories,
}

impl<'a> ScopedWarehouseRepository<'a> {
    pub(in crate::repositories) fn new(scoped: &'a ScopedRepositories) -> Self {
        Self { scoped }
    }

    pub fn list(&self) -> Result<Vec<WarehouseProjection>, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresWarehouseRepository::new(self.scoped.session()).list();
        }
        SqliteWarehouseRepository::new(self.scoped).list()
    }

    pub fn get(&self, id: &str) -> Result<Option<WarehouseProjection>, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresWarehouseRepository::new(self.scoped.session()).get(id);
        }
        SqliteWarehouseRepository::new(self.scoped).get(id)
    }

    pub fn find_by_name(&self, name: &str) -> Result<Option<WarehouseProjection>, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresWarehouseRepository::new(self.scoped.session()).find_by_name(name);
        }
        SqliteWarehouseRepository::new(self.scoped).find_by_name(name)
    }

    pub fn create(
        &self,
        input: &NewWarehouse,
    ) -> Result<WarehouseProjection, WarehouseMutationError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresWarehouseRepository::new(self.scoped.session()).create(input);
        }
        SqliteWarehouseRepository::new(self.scoped).create(input)
    }

    pub fn update(
        &self,
        id: &str,
        patch: &WarehousePatch,
    ) -> Result<WarehouseProjection, WarehouseMutationError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresWarehouseRepository::new(self.scoped.session()).update(id, patch);
        }
        SqliteWarehouseRepository::new(self.scoped).update(id, patch)
    }

    pub fn delete(&self, id: &str) -> Result<(), WarehouseMutationError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresWarehouseRepository::new(self.scoped.session()).delete(id);
        }
        SqliteWarehouseRepository::new(self.scoped).delete(id)
    }

    pub fn stats(&self) -> Result<Vec<WarehouseStatsProjection>, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresWarehouseRepository::new(self.scoped.session()).stats();
        }
        SqliteWarehouseRepository::new(self.scoped).stats()
    }

    pub fn devices(
        &self,
        id: &str,
        status: Option<&str>,
    ) -> Result<Vec<WarehouseDeviceProjection>, WarehouseMutationError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresWarehouseRepository::new(self.scoped.session()).devices(id, status);
        }
        SqliteWarehouseRepository::new(self.scoped).devices(id, status)
    }

    pub fn devices_paged(
        &self,
        id: &str,
        page: i64,
        page_size: i64,
        keyword: Option<&str>,
    ) -> Result<WarehouseDevicePage, WarehouseMutationError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresWarehouseRepository::new(self.scoped.session())
                .devices_paged(id, page, page_size, keyword);
        }
        SqliteWarehouseRepository::new(self.scoped).devices_paged(id, page, page_size, keyword)
    }

    pub fn advanced_low_stock(
        &self,
        warehouse_id: Option<&str>,
        threshold: i64,
    ) -> Result<Vec<WarehouseLowStockProjection>, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresWarehouseRepository::new(self.scoped.session())
                .advanced_low_stock(warehouse_id, threshold);
        }
        SqliteWarehouseRepository::new(self.scoped).advanced_low_stock(warehouse_id, threshold)
    }

    pub fn advanced_capacity_stats(
        &self,
    ) -> Result<Vec<WarehouseAdvancedCapacityProjection>, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresWarehouseRepository::new(self.scoped.session())
                .advanced_capacity_stats();
        }
        SqliteWarehouseRepository::new(self.scoped).advanced_capacity_stats()
    }

    pub fn move_device_between_warehouses(
        &self,
        serial_no: &str,
        from_warehouse_id: &str,
        to_warehouse_id: &str,
    ) -> Result<WarehouseDeviceMoveOutcome, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresWarehouseRepository::new(self.scoped.session())
                .move_device_between_warehouses(serial_no, from_warehouse_id, to_warehouse_id);
        }
        SqliteWarehouseRepository::new(self.scoped).move_device_between_warehouses(
            serial_no,
            from_warehouse_id,
            to_warehouse_id,
        )
    }

    pub fn routing_rules_for_province(
        &self,
        province: &str,
    ) -> Result<Vec<WarehouseRoutingRuleProjection>, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresWarehouseRepository::new(self.scoped.session())
                .routing_rules_for_province(province);
        }
        SqliteWarehouseRepository::new(self.scoped).routing_rules_for_province(province)
    }

    pub fn region_rules(
        &self,
        id: &str,
    ) -> Result<Vec<WarehouseRegionRuleProjection>, WarehouseMutationError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresWarehouseRepository::new(self.scoped.session()).region_rules(id);
        }
        SqliteWarehouseRepository::new(self.scoped).region_rules(id)
    }

    pub fn upsert_region_rule(
        &self,
        warehouse_id: &str,
        input: &UpsertWarehouseRegionRule,
    ) -> Result<Vec<WarehouseRegionRuleProjection>, WarehouseMutationError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresWarehouseRepository::new(self.scoped.session())
                .upsert_region_rule(warehouse_id, input);
        }
        SqliteWarehouseRepository::new(self.scoped).upsert_region_rule(warehouse_id, input)
    }

    pub fn delete_region_rule(
        &self,
        warehouse_id: &str,
        province: &str,
    ) -> Result<(), WarehouseMutationError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresWarehouseRepository::new(self.scoped.session())
                .delete_region_rule(warehouse_id, province);
        }
        SqliteWarehouseRepository::new(self.scoped).delete_region_rule(warehouse_id, province)
    }
}
