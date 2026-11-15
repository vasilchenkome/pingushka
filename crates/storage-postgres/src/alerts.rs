use async_trait::async_trait;
use pingushka_application::{
    alerts::{
        AgentResult, AlertChannel, AlertRepository, AlertRuleRecord, AlertState, ChannelKind,
        GroupTargetResult, OutboxDraft, OutboxEvent, OutboxItem, RuleSnapshot, RuleView,
    },
    error::PortError,
};
use pingushka_domain::{
    Timestamp,
    alert::{AlertEvent, AlertMode, AlertRule, AlertScope, AlertStatus},
    identity::{TargetGroupId, TargetId},
};
use serde_json::Value;
use sqlx::{Row, types::Json};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::PgStore;

fn map_error(error: sqlx::Error) -> PortError {
    if let sqlx::Error::Database(database_error) = &error {
        match database_error.code().as_deref() {
            Some("23505") => return PortError::Conflict,
            Some("23503") => return PortError::NotFound,
            Some("23514") | Some("22P02") => return PortError::InvalidData,
            _ => {}
        }
    }
    PortError::Unavailable
}

fn status(value: &str) -> AlertStatus {
    AlertStatus::try_from(value).unwrap_or(AlertStatus::Ok)
}

fn mode(value: &str) -> AlertMode {
    AlertMode::try_from(value).unwrap_or(AlertMode::All)
}

fn kind(value: &str) -> ChannelKind {
    ChannelKind::try_from(value).unwrap_or(ChannelKind::Telegram)
}

fn target_ref(kind: &str, spec: &Value) -> String {
    match kind {
        "http" => spec
            .get("url")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        "tcp" => {
            let host = spec.get("host").and_then(Value::as_str).unwrap_or_default();
            let port = spec.get("port").and_then(Value::as_u64).unwrap_or(0);
            format!("{host}:{port}")
        }
        "icmp" => spec
            .get("host")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        _ => String::new(),
    }
}

fn scope(row: &sqlx::postgres::PgRow) -> Result<AlertScope, PortError> {
    match (
        row.get::<Option<Uuid>, _>("target_id"),
        row.get::<Option<Uuid>, _>("group_id"),
    ) {
        (Some(target_id), None) => Ok(AlertScope::Target(TargetId::from_uuid(target_id))),
        (None, Some(group_id)) => Ok(AlertScope::Group(TargetGroupId::from_uuid(group_id))),
        _ => Err(PortError::InvalidData),
    }
}

fn restore_rule(row: &sqlx::postgres::PgRow) -> Result<AlertRule, PortError> {
    AlertRule::restore(
        scope(row)?,
        row.get("enabled"),
        mode(row.get::<String, _>("mode").as_str()),
        u16::try_from(row.get::<i32, _>("failure_threshold"))
            .map_err(|_| PortError::InvalidData)?,
        u16::try_from(row.get::<i32, _>("recovery_threshold"))
            .map_err(|_| PortError::InvalidData)?,
        u32::try_from(row.get::<i32, _>("cooldown_secs")).map_err(|_| PortError::InvalidData)?,
        row.get("created_at"),
        row.get("updated_at"),
    )
    .map_err(|_| PortError::InvalidData)
}

fn restore_state(
    row: &sqlx::postgres::PgRow,
    rule_id: Uuid,
    subject_kind: &'static str,
    subject_id: Uuid,
) -> Result<AlertState, PortError> {
    let state_status: Option<String> = row.get("status");
    Ok(AlertState {
        rule_id,
        subject_kind,
        subject_id,
        status: state_status.as_deref().map_or(AlertStatus::Ok, status),
        failures: row
            .get::<Option<i32>, _>("consecutive_failures")
            .unwrap_or(0)
            .try_into()
            .map_err(|_| PortError::InvalidData)?,
        successes: row
            .get::<Option<i32>, _>("consecutive_successes")
            .unwrap_or(0)
            .try_into()
            .map_err(|_| PortError::InvalidData)?,
        last_result_at: row.get("last_result_at"),
        last_notified_at: row.get("last_notified_at"),
        opened_at: row.get("opened_at"),
        updated_at: row
            .get::<Option<OffsetDateTime>, _>("state_updated_at")
            .unwrap_or_else(Timestamp::now_utc),
    })
}

fn initial_state(rule_id: Uuid, subject_kind: &'static str, subject_id: Uuid) -> AlertState {
    AlertState::initial(rule_id, subject_kind, subject_id, Timestamp::now_utc())
}

async fn rule_channels(store: &PgStore, rule_id: Uuid) -> Result<Vec<Uuid>, PortError> {
    Ok(
        sqlx::query("SELECT channel_id FROM alert_rule_channels WHERE rule_id = $1")
            .bind(rule_id)
            .fetch_all(store.pool())
            .await
            .map_err(map_error)?
            .into_iter()
            .map(|channel| channel.get("channel_id"))
            .collect(),
    )
}

async fn target_agents(store: &PgStore, target_id: Uuid) -> Result<Vec<AgentResult>, PortError> {
    Ok(sqlx::query(
        "SELECT a.id, a.name, cr.ok, cr.checked_at FROM target_agents ta JOIN agents a ON a.id = ta.agent_id LEFT JOIN LATERAL (SELECT ok, checked_at FROM check_results WHERE target_id = $1 AND agent_id = ta.agent_id ORDER BY checked_at DESC LIMIT 1) cr ON TRUE WHERE ta.target_id = $1 ORDER BY a.name",
    )
    .bind(target_id)
    .fetch_all(store.pool())
    .await
    .map_err(map_error)?
    .into_iter()
    .filter_map(|agent| {
        Some(AgentResult {
            agent_id: agent.get("id"),
            name: agent.get("name"),
            ok: agent.try_get::<Option<bool>, _>("ok").ok()??,
            checked_at: agent.try_get::<Option<OffsetDateTime>, _>("checked_at").ok()??,
        })
    })
    .collect())
}

async fn subject_state(
    store: &PgStore,
    rule_id: Uuid,
    subject_kind: &'static str,
    subject_id: Uuid,
) -> Result<AlertState, PortError> {
    let row = sqlx::query(
        "SELECT status, consecutive_failures, consecutive_successes, last_result_at, last_notified_at, opened_at, updated_at AS state_updated_at FROM alert_subject_states WHERE rule_id = $1 AND subject_kind = $2 AND subject_id = $3",
    )
    .bind(rule_id)
    .bind(subject_kind)
    .bind(subject_id)
    .fetch_optional(store.pool())
    .await
    .map_err(map_error)?;
    match row {
        Some(row) => restore_state(&row, rule_id, subject_kind, subject_id),
        None => Ok(initial_state(rule_id, subject_kind, subject_id)),
    }
}

#[async_trait]
impl AlertRepository for PgStore {
    async fn list_channels(&self) -> Result<Vec<AlertChannel>, PortError> {
        let rows = sqlx::query(
            "SELECT id, name, kind, config_encrypted, enabled, created_at, updated_at FROM notification_channels ORDER BY name",
        )
        .fetch_all(self.pool())
        .await
        .map_err(map_error)?;
        Ok(rows
            .into_iter()
            .map(|row| AlertChannel {
                id: row.get("id"),
                name: row.get("name"),
                kind: kind(row.get::<String, _>("kind").as_str()),
                config_encrypted: row.get("config_encrypted"),
                enabled: row.get("enabled"),
                created_at: row.get("created_at"),
                updated_at: row.get("updated_at"),
            })
            .collect())
    }

    async fn save_channel(&self, channel: &AlertChannel) -> Result<(), PortError> {
        sqlx::query(
            "INSERT INTO notification_channels (id, name, kind, config_encrypted, enabled, created_at, updated_at) VALUES ($1,$2,$3,$4,$5,$6,$7) ON CONFLICT (id) DO UPDATE SET name = EXCLUDED.name, kind = EXCLUDED.kind, config_encrypted = EXCLUDED.config_encrypted, enabled = EXCLUDED.enabled, updated_at = EXCLUDED.updated_at",
        )
        .bind(channel.id)
        .bind(&channel.name)
        .bind(channel.kind.as_str())
        .bind(&channel.config_encrypted)
        .bind(channel.enabled)
        .bind(channel.created_at)
        .bind(channel.updated_at)
        .execute(self.pool())
        .await
        .map_err(map_error)?;
        Ok(())
    }

    async fn delete_channel(&self, id: Uuid) -> Result<bool, PortError> {
        let result = sqlx::query("DELETE FROM notification_channels WHERE id = $1")
            .bind(id)
            .execute(self.pool())
            .await
            .map_err(map_error)?;
        Ok(result.rows_affected() > 0)
    }

    async fn list_rules(&self) -> Result<Vec<AlertRuleRecord>, PortError> {
        let rows = sqlx::query(
            "SELECT id, target_id, group_id, enabled, mode, failure_threshold, recovery_threshold, cooldown_secs, created_at, updated_at FROM alert_rules ORDER BY created_at",
        )
        .fetch_all(self.pool())
        .await
        .map_err(map_error)?;
        rows.into_iter()
            .map(|row| {
                Ok(AlertRuleRecord {
                    id: row.get("id"),
                    rule: restore_rule(&row)?,
                })
            })
            .collect()
    }

    async fn rule_views(&self) -> Result<Vec<RuleView>, PortError> {
        let rows = sqlx::query(
            "SELECT r.id, r.target_id, r.group_id, r.enabled, r.mode, r.failure_threshold, r.recovery_threshold, r.cooldown_secs, r.created_at, r.updated_at, t.name AS target_name, t.kind AS target_kind, t.spec_json, g.name AS group_name, s.status, s.consecutive_failures, s.consecutive_successes, s.last_result_at, s.last_notified_at, s.opened_at, s.updated_at AS state_updated_at FROM alert_rules r LEFT JOIN targets t ON t.id = r.target_id LEFT JOIN target_groups g ON g.id = r.group_id LEFT JOIN alert_subject_states s ON s.rule_id = r.id AND ((r.target_id IS NOT NULL AND s.subject_kind = 'target' AND s.subject_id = r.target_id) OR (r.group_id IS NOT NULL AND s.subject_kind = 'group' AND s.subject_id = r.group_id)) ORDER BY r.created_at",
        )
        .fetch_all(self.pool())
        .await
        .map_err(map_error)?;
        let mut views = Vec::with_capacity(rows.len());
        for row in rows {
            let id: Uuid = row.get("id");
            let rule = restore_rule(&row)?;
            let target_id = rule.target_id().map(|id| id.as_uuid());
            let group_id = rule.group_id().map(|id| id.as_uuid());
            let subject_kind = if target_id.is_some() {
                "target"
            } else {
                "group"
            };
            let subject_id = target_id.or(group_id).ok_or(PortError::InvalidData)?;
            let state = restore_state(&row, id, subject_kind, subject_id)?;
            let channels = rule_channels(self, id).await?;
            let target_name: Option<String> = row.get("target_name");
            let target_kind: Option<String> = row.get("target_kind");
            let spec: Option<Json<Value>> = row.try_get("spec_json").ok();
            views.push(RuleView {
                id,
                rule,
                state,
                channels,
                target_ref: target_name
                    .as_ref()
                    .zip(target_kind.as_ref())
                    .zip(spec.as_ref())
                    .map(|((_, kind), spec)| target_ref(kind, &spec.0)),
                target_name,
                target_kind,
                group_name: row.get("group_name"),
            });
        }
        Ok(views)
    }

    async fn save_rule(
        &self,
        rule: &AlertRuleRecord,
        channel_ids: &[Uuid],
    ) -> Result<Uuid, PortError> {
        let mut tx = self.pool().begin().await.map_err(map_error)?;
        let target_id = rule.rule.target_id().map(|id| id.as_uuid());
        let group_id = rule.rule.group_id().map(|id| id.as_uuid());
        let existing: Option<Uuid> = if let Some(target_id) = target_id {
            sqlx::query_scalar("SELECT id FROM alert_rules WHERE target_id = $1")
                .bind(target_id)
                .fetch_optional(&mut *tx)
                .await
                .map_err(map_error)?
        } else if let Some(group_id) = group_id {
            sqlx::query_scalar("SELECT id FROM alert_rules WHERE group_id = $1")
                .bind(group_id)
                .fetch_optional(&mut *tx)
                .await
                .map_err(map_error)?
        } else {
            None
        };
        let id = existing.unwrap_or(rule.id);
        if let Some(target_id) = target_id {
            sqlx::query(
                "INSERT INTO alert_rules (id, target_id, enabled, mode, failure_threshold, recovery_threshold, cooldown_secs, created_at, updated_at) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9) ON CONFLICT (target_id) DO UPDATE SET enabled = EXCLUDED.enabled, mode = EXCLUDED.mode, failure_threshold = EXCLUDED.failure_threshold, recovery_threshold = EXCLUDED.recovery_threshold, cooldown_secs = EXCLUDED.cooldown_secs, updated_at = EXCLUDED.updated_at",
            )
            .bind(id)
            .bind(target_id)
            .bind(rule.rule.is_enabled())
            .bind(rule.rule.mode().as_str())
            .bind(i32::from(rule.rule.failure_threshold()))
            .bind(i32::from(rule.rule.recovery_threshold()))
            .bind(i32::try_from(rule.rule.cooldown_secs()).map_err(|_| PortError::InvalidData)?)
            .bind(rule.rule.created_at())
            .bind(rule.rule.updated_at())
            .execute(&mut *tx)
            .await
            .map_err(map_error)?;
        } else if let Some(group_id) = group_id {
            sqlx::query(
                "INSERT INTO alert_rules (id, group_id, enabled, mode, failure_threshold, recovery_threshold, cooldown_secs, created_at, updated_at) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9) ON CONFLICT (group_id) WHERE group_id IS NOT NULL DO UPDATE SET enabled = EXCLUDED.enabled, mode = EXCLUDED.mode, failure_threshold = EXCLUDED.failure_threshold, recovery_threshold = EXCLUDED.recovery_threshold, cooldown_secs = EXCLUDED.cooldown_secs, updated_at = EXCLUDED.updated_at",
            )
            .bind(id)
            .bind(group_id)
            .bind(rule.rule.is_enabled())
            .bind(rule.rule.mode().as_str())
            .bind(i32::from(rule.rule.failure_threshold()))
            .bind(i32::from(rule.rule.recovery_threshold()))
            .bind(i32::try_from(rule.rule.cooldown_secs()).map_err(|_| PortError::InvalidData)?)
            .bind(rule.rule.created_at())
            .bind(rule.rule.updated_at())
            .execute(&mut *tx)
            .await
            .map_err(map_error)?;
        } else {
            return Err(PortError::InvalidData);
        }
        sqlx::query(
            "INSERT INTO alert_subject_states (rule_id, subject_kind, subject_id, status, consecutive_failures, consecutive_successes, updated_at) VALUES ($1, $2, $3, 'ok', 0, 0, $4) ON CONFLICT (rule_id, subject_kind, subject_id) DO NOTHING",
        )
        .bind(id)
        .bind(rule.rule.scope().kind())
        .bind(rule.rule.scope().subject_id())
        .bind(rule.rule.updated_at())
        .execute(&mut *tx)
        .await
        .map_err(map_error)?;
        sqlx::query("DELETE FROM alert_rule_channels WHERE rule_id = $1")
            .bind(id)
            .execute(&mut *tx)
            .await
            .map_err(map_error)?;
        for channel_id in channel_ids {
            sqlx::query(
                "INSERT INTO alert_rule_channels (rule_id, channel_id) VALUES ($1, $2) ON CONFLICT DO NOTHING",
            )
            .bind(id)
            .bind(channel_id)
            .execute(&mut *tx)
            .await
            .map_err(map_error)?;
        }
        tx.commit().await.map_err(map_error)?;
        Ok(id)
    }

    async fn delete_rule_for_target(&self, target_id: TargetId) -> Result<bool, PortError> {
        let result = sqlx::query("DELETE FROM alert_rules WHERE target_id = $1")
            .bind(target_id.as_uuid())
            .execute(self.pool())
            .await
            .map_err(map_error)?;
        Ok(result.rows_affected() > 0)
    }

    async fn delete_rule(&self, id: Uuid) -> Result<bool, PortError> {
        let result = sqlx::query("DELETE FROM alert_rules WHERE id = $1")
            .bind(id)
            .execute(self.pool())
            .await
            .map_err(map_error)?;
        Ok(result.rows_affected() > 0)
    }

    async fn snapshots(&self) -> Result<Vec<RuleSnapshot>, PortError> {
        let rows = sqlx::query(
            "SELECT r.id, r.target_id, r.group_id, r.enabled, r.mode, r.failure_threshold, r.recovery_threshold, r.cooldown_secs, r.created_at, r.updated_at, t.name AS target_name, t.kind AS target_kind, t.spec_json, t.interval_secs, g.name AS group_name FROM alert_rules r LEFT JOIN targets t ON t.id = r.target_id LEFT JOIN target_groups g ON g.id = r.group_id WHERE r.enabled ORDER BY r.created_at",
        )
        .fetch_all(self.pool())
        .await
        .map_err(map_error)?;
        let target_rule_ids: Vec<Uuid> =
            sqlx::query_scalar("SELECT target_id FROM alert_rules WHERE target_id IS NOT NULL")
                .fetch_all(self.pool())
                .await
                .map_err(map_error)?;
        let mut snapshots = Vec::new();
        for row in rows {
            let rule_id: Uuid = row.get("id");
            let rule = restore_rule(&row)?;
            let channels = rule_channels(self, rule_id).await?;
            if let Some(target_id) = rule.target_id() {
                let state = subject_state(self, rule_id, "target", target_id.as_uuid()).await?;
                let agents = target_agents(self, target_id.as_uuid()).await?;
                let target_kind: String = row.get("target_kind");
                let spec: Json<Value> = row.get("spec_json");
                snapshots.push(RuleSnapshot {
                    rule_id,
                    rule,
                    state,
                    channels,
                    interval_secs: u32::try_from(row.get::<i32, _>("interval_secs"))
                        .map_err(|_| PortError::InvalidData)?,
                    subject_kind: "target",
                    target_name: row.get("target_name"),
                    target_kind: Some(target_kind.clone()),
                    target_ref: Some(target_ref(&target_kind, &spec.0)),
                    group_name: None,
                    group_targets: Vec::new(),
                    agents,
                });
                continue;
            }
            let Some(group_id) = rule.group_id() else {
                continue;
            };
            let group_name: Option<String> = row.get("group_name");
            let group_targets = group_targets(self, group_id).await?;
            let state = subject_state(self, rule_id, "group", group_id.as_uuid()).await?;
            let interval_secs = group_targets
                .iter()
                .map(|target| target.interval_secs)
                .min()
                .unwrap_or(30);
            snapshots.push(RuleSnapshot {
                rule_id,
                rule: rule.clone(),
                state,
                channels: channels.clone(),
                interval_secs,
                subject_kind: "group",
                target_name: None,
                target_kind: None,
                target_ref: None,
                group_name: group_name.clone(),
                group_targets: group_targets
                    .iter()
                    .map(|target| GroupTargetResult {
                        id: target.id,
                        name: target.name.clone(),
                        ok: target.ok,
                        checked_at: target.checked_at,
                    })
                    .collect(),
                agents: Vec::new(),
            });
            for target in group_targets {
                if target_rule_ids.contains(&target.id) {
                    continue;
                }
                let state = subject_state(self, rule_id, "target", target.id).await?;
                let agents = target_agents(self, target.id).await?;
                let target_kind = target.kind.clone();
                snapshots.push(RuleSnapshot {
                    rule_id,
                    rule: rule.clone(),
                    state,
                    channels: channels.clone(),
                    interval_secs: target.interval_secs,
                    subject_kind: "target",
                    target_name: Some(target.name),
                    target_kind: Some(target_kind.clone()),
                    target_ref: Some(target_ref(&target_kind, &target.spec_json.0)),
                    group_name: group_name.clone(),
                    group_targets: Vec::new(),
                    agents,
                });
            }
        }
        Ok(snapshots)
    }

    async fn apply_transition(
        &self,
        state: &AlertState,
        drafts: &[OutboxDraft],
    ) -> Result<(), PortError> {
        let mut tx = self.pool().begin().await.map_err(map_error)?;
        sqlx::query(
            "INSERT INTO alert_subject_states (rule_id, subject_kind, subject_id, status, consecutive_failures, consecutive_successes, last_result_at, last_notified_at, opened_at, updated_at) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10) ON CONFLICT (rule_id, subject_kind, subject_id) DO UPDATE SET status = EXCLUDED.status, consecutive_failures = EXCLUDED.consecutive_failures, consecutive_successes = EXCLUDED.consecutive_successes, last_result_at = EXCLUDED.last_result_at, last_notified_at = EXCLUDED.last_notified_at, opened_at = EXCLUDED.opened_at, updated_at = EXCLUDED.updated_at",
        )
        .bind(state.rule_id)
        .bind(state.subject_kind)
        .bind(state.subject_id)
        .bind(state.status.as_str())
        .bind(i32::from(state.failures))
        .bind(i32::from(state.successes))
        .bind(state.last_result_at)
        .bind(state.last_notified_at)
        .bind(state.opened_at)
        .bind(state.updated_at)
        .execute(&mut *tx)
        .await
        .map_err(map_error)?;
        for draft in drafts {
            sqlx::query(
                "INSERT INTO notification_outbox (id, dedupe_key, rule_id, channel_id, target_id, group_id, event, payload, status, attempts, next_attempt_at, created_at) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,'pending',0,$9,$9) ON CONFLICT (dedupe_key) DO NOTHING",
            )
            .bind(Uuid::new_v4())
            .bind(&draft.dedupe_key)
            .bind(state.rule_id)
            .bind(draft.channel_id)
            .bind(draft.target_id)
            .bind(draft.group_id)
            .bind(draft.event.as_str())
            .bind(Json(&draft.payload))
            .bind(state.updated_at)
            .execute(&mut *tx)
            .await
            .map_err(map_error)?;
        }
        tx.commit().await.map_err(map_error)?;
        Ok(())
    }

    async fn due_outbox(&self, limit: u16) -> Result<Vec<OutboxItem>, PortError> {
        let mut tx = self.pool().begin().await.map_err(map_error)?;
        let rows = sqlx::query(
            "SELECT o.id, o.channel_id, c.name AS channel_name, c.kind, c.config_encrypted, o.event, o.payload, o.attempts FROM notification_outbox o JOIN notification_channels c ON c.id = o.channel_id WHERE (o.status = 'pending' OR (o.status = 'sending' AND o.next_attempt_at <= now() - interval '1 minute')) AND o.next_attempt_at <= now() ORDER BY o.next_attempt_at LIMIT $1 FOR UPDATE OF o SKIP LOCKED",
        )
        .bind(i64::from(limit))
        .fetch_all(&mut *tx)
        .await
        .map_err(map_error)?;
        if !rows.is_empty() {
            let ids: Vec<Uuid> = rows.iter().map(|row| row.get("id")).collect();
            sqlx::query("UPDATE notification_outbox SET status = 'sending' WHERE id = ANY($1)")
                .bind(&ids)
                .execute(&mut *tx)
                .await
                .map_err(map_error)?;
        }
        tx.commit().await.map_err(map_error)?;
        rows.into_iter()
            .map(|row| {
                let payload: Json<Value> = row.get("payload");
                Ok(OutboxItem {
                    id: row.get("id"),
                    channel_id: row.get("channel_id"),
                    channel_name: row.get("channel_name"),
                    kind: kind(row.get::<String, _>("kind").as_str()),
                    config_encrypted: row.get("config_encrypted"),
                    event: AlertEvent::try_from(row.get::<String, _>("event").as_str())
                        .map_err(|_| PortError::InvalidData)?,
                    payload: payload.0,
                    attempts: u16::try_from(row.get::<i32, _>("attempts"))
                        .map_err(|_| PortError::InvalidData)?,
                })
            })
            .collect()
    }

    async fn mark_outbox_sent(&self, id: Uuid) -> Result<(), PortError> {
        sqlx::query("UPDATE notification_outbox SET status = 'sent', sent_at = now(), last_error = NULL WHERE id = $1")
            .bind(id)
            .execute(self.pool())
            .await
            .map_err(map_error)?;
        Ok(())
    }

    async fn mark_outbox_retry(
        &self,
        id: Uuid,
        attempts: u16,
        next_attempt_at: Timestamp,
        error: &str,
    ) -> Result<(), PortError> {
        sqlx::query("UPDATE notification_outbox SET status = 'pending', attempts = $2, next_attempt_at = $3, last_error = $4 WHERE id = $1")
            .bind(id)
            .bind(i32::from(attempts))
            .bind(next_attempt_at)
            .bind(error)
            .execute(self.pool())
            .await
            .map_err(map_error)?;
        Ok(())
    }

    async fn mark_outbox_failed(
        &self,
        id: Uuid,
        attempts: u16,
        error: &str,
    ) -> Result<(), PortError> {
        sqlx::query("UPDATE notification_outbox SET status = 'failed', attempts = $2, last_error = $3 WHERE id = $1")
            .bind(id)
            .bind(i32::from(attempts))
            .bind(error)
            .execute(self.pool())
            .await
            .map_err(map_error)?;
        Ok(())
    }

    async fn list_events(&self, limit: u16) -> Result<Vec<OutboxEvent>, PortError> {
        let rows = sqlx::query(
            "SELECT o.id, o.event, t.name AS target_name, g.name AS group_name, c.name AS channel_name, o.status, o.attempts, o.last_error, o.created_at, o.sent_at FROM notification_outbox o LEFT JOIN targets t ON t.id = o.target_id LEFT JOIN target_groups g ON g.id = o.group_id JOIN notification_channels c ON c.id = o.channel_id ORDER BY o.created_at DESC LIMIT $1",
        )
        .bind(i64::from(limit))
        .fetch_all(self.pool())
        .await
        .map_err(map_error)?;
        rows.into_iter()
            .map(|row| {
                Ok(OutboxEvent {
                    id: row.get("id"),
                    event: AlertEvent::try_from(row.get::<String, _>("event").as_str())
                        .map_err(|_| PortError::InvalidData)?,
                    target_name: row.get("target_name"),
                    group_name: row.get("group_name"),
                    channel_name: row.get("channel_name"),
                    status: row.get("status"),
                    attempts: u16::try_from(row.get::<i32, _>("attempts"))
                        .map_err(|_| PortError::InvalidData)?,
                    last_error: row.get("last_error"),
                    created_at: row.get("created_at"),
                    sent_at: row.get("sent_at"),
                })
            })
            .collect()
    }

    async fn retry_event(&self, id: Uuid) -> Result<bool, PortError> {
        let result = sqlx::query(
            "UPDATE notification_outbox SET status = 'pending', next_attempt_at = now(), last_error = NULL WHERE id = $1 AND status = 'failed'",
        )
        .bind(id)
        .execute(self.pool())
        .await
        .map_err(map_error)?;
        Ok(result.rows_affected() > 0)
    }

    async fn prune_events(&self, before: Timestamp, limit: u16) -> Result<u16, PortError> {
        let result = sqlx::query(
            "DELETE FROM notification_outbox WHERE ctid IN (SELECT ctid FROM notification_outbox WHERE created_at < $1 LIMIT $2)",
        )
        .bind(before)
        .bind(i64::from(limit))
        .execute(self.pool())
        .await
        .map_err(map_error)?;
        Ok(u16::try_from(result.rows_affected()).unwrap_or(u16::MAX))
    }
}

struct GroupTarget {
    id: Uuid,
    name: String,
    kind: String,
    spec_json: Json<Value>,
    interval_secs: u32,
    ok: bool,
    checked_at: OffsetDateTime,
}

async fn group_targets(
    store: &PgStore,
    group_id: TargetGroupId,
) -> Result<Vec<GroupTarget>, PortError> {
    let rows = sqlx::query(
        "SELECT t.id, t.name, t.kind, t.spec_json, t.interval_secs, bool_and(cr.ok) FILTER (WHERE cr.checked_at IS NOT NULL) AS ok, max(cr.checked_at) AS checked_at FROM targets t JOIN target_agents ta ON ta.target_id = t.id LEFT JOIN LATERAL (SELECT ok, checked_at FROM check_results WHERE target_id = t.id AND agent_id = ta.agent_id ORDER BY checked_at DESC LIMIT 1) cr ON TRUE WHERE t.group_id = $1 AND t.enabled GROUP BY t.id, t.name, t.kind, t.spec_json, t.interval_secs HAVING max(cr.checked_at) IS NOT NULL ORDER BY t.name, t.id",
    )
    .bind(group_id.as_uuid())
    .fetch_all(store.pool())
    .await
    .map_err(map_error)?;
    rows.into_iter()
        .map(|row| {
            Ok(GroupTarget {
                id: row.get("id"),
                name: row.get("name"),
                kind: row.get("kind"),
                spec_json: row.get("spec_json"),
                interval_secs: u32::try_from(row.get::<i32, _>("interval_secs"))
                    .map_err(|_| PortError::InvalidData)?,
                ok: row.get::<Option<bool>, _>("ok").unwrap_or(false),
                checked_at: row.get("checked_at"),
            })
        })
        .collect()
}
