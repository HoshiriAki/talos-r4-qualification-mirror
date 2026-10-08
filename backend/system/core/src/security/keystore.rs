use secrecy::Secret;

/// 密钥管理 trait — 禁止硬编码密钥
pub trait KeyStore: Send + Sync {
    fn get_key(&self, key_id: &str) -> Result<Secret<Vec<u8>>, String>;
    fn rotate_key(&self, key_id: &str, new_key: Secret<Vec<u8>>) -> Result<(), String>;
    fn revoke_key(&self, key_id: &str) -> Result<(), String>;
}
