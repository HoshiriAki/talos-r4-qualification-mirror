use std::collections::HashMap;

use crate::repositories::pricing::{
    DynamicPriceRecord, ModelBasePriceRecord, PricingConfigRecord, SqlitePricingRepository,
};
use crate::repositories::{RepositoryError, ScopedRepositories};

#[cfg(feature = "postgres")]
use crate::repositories::pricing_postgres::PostgresPricingRepository;

pub struct ScopedPricingRepository<'a> {
    scoped: &'a ScopedRepositories,
}

impl<'a> ScopedPricingRepository<'a> {
    pub(in crate::repositories) fn new(scoped: &'a ScopedRepositories) -> Self {
        Self { scoped }
    }

    pub fn get_or_create_config(
        &self,
        default_weekday: f64,
        default_weekend: f64,
        default_fees_json: &str,
        now: &str,
    ) -> Result<PricingConfigRecord, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresPricingRepository::new(self.scoped.session()).get_or_create_config(
                default_weekday,
                default_weekend,
                default_fees_json,
                now,
            );
        }
        SqlitePricingRepository::new(self.scoped).get_or_create_config(
            default_weekday,
            default_weekend,
            default_fees_json,
            now,
        )
    }

    pub fn update_config(
        &self,
        base_weekday: f64,
        base_weekend: f64,
        holiday_rules_json: &str,
        updated_by: &str,
        default_fees_json: &str,
        now: &str,
    ) -> Result<PricingConfigRecord, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresPricingRepository::new(self.scoped.session()).update_config(
                base_weekday,
                base_weekend,
                holiday_rules_json,
                updated_by,
                default_fees_json,
                now,
            );
        }
        SqlitePricingRepository::new(self.scoped).update_config(
            base_weekday,
            base_weekend,
            holiday_rules_json,
            updated_by,
            default_fees_json,
            now,
        )
    }

    pub fn save_config(
        &self,
        base_weekday: f64,
        base_weekend: f64,
        holiday_rules_json: &str,
        receive_shipping_fees_json: &str,
        dynamic_prices: &HashMap<String, f64>,
        updated_by: &str,
        now: &str,
    ) -> Result<PricingConfigRecord, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresPricingRepository::new(self.scoped.session()).save_config(
                base_weekday,
                base_weekend,
                holiday_rules_json,
                receive_shipping_fees_json,
                dynamic_prices,
                updated_by,
                now,
            );
        }
        SqlitePricingRepository::new(self.scoped).save_config(
            base_weekday,
            base_weekend,
            holiday_rules_json,
            receive_shipping_fees_json,
            dynamic_prices,
            updated_by,
            now,
        )
    }

    pub fn upsert_dynamic_price(
        &self,
        date_key: &str,
        price: f64,
        updated_by: &str,
        now: &str,
    ) -> Result<(), RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresPricingRepository::new(self.scoped.session())
                .upsert_dynamic_price(date_key, price, updated_by, now);
        }
        SqlitePricingRepository::new(self.scoped)
            .upsert_dynamic_price(date_key, price, updated_by, now)
    }

    pub fn delete_dynamic_price(&self, date_key: &str) -> Result<(), RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresPricingRepository::new(self.scoped.session())
                .delete_dynamic_price(date_key);
        }
        SqlitePricingRepository::new(self.scoped).delete_dynamic_price(date_key)
    }

    pub fn list_dynamic_prices(&self) -> Result<Vec<DynamicPriceRecord>, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresPricingRepository::new(self.scoped.session()).list_dynamic_prices();
        }
        SqlitePricingRepository::new(self.scoped).list_dynamic_prices()
    }

    pub fn get_model_base_price(
        &self,
        model_id: &str,
    ) -> Result<Option<ModelBasePriceRecord>, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresPricingRepository::new(self.scoped.session())
                .get_model_base_price(model_id);
        }
        SqlitePricingRepository::new(self.scoped).get_model_base_price(model_id)
    }
}
