use pingushka_domain::{
    enrollment::AgentEnrollment,
    identity::{AgentId, TokenDigest},
};
use time::OffsetDateTime;

#[test]
fn enrolment_requires_expiry_after_creation() {
    let now = OffsetDateTime::now_utc();
    let result = AgentEnrollment::new(AgentId::new(), digest(), now, now);
    assert!(result.is_err());
}

#[test]
fn enrolment_token_is_single_use() {
    let now = OffsetDateTime::now_utc();
    let mut enrollment = AgentEnrollment::new(
        AgentId::new(),
        digest(),
        now,
        now + time::Duration::minutes(30),
    )
    .expect("valid enrolment");
    assert!(enrollment.consume(now).is_ok());
    assert!(enrollment.consume(now).is_err());
}

#[test]
fn expired_enrolment_cannot_be_consumed() {
    let now = OffsetDateTime::now_utc();
    let mut enrollment = AgentEnrollment::new(
        AgentId::new(),
        digest(),
        now,
        now + time::Duration::minutes(30),
    )
    .expect("valid enrolment");
    assert!(
        enrollment
            .consume(now + time::Duration::minutes(31))
            .is_err()
    );
}

fn digest() -> TokenDigest {
    TokenDigest::new([7_u8; 32])
}
