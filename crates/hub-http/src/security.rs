use argon2::{
    Argon2,
    password_hash::{PasswordHasher as _, PasswordVerifier as _, phc::PasswordHash as EncodedHash},
};
use chacha20poly1305::{
    KeyInit, XChaCha20Poly1305, XNonce,
    aead::{Aead, Payload},
};
use pingushka_application::alerts::{AlertServiceError, SecretCipher};
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

pub struct AlertSecretCipher {
    cipher: XChaCha20Poly1305,
}

impl AlertSecretCipher {
    pub fn new(key: [u8; 32]) -> Self {
        Self {
            cipher: XChaCha20Poly1305::new((&key).into()),
        }
    }

    pub fn from_configured_key(value: &str) -> Result<Self, String> {
        let value = value.trim();
        let bytes = if value.len() == 64 && value.chars().all(|ch| ch.is_ascii_hexdigit()) {
            (0..value.len())
                .step_by(2)
                .map(|index| u8::from_str_radix(&value[index..index + 2], 16))
                .collect::<Result<Vec<_>, _>>()
                .map_err(|_| "ALERT_SECRET_KEY is not valid hex".to_owned())?
        } else {
            use base64::Engine as _;
            base64::engine::general_purpose::STANDARD
                .decode(value)
                .map_err(|_| "ALERT_SECRET_KEY must be 32 bytes hex or base64".to_owned())?
        };
        let key: [u8; 32] = bytes
            .try_into()
            .map_err(|_| "ALERT_SECRET_KEY must decode to 32 bytes".to_owned())?;
        Ok(Self::new(key))
    }
}

impl SecretCipher for AlertSecretCipher {
    fn encrypt(&self, plaintext: &[u8]) -> Result<Vec<u8>, AlertServiceError> {
        let mut nonce = [0_u8; 24];
        SysRng
            .try_fill_bytes(&mut nonce)
            .map_err(|error| AlertServiceError::Cipher(error.to_string()))?;
        let ciphertext = self
            .cipher
            .encrypt(
                &XNonce::from(nonce),
                Payload {
                    msg: plaintext,
                    aad: b"pingushka-alert-channel-v1",
                },
            )
            .map_err(|_| AlertServiceError::Cipher("encryption failed".to_owned()))?;
        let mut output = Vec::with_capacity(nonce.len() + ciphertext.len());
        output.extend_from_slice(&nonce);
        output.extend_from_slice(&ciphertext);
        Ok(output)
    }

    fn decrypt(&self, ciphertext: &[u8]) -> Result<Vec<u8>, AlertServiceError> {
        if ciphertext.len() <= 24 {
            return Err(AlertServiceError::Cipher(
                "ciphertext is too short".to_owned(),
            ));
        }
        let (nonce, encrypted) = ciphertext.split_at(24);
        let nonce: [u8; 24] = nonce
            .try_into()
            .map_err(|_| AlertServiceError::Cipher("invalid nonce".to_owned()))?;
        self.cipher
            .decrypt(
                &XNonce::from(nonce),
                Payload {
                    msg: encrypted,
                    aad: b"pingushka-alert-channel-v1",
                },
            )
            .map_err(|_| AlertServiceError::Cipher("decryption failed".to_owned()))
    }
}
