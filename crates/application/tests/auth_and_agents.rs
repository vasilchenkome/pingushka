use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use async_trait::async_trait;
use pingushka_application::{
    error::PortError,
    ports::{
        AgentControlGateway, AgentEnrollmentRepository, AgentRepository, Clock, PasswordHasher,
        SecurityError, SessionRepository, TokenDigester, TokenGenerator, UserRepository,
    },
    use_cases::{
        AuthenticateSession, EnrollAgent, EnrollAgentError, Login, LoginError, RegisterAgent,
        RotateAgentToken, SetupAdmin, SetupAdminError,
    },
};
use pingushka_domain::{
    Timestamp,
    agent::Agent,
    enrollment::AgentEnrollment,
    identity::{AgentId, TargetId, TokenDigest, UserId},
    user::{PasswordHash, Session, User, Username},
};
use sha2::{Digest, Sha256};
use time::OffsetDateTime;

struct FakeClock {
    now: Mutex<OffsetDateTime>,
}

impl FakeClock {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            now: Mutex::new(
                OffsetDateTime::from_unix_timestamp(1_700_000_000).expect("valid timestamp"),
            ),
        })
    }

    fn advance(&self, duration: Duration) {
        let mut now = self.now.lock().expect("clock lock");
        *now += duration;
    }
}

impl Clock for FakeClock {
    fn now(&self) -> Timestamp {
        *self.now.lock().expect("clock lock")
    }
}

struct FakeHasher;

impl PasswordHasher for FakeHasher {
    fn hash_password(&self, password: &str) -> Result<PasswordHash, SecurityError> {
        PasswordHash::new(format!("hashed:{password}")).map_err(|_| SecurityError::HashingFailed)
    }

    fn verify_password(&self, password: &str, hash: &PasswordHash) -> Result<bool, SecurityError> {
        Ok(hash.as_encoded() == format!("hashed:{password}"))
    }
}

struct FakeTokens {
    counter: AtomicU64,
}

impl FakeTokens {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            counter: AtomicU64::new(0),
        })
    }
}

impl TokenGenerator for FakeTokens {
    fn generate(&self) -> Result<String, SecurityError> {
        let value = self.counter.fetch_add(1, Ordering::SeqCst) + 1;
        Ok(format!("token-{value}"))
    }
}

struct FakeDigester;

impl TokenDigester for FakeDigester {
    fn digest(&self, token: &str) -> TokenDigest {
        let digest = Sha256::digest(token.as_bytes());
        let mut bytes = [0_u8; 32];
        bytes.copy_from_slice(&digest);
        TokenDigest::new(bytes)
    }
}

#[derive(Default)]
struct FakeUsers {
    users: Mutex<Vec<User>>,
}

#[async_trait]
impl UserRepository for FakeUsers {
    async fn save(&self, user: &User) -> Result<(), PortError> {
        let mut users = self.users.lock().expect("users lock");
        users.retain(|existing| existing.id() != user.id());
        users.push(user.clone());
        Ok(())
    }

    async fn save_if_first(&self, user: &User) -> Result<bool, PortError> {
        let mut users = self.users.lock().expect("users lock");
        if !users.is_empty() {
            return Ok(false);
        }
        users.push(user.clone());
        Ok(true)
    }

    async fn find(&self, id: UserId) -> Result<Option<User>, PortError> {
        Ok(self
            .users
            .lock()
            .expect("users lock")
            .iter()
            .find(|user| user.id() == id)
            .cloned())
    }

    async fn find_by_username(&self, username: &Username) -> Result<Option<User>, PortError> {
        Ok(self
            .users
            .lock()
            .expect("users lock")
            .iter()
            .find(|user| user.username() == username)
            .cloned())
    }

    async fn count(&self) -> Result<u64, PortError> {
        Ok(self.users.lock().expect("users lock").len() as u64)
    }
}

#[derive(Default)]
struct FakeSessions {
    sessions: Mutex<Vec<Session>>,
}

#[async_trait]
impl SessionRepository for FakeSessions {
    async fn save(&self, session: &Session) -> Result<(), PortError> {
        self.sessions
            .lock()
            .expect("sessions lock")
            .push(session.clone());
        Ok(())
    }

    async fn find_active_by_digest(
        &self,
        digest: TokenDigest,
        now: Timestamp,
    ) -> Result<Option<Session>, PortError> {
        Ok(self
            .sessions
            .lock()
            .expect("sessions lock")
            .iter()
            .find(|session| session.token_digest() == digest && session.expires_at() > now)
            .cloned())
    }

    async fn revoke(&self, digest: TokenDigest) -> Result<bool, PortError> {
        let mut sessions = self.sessions.lock().expect("sessions lock");
        let before = sessions.len();
        sessions.retain(|session| session.token_digest() != digest);
        Ok(sessions.len() != before)
    }

    async fn prune_expired(&self, now: Timestamp, limit: u16) -> Result<u16, PortError> {
        let mut sessions = self.sessions.lock().expect("sessions lock");
        let expired: Vec<TokenDigest> = sessions
            .iter()
            .filter(|session| session.expires_at() <= now)
            .take(usize::from(limit))
            .map(Session::token_digest)
            .collect();
        sessions.retain(|session| !expired.contains(&session.token_digest()));
        Ok(expired.len() as u16)
    }
}

#[derive(Default)]
struct FakeEnrollments {
    enrollments: Mutex<Vec<AgentEnrollment>>,
}

#[async_trait]
impl AgentEnrollmentRepository for FakeEnrollments {
    async fn save(&self, enrollment: &AgentEnrollment) -> Result<(), PortError> {
        self.enrollments
            .lock()
            .expect("enrollments lock")
            .push(enrollment.clone());
        Ok(())
    }

    async fn consume(
        &self,
        digest: TokenDigest,
        now: Timestamp,
    ) -> Result<Option<AgentId>, PortError> {
        let mut enrollments = self.enrollments.lock().expect("enrollments lock");
        let Some(enrollment) = enrollments
            .iter_mut()
            .find(|enrollment| enrollment.token_digest() == digest)
        else {
            return Ok(None);
        };
        match enrollment.consume(now) {
            Ok(()) => Ok(Some(enrollment.agent_id())),
            Err(_) => Ok(None),
        }
    }

    async fn prune_expired(&self, now: Timestamp, limit: u16) -> Result<u16, PortError> {
        let mut enrollments = self.enrollments.lock().expect("enrollments lock");
        let expired: Vec<TokenDigest> = enrollments
            .iter()
            .filter(|enrollment| enrollment.expires_at() <= now)
            .take(usize::from(limit))
            .map(AgentEnrollment::token_digest)
            .collect();
        enrollments.retain(|enrollment| !expired.contains(&enrollment.token_digest()));
        Ok(expired.len() as u16)
    }
}

#[derive(Default)]
struct FakeAgents {
    agents: Mutex<Vec<Agent>>,
}

#[async_trait]
impl AgentRepository for FakeAgents {
    async fn save(&self, agent: &Agent) -> Result<(), PortError> {
        let mut agents = self.agents.lock().expect("agents lock");
        agents.retain(|existing| existing.id() != agent.id());
        agents.push(agent.clone());
        Ok(())
    }

    async fn find(&self, id: AgentId) -> Result<Option<Agent>, PortError> {
        Ok(self
            .agents
            .lock()
            .expect("agents lock")
            .iter()
            .find(|agent| agent.id() == id)
            .cloned())
    }

    async fn find_by_token_digest(&self, digest: TokenDigest) -> Result<Option<Agent>, PortError> {
        Ok(self
            .agents
            .lock()
            .expect("agents lock")
            .iter()
            .find(|agent| agent.token_digest() == Some(digest))
            .cloned())
    }

    async fn list(&self) -> Result<Vec<Agent>, PortError> {
        Ok(self.agents.lock().expect("agents lock").clone())
    }

    async fn delete(&self, id: AgentId) -> Result<bool, PortError> {
        let mut agents = self.agents.lock().expect("agents lock");
        let before = agents.len();
        agents.retain(|agent| agent.id() != id);
        Ok(agents.len() != before)
    }

    async fn mark_connected(
        &self,
        id: AgentId,
        version: &str,
        now: Timestamp,
    ) -> Result<(), PortError> {
        let mut agents = self.agents.lock().expect("agents lock");
        let agent = agents
            .iter_mut()
            .find(|agent| agent.id() == id)
            .ok_or(PortError::NotFound)?;
        agent
            .mark_connected(version, now)
            .map_err(|_| PortError::InvalidData)
    }

    async fn mark_disconnected(&self, id: AgentId) -> Result<(), PortError> {
        let mut agents = self.agents.lock().expect("agents lock");
        if let Some(agent) = agents.iter_mut().find(|agent| agent.id() == id) {
            agent.mark_disconnected();
        }
        Ok(())
    }
}

#[derive(Default)]
struct FakeGateway {
    disconnects: Mutex<Vec<AgentId>>,
    configurations: Mutex<Vec<AgentId>>,
}

#[async_trait]
impl AgentControlGateway for FakeGateway {
    async fn send_configuration(&self, agent_id: AgentId) -> Result<(), PortError> {
        self.configurations.lock().expect("lock").push(agent_id);
        Ok(())
    }

    async fn trigger_check(
        &self,
        _agent_id: AgentId,
        _target_id: TargetId,
    ) -> Result<(), PortError> {
        Ok(())
    }

    async fn disconnect(&self, agent_id: AgentId) -> Result<(), PortError> {
        self.disconnects.lock().expect("lock").push(agent_id);
        Ok(())
    }
}

struct Fixture {
    clock: Arc<FakeClock>,
    users: Arc<FakeUsers>,
    sessions: Arc<FakeSessions>,
    agents: Arc<FakeAgents>,
    gateway: Arc<FakeGateway>,
    setup: SetupAdmin,
    login: Login,
    authenticate: AuthenticateSession,
    register: RegisterAgent,
    rotate: RotateAgentToken,
    enroll: EnrollAgent,
}

fn fixture() -> Fixture {
    let clock = FakeClock::new();
    let users = Arc::new(FakeUsers::default());
    let sessions = Arc::new(FakeSessions::default());
    let agents = Arc::new(FakeAgents::default());
    let enrollments = Arc::new(FakeEnrollments::default());
    let gateway = Arc::new(FakeGateway::default());
    let tokens = FakeTokens::new();
    let hasher = Arc::new(FakeHasher);
    let digester = Arc::new(FakeDigester);
    let setup = SetupAdmin::new(users.clone(), hasher.clone(), clock.clone());
    let login = Login::new(
        users.clone(),
        sessions.clone(),
        hasher,
        digester.clone(),
        tokens.clone(),
        clock.clone(),
        Duration::from_secs(7 * 86_400),
    );
    let authenticate = AuthenticateSession::new(
        sessions.clone(),
        users.clone(),
        digester.clone(),
        clock.clone(),
    );
    let register = RegisterAgent::new(
        agents.clone(),
        enrollments.clone(),
        tokens.clone(),
        digester.clone(),
        clock.clone(),
        Duration::from_secs(30 * 60),
    );
    let rotate = RotateAgentToken::new(
        agents.clone(),
        enrollments.clone(),
        gateway.clone(),
        tokens.clone(),
        digester.clone(),
        clock.clone(),
        Duration::from_secs(30 * 60),
    );
    let enroll = EnrollAgent::new(
        agents.clone(),
        enrollments.clone(),
        tokens,
        digester,
        clock.clone(),
    );
    Fixture {
        clock,
        users,
        sessions,
        agents,
        gateway,
        setup,
        login,
        authenticate,
        register,
        rotate,
        enroll,
    }
}

const PASSWORD: &str = "correct-horse-battery";

#[tokio::test]
async fn setup_creates_exactly_one_administrator() {
    let fixture = fixture();
    let user = fixture
        .setup
        .execute("admin", PASSWORD)
        .await
        .expect("first setup succeeds");
    assert_eq!(user.username().as_str(), "admin");
    let second = fixture.setup.execute("other", PASSWORD).await;
    assert!(matches!(second, Err(SetupAdminError::AlreadyConfigured)));
    assert_eq!(fixture.users.count().await.unwrap(), 1);
}

#[tokio::test]
async fn setup_rejects_weak_password_and_bad_username() {
    let fixture = fixture();
    assert!(matches!(
        fixture.setup.execute("admin", "short").await,
        Err(SetupAdminError::InvalidPassword)
    ));
    assert!(matches!(
        fixture.setup.execute("a", PASSWORD).await,
        Err(SetupAdminError::InvalidUsername)
    ));
}

#[tokio::test]
async fn login_issues_session_that_authenticates_until_expiry() {
    let fixture = fixture();
    fixture.setup.execute("admin", PASSWORD).await.unwrap();
    let issued = fixture.login.execute("admin", PASSWORD).await.unwrap();
    assert_eq!(issued.token(), "token-1");
    let authenticated = fixture
        .authenticate
        .execute(issued.token())
        .await
        .unwrap()
        .expect("session is active");
    assert_eq!(authenticated.user().username().as_str(), "admin");
    fixture.clock.advance(Duration::from_secs(7 * 86_400 + 1));
    assert!(
        fixture
            .authenticate
            .execute(issued.token())
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn login_rejects_unknown_user_and_wrong_password() {
    let fixture = fixture();
    fixture.setup.execute("admin", PASSWORD).await.unwrap();
    assert!(matches!(
        fixture.login.execute("admin", "wrong-password").await,
        Err(LoginError::InvalidCredentials)
    ));
    assert!(matches!(
        fixture.login.execute("nobody", PASSWORD).await,
        Err(LoginError::InvalidCredentials)
    ));
    assert_eq!(fixture.sessions.sessions.lock().unwrap().len(), 0);
}

#[tokio::test]
async fn agent_enrolment_is_single_use_and_sets_credential() {
    let fixture = fixture();
    let provision = fixture.register.execute("edge-1").await.unwrap();
    let enrollment_token = provision.enrollment_token().to_owned();
    assert!(provision.agent().token_digest().is_none());

    let enrolled = fixture.enroll.execute(&enrollment_token).await.unwrap();
    assert_eq!(enrolled.agent_id(), provision.agent().id());
    let agent = fixture
        .agents
        .find(provision.agent().id())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        agent.token_digest(),
        Some(FakeDigester.digest(enrolled.credential()))
    );

    assert!(matches!(
        fixture.enroll.execute(&enrollment_token).await,
        Err(EnrollAgentError::InvalidEnrollment)
    ));
}

#[tokio::test]
async fn expired_enrolment_token_is_rejected() {
    let fixture = fixture();
    let provision = fixture.register.execute("edge-1").await.unwrap();
    fixture.clock.advance(Duration::from_secs(31 * 60));
    assert!(matches!(
        fixture.enroll.execute(provision.enrollment_token()).await,
        Err(EnrollAgentError::InvalidEnrollment)
    ));
}

#[tokio::test]
async fn rotation_revokes_credential_and_disconnects_agent() {
    let fixture = fixture();
    let provision = fixture.register.execute("edge-1").await.unwrap();
    let agent_id = provision.agent().id();
    let enrolled = fixture
        .enroll
        .execute(provision.enrollment_token())
        .await
        .unwrap();
    fixture
        .rotate
        .execute(agent_id)
        .await
        .expect("rotation succeeds");
    assert_eq!(
        fixture.gateway.disconnects.lock().unwrap().as_slice(),
        [agent_id]
    );
    let agent = fixture.agents.find(agent_id).await.unwrap().unwrap();
    assert!(agent.token_digest().is_none());
    assert_ne!(
        agent.token_digest(),
        Some(FakeDigester.digest(enrolled.credential()))
    );
}
