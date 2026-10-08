//! MD5 签名算法 — 顺丰丰桥 API 标准
//!
//! 算法: URLEncode(msgData + timestamp + checkWord, UTF-8) → MD5 → Base64
//! 来源: 顺丰丰桥开发规范

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};

pub fn compute_msg_digest(msg_data: &str, timestamp: i64, check_word: &str) -> String {
    let raw = format!("{}{}{}", msg_data, timestamp, check_word);
    let encoded = urlencoding::encode(&raw);
    let digest = md5::compute(encoded.as_bytes());
    BASE64.encode(digest.as_ref())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn digest_is_deterministic() {
        let a = compute_msg_digest("test", 1234567890, "secret");
        let b = compute_msg_digest("test", 1234567890, "secret");
        assert_eq!(a, b, "相同输入应产生相同签名");
    }

    #[test]
    fn different_msg_data_produces_different_digest() {
        let a = compute_msg_digest("data1", 1234567890, "secret");
        let b = compute_msg_digest("data2", 1234567890, "secret");
        assert_ne!(a, b);
    }

    #[test]
    fn different_timestamp_produces_different_digest() {
        let a = compute_msg_digest("test", 1234567890, "secret");
        let b = compute_msg_digest("test", 1234567891, "secret");
        assert_ne!(a, b);
    }

    #[test]
    fn different_check_word_produces_different_digest() {
        let a = compute_msg_digest("test", 1234567890, "secret1");
        let b = compute_msg_digest("test", 1234567890, "secret2");
        assert_ne!(a, b);
    }

    #[test]
    fn digest_is_base64_string() {
        let result = compute_msg_digest("hello", 1, "key");
        // Base64 output: only A-Z, a-z, 0-9, +, /, = allowed
        for ch in result.chars() {
            assert!(
                ch.is_ascii_alphanumeric() || ch == '+' || ch == '/' || ch == '=',
                "无效的 Base64 字符: {}",
                ch
            );
        }
    }

    #[test]
    fn empty_inputs_work() {
        let result = compute_msg_digest("", 0, "");
        assert!(!result.is_empty());
    }
}
