//! SfWebhookProcessor — 验签 + 解析 SF 回调

use crate::signer::compute_msg_digest;
use serde_json::Value;
use std::collections::HashMap;

pub struct SfWebhookProcessor {
    check_word: String,
}

impl SfWebhookProcessor {
    pub fn new(check_word: String) -> Self {
        Self { check_word }
    }

    /// 验证 SF 回调签名
    pub fn verify_signature(&self, form_body: &str) -> Result<bool, String> {
        let params = parse_form(form_body)?;
        let msg_data = params
            .get("msgData")
            .ok_or_else(|| "缺少 msgData 字段".to_string())?;
        let timestamp = params
            .get("timestamp")
            .ok_or_else(|| "缺少 timestamp 字段".to_string())?
            .parse::<i64>()
            .map_err(|e| format!("timestamp 解析失败: {}", e))?;
        let provided = params
            .get("msgDigest")
            .ok_or_else(|| "缺少 msgDigest 字段".to_string())?;
        let expected = compute_msg_digest(msg_data, timestamp, &self.check_word);
        Ok(expected == *provided)
    }

    /// 解析 SF 回调 msgData JSON
    pub fn parse_webhook(&self, form_body: &str) -> Result<Value, String> {
        let params = parse_form(form_body)?;
        let msg_data = params
            .get("msgData")
            .ok_or_else(|| "缺少 msgData 字段".to_string())?;
        serde_json::from_str(msg_data).map_err(|e| format!("msgData JSON 解析失败: {}", e))
    }
}

/// 解析 application/x-www-form-urlencoded 字符串
pub(crate) fn parse_form(body: &str) -> Result<HashMap<String, String>, String> {
    let mut map = HashMap::new();
    for pair in body.split('&') {
        if pair.is_empty() {
            continue;
        }
        let (k, v) = if let Some(pos) = pair.find('=') {
            (&pair[..pos], &pair[pos + 1..])
        } else {
            (pair, "")
        };
        let key = urlencoding::decode(k)
            .map_err(|e| format!("key 解码失败: {}", e))?
            .into_owned();
        let val = urlencoding::decode(v)
            .map_err(|e| format!("value 解码失败: {}", e))?
            .into_owned();
        map.insert(key, val);
    }
    Ok(map)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_form_single_pair() {
        let result = parse_form("partnerID=ABC123").unwrap();
        assert_eq!(result.get("partnerID").unwrap(), "ABC123");
    }

    #[test]
    fn parse_form_multiple_pairs() {
        let result = parse_form("partnerID=ABC&requestID=123&serviceCode=EXP").unwrap();
        assert_eq!(result.get("partnerID").unwrap(), "ABC");
        assert_eq!(result.get("requestID").unwrap(), "123");
        assert_eq!(result.get("serviceCode").unwrap(), "EXP");
    }

    #[test]
    fn parse_form_url_encoded_value() {
        let result = parse_form("msgData=%7B%22key%22%3A%22value%22%7D").unwrap();
        assert_eq!(result.get("msgData").unwrap(), "{\"key\":\"value\"}");
    }

    #[test]
    fn parse_form_empty_body() {
        let result = parse_form("").unwrap();
        assert!(result.is_empty());
    }
}
