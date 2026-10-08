use secrecy::Secret;

/// 密码哈希 trait — 系统注入，禁止 DIY 加密
pub trait PasswordHasher: Send + Sync {
    fn hash(&self, password: &Secret<String>) -> Result<String, String>;
    fn verify(&self, password: &Secret<String>, hash: &str) -> Result<bool, String>;
}
