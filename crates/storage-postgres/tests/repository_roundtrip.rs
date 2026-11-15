use pingushka_application::{
    alerts::{AlertChannel, AlertRepository, AlertRuleRecord, ChannelKind, OutboxDraft},
    ports::{
        AgentEnrollmentRepository, AgentRepository, CheckResultRepository, SessionRepository,
        TargetRepository, UserRepository,
    },
    queries::{HistoryFilter, SummaryFilter},
};
use pingushka_domain::{
    Timestamp,
    agent::{Agent, AgentName},
    alert::{AlertEvent, AlertMode, AlertRule, AlertScope},
    check::{
        CheckFailure, CheckFailureKind, CheckMode, CheckOutcome, CheckResult, LatencyMs, StatusCode,
    },
    enrollment::AgentEnrollment,
    identity::{AgentId, CheckResultId, TargetId, TokenDigest, UserId},
    organization::TargetOrganization,
    target::{CheckIntervalSecs, HttpUrl, Target, TargetConfig, TargetName, TargetSpec, TimeoutMs},
    user::{PasswordHash, Session, User, Username},
};
use pingushka_storage_postgres::PgStore;
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;

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
    Target::new(id, config, TargetOrganization::empty(), now)
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

#[sqlx::test(migrations = false)]
async fn alert_channels_rules_and_outbox_roundtrip(pool: PgPool) {
    let store = migrated_store(pool).await;
    let now = time(1_700_000_000);
    let target = target(TargetId::new(), now);
    TargetRepository::save(&store, &target).await.unwrap();

    let channel = AlertChannel {
        id: Uuid::new_v4(),
        name: "hooks".to_owned(),
        kind: ChannelKind::Webhook,
        config_encrypted: vec![1, 2, 3],
        enabled: true,
        created_at: now,
        updated_at: now,
    };
    AlertRepository::save_channel(&store, &channel)
        .await
        .unwrap();

    let rule = AlertRule::new(
        AlertScope::Target(target.id()),
        AlertMode::All,
        3,
        1,
        60,
        now,
    )
    .unwrap();
    let rule_id = AlertRepository::save_rule(
        &store,
        &AlertRuleRecord {
            id: Uuid::new_v4(),
            rule,
        },
        &[channel.id],
    )
    .await
    .unwrap();

    let views = store.rule_views().await.unwrap();
    assert_eq!(views.len(), 1);
    assert_eq!(views[0].id, rule_id);
    assert_eq!(views[0].channels, vec![channel.id]);
    assert_eq!(views[0].target_kind.as_deref(), Some("http"));

    let payload = json!({"event": "down", "target": {"name": target.name().as_str()}});
    store
        .apply_transition(
            &views[0].state,
            &[OutboxDraft {
                channel_id: channel.id,
                target_id: Some(target.id().as_uuid()),
                group_id: None,
                dedupe_key: format!("{rule_id}:down:1"),
                event: AlertEvent::Down,
                payload: payload.clone(),
            }],
        )
        .await
        .unwrap();

    let due = store.due_outbox(10).await.unwrap();
    assert_eq!(due.len(), 1);
    assert_eq!(due[0].kind, ChannelKind::Webhook);
    assert_eq!(due[0].payload, payload);
    store.mark_outbox_sent(due[0].id).await.unwrap();

    let events = store.list_events(10).await.unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].status, "sent");
    assert!(!store.retry_event(events[0].id).await.unwrap());
}

#[sqlx::test(migrations = false)]
async fn group_rule_expands_to_targets_without_their_own_rule(pool: PgPool) {
    use pingushka_application::ports::TargetGroupRepository;
    use pingushka_domain::organization::{TargetGroup, TargetGroupName, TargetTagSet};

    let store = migrated_store(pool).await;
    let now = time(1_700_000_000);
    let group = TargetGroup::new(TargetGroupName::new("Neva").unwrap(), now);
    TargetGroupRepository::save(&store, &group).await.unwrap();

    let config = TargetConfig::new(
        TargetName::new("api health").unwrap(),
        TargetSpec::http(HttpUrl::parse("https://example.test/health").unwrap()),
        CheckIntervalSecs::new(30).unwrap(),
        TimeoutMs::new(5000).unwrap(),
    )
    .unwrap();
    let target = Target::new(
        TargetId::new(),
        config,
        TargetOrganization::new(Some(group.id()), TargetTagSet::empty()),
        now,
    );
    TargetRepository::save(&store, &target).await.unwrap();
    let agent = Agent::new(AgentId::new(), AgentName::new("edge-group").unwrap(), now);
    AgentRepository::save(&store, &agent).await.unwrap();
    store
        .assign_agents(target.id(), &[agent.id()])
        .await
        .unwrap();
    let result = CheckResult::new(
        CheckResultId::new(),
        target.id(),
        agent.id(),
        now,
        CheckOutcome::new(
            false,
            LatencyMs::new(20),
            None,
            CheckMode::Http,
            Some(CheckFailure::new(CheckFailureKind::Timeout, "timed out").unwrap()),
        )
        .unwrap(),
    );
    store.record_batch(&[result]).await.unwrap();

    let group_rule =
        AlertRule::new(AlertScope::Group(group.id()), AlertMode::All, 1, 1, 60, now).unwrap();
    AlertRepository::save_rule(
        &store,
        &AlertRuleRecord {
            id: Uuid::new_v4(),
            rule: group_rule,
        },
        &[],
    )
    .await
    .unwrap();

    let snapshots = AlertRepository::snapshots(&store).await.unwrap();
    assert_eq!(snapshots.len(), 2);
    let group_snapshot = snapshots
        .iter()
        .find(|snapshot| snapshot.subject_kind == "group")
        .expect("group incident snapshot");
    assert_eq!(group_snapshot.group_targets.len(), 1);
    let template = snapshots
        .iter()
        .find(|snapshot| snapshot.subject_kind == "target")
        .expect("group template snapshot");
    assert_eq!(template.target_name.as_deref(), Some("api health"));
    assert_eq!(template.target_kind.as_deref(), Some("http"));
    assert!(template.target_ref.is_some());

    let target_rule = AlertRule::new(
        AlertScope::Target(target.id()),
        AlertMode::All,
        1,
        1,
        60,
        now,
    )
    .unwrap();
    AlertRepository::save_rule(
        &store,
        &AlertRuleRecord {
            id: Uuid::new_v4(),
            rule: target_rule,
        },
        &[],
    )
    .await
    .unwrap();
    let snapshots = AlertRepository::snapshots(&store).await.unwrap();
    assert_eq!(snapshots.len(), 2);
    assert!(
        snapshots
            .iter()
            .all(|snapshot| snapshot.subject_kind == "group" || snapshot.rule.target_id().is_some())
    );
}

#[sqlx::test(migrations = false)]
async fn groups_tags_and_group_rules_roundtrip(pool: PgPool) {
    use pingushka_application::ports::{DeleteGroupOutcome, TargetGroupRepository};
    use pingushka_domain::organization::{TargetGroup, TargetGroupName, TargetTagSet};

    let store = migrated_store(pool).await;
    let now = time(1_700_000_000);
    let group = TargetGroup::new(TargetGroupName::new("Neva").unwrap(), now);
    TargetGroupRepository::save(&store, &group).await.unwrap();
    assert_eq!(
        TargetGroupRepository::find_by_name(&store, group.name())
            .await
            .unwrap(),
        Some(group.clone())
    );

    let tags = TargetTagSet::parse(["Prod", "api"]).unwrap();
    let organization = TargetOrganization::new(Some(group.id()), tags.clone());
    let config = TargetConfig::new(
        TargetName::new("api health").unwrap(),
        TargetSpec::http(HttpUrl::parse("https://example.test/health").unwrap()),
        CheckIntervalSecs::new(30).unwrap(),
        TimeoutMs::new(5000).unwrap(),
    )
    .unwrap();
    let target = Target::new(TargetId::new(), config, organization, now);
    TargetRepository::save(&store, &target).await.unwrap();

    let stored = TargetRepository::find(&store, target.id())
        .await
        .unwrap()
        .expect("target exists");
    assert_eq!(stored.group_id(), Some(group.id()));
    assert_eq!(stored.tags(), tags.as_slice());

    let counts = TargetGroupRepository::list_with_counts(&store)
        .await
        .unwrap();
    assert_eq!(counts.len(), 1);
    assert_eq!(counts[0].target_count(), 1);
    assert_eq!(counts[0].rule_count(), 0);

    let rule =
        AlertRule::new(AlertScope::Group(group.id()), AlertMode::All, 3, 1, 60, now).unwrap();
    AlertRepository::save_rule(
        &store,
        &AlertRuleRecord {
            id: Uuid::new_v4(),
            rule,
        },
        &[],
    )
    .await
    .unwrap();

    assert_eq!(
        TargetGroupRepository::delete(&store, group.id())
            .await
            .unwrap(),
        DeleteGroupOutcome::HasRules
    );
    let counts = TargetGroupRepository::list_with_counts(&store)
        .await
        .unwrap();
    assert_eq!(counts[0].rule_count(), 1);
}
