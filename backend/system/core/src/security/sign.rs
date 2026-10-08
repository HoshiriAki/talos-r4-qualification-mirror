/// 签名 trait — ML-DSA-87 默认
pub trait Signer: Send + Sync {
    fn sign(&self, message: &[u8]) -> Result<Vec<u8>, String>;
    fn verify(&self, message: &[u8], signature: &[u8]) -> Result<bool, String>;
}
