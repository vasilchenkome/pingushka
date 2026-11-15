use std::{
    collections::{HashMap, HashSet},
    error::Error,
    fmt,
    sync::{Arc, Mutex},
    time::Duration,
};

use pingushka_domain::{
    Timestamp,
    identity::{AgentId, TargetGroupId, TargetId},
    organization::{TargetGroup, TargetGroupName, TargetOrganization, TargetTag, TargetTagSet},
    target::{Target, TargetConfig, TargetError},
};

use crate::{
    error::PortError,
    ports::{AgentControlGateway, Clock, TargetGroupRepository, TargetRepository},
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TargetUseCaseError {
    NotFound,
    Disabled,
    TooSoon,
    EmptySelection,
    TooManyTargets,
    Invalid(TargetError),
    Repository(PortError),
}

impl fmt::Display for TargetUseCaseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound => formatter.write_str("target was not found"),
            Self::Disabled => formatter.write_str("target is disabled"),
            Self::TooSoon => formatter.write_str("manual check cooldown is still active"),
            Self::EmptySelection => formatter.write_str("no targets were selected"),
            Self::TooManyTargets => formatter.write_str("too many targets were selected"),
            Self::Invalid(error) => write!(formatter, "{error}"),
            Self::Repository(error) => write!(formatter, "{error}"),
        }
    }
}

impl Error for TargetUseCaseError {}

impl From<PortError> for TargetUseCaseError {
    fn from(error: PortError) -> Self {
        Self::Repository(error)
    }
}

async fn notify_agents(
    control: &dyn AgentControlGateway,
    agent_ids: impl IntoIterator<Item = AgentId>,
) {
    let unique: HashSet<AgentId> = agent_ids.into_iter().collect();
    for agent_id in unique {
        let _ = control.send_configuration(agent_id).await;
    }
}

pub struct CreateTarget {
    targets: Arc<dyn TargetRepository>,
    control: Arc<dyn AgentControlGateway>,
    clock: Arc<dyn Clock>,
}

impl CreateTarget {
    pub fn new(
        targets: Arc<dyn TargetRepository>,
        control: Arc<dyn AgentControlGateway>,
        clock: Arc<dyn Clock>,
    ) -> Self {
        Self {
            targets,
            control,
            clock,
        }
    }

    pub async fn execute(
        &self,
        config: TargetConfig,
        organization: TargetOrganization,
        enabled: bool,
        agent_ids: Vec<AgentId>,
    ) -> Result<Target, TargetUseCaseError> {
        let now = self.clock.now();
        let mut target = Target::new(TargetId::new(), config, organization, now);
        if !enabled {
            target.set_enabled(false, now);
        }
        self.targets.save(&target).await?;
        self.targets.assign_agents(target.id(), &agent_ids).await?;
        notify_agents(self.control.as_ref(), agent_ids).await;
        Ok(target)
    }
}

pub struct UpdateTarget {
    targets: Arc<dyn TargetRepository>,
    control: Arc<dyn AgentControlGateway>,
    clock: Arc<dyn Clock>,
}

impl UpdateTarget {
    pub fn new(
        targets: Arc<dyn TargetRepository>,
        control: Arc<dyn AgentControlGateway>,
        clock: Arc<dyn Clock>,
    ) -> Self {
        Self {
            targets,
            control,
            clock,
        }
    }

    pub async fn execute(
        &self,
        target_id: TargetId,
        config: TargetConfig,
        organization: TargetOrganization,
        enabled: bool,
        agent_ids: Vec<AgentId>,
    ) -> Result<Target, TargetUseCaseError> {
        let mut target = self
            .targets
            .find(target_id)
            .await?
            .ok_or(TargetUseCaseError::NotFound)?;
        let previous_agents = self.targets.agent_ids(target_id).await?;
        target
            .update(config, organization, enabled, self.clock.now())
            .map_err(TargetUseCaseError::Invalid)?;
        self.targets.save(&target).await?;
        self.targets.assign_agents(target.id(), &agent_ids).await?;
        notify_agents(
            self.control.as_ref(),
            previous_agents.into_iter().chain(agent_ids),
        )
        .await;
        Ok(target)
    }
}

pub struct DeleteTarget {
    targets: Arc<dyn TargetRepository>,
    control: Arc<dyn AgentControlGateway>,
}

impl DeleteTarget {
    pub fn new(targets: Arc<dyn TargetRepository>, control: Arc<dyn AgentControlGateway>) -> Self {
        Self { targets, control }
    }

    pub async fn execute(&self, target_id: TargetId) -> Result<(), TargetUseCaseError> {
        let agent_ids = self.targets.agent_ids(target_id).await?;
        if !self.targets.delete(target_id).await? {
            return Err(TargetUseCaseError::NotFound);
        }
        notify_agents(self.control.as_ref(), agent_ids).await;
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub enum GroupChange {
    Keep,
    Clear,
    Set(TargetGroupId),
    CreateAndSet(TargetGroupName),
}

#[derive(Clone, Debug)]
pub enum TagChange {
    Keep,
    Add(TargetTagSet),
    Remove(TargetTagSet),
    Replace(TargetTagSet),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BulkUpdateResult {
    pub updated: usize,
    pub group_id: Option<TargetGroupId>,
}

pub struct BulkUpdateTargets {
    targets: Arc<dyn TargetRepository>,
    groups: Arc<dyn TargetGroupRepository>,
    clock: Arc<dyn Clock>,
}

impl BulkUpdateTargets {
    pub fn new(
        targets: Arc<dyn TargetRepository>,
        groups: Arc<dyn TargetGroupRepository>,
        clock: Arc<dyn Clock>,
    ) -> Self {
        Self {
            targets,
            groups,
            clock,
        }
    }

    pub async fn execute(
        &self,
        target_ids: &[TargetId],
        group_change: GroupChange,
        tag_change: TagChange,
    ) -> Result<BulkUpdateResult, TargetUseCaseError> {
        if target_ids.is_empty() {
            return Err(TargetUseCaseError::EmptySelection);
        }
        if target_ids.len() > 200 {
            return Err(TargetUseCaseError::TooManyTargets);
        }

        let unique_ids: Vec<TargetId> = target_ids
            .iter()
            .copied()
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();
        let targets = self.targets.find_many(&unique_ids).await?;
        if targets.len() != unique_ids.len() {
            return Err(TargetUseCaseError::NotFound);
        }

        let created_group = match &group_change {
            GroupChange::CreateAndSet(name) => Some(self.ensure_group(name).await?),
            _ => None,
        };

        let mut updates = Vec::with_capacity(targets.len());
        for target in &targets {
            let group_id = match &group_change {
                GroupChange::Keep => target.group_id(),
                GroupChange::Clear => None,
                GroupChange::Set(id) => Some(*id),
                GroupChange::CreateAndSet(_) => created_group.as_ref().map(|group| group.id()),
            };
            let tags = apply_tag_change(target.tag_set(), &tag_change)?;
            updates.push((target.id(), TargetOrganization::new(group_id, tags)));
        }

        self.targets.save_organizations(&updates).await?;
        Ok(BulkUpdateResult {
            updated: updates.len(),
            group_id: created_group.map(|group| group.id()),
        })
    }

    async fn ensure_group(
        &self,
        name: &TargetGroupName,
    ) -> Result<TargetGroup, TargetUseCaseError> {
        if let Some(group) = self.groups.find_by_name(name).await? {
            return Ok(group);
        }
        let group = TargetGroup::new(name.clone(), self.clock.now());
        match self.groups.save(&group).await {
            Ok(()) => Ok(group),
            Err(PortError::Conflict) => self
                .groups
                .find_by_name(name)
                .await?
                .ok_or(TargetUseCaseError::Repository(PortError::Conflict)),
            Err(error) => Err(TargetUseCaseError::Repository(error)),
        }
    }
}

fn apply_tag_change(
    current: &TargetTagSet,
    change: &TagChange,
) -> Result<TargetTagSet, TargetUseCaseError> {
    match change {
        TagChange::Keep => Ok(current.clone()),
        TagChange::Replace(tags) => Ok(tags.clone()),
        TagChange::Add(tags) => TargetTagSet::new(
            current
                .as_slice()
                .iter()
                .cloned()
                .chain(tags.as_slice().iter().cloned()),
        )
        .map_err(TargetUseCaseError::Invalid),
        TagChange::Remove(tags) => {
            let removed: HashSet<&str> = tags.as_slice().iter().map(TargetTag::as_str).collect();
            TargetTagSet::new(
                current
                    .as_slice()
                    .iter()
                    .filter(|tag| !removed.contains(tag.as_str()))
                    .cloned(),
            )
            .map_err(TargetUseCaseError::Invalid)
        }
    }
}

pub struct TriggerManualCheck {
    targets: Arc<dyn TargetRepository>,
    control: Arc<dyn AgentControlGateway>,
    clock: Arc<dyn Clock>,
    cooldown: Duration,
    last_checks: Mutex<HashMap<TargetId, Timestamp>>,
}

impl TriggerManualCheck {
    pub fn new(
        targets: Arc<dyn TargetRepository>,
        control: Arc<dyn AgentControlGateway>,
        clock: Arc<dyn Clock>,
        cooldown: Duration,
    ) -> Self {
        Self {
            targets,
            control,
            clock,
            cooldown,
            last_checks: Mutex::new(HashMap::new()),
        }
    }

    pub async fn execute(&self, target_id: TargetId) -> Result<usize, TargetUseCaseError> {
        let target = self
            .targets
            .find(target_id)
            .await?
            .ok_or(TargetUseCaseError::NotFound)?;
        if !target.is_enabled() {
            return Err(TargetUseCaseError::Disabled);
        }
        let now = self.clock.now();
        {
            let mut last_checks = self
                .last_checks
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if let Some(previous) = last_checks.get(&target_id) {
                let elapsed = now - *previous;
                if elapsed.is_negative() || elapsed.unsigned_abs() < self.cooldown {
                    return Err(TargetUseCaseError::TooSoon);
                }
            }
            last_checks.insert(target_id, now);
        }
        let agent_ids = self.targets.agent_ids(target_id).await?;
        let mut triggered = 0;
        for agent_id in agent_ids {
            if self
                .control
                .trigger_check(agent_id, target_id)
                .await
                .is_ok()
            {
                triggered += 1;
            }
        }
        Ok(triggered)
    }
}
