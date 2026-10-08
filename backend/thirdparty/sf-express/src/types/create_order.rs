//! EXP_RECE_CREATE_ORDER 输入/输出类型

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use system_core::{FieldError, Sanitize, Validate, ValidationResult};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateOrderInput {
    pub language: String,
    pub order_id: String,
    pub express_type_id: i32,
    pub is_gen_waybill_no: i32,
    pub is_unified_waybill_no: i32,
    #[serde(default)]
    pub pay_method: Option<i32>,
    #[serde(default)]
    pub is_docall: Option<i32>,
    #[serde(default)]
    pub send_start_tm: Option<String>,
    #[serde(default)]
    pub remark: Option<String>,
    #[serde(default)]
    pub monthly_card: Option<String>,
    pub cargo_details: Vec<CargoDetail>,
    pub contact_info_list: Vec<ContactInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ContactInfo {
    pub contact_type: i32,
    #[serde(default)]
    pub contact: Option<String>,
    #[serde(default)]
    pub mobile: Option<String>,
    pub country: String,
    #[serde(default)]
    pub province: Option<String>,
    #[serde(default)]
    pub city: Option<String>,
    #[serde(default)]
    pub county: Option<String>,
    pub address: String,
    #[serde(default)]
    pub company: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CargoDetail {
    pub name: String,
    #[serde(default)]
    pub count: Option<i32>,
    #[serde(default)]
    pub unit: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateOrderOutput {
    pub order_id: String,
    pub mail_no: String,
    #[serde(default)]
    pub waybill_no_list: Vec<String>,
    pub filter_result: i32,
    #[serde(default)]
    pub origin_code: Option<String>,
    #[serde(default)]
    pub dest_code: Option<String>,
}

impl Sanitize for CreateOrderInput {
    fn sanitize(&mut self) {
        self.language = self.language.trim().to_string();
        self.order_id = self.order_id.trim().to_string();
        if let Some(ref mut s) = self.send_start_tm {
            *s = s.trim().to_string();
        }
        if let Some(ref mut s) = self.remark {
            *s = s.trim().to_string();
        }
        if let Some(ref mut s) = self.monthly_card {
            *s = s.trim().to_string();
        }
        for c in &mut self.contact_info_list {
            if let Some(ref mut s) = c.contact {
                *s = s.trim().to_string();
            }
            if let Some(ref mut s) = c.mobile {
                *s = s.trim().to_string();
            }
            c.country = c.country.trim().to_string();
            if let Some(ref mut s) = c.province {
                *s = s.trim().to_string();
            }
            if let Some(ref mut s) = c.city {
                *s = s.trim().to_string();
            }
            if let Some(ref mut s) = c.county {
                *s = s.trim().to_string();
            }
            c.address = c.address.trim().to_string();
            if let Some(ref mut s) = c.company {
                *s = s.trim().to_string();
            }
        }
        for c in &mut self.cargo_details {
            c.name = c.name.trim().to_string();
            if let Some(ref mut s) = c.unit {
                *s = s.trim().to_string();
            }
        }
    }
}

impl Validate for CreateOrderInput {
    fn validate(&self) -> ValidationResult {
        let mut errors = Vec::new();
        if self.order_id.is_empty() {
            errors.push(FieldError {
                field: "orderId".into(),
                code: "VAL_REQUIRED".into(),
                message: "order_id 不能为空".into(),
            });
        }
        if self.language.is_empty() {
            errors.push(FieldError {
                field: "language".into(),
                code: "VAL_REQUIRED".into(),
                message: "language 不能为空".into(),
            });
        }
        if self.contact_info_list.is_empty() {
            errors.push(FieldError {
                field: "contactInfoList".into(),
                code: "VAL_REQUIRED".into(),
                message: "contact_info_list 至少需要一个联系人".into(),
            });
        }
        if self.cargo_details.is_empty() {
            errors.push(FieldError {
                field: "cargoDetails".into(),
                code: "VAL_REQUIRED".into(),
                message: "cargo_details 至少需要一个货物项".into(),
            });
        }
        for (i, c) in self.contact_info_list.iter().enumerate() {
            if c.address.is_empty() {
                errors.push(FieldError {
                    field: format!("contactInfoList[{}].address", i),
                    code: "VAL_REQUIRED".into(),
                    message: "地址不能为空".into(),
                });
            }
            if c.mobile.is_none() && c.contact.is_none() {
                errors.push(FieldError {
                    field: format!("contactInfoList[{}].mobile", i),
                    code: "VAL_REQUIRED".into(),
                    message: "手机或联系人至少填一个".into(),
                });
            }
        }
        ValidationResult { errors }
    }
}
