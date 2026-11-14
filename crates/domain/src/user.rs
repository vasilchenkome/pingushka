use std::{error::Error, fmt};

use time::OffsetDateTime;

use crate::{
    Timestamp,
    identity::{TokenDigest, UserId},
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum UserError {
    InvalidUsername,
    EmptyPasswordHash,
    PasswordHashTooLong,
    SessionExpiryNotAfterCreation,
}

impl fmt::Display for UserError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::InvalidUsername => {
                "username must be 3 to 64 ASCII letters, digits, dots, underscores, or hyphens"
            }
            Self::EmptyPasswordHash => "password hash cannot be empty",
            Self::PasswordHashTooLong => "password hash is too long",
            Self::SessionExpiryNotAfterCreation => "session expiry must be after creation",
        };
        formatter.write_str(message)
    }
}

impl Error for UserError {}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct Username(String);

impl Username {
    pub fn new(value: impl AsRef<str>) -> Result<Self, UserError> {
        let value = value.as_ref().trim();
        let valid_length = (3..=64).contains(&value.len());
        let valid_characters = value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte));
        if !valid_length || !valid_characters {
            return Err(UserError::InvalidUsername);
        }
        Ok(Self(value.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PasswordHash(String);

impl PasswordHash {
    pub fn new(value: impl AsRef<str>) -> Result<Self, UserError> {
        let value = value.as_ref();
        if value.is_empty() {
            return Err(UserError::EmptyPasswordHash);
        }
        if value.len() > 512 {
            return Err(UserError::PasswordHashTooLong);
        }
        Ok(Self(value.to_owned()))
    }

    pub fn as_encoded(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct User {
    id: UserId,
    username: Username,
    password_hash: PasswordHash,
    created_at: Timestamp,
}

impl User {
    pub fn new(
        id: UserId,
        username: Username,
        password_hash: PasswordHash,
        created_at: Timestamp,
    ) -> Self {
        Self {
            id,
            username,
            password_hash,
            created_at,
        }
    }

    pub fn id(&self) -> UserId {
        self.id
    }

    pub fn username(&self) -> &Username {
        &self.username
    }

    pub fn password_hash(&self) -> &PasswordHash {
        &self.password_hash
    }

    pub const fn created_at(&self) -> Timestamp {
        self.created_at
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Session {
    user_id: UserId,
    token_digest: TokenDigest,
    created_at: OffsetDateTime,
    expires_at: OffsetDateTime,
}

impl Session {
    pub fn new(
        user_id: UserId,
        token_digest: TokenDigest,
        created_at: OffsetDateTime,
        expires_at: OffsetDateTime,
    ) -> Result<Self, UserError> {
        if expires_at <= created_at {
            return Err(UserError::SessionExpiryNotAfterCreation);
        }
        Ok(Self {
            user_id,
            token_digest,
            created_at,
            expires_at,
        })
    }

    pub fn user_id(&self) -> UserId {
        self.user_id
    }

    pub fn token_digest(&self) -> TokenDigest {
        self.token_digest
    }

    pub const fn created_at(&self) -> OffsetDateTime {
        self.created_at
    }

    pub const fn expires_at(&self) -> OffsetDateTime {
        self.expires_at
    }
}
