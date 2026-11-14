use pingushka_application::ports::{PasswordHasher, TokenDigester, TokenGenerator};
use pingushka_hub_http::security::{Argon2PasswordHasher, OsTokenGenerator, Sha256TokenDigester};

#[test]
fn password_hasher_uses_argon2id_and_verifies_credentials() {
    let hasher = Argon2PasswordHasher;
    let encoded = hasher
        .hash_password("correct horse battery staple")
        .unwrap();

    assert!(encoded.as_encoded().starts_with("$argon2id$"));
    assert!(
        hasher
            .verify_password("correct horse battery staple", &encoded)
            .unwrap()
    );
    assert!(!hasher.verify_password("wrong password", &encoded).unwrap());
}

#[test]
fn token_digester_returns_deterministic_fixed_size_digests() {
    let digester = Sha256TokenDigester;
    let first = digester.digest("enrollment-secret");
    let repeated = digester.digest("enrollment-secret");
    let other = digester.digest("different-secret");

    assert_eq!(first, repeated);
    assert_ne!(first, other);
    assert_eq!(first.as_bytes().len(), 32);
    let actual_hex = first
        .as_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    assert_eq!(
        actual_hex,
        "ee61664868488406136af84cb408e1860e17dc40f295b1fd5dd65c9552403acc"
    );
}

#[test]
fn os_token_generator_returns_unique_256_bit_hex_tokens() {
    let generator = OsTokenGenerator;
    let first = generator.generate().unwrap();
    let second = generator.generate().unwrap();

    assert_eq!(first.len(), 64);
    assert!(first.chars().all(|character| character.is_ascii_hexdigit()));
    assert_ne!(first, second);
}
