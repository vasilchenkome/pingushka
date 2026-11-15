use std::{error::Error, fmt, sync::Arc};

use async_trait::async_trait;
use pingushka_domain::{
    Timestamp,
    alert::{AlertEvent, AlertMode, AlertRule, AlertStatus},
    identity::TargetId,
};
use serde_json::{Value, json};
use time::{Duration, format_description::well_known::Rfc3339};
use uuid::Uuid;

use crate::{error::PortError, ports::Clock};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChannelKind {
    Telegram,
    Webhook,
    Email,
}

impl ChannelKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Telegram => "telegram",
            Self::Webhook => "webhook",
            Self::Email => "email",
        }
    }
}

impl TryFrom<&str> for ChannelKind {
    type Error = AlertServiceError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "telegram" => Ok(Self::Telegram),
            "webhook" => Ok(Self::Webhook),
            "email" => Ok(Self::Email),
            _ => Err(AlertServiceError::InvalidData("unsupported channel kind")),
        }
    }
}

#[derive(Clone, Debug)]
pub struct AlertChannel {
    pub id: Uuid,
    pub name: String,
    pub kind: ChannelKind,
    pub config_encrypted: Vec<u8>,
    pub enabled: bool,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

#[derive(Clone, Debug)]
pub struct AlertRuleRecord {
    pub id: Uuid,
    pub rule: AlertRule,
}

#[derive(Clone, Debug)]
pub struct AlertState {
    pub rule_id: Uuid,
    pub status: AlertStatus,
    pub failures: u16,
    pub successes: u16,
    pub last_result_at: Option<Timestamp>,
    pub last_notified_at: Option<Timestamp>,
    pub opened_at: Option<Timestamp>,
    pub updated_at: Timestamp,
}

impl AlertState {
    pub fn initial(rule_id: Uuid, now: Timestamp) -> Self {
        Self {
            rule_id,
            status: AlertStatus::Ok,
            failures: 0,
            successes: 0,
            last_result_at: None,
            last_notified_at: None,
            opened_at: None,
            updated_at: now,
        }
    }
}

#[derive(Clone, Debug)]
pub struct AgentResult {
    pub agent_id: Uuid,
    pub name: String,
    pub ok: bool,
    pub checked_at: Timestamp,
}

#[derive(Clone, Debug)]
pub struct RuleView {
    pub id: Uuid,
    pub rule: AlertRule,
    pub state: AlertState,
    pub channels: Vec<Uuid>,
    pub target_name: String,
    pub target_kind: String,
    pub target_ref: String,
}

#[derive(Clone, Debug)]
pub struct RuleSnapshot {
    pub rule_id: Uuid,
    pub rule: AlertRule,
    pub state: AlertState,
    pub channels: Vec<Uuid>,
    pub interval_secs: u32,
    pub target_name: String,
    pub target_kind: String,
    pub target_ref: String,
    pub agents: Vec<AgentResult>,
}

#[derive(Clone, Debug)]
pub struct OutboxDraft {
    pub channel_id: Uuid,
    pub target_id: Uuid,
    pub dedupe_key: String,
    pub event: AlertEvent,
    pub payload: Value,
}

#[derive(Clone, Debug)]
pub struct OutboxItem {
    pub id: Uuid,
    pub channel_id: Uuid,
    pub channel_name: String,
    pub kind: ChannelKind,
    pub config_encrypted: Vec<u8>,
    pub event: AlertEvent,
    pub payload: Value,
    pub attempts: u16,
}

#[derive(Clone, Debug)]
pub struct OutboxEvent {
    pub id: Uuid,
    pub event: AlertEvent,
    pub target_name: String,
    pub channel_name: String,
    pub status: String,
    pub attempts: u16,
    pub last_error: Option<String>,
    pub created_at: Timestamp,
    pub sent_at: Option<Timestamp>,
}

#[async_trait]
pub trait AlertRepository: Send + Sync {
    async fn list_channels(&self) -> Result<Vec<AlertChannel>, PortError>;
    async fn save_channel(&self, channel: &AlertChannel) -> Result<(), PortError>;
    async fn delete_channel(&self, id: Uuid) -> Result<bool, PortError>;
    async fn list_rules(&self) -> Result<Vec<AlertRuleRecord>, PortError>;
    async fn rule_views(&self) -> Result<Vec<RuleView>, PortError>;
    async fn save_rule(
        &self,
        rule: &AlertRuleRecord,
        channel_ids: &[Uuid],
    ) -> Result<Uuid, PortError>;
    async fn delete_rule_for_target(&self, target_id: TargetId) -> Result<bool, PortError>;
    async fn snapshots(&self) -> Result<Vec<RuleSnapshot>, PortError>;
    async fn apply_transition(
        &self,
        state: &AlertState,
        drafts: &[OutboxDraft],
    ) -> Result<(), PortError>;
    async fn due_outbox(&self, limit: u16) -> Result<Vec<OutboxItem>, PortError>;
    async fn mark_outbox_sent(&self, id: Uuid) -> Result<(), PortError>;
    async fn mark_outbox_retry(
        &self,
        id: Uuid,
        attempts: u16,
        next_attempt_at: Timestamp,
        error: &str,
    ) -> Result<(), PortError>;
    async fn mark_outbox_failed(
        &self,
        id: Uuid,
        attempts: u16,
        error: &str,
    ) -> Result<(), PortError>;
    async fn list_events(&self, limit: u16) -> Result<Vec<OutboxEvent>, PortError>;
    async fn retry_event(&self, id: Uuid) -> Result<bool, PortError>;
    async fn prune_events(&self, before: Timestamp, limit: u16) -> Result<u16, PortError>;
}

pub trait SecretCipher: Send + Sync {
    fn encrypt(&self, plaintext: &[u8]) -> Result<Vec<u8>, AlertServiceError>;
    fn decrypt(&self, ciphertext: &[u8]) -> Result<Vec<u8>, AlertServiceError>;
}

#[async_trait]
pub trait NotificationSender: Send + Sync {
    async fn send(&self, kind: ChannelKind, config: &Value, payload: &Value) -> Result<(), String>;
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AlertServiceError {
    Repository(PortError),
    InvalidData(&'static str),
    Cipher(String),
    Sender(String),
}

impl fmt::Display for AlertServiceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Repository(error) => write!(formatter, "{error}"),
            Self::InvalidData(message) => formatter.write_str(message),
            Self::Cipher(message) => write!(formatter, "secret cipher failed: {message}"),
            Self::Sender(message) => write!(formatter, "notification failed: {message}"),
        }
    }
}

impl Error for AlertServiceError {}

impl From<PortError> for AlertServiceError {
    fn from(error: PortError) -> Self {
        Self::Repository(error)
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct AlertRunReport {
    pub rules: usize,
    pub events: usize,
}

pub struct EvaluateAlerts {
    repository: Arc<dyn AlertRepository>,
    clock: Arc<dyn Clock>,
}

impl EvaluateAlerts {
    pub fn new(repository: Arc<dyn AlertRepository>, clock: Arc<dyn Clock>) -> Self {
        Self { repository, clock }
    }

    pub async fn execute(&self) -> Result<AlertRunReport, AlertServiceError> {
        let now = self.clock.now();
        let snapshots = self.repository.snapshots().await?;
        let mut report = AlertRunReport::default();
        for snapshot in snapshots {
            if !snapshot.rule.is_enabled() {
                continue;
            }
            report.rules += 1;
            let fresh_after = now
                - Duration::seconds(i64::from(snapshot.interval_secs.max(5)) * 2)
                    .max(Duration::seconds(120));
            let fresh: Vec<&AgentResult> = snapshot
                .agents
                .iter()
                .filter(|agent| agent.checked_at >= fresh_after)
                .collect();
            if fresh.is_empty() {
                continue;
            }
            let newest = fresh
                .iter()
                .map(|agent| agent.checked_at)
                .max()
                .unwrap_or(now);
            if newest
                <= snapshot
                    .state
                    .last_result_at
                    .unwrap_or(Timestamp::UNIX_EPOCH)
                && snapshot.state.status != AlertStatus::Pending
            {
                continue;
            }
            let failed = match snapshot.rule.mode() {
                AlertMode::All => fresh.iter().all(|agent| !agent.ok),
                AlertMode::Any => fresh.iter().any(|agent| !agent.ok),
            };
            let cooldown = Duration::seconds(i64::from(snapshot.rule.cooldown_secs()));
            let cooldown_ok = snapshot
                .state
                .last_notified_at
                .is_none_or(|last| now >= last + cooldown);
            let mut state = snapshot.state.clone();
            let mut drafts = Vec::new();
            if failed {
                state.failures = state.failures.saturating_add(1);
                state.successes = 0;
                if state.status != AlertStatus::Firing {
                    if state.failures >= snapshot.rule.failure_threshold() {
                        if cooldown_ok {
                            state.status = AlertStatus::Firing;
                            state.opened_at = Some(now);
                            state.last_notified_at = Some(now);
                            let payload = payload(&snapshot, AlertEvent::Down, state.failures, now);
                            drafts = drafts_for(&snapshot, AlertEvent::Down, &payload, now);
                        } else {
                            state.status = AlertStatus::Pending;
                        }
                    } else {
                        state.status = AlertStatus::Ok;
                    }
                }
            } else {
                state.failures = 0;
                state.successes = state.successes.saturating_add(1);
                if state.status == AlertStatus::Firing
                    && state.successes >= snapshot.rule.recovery_threshold()
                {
                    let opened = state.opened_at.unwrap_or(now);
                    state.status = AlertStatus::Ok;
                    state.successes = 0;
                    state.opened_at = None;
                    state.last_notified_at = Some(now);
                    let payload = payload(&snapshot, AlertEvent::Recovered, 0, opened);
                    drafts = drafts_for(&snapshot, AlertEvent::Recovered, &payload, opened);
                } else if state.status == AlertStatus::Pending {
                    state.status = AlertStatus::Ok;
                    state.successes = 0;
                }
            }
            state.last_result_at = Some(newest);
            state.updated_at = now;
            report.events += drafts.len();
            self.repository.apply_transition(&state, &drafts).await?;
        }
        Ok(report)
    }
}

fn payload(snapshot: &RuleSnapshot, event: AlertEvent, failures: u16, since: Timestamp) -> Value {
    json!({
        "event": event.as_str(),
        "target": {
            "id": snapshot.rule.target_id().as_uuid(),
            "name": snapshot.target_name,
            "kind": snapshot.target_kind,
            "ref": snapshot.target_ref,
        },
        "failures": failures,
        "since": since.format(&Rfc3339).unwrap_or_default(),
        "agents": snapshot.agents.iter().map(|agent| json!({
            "id": agent.agent_id,
            "name": agent.name,
            "ok": agent.ok,
        })).collect::<Vec<_>>(),
    })
}

fn drafts_for(
    snapshot: &RuleSnapshot,
    event: AlertEvent,
    payload: &Value,
    opened: Timestamp,
) -> Vec<OutboxDraft> {
    snapshot
        .channels
        .iter()
        .map(|channel_id| OutboxDraft {
            channel_id: *channel_id,
            target_id: snapshot.rule.target_id().as_uuid(),
            dedupe_key: format!(
                "{}:{}:{}:{}",
                snapshot.rule_id,
                channel_id,
                event.as_str(),
                opened.unix_timestamp()
            ),
            event,
            payload: payload.clone(),
        })
        .collect()
}

pub struct DispatchNotifications {
    repository: Arc<dyn AlertRepository>,
    cipher: Arc<dyn SecretCipher>,
    sender: Arc<dyn NotificationSender>,
    max_attempts: u16,
}

impl DispatchNotifications {
    pub fn new(
        repository: Arc<dyn AlertRepository>,
        cipher: Arc<dyn SecretCipher>,
        sender: Arc<dyn NotificationSender>,
    ) -> Self {
        Self {
            repository,
            cipher,
            sender,
            max_attempts: 5,
        }
    }

    pub async fn execute(&self) -> Result<usize, AlertServiceError> {
        let items = self.repository.due_outbox(20).await?;
        let mut sent = 0_usize;
        for item in items {
            let config = match self.cipher.decrypt(&item.config_encrypted) {
                Ok(bytes) => match serde_json::from_slice::<Value>(&bytes) {
                    Ok(value) => value,
                    Err(error) => {
                        self.repository
                            .mark_outbox_failed(item.id, item.attempts + 1, &error.to_string())
                            .await?;
                        continue;
                    }
                },
                Err(error) => {
                    self.repository
                        .mark_outbox_failed(item.id, item.attempts + 1, &error.to_string())
                        .await?;
                    continue;
                }
            };
            match self.sender.send(item.kind, &config, &item.payload).await {
                Ok(()) => {
                    self.repository.mark_outbox_sent(item.id).await?;
                    sent += 1;
                }
                Err(error) => {
                    let attempts = item.attempts.saturating_add(1);
                    if attempts >= self.max_attempts {
                        self.repository
                            .mark_outbox_failed(item.id, attempts, &error)
                            .await?;
                    } else {
                        let delay = retry_delay(attempts);
                        self.repository
                            .mark_outbox_retry(
                                item.id,
                                attempts,
                                Timestamp::now_utc() + delay,
                                &error,
                            )
                            .await?;
                    }
                }
            }
        }
        Ok(sent)
    }
}

fn retry_delay(attempts: u16) -> Duration {
    match attempts {
        1 => Duration::seconds(30),
        2 => Duration::minutes(2),
        3 => Duration::minutes(10),
        4 => Duration::hours(1),
        _ => Duration::hours(6),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;
    use crate::ports::Clock;
    use pingushka_domain::identity::TargetId;

    struct FixedClock(Timestamp);

    impl Clock for FixedClock {
        fn now(&self) -> Timestamp {
            self.0
        }
    }

    #[derive(Default)]
    struct FakeRepository {
        snapshots: Mutex<Vec<RuleSnapshot>>,
        applied: Mutex<Option<(AlertState, Vec<OutboxDraft>)>>,
    }

    #[async_trait]
    impl AlertRepository for FakeRepository {
        async fn list_channels(&self) -> Result<Vec<AlertChannel>, PortError> {
            unimplemented!()
        }

        async fn save_channel(&self, _channel: &AlertChannel) -> Result<(), PortError> {
            unimplemented!()
        }

        async fn delete_channel(&self, _id: Uuid) -> Result<bool, PortError> {
            unimplemented!()
        }

        async fn list_rules(&self) -> Result<Vec<AlertRuleRecord>, PortError> {
            unimplemented!()
        }

        async fn rule_views(&self) -> Result<Vec<RuleView>, PortError> {
            unimplemented!()
        }

        async fn save_rule(
            &self,
            _rule: &AlertRuleRecord,
            _channel_ids: &[Uuid],
        ) -> Result<Uuid, PortError> {
            unimplemented!()
        }

        async fn delete_rule_for_target(&self, _target_id: TargetId) -> Result<bool, PortError> {
            unimplemented!()
        }

        async fn snapshots(&self) -> Result<Vec<RuleSnapshot>, PortError> {
            Ok(self.snapshots.lock().unwrap().clone())
        }

        async fn apply_transition(
            &self,
            state: &AlertState,
            drafts: &[OutboxDraft],
        ) -> Result<(), PortError> {
            *self.applied.lock().unwrap() = Some((state.clone(), drafts.to_vec()));
            Ok(())
        }

        async fn due_outbox(&self, _limit: u16) -> Result<Vec<OutboxItem>, PortError> {
            unimplemented!()
        }

        async fn mark_outbox_sent(&self, _id: Uuid) -> Result<(), PortError> {
            unimplemented!()
        }

        async fn mark_outbox_retry(
            &self,
            _id: Uuid,
            _attempts: u16,
            _next_attempt_at: Timestamp,
            _error: &str,
        ) -> Result<(), PortError> {
            unimplemented!()
        }

        async fn mark_outbox_failed(
            &self,
            _id: Uuid,
            _attempts: u16,
            _error: &str,
        ) -> Result<(), PortError> {
            unimplemented!()
        }

        async fn list_events(&self, _limit: u16) -> Result<Vec<OutboxEvent>, PortError> {
            unimplemented!()
        }

        async fn retry_event(&self, _id: Uuid) -> Result<bool, PortError> {
            unimplemented!()
        }

        async fn prune_events(&self, _before: Timestamp, _limit: u16) -> Result<u16, PortError> {
            unimplemented!()
        }
    }

    fn snapshot(now: Timestamp, state: AlertState, ok: bool) -> RuleSnapshot {
        let rule = AlertRule::new(TargetId::new(), AlertMode::All, 3, 1, 900, now).unwrap();
        RuleSnapshot {
            rule_id: state.rule_id,
            rule,
            state,
            channels: vec![Uuid::new_v4()],
            interval_secs: 30,
            target_name: "example".to_owned(),
            target_kind: "http".to_owned(),
            target_ref: "https://example.test".to_owned(),
            agents: vec![AgentResult {
                agent_id: Uuid::new_v4(),
                name: "edge".to_owned(),
                ok,
                checked_at: now,
            }],
        }
    }

    #[tokio::test]
    async fn third_failure_opens_one_incident_and_recovery_closes_it() {
        let now = Timestamp::from_unix_timestamp(1_700_000_000).unwrap();
        let rule_id = Uuid::new_v4();
        let mut state = AlertState::initial(rule_id, now - Duration::seconds(60));
        state.failures = 2;
        let repo = Arc::new(FakeRepository {
            snapshots: Mutex::new(vec![snapshot(now, state, false)]),
            applied: Mutex::new(None),
        });
        let evaluator = EvaluateAlerts::new(repo.clone(), Arc::new(FixedClock(now)));
        let report = evaluator.execute().await.unwrap();
        assert_eq!(report.events, 1);
        let (next, drafts) = repo.applied.lock().unwrap().clone().unwrap();
        assert_eq!(next.status, AlertStatus::Firing);
        assert_eq!(drafts.len(), 1);
        assert_eq!(drafts[0].event, AlertEvent::Down);

        let mut fired = next;
        fired.successes = 0;
        repo.snapshots.lock().unwrap()[0] = snapshot(now + Duration::seconds(30), fired, true);
        let report = EvaluateAlerts::new(
            repo.clone(),
            Arc::new(FixedClock(now + Duration::seconds(30))),
        )
        .execute()
        .await
        .unwrap();
        assert_eq!(report.events, 1);
        let (next, drafts) = repo.applied.lock().unwrap().clone().unwrap();
        assert_eq!(next.status, AlertStatus::Ok);
        assert_eq!(drafts[0].event, AlertEvent::Recovered);
    }

    #[tokio::test]
    async fn cooldown_holds_second_incident_in_pending() {
        let now = Timestamp::from_unix_timestamp(1_700_000_000).unwrap();
        let rule_id = Uuid::new_v4();
        let mut state = AlertState::initial(rule_id, now - Duration::seconds(60));
        state.failures = 2;
        state.last_notified_at = Some(now - Duration::seconds(10));
        let repo = Arc::new(FakeRepository {
            snapshots: Mutex::new(vec![snapshot(now, state, false)]),
            applied: Mutex::new(None),
        });
        let report = EvaluateAlerts::new(repo.clone(), Arc::new(FixedClock(now)))
            .execute()
            .await
            .unwrap();
        assert_eq!(report.events, 0);
        let (next, drafts) = repo.applied.lock().unwrap().clone().unwrap();
        assert_eq!(next.status, AlertStatus::Pending);
        assert!(drafts.is_empty());
    }
}
