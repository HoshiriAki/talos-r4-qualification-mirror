//! SfClient — SF API HTTP 调用。
//!
//! 流程: 签名 → form-encode → HTTP POST → 解析响应
//! 纯函数设计 — 不持有状态，所有依赖通过参数传入。

use serde_json::Value;
use system_core::transport::http_client::{HttpClient, HttpRequest};

use crate::err_payload;
use crate::signer::compute_msg_digest;

pub struct SfClient;

impl SfClient {
    /// 调用 SF API：签名 → 表单编码 → HTTP POST → 解析响应
    ///
    /// - `http`: HTTP 客户端 trait 对象（非 reqwest 直接依赖）
    /// - `service_code`: SF API 服务代码（如 "EXP_RECE_CREATE_ORDER"）
    /// - `msg_data`: 业务数据 JSON
    ///
    /// 返回: SF 统一响应 `{ apiResultCode, apiErrorMsg, apiResultData }`
    pub fn call(
        http: &dyn HttpClient,
        partner_id: &str,
        check_word: &str,
        base_url: &str,
        service_code: &str,
        msg_data: &Value,
    ) -> Result<Value, String> {
        let timestamp = chrono::Utc::now().timestamp_millis();
        let msg_data_str = serde_json::to_string(msg_data).map_err(|e| {
            err_payload("SF_JSON_SERIALIZE_ERROR", &format!("JSON序列化失败: {}", e))
        })?;

        let request_id = uuid::Uuid::new_v4().to_string().replace('-', "");
        let msg_digest = compute_msg_digest(&msg_data_str, timestamp, check_word);

        // 构建 form-encoded body
        let form_params: [(&str, String); 6] = [
            ("partnerID", partner_id.to_string()),
            ("requestID", request_id),
            ("serviceCode", service_code.to_string()),
            ("timestamp", timestamp.to_string()),
            ("msgDigest", msg_digest),
            ("msgData", msg_data_str),
        ];
        let body = form_params
            .iter()
            .map(|(k, v)| format!("{}={}", urlencoding::encode(k), urlencoding::encode(v)))
            .collect::<Vec<_>>()
            .join("&");

        let request = HttpRequest::post(base_url, body.into_bytes())
            .with_header(
                "Content-Type",
                "application/x-www-form-urlencoded; charset=UTF-8",
            )
            .with_timeout(15000);

        let response = http
            .send(request)
            .map_err(|e| err_payload("SF_NETWORK_ERROR", &format!("网络请求失败: {}", e)))?;

        if !response.is_success() {
            return Err(err_payload(
                "SF_HTTP_ERROR",
                &format!("HTTP {}", response.status),
            ));
        }

        let result: Value = serde_json::from_slice(&response.body)
            .map_err(|e| err_payload("SF_RESPONSE_PARSE_ERROR", &format!("响应解析失败: {}", e)))?;

        // SF API 级错误检查
        if let Some(code) = result.get("apiResultCode").and_then(|v| v.as_str())
            && code != "A1000"
        {
            let msg = result
                .get("apiErrorMsg")
                .and_then(|v| v.as_str())
                .unwrap_or("未知错误");
            return Err(err_payload("SF_API_ERROR", &format!("{}: {}", code, msg)));
        }

        Ok(result)
    }
}
