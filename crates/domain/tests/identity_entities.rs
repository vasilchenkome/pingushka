use pingushka_domain::{
    Timestamp,
    agent::{Agent, AgentName},
    identity::{AgentId, TokenDigest, UserId},
    user::{PasswordHash, Session, User, Username},
};

fn now() -> Timestamp {
    Timestamp::from_unix_timestamp(1_700_000_000).expect("valid test timestamp")
}

#[test]
fn new_agent_starts_disconnected_and_records_connection_state() {
    let connected_at = now() + time::Duration::seconds(2);
    let mut agent = Agent::new(AgentId::new(), AgentName::new("edge-a").unwrap(), now());

    assert!(!agent.is_connected());
    assert!(agent.last_seen_at().is_none());

    agent
        .mark_connected("0.1.0", connected_at)
        .expect("valid agent version");

    assert!(agent.is_connected());
    assert_eq!(agent.version(), Some("0.1.0"));
    assert_eq!(agent.last_seen_at(), Some(connected_at));
}

#[test]
fn user_keeps_hashed_password_without_exposing_plaintext() {
    let password_hash = PasswordHash::new("$argon2id$v=19$m=65536,t=2,p=1$salt$hash")
        .expect("non-empty encoded hash");
    let user = User::new(
        UserId::new(),
        Username::new("demo").expect("valid username"),
        password_hash,
        now(),
    );

    assert_eq!(user.username().as_str(), "demo");
    assert!(user.password_hash().as_encoded().starts_with("$argon2id$"));
}

#[test]
fn session_requires_expiry_after_creation() {
    let created_at = now();
    let digest = TokenDigest::new([7; 32]);

    assert!(
        Session::new(
            UserId::new(),
            digest,
            created_at,
            created_at + time::Duration::seconds(1),
        )
        .is_ok()
    );
    assert!(Session::new(UserId::new(), digest, created_at, created_at).is_err());
}
