#![cfg(feature = "postgres")]

use std::collections::HashMap;

use crate::repositories::RepositoryError;
use crate::repositories::pricing::{DynamicPriceRecord, ModelBasePriceRecord, PricingConfigRecord};
use crate::repositories::session::RepositorySession;

pub(in crate::repositories) struct PostgresPricingRepository<'a> {
    session: &'a RepositorySession,
}

impl<'a> PostgresPricingRepository<'a> {
    pub(in crate::repositories) fn new(session: &'a RepositorySession) -> Self {
        Self { session }
    }

    pub(in crate::repositories) fn get_or_create_config(
        &self,
        default_weekday: f64,
        default_weekend: f64,
        default_fees_json: &str,
        now: &str,
    ) -> Result<PricingConfigRecord, RepositoryError> {
        crate::repositories::pricing_postgres_mutation::get_or_create_config(
            self.session,
            default_weekday,
            default_weekend,
            default_fees_json,
            now,
        )
    }

    pub(in crate::repositories) fn update_config(
        &self,
        base_weekday: f64,
        base_weekend: f64,
        holiday_rules_json: &str,
        updated_by: &str,
        default_fees_json: &str,
        now: &str,
    ) -> Result<PricingConfigRecord, RepositoryError> {
        crate::repositories::pricing_postgres_mutation::update_config(
            self.session,
            base_weekday,
            base_weekend,
            holiday_rules_json,
            updated_by,
            default_fees_json,
            now,
        )
    }

    pub(in crate::repositories) fn save_config(
        &self,
        base_weekday: f64,
        base_weekend: f64,
        holiday_rules_json: &str,
        receive_shipping_fees_json: &str,
        dynamic_prices: &HashMap<String, f64>,
        updated_by: &str,
        now: &str,
    ) -> Result<PricingConfigRecord, RepositoryError> {
        crate::repositories::pricing_postgres_mutation::save_config(
            self.session,
            base_weekday,
            base_weekend,
            holiday_rules_json,
            receive_shipping_fees_json,
            dynamic_prices,
            updated_by,
            now,
        )
    }

    pub(in crate::repositories) fn upsert_dynamic_price(
        &self,
        date_key: &str,
        price: f64,
        updated_by: &str,
        now: &str,
    ) -> Result<(), RepositoryError> {
        crate::repositories::pricing_postgres_mutation::upsert_dynamic_price(
            self.session,
            date_key,
            price,
            updated_by,
            now,
        )
    }

    pub(in crate::repositories) fn delete_dynamic_price(
        &self,
        date_key: &str,
    ) -> Result<(), RepositoryError> {
        crate::repositories::pricing_postgres_mutation::delete_dynamic_price(self.session, date_key)
    }

    pub(in crate::repositories) fn list_dynamic_prices(
        &self,
    ) -> Result<Vec<DynamicPriceRecord>, RepositoryError> {
        crate::repositories::pricing_postgres_read::list_dynamic_prices(self.session)
    }

    pub(in crate::repositories) fn get_model_base_price(
        &self,
        model_id: &str,
    ) -> Result<Option<ModelBasePriceRecord>, RepositoryError> {
        crate::repositories::pricing_postgres_read::get_model_base_price(self.session, model_id)
    }
}
