/// 对称加密 trait — AES-256-GCM-SIV + AAD
pub trait SymmetricEncryptor: Send + Sync {
    fn encrypt(&self, plaintext: &[u8], aad: &[u8]) -> Result<Vec<u8>, String>;
    fn decrypt(&self, ciphertext: &[u8], aad: &[u8]) -> Result<Vec<u8>, String>;
    /// ML-KEM 密钥封装（后量子）
    fn encapsulate(&self, public_key: &[u8]) -> Result<(Vec<u8>, Vec<u8>), String>;
}

/// 通用加密 trait — 非对称 + 对称
pub trait Encryptor: Send + Sync {
    fn encrypt(&self, plaintext: &[u8]) -> Result<Vec<u8>, String>;
    fn decrypt(&self, ciphertext: &[u8]) -> Result<Vec<u8>, String>;
}
