use pingushka_application::{
    ports::{
        AgentEnrollmentRepository, AgentRepository, CheckResultRepository, SessionRepository,
        TargetRepository, UserRepository,
    },
    queries::{HistoryFilter, SummaryFilter},
};
use pingushka_domain::{
    Timestamp,
    agent::{Agent, AgentName},
    check::{
        CheckFailure, CheckFailureKind, CheckMode, CheckOutcome, CheckResult, LatencyMs, StatusCode,
    },
    enrollment::AgentEnrollment,
    identity::{AgentId, CheckResultId, TargetId, TokenDigest, UserId},
    target::{CheckIntervalSecs, HttpUrl, Target, TargetConfig, TargetName, TargetSpec, TimeoutMs},
    user::{PasswordHash, Session, User, Username},
};
use pingushka_storage_postgres::PgStore;
use sqlx::PgPool;

async fn migrated_store(pool: PgPool) -> PgStore {
    let store = PgStore::new(pool);
    store.migrate().await.expect("apply schema migrations");
    store
}

fn time(seconds: i64) -> Timestamp {
    Timestamp::from_unix_timestamp(seconds).expect("valid timestamp")
}

fn target(id: TargetId, now: Timestamp) -> Target {
    let config = TargetConfig::new(
        TargetName::new("public health endpoint").unwrap(),
        TargetSpec::http(HttpUrl::parse("https://example.test/health").unwrap()),
        CheckIntervalSecs::new(30).unwrap(),
        TimeoutMs::new(5000).unwrap(),
    )
    .unwrap();
    Target::new(id, config, now)
}

#[sqlx::test(migrations = false)]
async fn target_assignment_and_agent_rows_roundtrip(pool: PgPool) {
    let store = migrated_store(pool).await;
    let now = time(1_700_000_000);
    let target = target(TargetId::new(), now);
    let first_agent = Agent::new(AgentId::new(), AgentName::new("edge-west").unwrap(), now);
    let second_agent = Agent::new(AgentId::new(), AgentName::new("edge-east").unwrap(), now);

    TargetRepository::save(&store, &target).await.unwrap();
    AgentRepository::save(&store, &first_agent).await.unwrap();
    AgentRepository::save(&store, &second_agent).await.unwrap();
    store
        .assign_agents(target.id(), &[first_agent.id(), second_agent.id()])
        .await
        .unwrap();

    assert_eq!(
        TargetRepository::find(&store, target.id()).await.unwrap(),
        Some(target.clone())
    );
    assert_eq!(
        TargetRepository::list(&store).await.unwrap(),
        vec![target.clone()]
    );
    assert_eq!(
        TargetRepository::list_for_agent(&store, first_agent.id())
            .await
            .unwrap(),
        vec![target.clone()]
    );
    assert_eq!(
        TargetRepository::list_for_agent(&store, second_agent.id())
            .await
            .unwrap(),
        vec![target]
    );
    assert_eq!(
        AgentRepository::find(&store, first_agent.id())
            .await
            .unwrap(),
        Some(first_agent)
    );
    assert_eq!(AgentRepository::list(&store).await.unwrap().len(), 2);
}

#[sqlx::test(migrations = false)]
async fn users_and_sessions_roundtrip_and_expired_sessions_are_hidden(pool: PgPool) {
    let store = migrated_store(pool).await;
    let now = time(1_700_000_000);
    let user = User::new(
        UserId::new(),
        Username::new("demo").unwrap(),
        PasswordHash::new("$argon2id$v=19$m=65536,t=2,p=1$salt$encoded").unwrap(),
        now,
    );
    let active_digest = TokenDigest::new([1; 32]);
    let expired_digest = TokenDigest::new([2; 32]);
    let active = Session::new(user.id(), active_digest, now, time(1_700_003_600)).unwrap();
    let expired = Session::new(
        user.id(),
        expired_digest,
        time(1_699_900_000),
        time(1_699_999_999),
    )
    .unwrap();

    UserRepository::save(&store, &user).await.unwrap();
    SessionRepository::save(&store, &active).await.unwrap();
    SessionRepository::save(&store, &expired).await.unwrap();

    assert_eq!(
        UserRepository::find_by_username(&store, user.username())
            .await
            .unwrap(),
        Some(user.clone())
    );
    assert_eq!(UserRepository::count(&store).await.unwrap(), 1);
    assert_eq!(
        SessionRepository::find_active_by_digest(&store, active_digest, now)
            .await
            .unwrap(),
        Some(active)
    );
    assert!(
        SessionRepository::find_active_by_digest(&store, expired_digest, now)
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        SessionRepository::revoke(&store, active_digest)
            .await
            .unwrap()
    );
    assert!(
        !SessionRepository::revoke(&store, active_digest)
            .await
            .unwrap()
    );
}

#[sqlx::test(migrations = false)]
async fn result_batches_are_idempotent_and_queries_return_aggregates(pool: PgPool) {
    let store = migrated_store(pool).await;
    let start = time(1_700_000_000);
    let target_id = TargetId::new();
    let agent_id = AgentId::new();
    let target = target(target_id, start);
    let agent = Agent::new(agent_id, AgentName::new("edge-north").unwrap(), start);
    TargetRepository::save(&store, &target).await.unwrap();
    AgentRepository::save(&store, &agent).await.unwrap();

    let success = CheckResult::new(
        CheckResultId::new(),
        target_id,
        agent_id,
        start,
        CheckOutcome::new(
            true,
            LatencyMs::new(10),
            Some(StatusCode::new(200).unwrap()),
            CheckMode::Http,
            None,
        )
        .unwrap(),
    );
    let failure = CheckResult::new(
        CheckResultId::new(),
        target_id,
        agent_id,
        time(1_700_000_001),
        CheckOutcome::new(
            false,
            LatencyMs::new(20),
            None,
            CheckMode::Tcp,
            Some(CheckFailure::new(CheckFailureKind::Timeout, "timed out").unwrap()),
        )
        .unwrap(),
    );
    let batch = vec![success, failure];

    assert_eq!(store.record_batch(&batch).await.unwrap(), 2);
    assert_eq!(store.record_batch(&batch).await.unwrap(), 0);

    let history = store
        .history(
            HistoryFilter::new(target_id, Some(agent_id), start, time(1_700_000_010), 100).unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(history.len(), 2);
    assert!(!history[0].is_successful());

    let summary = store
        .summarize(
            SummaryFilter::new(Some(target_id), Some(agent_id), start, time(1_700_000_010))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(summary.total_checks(), 2);
    assert_eq!(summary.successful_checks(), 1);
    assert_eq!(summary.average_latency_ms(), Some(15.0));
    assert_eq!(summary.p95_latency_ms(), Some(19.5));
    assert_eq!(summary.uptime_percent(), Some(50.0));

    assert_eq!(store.prune_before(time(1_700_000_002), 1).await.unwrap(), 1);
    assert_eq!(
        store.prune_before(time(1_700_000_002), 10).await.unwrap(),
        1
    );
}

#[sqlx::test(migrations = false)]
async fn enrolment_tokens_are_consumed_once_and_assignments_are_listed(pool: PgPool) {
    let store = migrated_store(pool).await;
    let now = time(1_700_000_000);
    let agent = Agent::new(AgentId::new(), AgentName::new("edge-south").unwrap(), now);
    AgentRepository::save(&store, &agent).await.unwrap();

    let target = target(TargetId::new(), now);
    TargetRepository::save(&store, &target).await.unwrap();
    store
        .assign_agents(target.id(), &[agent.id()])
        .await
        .unwrap();

    let digest = TokenDigest::new([9; 32]);
    let enrollment = AgentEnrollment::new(agent.id(), digest, now, time(1_700_001_800)).unwrap();
    AgentEnrollmentRepository::save(&store, &enrollment)
        .await
        .unwrap();

    assert_eq!(store.consume(digest, now).await.unwrap(), Some(agent.id()));
    assert_eq!(store.consume(digest, now).await.unwrap(), None);

    let expired_digest = TokenDigest::new([8; 32]);
    let expired = AgentEnrollment::new(
        agent.id(),
        expired_digest,
        time(1_699_000_000),
        time(1_699_001_800),
    )
    .unwrap();
    AgentEnrollmentRepository::save(&store, &expired)
        .await
        .unwrap();
    assert_eq!(store.consume(expired_digest, now).await.unwrap(), None);

    let listed = store.list_with_agents().await.unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].target().id(), target.id());
    assert_eq!(listed[0].agent_ids(), &[agent.id()]);
    assert_eq!(
        store.agent_ids(target.id()).await.unwrap(),
        vec![agent.id()]
    );

    let user = User::new(
        UserId::new(),
        Username::new("root").unwrap(),
        PasswordHash::new("$argon2id$v=19$m=65536,t=2,p=1$salt$encoded").unwrap(),
        now,
    );
    assert!(store.save_if_first(&user).await.unwrap());
    assert!(!store.save_if_first(&user).await.unwrap());
    assert_eq!(
        UserRepository::find(&store, user.id()).await.unwrap(),
        Some(user)
    );
}
