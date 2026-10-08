//! Post-quantum cryptography traits — trait definitions only.
//! Implementations will be added during the PQC roadmap phase.
/// Key Encapsulation Mechanism (e.g., Kyber/ML-KEM)
pub trait KeyEncapsulation {
    type Error;

    /// Encapsulate: generate a shared secret + ciphertext from a public key.
    fn encapsulate(&self, public_key: &[u8]) -> Result<(Vec<u8>, Vec<u8>), Self::Error>;

    /// Decapsulate: recover the shared secret from a ciphertext + secret key.
    fn decapsulate(&self, secret_key: &[u8], ciphertext: &[u8]) -> Result<Vec<u8>, Self::Error>;
}

/// Digital signature scheme (e.g., Dilithium/ML-DSA)
pub trait Signer {
    type Error;

    /// Sign a message with the secret key.
    fn sign(&self, message: &[u8]) -> Result<Vec<u8>, Self::Error>;

    /// Verify a signature against a message and public key.
    fn verify(
        &self,
        public_key: &[u8],
        message: &[u8],
        signature: &[u8],
    ) -> Result<bool, Self::Error>;
}
