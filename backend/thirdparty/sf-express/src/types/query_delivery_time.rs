//! EXP_RECE_QUERY_DELIVERTM 输入/输出类型

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use system_core::{FieldError, Sanitize, Validate, ValidationResult};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct QueryDeliveryTimeInput {
    pub language: String,
    pub origin: AddressInfo,
    pub dest: AddressInfo,
    pub express_type_id: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AddressInfo {
    pub country: String,
    #[serde(default)]
    pub province: Option<String>,
    #[serde(default)]
    pub city: Option<String>,
    #[serde(default)]
    pub county: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DeliveryTimeOutput {
    pub deliver_time: String,
    #[serde(default)]
    pub collect_time: Option<String>,
}

impl Sanitize for QueryDeliveryTimeInput {
    fn sanitize(&mut self) {
        self.language = self.language.trim().to_string();
        self.origin.country = self.origin.country.trim().to_string();
        if let Some(ref mut s) = self.origin.province {
            *s = s.trim().to_string();
        }
        if let Some(ref mut s) = self.origin.city {
            *s = s.trim().to_string();
        }
        if let Some(ref mut s) = self.origin.county {
            *s = s.trim().to_string();
        }
        self.dest.country = self.dest.country.trim().to_string();
        if let Some(ref mut s) = self.dest.province {
            *s = s.trim().to_string();
        }
        if let Some(ref mut s) = self.dest.city {
            *s = s.trim().to_string();
        }
        if let Some(ref mut s) = self.dest.county {
            *s = s.trim().to_string();
        }
    }
}

impl Validate for QueryDeliveryTimeInput {
    fn validate(&self) -> ValidationResult {
        let mut errors = Vec::new();
        if self.origin.country.is_empty() {
            errors.push(FieldError {
                field: "origin.country".into(),
                code: "VAL_REQUIRED".into(),
                message: "出发地国家不能为空".into(),
            });
        }
        if self.dest.country.is_empty() {
            errors.push(FieldError {
                field: "dest.country".into(),
                code: "VAL_REQUIRED".into(),
                message: "目的地国家不能为空".into(),
            });
        }
        ValidationResult { errors }
    }
}
