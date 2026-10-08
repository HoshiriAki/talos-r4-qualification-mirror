//! EXP_RECE_SEARCH_ROUTES 输入/输出类型

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use system_core::{FieldError, Sanitize, Validate, ValidationResult};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct QueryRoutesInput {
    pub language: String,
    pub tracking_type: i32,
    pub tracking_number: Vec<String>,
    #[serde(default)]
    pub method_type: Option<i32>,
    #[serde(default)]
    pub check_phone_no: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RouteResps {
    #[serde(default)]
    pub route_resps: Vec<RouteInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RouteInfo {
    pub mail_no: String,
    #[serde(default)]
    pub routes: Vec<RouteNode>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RouteNode {
    pub accept_time: String,
    pub accept_address: String,
    pub remark: String,
    pub op_code: String,
    pub first_status_code: String,
    pub first_status_name: String,
    pub secondary_status_code: String,
    pub secondary_status_name: String,
}

impl Sanitize for QueryRoutesInput {
    fn sanitize(&mut self) {
        self.language = self.language.trim().to_string();
        self.tracking_number = self
            .tracking_number
            .iter()
            .map(|s| s.trim().to_string())
            .collect();
        if let Some(ref mut s) = self.check_phone_no {
            *s = s.trim().to_string();
        }
    }
}

impl Validate for QueryRoutesInput {
    fn validate(&self) -> ValidationResult {
        let mut errors = Vec::new();
        if self.tracking_number.is_empty() {
            errors.push(FieldError {
                field: "trackingNumber".into(),
                code: "VAL_REQUIRED".into(),
                message: "tracking_number 不能为空".into(),
            });
        }
        if self.tracking_number.len() > 10 {
            errors.push(FieldError {
                field: "trackingNumber".into(),
                code: "VAL_TOO_MANY".into(),
                message: "最多查询10个运单".into(),
            });
        }
        ValidationResult { errors }
    }
}
