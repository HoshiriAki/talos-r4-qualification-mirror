use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SfApiError {
    pub api_result_code: String,
    pub api_error_msg: String,
    pub api_error_code: Option<String>,
}
