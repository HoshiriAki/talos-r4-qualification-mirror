//! EXP_RECE_UPDATE_ORDER 输入/输出类型

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use system_core::{FieldError, Sanitize, Validate, ValidationResult};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CancelOrderInput {
    pub language: String,
    pub order_id: String,
    pub deal_type: i32,
    #[serde(default)]
    pub remark: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CancelOrderOutput {
    pub order_id: String,
    pub mail_no: String,
    pub success: bool,
}

impl Sanitize for CancelOrderInput {
    fn sanitize(&mut self) {
        self.language = self.language.trim().to_string();
        self.order_id = self.order_id.trim().to_string();
        if let Some(ref mut s) = self.remark {
            *s = s.trim().to_string();
        }
    }
}

impl Validate for CancelOrderInput {
    fn validate(&self) -> ValidationResult {
        let mut errors = Vec::new();
        if self.order_id.is_empty() {
            errors.push(FieldError {
                field: "orderId".into(),
                code: "VAL_REQUIRED".into(),
                message: "order_id 不能为空".into(),
            });
        }
        ValidationResult { errors }
    }
}
