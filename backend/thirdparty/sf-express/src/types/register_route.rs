//! EXP_RECE_REGISTER_ROUTE 输入/输出类型

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use system_core::{FieldError, Sanitize, Validate, ValidationResult};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RegisterRouteInput {
    pub language: String,
    pub tracking_number: Vec<String>,
    pub tracking_type: i32,
    pub callback_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RegisterRouteOutput {
    pub success_count: i32,
    pub fail_count: i32,
    #[serde(default)]
    pub mail_no_list: Vec<String>,
}

impl Sanitize for RegisterRouteInput {
    fn sanitize(&mut self) {
        self.language = self.language.trim().to_string();
        self.callback_url = self.callback_url.trim().to_string();
        self.tracking_number = self
            .tracking_number
            .iter()
            .map(|s| s.trim().to_string())
            .collect();
    }
}

impl Validate for RegisterRouteInput {
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
                message: "tracking_number 最多10个".into(),
            });
        }
        if self.callback_url.is_empty() {
            errors.push(FieldError {
                field: "callbackUrl".into(),
                code: "VAL_REQUIRED".into(),
                message: "callback_url 不能为空".into(),
            });
        }
        ValidationResult { errors }
    }
}
