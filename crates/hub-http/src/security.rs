use argon2::{
    Argon2,
    password_hash::{PasswordHasher as _, PasswordVerifier as _, phc::PasswordHash as EncodedHash},
};
use pingushka_application::ports::{PasswordHasher, SecurityError, TokenDigester, TokenGenerator};
use pingushka_domain::{identity::TokenDigest, user::PasswordHash};
use rand::{TryRng, rngs::SysRng};
use sha2::{Digest, Sha256};

pub struct Argon2PasswordHasher;

impl PasswordHasher for Argon2PasswordHasher {
    fn hash_password(&self, password: &str) -> Result<PasswordHash, SecurityError> {
        let encoded = Argon2::default()
            .hash_password(password.as_bytes())
            .map_err(|_| SecurityError::HashingFailed)?
            .to_string();
        PasswordHash::new(encoded).map_err(|_| SecurityError::HashingFailed)
    }

    fn verify_password(&self, password: &str, hash: &PasswordHash) -> Result<bool, SecurityError> {
        let parsed =
            EncodedHash::new(hash.as_encoded()).map_err(|_| SecurityError::InvalidPasswordHash)?;
        Ok(Argon2::default()
            .verify_password(password.as_bytes(), &parsed)
            .is_ok())
    }
}

pub struct OsTokenGenerator;

impl TokenGenerator for OsTokenGenerator {
    fn generate(&self) -> Result<String, SecurityError> {
        let mut bytes = [0_u8; 32];
        SysRng
            .try_fill_bytes(&mut bytes)
            .map_err(|_| SecurityError::RandomnessUnavailable)?;
        Ok(hex_encode(&bytes))
    }
}

pub struct Sha256TokenDigester;

impl TokenDigester for Sha256TokenDigester {
    fn digest(&self, token: &str) -> TokenDigest {
        let digest = Sha256::digest(token.as_bytes());
        let mut bytes = [0_u8; 32];
        bytes.copy_from_slice(&digest);
        TokenDigest::new(bytes)
    }
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        encoded.push(char::from(HEX[(byte >> 4) as usize]));
        encoded.push(char::from(HEX[(byte & 0x0f) as usize]));
    }
    encoded
}
