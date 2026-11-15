use std::{error::Error, fmt, sync::Arc};

use pingushka_domain::{
    identity::TargetGroupId,
    organization::{TargetGroup, TargetGroupName, TargetGroupWithCounts},
    target::TargetError,
};

use crate::{
    error::PortError,
    ports::{Clock, DeleteGroupOutcome, TargetGroupRepository},
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TargetGroupUseCaseError {
    NotFound,
    AlreadyExists,
    HasRules,
    Invalid(TargetError),
    Repository(PortError),
}

impl fmt::Display for TargetGroupUseCaseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound => formatter.write_str("target group was not found"),
            Self::AlreadyExists => formatter.write_str("target group already exists"),
            Self::HasRules => formatter.write_str("target group still has alert rules"),
            Self::Invalid(error) => write!(formatter, "{error}"),
            Self::Repository(error) => write!(formatter, "{error}"),
        }
    }
}

impl Error for TargetGroupUseCaseError {}

impl From<PortError> for TargetGroupUseCaseError {
    fn from(error: PortError) -> Self {
        Self::Repository(error)
    }
}

pub struct CreateTargetGroup {
    groups: Arc<dyn TargetGroupRepository>,
    clock: Arc<dyn Clock>,
}

impl CreateTargetGroup {
    pub fn new(groups: Arc<dyn TargetGroupRepository>, clock: Arc<dyn Clock>) -> Self {
        Self { groups, clock }
    }

    pub async fn execute(
        &self,
        name: TargetGroupName,
    ) -> Result<TargetGroup, TargetGroupUseCaseError> {
        let group = TargetGroup::new(name, self.clock.now());
        match self.groups.save(&group).await {
            Ok(()) => Ok(group),
            Err(PortError::Conflict) => Err(TargetGroupUseCaseError::AlreadyExists),
            Err(error) => Err(error.into()),
        }
    }
}

pub struct RenameTargetGroup {
    groups: Arc<dyn TargetGroupRepository>,
    clock: Arc<dyn Clock>,
}

impl RenameTargetGroup {
    pub fn new(groups: Arc<dyn TargetGroupRepository>, clock: Arc<dyn Clock>) -> Self {
        Self { groups, clock }
    }

    pub async fn execute(
        &self,
        id: TargetGroupId,
        name: TargetGroupName,
    ) -> Result<TargetGroup, TargetGroupUseCaseError> {
        let mut group = self
            .groups
            .find(id)
            .await?
            .ok_or(TargetGroupUseCaseError::NotFound)?;
        group
            .rename(name, self.clock.now())
            .map_err(TargetGroupUseCaseError::Invalid)?;
        match self.groups.save(&group).await {
            Ok(()) => Ok(group),
            Err(PortError::Conflict) => Err(TargetGroupUseCaseError::AlreadyExists),
            Err(error) => Err(error.into()),
        }
    }
}

pub struct DeleteTargetGroup {
    groups: Arc<dyn TargetGroupRepository>,
}

impl DeleteTargetGroup {
    pub fn new(groups: Arc<dyn TargetGroupRepository>) -> Self {
        Self { groups }
    }

    pub async fn execute(&self, id: TargetGroupId) -> Result<(), TargetGroupUseCaseError> {
        match self.groups.delete(id).await? {
            DeleteGroupOutcome::Deleted => Ok(()),
            DeleteGroupOutcome::NotFound => Err(TargetGroupUseCaseError::NotFound),
            DeleteGroupOutcome::HasRules => Err(TargetGroupUseCaseError::HasRules),
        }
    }
}

pub struct ListTargetGroups {
    groups: Arc<dyn TargetGroupRepository>,
}

impl ListTargetGroups {
    pub fn new(groups: Arc<dyn TargetGroupRepository>) -> Self {
        Self { groups }
    }

    pub async fn execute(&self) -> Result<Vec<TargetGroupWithCounts>, TargetGroupUseCaseError> {
        self.groups.list_with_counts().await.map_err(Into::into)
    }
}
