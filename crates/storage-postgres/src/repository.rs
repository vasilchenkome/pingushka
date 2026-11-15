use std::collections::HashMap;

use async_trait::async_trait;
use pingushka_application::{
    error::PortError,
    ports::{
        AgentEnrollmentRepository, AgentRepository, CheckResultRepository, DeleteGroupOutcome,
        HealthCheck, SessionRepository, TargetGroupRepository, TargetRepository, UserRepository,
    },
    queries::{CheckSummary, HistoryFilter, SummaryFilter, TargetWithAgents},
};
use pingushka_domain::{
    Timestamp,
    agent::Agent,
    check::CheckResult,
    enrollment::AgentEnrollment,
    identity::{AgentId, TargetGroupId, TargetId, TokenDigest, UserId},
    organization::{TargetGroup, TargetGroupName, TargetGroupWithCounts, TargetTag, TargetTagSet},
    target::Target,
    user::{Session, User, Username},
};
use sqlx::{PgPool, Postgres, QueryBuilder, Transaction, types::Json};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{
    PgStore,
    mapper::{
        AgentRow, CheckResultRow, SessionRow, StoredTargetSpec, TargetGroupRow, TargetRow, UserRow,
        target_kind_from_spec,
    },
};

fn map_database_error(error: sqlx::Error) -> PortError {
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

fn row_to_domain<Source, Domain>(row: Source) -> Result<Domain, PortError>
where
    Source: TryInto<Domain, Error = PortError>,
{
    row.try_into()
}

async fn load_tags(
    pool: &PgPool,
    target_ids: &[Uuid],
) -> Result<HashMap<Uuid, TargetTagSet>, PortError> {
    if target_ids.is_empty() {
        return Ok(HashMap::new());
    }
    let rows = sqlx::query_as::<_, (Uuid, String)>(
        "SELECT target_id, tag FROM target_tags WHERE target_id = ANY($1) ORDER BY tag",
    )
    .bind(target_ids)
    .fetch_all(pool)
    .await
    .map_err(map_database_error)?;
    let mut grouped: HashMap<Uuid, Vec<String>> = HashMap::new();
    for (target_id, tag) in rows {
        grouped.entry(target_id).or_default().push(tag);
    }
    grouped
        .into_iter()
        .map(|(target_id, tags)| {
            TargetTagSet::parse(tags)
                .map(|set| (target_id, set))
                .map_err(|_| PortError::InvalidData)
        })
        .collect()
}

async fn load_targets(pool: &PgPool, rows: Vec<TargetRow>) -> Result<Vec<Target>, PortError> {
    let ids: Vec<Uuid> = rows.iter().map(|row| row.id).collect();
    let mut tags = load_tags(pool, &ids).await?;
    rows.into_iter()
        .map(|row| {
            let tags = tags.remove(&row.id).unwrap_or_else(TargetTagSet::empty);
            row.with_tags(tags)
        })
        .collect()
}

async fn replace_tags(
    transaction: &mut Transaction<'_, Postgres>,
    target_id: Uuid,
    tags: &[TargetTag],
) -> Result<(), PortError> {
    sqlx::query("DELETE FROM target_tags WHERE target_id = $1")
        .bind(target_id)
        .execute(&mut **transaction)
        .await
        .map_err(map_database_error)?;
    if tags.is_empty() {
        return Ok(());
    }
    let mut query = QueryBuilder::<Postgres>::new("INSERT INTO target_tags (target_id, tag) ");
    query.push_values(tags, |mut values, tag| {
        values.push_bind(target_id).push_bind(tag.as_str());
    });
    query
        .build()
        .execute(&mut **transaction)
        .await
        .map_err(map_database_error)?;
    Ok(())
}

#[async_trait]
impl HealthCheck for PgStore {
    async fn check(&self) -> Result<(), PortError> {
        sqlx::query_scalar::<_, i32>("SELECT 1")
            .fetch_one(self.pool())
            .await
            .map_err(map_database_error)?;
        Ok(())
    }
}

#[async_trait]
impl TargetRepository for PgStore {
    async fn save(&self, target: &Target) -> Result<(), PortError> {
        let spec = StoredTargetSpec::from_domain(target.spec());
        let kind = target_kind_from_spec(target.spec());
        let interval_secs =
            i32::try_from(target.interval().get()).map_err(|_| PortError::InvalidData)?;
        let timeout_ms =
            i32::try_from(target.timeout().get()).map_err(|_| PortError::InvalidData)?;
        let mut transaction = self.pool().begin().await.map_err(map_database_error)?;
        sqlx::query(
            "INSERT INTO targets (id, name, kind, spec_json, interval_secs, timeout_ms, enabled, group_id, created_at, updated_at) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10) ON CONFLICT (id) DO UPDATE SET name = EXCLUDED.name, kind = EXCLUDED.kind, spec_json = EXCLUDED.spec_json, interval_secs = EXCLUDED.interval_secs, timeout_ms = EXCLUDED.timeout_ms, enabled = EXCLUDED.enabled, group_id = EXCLUDED.group_id, updated_at = EXCLUDED.updated_at",
        )
        .bind(target.id().as_uuid())
        .bind(target.name().as_str())
        .bind(kind)
        .bind(Json(spec))
        .bind(interval_secs)
        .bind(timeout_ms)
        .bind(target.is_enabled())
        .bind(target.group_id().map(TargetGroupId::as_uuid))
        .bind(target.created_at())
        .bind(target.updated_at())
        .execute(&mut *transaction)
        .await
        .map_err(map_database_error)?;
        replace_tags(&mut transaction, target.id().as_uuid(), target.tags()).await?;
        transaction.commit().await.map_err(map_database_error)
    }

    async fn find(&self, id: TargetId) -> Result<Option<Target>, PortError> {
        let row = sqlx::query_as::<_, TargetRow>(
            "SELECT id, name, kind, spec_json, interval_secs, timeout_ms, enabled, group_id, created_at, updated_at FROM targets WHERE id = $1",
        )
        .bind(id.as_uuid())
        .fetch_optional(self.pool())
        .await
        .map_err(map_database_error)?;
        let Some(row) = row else {
            return Ok(None);
        };
        let tags = load_tags(self.pool(), &[row.id]).await?;
        let tags = tags
            .get(&row.id)
            .cloned()
            .unwrap_or_else(TargetTagSet::empty);
        row.with_tags(tags).map(Some)
    }

    async fn list(&self) -> Result<Vec<Target>, PortError> {
        let rows = sqlx::query_as::<_, TargetRow>(
            "SELECT id, name, kind, spec_json, interval_secs, timeout_ms, enabled, group_id, created_at, updated_at FROM targets ORDER BY created_at, id",
        )
        .fetch_all(self.pool())
        .await
        .map_err(map_database_error)?;
        load_targets(self.pool(), rows).await
    }

    async fn list_with_agents(&self) -> Result<Vec<TargetWithAgents>, PortError> {
        let targets = <Self as TargetRepository>::list(self).await?;
        let rows = sqlx::query_as::<_, (Uuid, Uuid)>(
            "SELECT target_id, agent_id FROM target_agents ORDER BY assigned_at, agent_id",
        )
        .fetch_all(self.pool())
        .await
        .map_err(map_database_error)?;
        let mut assignments: HashMap<Uuid, Vec<AgentId>> = HashMap::new();
        for (target_id, agent_id) in rows {
            assignments
                .entry(target_id)
                .or_default()
                .push(AgentId::from_uuid(agent_id));
        }
        Ok(targets
            .into_iter()
            .map(|target| {
                let agent_ids = assignments
                    .remove(&target.id().as_uuid())
                    .unwrap_or_default();
                TargetWithAgents::new(target, agent_ids)
            })
            .collect())
    }

    async fn agent_ids(&self, target_id: TargetId) -> Result<Vec<AgentId>, PortError> {
        let rows = sqlx::query_scalar::<_, Uuid>(
            "SELECT agent_id FROM target_agents WHERE target_id = $1 ORDER BY assigned_at, agent_id",
        )
        .bind(target_id.as_uuid())
        .fetch_all(self.pool())
        .await
        .map_err(map_database_error)?;
        Ok(rows.into_iter().map(AgentId::from_uuid).collect())
    }

    async fn list_for_agent(&self, agent_id: AgentId) -> Result<Vec<Target>, PortError> {
        let rows = sqlx::query_as::<_, TargetRow>(
            "SELECT t.id, t.name, t.kind, t.spec_json, t.interval_secs, t.timeout_ms, t.enabled, t.group_id, t.created_at, t.updated_at FROM targets t JOIN target_agents ta ON ta.target_id = t.id WHERE ta.agent_id = $1 AND t.enabled = TRUE ORDER BY t.created_at, t.id",
        )
        .bind(agent_id.as_uuid())
        .fetch_all(self.pool())
        .await
        .map_err(map_database_error)?;
        load_targets(self.pool(), rows).await
    }

    async fn assign_agents(
        &self,
        target_id: TargetId,
        agent_ids: &[AgentId],
    ) -> Result<(), PortError> {
        let mut transaction = self.pool().begin().await.map_err(map_database_error)?;
        let exists =
            sqlx::query_scalar::<_, bool>("SELECT EXISTS(SELECT 1 FROM targets WHERE id = $1)")
                .bind(target_id.as_uuid())
                .fetch_one(&mut *transaction)
                .await
                .map_err(map_database_error)?;
        if !exists {
            return Err(PortError::NotFound);
        }
        sqlx::query("DELETE FROM target_agents WHERE target_id = $1")
            .bind(target_id.as_uuid())
            .execute(&mut *transaction)
            .await
            .map_err(map_database_error)?;
        let mut unique_agents = std::collections::HashSet::new();
        let assignments = agent_ids
            .iter()
            .filter(|agent_id| unique_agents.insert(agent_id.as_uuid()))
            .collect::<Vec<_>>();
        if !assignments.is_empty() {
            let now = OffsetDateTime::now_utc();
            let mut query = QueryBuilder::<Postgres>::new(
                "INSERT INTO target_agents (target_id, agent_id, assigned_at) ",
            );
            query.push_values(assignments, |mut values, agent_id| {
                values
                    .push_bind(target_id.as_uuid())
                    .push_bind(agent_id.as_uuid())
                    .push_bind(now);
            });
            query.push(" ON CONFLICT (target_id, agent_id) DO NOTHING");
            query
                .build()
                .execute(&mut *transaction)
                .await
                .map_err(map_database_error)?;
        }
        transaction.commit().await.map_err(map_database_error)
    }

    async fn find_many(&self, ids: &[TargetId]) -> Result<Vec<Target>, PortError> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let uuids: Vec<Uuid> = ids.iter().map(|id| id.as_uuid()).collect();
        let rows = sqlx::query_as::<_, TargetRow>(
            "SELECT id, name, kind, spec_json, interval_secs, timeout_ms, enabled, group_id, created_at, updated_at FROM targets WHERE id = ANY($1) ORDER BY created_at, id",
        )
        .bind(uuids)
        .fetch_all(self.pool())
        .await
        .map_err(map_database_error)?;
        load_targets(self.pool(), rows).await
    }

    async fn save_organizations(
        &self,
        updates: &[(TargetId, pingushka_domain::organization::TargetOrganization)],
    ) -> Result<(), PortError> {
        let mut transaction = self.pool().begin().await.map_err(map_database_error)?;
        for (target_id, organization) in updates {
            let result =
                sqlx::query("UPDATE targets SET group_id = $2, updated_at = $3 WHERE id = $1")
                    .bind(target_id.as_uuid())
                    .bind(organization.group_id().map(TargetGroupId::as_uuid))
                    .bind(OffsetDateTime::now_utc())
                    .execute(&mut *transaction)
                    .await
                    .map_err(map_database_error)?;
            if result.rows_affected() == 0 {
                return Err(PortError::NotFound);
            }
            replace_tags(&mut transaction, target_id.as_uuid(), organization.tags()).await?;
        }
        transaction.commit().await.map_err(map_database_error)
    }

    async fn delete(&self, id: TargetId) -> Result<bool, PortError> {
        let result = sqlx::query("DELETE FROM targets WHERE id = $1")
            .bind(id.as_uuid())
            .execute(self.pool())
            .await
            .map_err(map_database_error)?;
        Ok(result.rows_affected() > 0)
    }
}

#[async_trait]
impl TargetGroupRepository for PgStore {
    async fn save(&self, group: &TargetGroup) -> Result<(), PortError> {
        sqlx::query(
            "INSERT INTO target_groups (id, name, created_at, updated_at) VALUES ($1, $2, $3, $4) ON CONFLICT (id) DO UPDATE SET name = EXCLUDED.name, updated_at = EXCLUDED.updated_at",
        )
        .bind(group.id().as_uuid())
        .bind(group.name().as_str())
        .bind(group.created_at())
        .bind(group.updated_at())
        .execute(self.pool())
        .await
        .map_err(map_database_error)?;
        Ok(())
    }

    async fn find(&self, id: TargetGroupId) -> Result<Option<TargetGroup>, PortError> {
        let row = sqlx::query_as::<_, (Uuid, String, OffsetDateTime, OffsetDateTime)>(
            "SELECT id, name, created_at, updated_at FROM target_groups WHERE id = $1",
        )
        .bind(id.as_uuid())
        .fetch_optional(self.pool())
        .await
        .map_err(map_database_error)?;
        row.map(|(id, name, created_at, updated_at)| {
            TargetGroup::restore(
                TargetGroupId::from_uuid(id),
                TargetGroupName::new(name).map_err(|_| PortError::InvalidData)?,
                created_at,
                updated_at,
            )
            .map_err(|_| PortError::InvalidData)
        })
        .transpose()
    }

    async fn find_by_name(&self, name: &TargetGroupName) -> Result<Option<TargetGroup>, PortError> {
        let row = sqlx::query_as::<_, (Uuid, String, OffsetDateTime, OffsetDateTime)>(
            "SELECT id, name, created_at, updated_at FROM target_groups WHERE lower(name) = lower($1)",
        )
        .bind(name.as_str())
        .fetch_optional(self.pool())
        .await
        .map_err(map_database_error)?;
        row.map(|(id, name, created_at, updated_at)| {
            TargetGroup::restore(
                TargetGroupId::from_uuid(id),
                TargetGroupName::new(name).map_err(|_| PortError::InvalidData)?,
                created_at,
                updated_at,
            )
            .map_err(|_| PortError::InvalidData)
        })
        .transpose()
    }

    async fn list_with_counts(&self) -> Result<Vec<TargetGroupWithCounts>, PortError> {
        let rows = sqlx::query_as::<_, TargetGroupRow>(
            "SELECT g.id, g.name, g.created_at, g.updated_at, (SELECT COUNT(*) FROM targets t WHERE t.group_id = g.id)::BIGINT AS target_count, (SELECT COUNT(*) FROM alert_rules r WHERE r.group_id = g.id)::BIGINT AS rule_count FROM target_groups g ORDER BY lower(g.name), g.id",
        )
        .fetch_all(self.pool())
        .await
        .map_err(map_database_error)?;
        rows.into_iter().map(TryInto::try_into).collect()
    }

    async fn delete(&self, id: TargetGroupId) -> Result<DeleteGroupOutcome, PortError> {
        let mut transaction = self.pool().begin().await.map_err(map_database_error)?;
        let exists = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM target_groups WHERE id = $1)",
        )
        .bind(id.as_uuid())
        .fetch_one(&mut *transaction)
        .await
        .map_err(map_database_error)?;
        if !exists {
            return Ok(DeleteGroupOutcome::NotFound);
        }
        let has_rules = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM alert_rules WHERE group_id = $1)",
        )
        .bind(id.as_uuid())
        .fetch_one(&mut *transaction)
        .await
        .map_err(map_database_error)?;
        if has_rules {
            return Ok(DeleteGroupOutcome::HasRules);
        }
        sqlx::query("DELETE FROM target_groups WHERE id = $1")
            .bind(id.as_uuid())
            .execute(&mut *transaction)
            .await
            .map_err(map_database_error)?;
        transaction.commit().await.map_err(map_database_error)?;
        Ok(DeleteGroupOutcome::Deleted)
    }
}

#[async_trait]
impl AgentRepository for PgStore {
    async fn save(&self, agent: &Agent) -> Result<(), PortError> {
        let token_hash = agent
            .token_digest()
            .map(|digest| digest.as_bytes().to_vec());
        sqlx::query(
            "INSERT INTO agents (id, name, token_hash, created_at, last_seen_at, connected, version) VALUES ($1, $2, $3, $4, $5, $6, $7) ON CONFLICT (id) DO UPDATE SET name = EXCLUDED.name, token_hash = EXCLUDED.token_hash, last_seen_at = EXCLUDED.last_seen_at, connected = EXCLUDED.connected, version = EXCLUDED.version",
        )
        .bind(agent.id().as_uuid())
        .bind(agent.name().as_str())
        .bind(token_hash)
        .bind(agent.created_at())
        .bind(agent.last_seen_at())
        .bind(agent.is_connected())
        .bind(agent.version())
        .execute(self.pool())
        .await
        .map_err(map_database_error)?;
        Ok(())
    }

    async fn find(&self, id: AgentId) -> Result<Option<Agent>, PortError> {
        let row = sqlx::query_as::<_, AgentRow>(
            "SELECT id, name, token_hash, created_at, last_seen_at, connected, version FROM agents WHERE id = $1",
        )
        .bind(id.as_uuid())
        .fetch_optional(self.pool())
        .await
        .map_err(map_database_error)?;
        row.map(row_to_domain).transpose()
    }

    async fn find_by_token_digest(&self, digest: TokenDigest) -> Result<Option<Agent>, PortError> {
        let row = sqlx::query_as::<_, AgentRow>(
            "SELECT id, name, token_hash, created_at, last_seen_at, connected, version FROM agents WHERE token_hash = $1",
        )
        .bind(digest.as_bytes().to_vec())
        .fetch_optional(self.pool())
        .await
        .map_err(map_database_error)?;
        row.map(row_to_domain).transpose()
    }

    async fn list(&self) -> Result<Vec<Agent>, PortError> {
        let rows = sqlx::query_as::<_, AgentRow>(
            "SELECT id, name, token_hash, created_at, last_seen_at, connected, version FROM agents ORDER BY created_at, id",
        )
        .fetch_all(self.pool())
        .await
        .map_err(map_database_error)?;
        rows.into_iter().map(row_to_domain).collect()
    }

    async fn delete(&self, id: AgentId) -> Result<bool, PortError> {
        let result = sqlx::query("DELETE FROM agents WHERE id = $1")
            .bind(id.as_uuid())
            .execute(self.pool())
            .await
            .map_err(map_database_error)?;
        Ok(result.rows_affected() > 0)
    }

    async fn mark_connected(
        &self,
        id: AgentId,
        version: &str,
        now: Timestamp,
    ) -> Result<(), PortError> {
        let result = sqlx::query(
            "UPDATE agents SET connected = TRUE, version = $2, last_seen_at = $3 WHERE id = $1",
        )
        .bind(id.as_uuid())
        .bind(version)
        .bind(now)
        .execute(self.pool())
        .await
        .map_err(map_database_error)?;
        if result.rows_affected() == 0 {
            return Err(PortError::NotFound);
        }
        Ok(())
    }

    async fn mark_disconnected(&self, id: AgentId) -> Result<(), PortError> {
        sqlx::query("UPDATE agents SET connected = FALSE WHERE id = $1")
            .bind(id.as_uuid())
            .execute(self.pool())
            .await
            .map_err(map_database_error)?;
        Ok(())
    }
}

#[async_trait]
impl UserRepository for PgStore {
    async fn save(&self, user: &User) -> Result<(), PortError> {
        sqlx::query(
            "INSERT INTO users (id, username, password_hash, created_at) VALUES ($1, $2, $3, $4) ON CONFLICT (id) DO UPDATE SET username = EXCLUDED.username, password_hash = EXCLUDED.password_hash",
        )
        .bind(user.id().as_uuid())
        .bind(user.username().as_str())
        .bind(user.password_hash().as_encoded())
        .bind(user.created_at())
        .execute(self.pool())
        .await
        .map_err(map_database_error)?;
        Ok(())
    }

    async fn save_if_first(&self, user: &User) -> Result<bool, PortError> {
        let mut transaction = self.pool().begin().await.map_err(map_database_error)?;
        sqlx::query("SELECT pg_advisory_xact_lock($1)")
            .bind(7_301_234_567_i64)
            .execute(&mut *transaction)
            .await
            .map_err(map_database_error)?;
        let existing = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM users")
            .fetch_one(&mut *transaction)
            .await
            .map_err(map_database_error)?;
        if existing > 0 {
            return Ok(false);
        }
        sqlx::query(
            "INSERT INTO users (id, username, password_hash, created_at) VALUES ($1, $2, $3, $4)",
        )
        .bind(user.id().as_uuid())
        .bind(user.username().as_str())
        .bind(user.password_hash().as_encoded())
        .bind(user.created_at())
        .execute(&mut *transaction)
        .await
        .map_err(map_database_error)?;
        transaction.commit().await.map_err(map_database_error)?;
        Ok(true)
    }

    async fn find(&self, id: UserId) -> Result<Option<User>, PortError> {
        let row = sqlx::query_as::<_, UserRow>(
            "SELECT id, username, password_hash, created_at FROM users WHERE id = $1",
        )
        .bind(id.as_uuid())
        .fetch_optional(self.pool())
        .await
        .map_err(map_database_error)?;
        row.map(row_to_domain).transpose()
    }

    async fn find_by_username(&self, username: &Username) -> Result<Option<User>, PortError> {
        let row = sqlx::query_as::<_, UserRow>(
            "SELECT id, username, password_hash, created_at FROM users WHERE username = $1",
        )
        .bind(username.as_str())
        .fetch_optional(self.pool())
        .await
        .map_err(map_database_error)?;
        row.map(row_to_domain).transpose()
    }

    async fn count(&self) -> Result<u64, PortError> {
        let count = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM users")
            .fetch_one(self.pool())
            .await
            .map_err(map_database_error)?;
        u64::try_from(count).map_err(|_| PortError::InvalidData)
    }
}

#[async_trait]
impl SessionRepository for PgStore {
    async fn save(&self, session: &Session) -> Result<(), PortError> {
        sqlx::query(
            "INSERT INTO sessions (token_hash, user_id, created_at, expires_at) VALUES ($1, $2, $3, $4) ON CONFLICT (token_hash) DO UPDATE SET user_id = EXCLUDED.user_id, created_at = EXCLUDED.created_at, expires_at = EXCLUDED.expires_at",
        )
        .bind(session.token_digest().as_bytes().to_vec())
        .bind(session.user_id().as_uuid())
        .bind(session.created_at())
        .bind(session.expires_at())
        .execute(self.pool())
        .await
        .map_err(map_database_error)?;
        Ok(())
    }

    async fn find_active_by_digest(
        &self,
        digest: TokenDigest,
        now: Timestamp,
    ) -> Result<Option<Session>, PortError> {
        let row = sqlx::query_as::<_, SessionRow>(
            "SELECT token_hash, user_id, created_at, expires_at FROM sessions WHERE token_hash = $1 AND expires_at > $2",
        )
        .bind(digest.as_bytes().to_vec())
        .bind(now)
        .fetch_optional(self.pool())
        .await
        .map_err(map_database_error)?;
        row.map(row_to_domain).transpose()
    }

    async fn revoke(&self, digest: TokenDigest) -> Result<bool, PortError> {
        let result = sqlx::query("DELETE FROM sessions WHERE token_hash = $1")
            .bind(digest.as_bytes().to_vec())
            .execute(self.pool())
            .await
            .map_err(map_database_error)?;
        Ok(result.rows_affected() > 0)
    }

    async fn prune_expired(&self, now: Timestamp, limit: u16) -> Result<u16, PortError> {
        let rows = sqlx::query_scalar::<_, Vec<u8>>(
            "WITH expired AS (SELECT token_hash FROM sessions WHERE expires_at <= $1 ORDER BY expires_at LIMIT $2) DELETE FROM sessions WHERE token_hash IN (SELECT token_hash FROM expired) RETURNING token_hash",
        )
        .bind(now)
        .bind(i64::from(limit))
        .fetch_all(self.pool())
        .await
        .map_err(map_database_error)?;
        u16::try_from(rows.len()).map_err(|_| PortError::InvalidData)
    }
}

#[async_trait]
impl CheckResultRepository for PgStore {
    async fn record_batch(&self, results: &[CheckResult]) -> Result<usize, PortError> {
        if results.is_empty() {
            return Ok(0);
        }
        if results.len() > 100
            || results
                .iter()
                .any(|result| result.latency().get() > i32::MAX as u32)
        {
            return Err(PortError::InvalidData);
        }
        let mut transaction = self.pool().begin().await.map_err(map_database_error)?;
        let mut query = QueryBuilder::<Postgres>::new(
            "INSERT INTO check_results (result_id, target_id, agent_id, checked_at, ok, latency_ms, status_code, check_mode, error_kind, error_message) ",
        );
        query.push_values(results, |mut values, result| {
            values
                .push_bind(result.id().as_uuid())
                .push_bind(result.target_id().as_uuid())
                .push_bind(result.agent_id().as_uuid())
                .push_bind(result.checked_at())
                .push_bind(result.is_successful())
                .push_bind(result.latency().get() as i32)
                .push_bind(result.status_code().map(|code| code.get() as i16))
                .push_bind(result.mode().as_str())
                .push_bind(result.failure().map(|failure| failure.kind().as_str()))
                .push_bind(result.failure().map(|failure| failure.message()));
        });
        query.push(" ON CONFLICT (result_id) DO NOTHING");
        let result = query
            .build()
            .execute(&mut *transaction)
            .await
            .map_err(map_database_error)?;
        transaction.commit().await.map_err(map_database_error)?;
        usize::try_from(result.rows_affected()).map_err(|_| PortError::InvalidData)
    }

    async fn history(&self, filter: HistoryFilter) -> Result<Vec<CheckResult>, PortError> {
        let rows = sqlx::query_as::<_, CheckResultRow>(
            "SELECT result_id, target_id, agent_id, checked_at, ok, latency_ms, status_code, check_mode, error_kind, error_message FROM check_results WHERE target_id = $1 AND checked_at >= $2 AND checked_at < $3 AND ($4::UUID IS NULL OR agent_id = $4) ORDER BY checked_at DESC, result_id DESC LIMIT $5",
        )
        .bind(filter.target_id().as_uuid())
        .bind(filter.from())
        .bind(filter.to())
        .bind(filter.agent_id().map(AgentId::as_uuid))
        .bind(i64::from(filter.limit()))
        .fetch_all(self.pool())
        .await
        .map_err(map_database_error)?;
        rows.into_iter().map(row_to_domain).collect()
    }

    async fn summarize(&self, filter: SummaryFilter) -> Result<CheckSummary, PortError> {
        let (total, successful, average, p95) = sqlx::query_as::<_, (i64, i64, Option<f64>, Option<f64>)>(
            "SELECT COUNT(*)::BIGINT, COUNT(*) FILTER (WHERE ok)::BIGINT, AVG(latency_ms)::DOUBLE PRECISION, percentile_cont(0.95) WITHIN GROUP (ORDER BY latency_ms)::DOUBLE PRECISION FROM check_results WHERE checked_at >= $1 AND checked_at < $2 AND ($3::UUID IS NULL OR target_id = $3) AND ($4::UUID IS NULL OR agent_id = $4)",
        )
        .bind(filter.from())
        .bind(filter.to())
        .bind(filter.target_id().map(TargetId::as_uuid))
        .bind(filter.agent_id().map(AgentId::as_uuid))
        .fetch_one(self.pool())
        .await
        .map_err(map_database_error)?;
        Ok(CheckSummary::new(
            u64::try_from(total).map_err(|_| PortError::InvalidData)?,
            u64::try_from(successful).map_err(|_| PortError::InvalidData)?,
            average,
            p95,
        ))
    }

    async fn prune_before(&self, before: Timestamp, limit: u16) -> Result<u16, PortError> {
        let rows = sqlx::query_scalar::<_, Uuid>(
            "WITH expired AS (SELECT result_id FROM check_results WHERE checked_at < $1 ORDER BY checked_at LIMIT $2) DELETE FROM check_results WHERE result_id IN (SELECT result_id FROM expired) RETURNING result_id",
        )
        .bind(before)
        .bind(i64::from(limit))
        .fetch_all(self.pool())
        .await
        .map_err(map_database_error)?;
        u16::try_from(rows.len()).map_err(|_| PortError::InvalidData)
    }
}

#[async_trait]
impl AgentEnrollmentRepository for PgStore {
    async fn save(&self, enrollment: &AgentEnrollment) -> Result<(), PortError> {
        sqlx::query(
            "INSERT INTO agent_enrollments (id, agent_id, token_hash, expires_at, consumed_at, created_at) VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(Uuid::new_v4())
        .bind(enrollment.agent_id().as_uuid())
        .bind(enrollment.token_digest().as_bytes().to_vec())
        .bind(enrollment.expires_at())
        .bind(enrollment.consumed_at())
        .bind(enrollment.created_at())
        .execute(self.pool())
        .await
        .map_err(map_database_error)?;
        Ok(())
    }

    async fn consume(
        &self,
        digest: TokenDigest,
        now: Timestamp,
    ) -> Result<Option<AgentId>, PortError> {
        let agent_id = sqlx::query_scalar::<_, Uuid>(
            "UPDATE agent_enrollments SET consumed_at = $2 WHERE token_hash = $1 AND consumed_at IS NULL AND expires_at > $2 RETURNING agent_id",
        )
        .bind(digest.as_bytes().to_vec())
        .bind(now)
        .fetch_optional(self.pool())
        .await
        .map_err(map_database_error)?;
        Ok(agent_id.map(AgentId::from_uuid))
    }

    async fn prune_expired(&self, now: Timestamp, limit: u16) -> Result<u16, PortError> {
        let rows = sqlx::query_scalar::<_, Uuid>(
            "WITH expired AS (SELECT id FROM agent_enrollments WHERE expires_at <= $1 ORDER BY expires_at LIMIT $2) DELETE FROM agent_enrollments WHERE id IN (SELECT id FROM expired) RETURNING id",
        )
        .bind(now)
        .bind(i64::from(limit))
        .fetch_all(self.pool())
        .await
        .map_err(map_database_error)?;
        u16::try_from(rows.len()).map_err(|_| PortError::InvalidData)
    }
}
