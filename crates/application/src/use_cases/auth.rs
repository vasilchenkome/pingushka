use std::{error::Error, fmt, sync::Arc, time::Duration};

use pingushka_domain::{
    Timestamp,
    identity::{TokenDigest, UserId},
    user::{Session, User, Username},
};

use crate::{
    error::PortError,
    ports::{
        Clock, PasswordHasher, SecurityError, SessionRepository, TokenDigester, TokenGenerator,
        UserRepository,
    },
};

const MIN_PASSWORD_LEN: usize = 12;
const MAX_PASSWORD_LEN: usize = 256;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SetupAdminError {
    InvalidUsername,
    InvalidPassword,
    AlreadyConfigured,
    Security(SecurityError),
    Repository(PortError),
}

impl fmt::Display for SetupAdminError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidUsername => formatter.write_str(
                "username must be 3 to 64 ASCII letters, digits, dots, underscores, or hyphens",
            ),
            Self::InvalidPassword => write!(
                formatter,
                "password must be {MIN_PASSWORD_LEN} to {MAX_PASSWORD_LEN} characters"
            ),
            Self::AlreadyConfigured => formatter.write_str("administrator already exists"),
            Self::Security(error) => write!(formatter, "{error}"),
            Self::Repository(error) => write!(formatter, "{error}"),
        }
    }
}

impl Error for SetupAdminError {}

pub struct SetupAdmin {
    users: Arc<dyn UserRepository>,
    hasher: Arc<dyn PasswordHasher>,
    clock: Arc<dyn Clock>,
}

impl SetupAdmin {
    pub fn new(
        users: Arc<dyn UserRepository>,
        hasher: Arc<dyn PasswordHasher>,
        clock: Arc<dyn Clock>,
    ) -> Self {
        Self {
            users,
            hasher,
            clock,
        }
    }

    pub async fn execute(&self, username: &str, password: &str) -> Result<User, SetupAdminError> {
        let username = Username::new(username).map_err(|_| SetupAdminError::InvalidUsername)?;
        validate_password(password)?;
        let password_hash = self
            .hasher
            .hash_password(password)
            .map_err(SetupAdminError::Security)?;
        let user = User::new(UserId::new(), username, password_hash, self.clock.now());
        if !self
            .users
            .save_if_first(&user)
            .await
            .map_err(SetupAdminError::Repository)?
        {
            return Err(SetupAdminError::AlreadyConfigured);
        }
        Ok(user)
    }
}

fn validate_password(password: &str) -> Result<(), SetupAdminError> {
    let length = password.chars().count();
    if !(MIN_PASSWORD_LEN..=MAX_PASSWORD_LEN).contains(&length) {
        return Err(SetupAdminError::InvalidPassword);
    }
    Ok(())
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LoginError {
    InvalidCredentials,
    Security(SecurityError),
    Repository(PortError),
}

impl fmt::Display for LoginError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidCredentials => formatter.write_str("invalid username or password"),
            Self::Security(error) => write!(formatter, "{error}"),
            Self::Repository(error) => write!(formatter, "{error}"),
        }
    }
}

impl Error for LoginError {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IssuedSession {
    token: String,
    session: Session,
    user: User,
}

impl IssuedSession {
    pub fn token(&self) -> &str {
        &self.token
    }

    pub fn session(&self) -> &Session {
        &self.session
    }

    pub fn user(&self) -> &User {
        &self.user
    }
}

pub struct Login {
    users: Arc<dyn UserRepository>,
    sessions: Arc<dyn SessionRepository>,
    hasher: Arc<dyn PasswordHasher>,
    digester: Arc<dyn TokenDigester>,
    generator: Arc<dyn TokenGenerator>,
    clock: Arc<dyn Clock>,
    session_ttl: Duration,
}

impl Login {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        users: Arc<dyn UserRepository>,
        sessions: Arc<dyn SessionRepository>,
        hasher: Arc<dyn PasswordHasher>,
        digester: Arc<dyn TokenDigester>,
        generator: Arc<dyn TokenGenerator>,
        clock: Arc<dyn Clock>,
        session_ttl: Duration,
    ) -> Self {
        Self {
            users,
            sessions,
            hasher,
            digester,
            generator,
            clock,
            session_ttl,
        }
    }

    pub async fn execute(
        &self,
        username: &str,
        password: &str,
    ) -> Result<IssuedSession, LoginError> {
        let username = Username::new(username).map_err(|_| LoginError::InvalidCredentials)?;
        if password.chars().count() > MAX_PASSWORD_LEN {
            return Err(LoginError::InvalidCredentials);
        }
        let user = self
            .users
            .find_by_username(&username)
            .await
            .map_err(LoginError::Repository)?
            .ok_or(LoginError::InvalidCredentials)?;
        let matches = self
            .hasher
            .verify_password(password, user.password_hash())
            .map_err(LoginError::Security)?;
        if !matches {
            return Err(LoginError::InvalidCredentials);
        }
        let token = self.generator.generate().map_err(LoginError::Security)?;
        let digest = self.digester.digest(&token);
        let now = self.clock.now();
        let session = Session::new(user.id(), digest, now, now + self.session_ttl)
            .map_err(|_| LoginError::Repository(PortError::InvalidData))?;
        self.sessions
            .save(&session)
            .await
            .map_err(LoginError::Repository)?;
        Ok(IssuedSession {
            token,
            session,
            user,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuthenticatedSession {
    session: Session,
    user: User,
}

impl AuthenticatedSession {
    pub fn session(&self) -> &Session {
        &self.session
    }

    pub fn user(&self) -> &User {
        &self.user
    }
}

pub struct AuthenticateSession {
    sessions: Arc<dyn SessionRepository>,
    users: Arc<dyn UserRepository>,
    digester: Arc<dyn TokenDigester>,
    clock: Arc<dyn Clock>,
}

impl AuthenticateSession {
    pub fn new(
        sessions: Arc<dyn SessionRepository>,
        users: Arc<dyn UserRepository>,
        digester: Arc<dyn TokenDigester>,
        clock: Arc<dyn Clock>,
    ) -> Self {
        Self {
            sessions,
            users,
            digester,
            clock,
        }
    }

    pub async fn execute(&self, token: &str) -> Result<Option<AuthenticatedSession>, PortError> {
        let digest: TokenDigest = self.digester.digest(token);
        let Some(session) = self
            .sessions
            .find_active_by_digest(digest, self.clock.now())
            .await?
        else {
            return Ok(None);
        };
        let Some(user) = self.users.find(session.user_id()).await? else {
            return Ok(None);
        };
        Ok(Some(AuthenticatedSession { session, user }))
    }
}

pub struct Logout {
    sessions: Arc<dyn SessionRepository>,
    digester: Arc<dyn TokenDigester>,
}

impl Logout {
    pub fn new(sessions: Arc<dyn SessionRepository>, digester: Arc<dyn TokenDigester>) -> Self {
        Self { sessions, digester }
    }

    pub async fn execute(&self, token: &str) -> Result<bool, PortError> {
        let digest = self.digester.digest(token);
        self.sessions.revoke(digest).await
    }
}

pub fn session_expiry(now: Timestamp, ttl: Duration) -> Timestamp {
    now + ttl
}
