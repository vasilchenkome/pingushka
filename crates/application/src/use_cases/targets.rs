use std::{
    collections::{HashMap, HashSet},
    error::Error,
    fmt,
    sync::{Arc, Mutex},
    time::Duration,
};

use pingushka_domain::{
    Timestamp,
    identity::{AgentId, TargetId},
    target::{Target, TargetConfig, TargetError},
};

use crate::{
    error::PortError,
    ports::{AgentControlGateway, Clock, TargetRepository},
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TargetUseCaseError {
    NotFound,
    Disabled,
    TooSoon,
    Invalid(TargetError),
    Repository(PortError),
}

impl fmt::Display for TargetUseCaseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound => formatter.write_str("target was not found"),
            Self::Disabled => formatter.write_str("target is disabled"),
            Self::TooSoon => formatter.write_str("manual check cooldown is still active"),
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
        enabled: bool,
        agent_ids: Vec<AgentId>,
    ) -> Result<Target, TargetUseCaseError> {
        let now = self.clock.now();
        let mut target = Target::new(TargetId::new(), config, now);
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
            .update(config, enabled, self.clock.now())
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
