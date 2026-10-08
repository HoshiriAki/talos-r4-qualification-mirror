//! COM_RECE_CLOUD_PRINT_WAYBILLS 输入/输出类型

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use system_core::{FieldError, Sanitize, Validate, ValidationResult};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CloudPrintInput {
    pub template_code: String,
    pub version: String,
    pub sync: bool,
    pub file_type: String,
    pub documents: Vec<PrintDocument>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PrintDocument {
    pub master_waybill_no: String,
    #[serde(default)]
    pub branch_waybill_no: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CloudPrintOutput {
    #[serde(default)]
    pub files: Vec<PrintFile>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PrintFile {
    pub url: String,
    #[serde(default)]
    pub token: Option<String>,
    pub master_waybill_no: String,
}

impl Sanitize for CloudPrintInput {
    fn sanitize(&mut self) {
        self.template_code = self.template_code.trim().to_string();
        self.version = self.version.trim().to_string();
        self.file_type = self.file_type.trim().to_string();
        for d in &mut self.documents {
            d.master_waybill_no = d.master_waybill_no.trim().to_string();
            if let Some(ref mut s) = d.branch_waybill_no {
                *s = s.trim().to_string();
            }
        }
    }
}

impl Validate for CloudPrintInput {
    fn validate(&self) -> ValidationResult {
        let mut errors = Vec::new();
        if self.documents.is_empty() {
            errors.push(FieldError {
                field: "documents".into(),
                code: "VAL_REQUIRED".into(),
                message: "documents 不能为空".into(),
            });
        }
        if self.documents.len() > 20 {
            errors.push(FieldError {
                field: "documents".into(),
                code: "VAL_TOO_MANY".into(),
                message: "最多20个运单".into(),
            });
        }
        ValidationResult { errors }
    }
}
