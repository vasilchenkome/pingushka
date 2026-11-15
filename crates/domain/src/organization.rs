use std::collections::BTreeSet;

use time::OffsetDateTime;

use crate::{
    identity::TargetGroupId,
    target::{TargetError, TargetName},
};

pub const MAX_TAGS: usize = 10;
pub const MAX_TAG_LEN: usize = 32;
pub const MAX_GROUP_LEN: usize = 64;

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct TargetGroupName(String);

impl TargetGroupName {
    pub fn new(value: impl AsRef<str>) -> Result<Self, TargetError> {
        let value = value.as_ref().trim();
        if value.is_empty() {
            return Err(TargetError::EmptyGroup);
        }
        if value.chars().count() > MAX_GROUP_LEN {
            return Err(TargetError::GroupTooLong);
        }
        if value.chars().any(char::is_control) {
            return Err(TargetError::InvalidGroup);
        }
        Ok(Self(value.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct TargetTag(String);

impl TargetTag {
    pub fn new(value: impl AsRef<str>) -> Result<Self, TargetError> {
        let value = value.as_ref().trim().to_lowercase();
        if value.is_empty() {
            return Err(TargetError::EmptyTag);
        }
        if value.chars().count() > MAX_TAG_LEN {
            return Err(TargetError::TagTooLong);
        }
        if value.chars().any(|ch| {
            ch.is_control() || ch.is_whitespace() || !(ch.is_alphanumeric() || "-_.".contains(ch))
        }) {
            return Err(TargetError::InvalidTag);
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct TargetTagSet(Vec<TargetTag>);

impl TargetTagSet {
    pub fn new(values: impl IntoIterator<Item = TargetTag>) -> Result<Self, TargetError> {
        let mut unique = BTreeSet::new();
        for value in values {
            unique.insert(value);
        }
        if unique.len() > MAX_TAGS {
            return Err(TargetError::TooManyTags);
        }
        Ok(Self(unique.into_iter().collect()))
    }

    pub fn parse(values: impl IntoIterator<Item = impl AsRef<str>>) -> Result<Self, TargetError> {
        let tags = values
            .into_iter()
            .map(TargetTag::new)
            .collect::<Result<Vec<_>, _>>()?;
        Self::new(tags)
    }

    pub fn empty() -> Self {
        Self(Vec::new())
    }

    pub fn as_slice(&self) -> &[TargetTag] {
        &self.0
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TargetOrganization {
    group_id: Option<TargetGroupId>,
    tags: TargetTagSet,
}

impl TargetOrganization {
    pub fn new(group_id: Option<TargetGroupId>, tags: TargetTagSet) -> Self {
        Self { group_id, tags }
    }

    pub fn empty() -> Self {
        Self {
            group_id: None,
            tags: TargetTagSet::empty(),
        }
    }

    pub const fn group_id(&self) -> Option<TargetGroupId> {
        self.group_id
    }

    pub fn tags(&self) -> &[TargetTag] {
        self.tags.as_slice()
    }

    pub fn tag_set(&self) -> &TargetTagSet {
        &self.tags
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TargetGroup {
    id: TargetGroupId,
    name: TargetGroupName,
    created_at: OffsetDateTime,
    updated_at: OffsetDateTime,
}

impl TargetGroup {
    pub fn new(name: TargetGroupName, now: OffsetDateTime) -> Self {
        Self {
            id: TargetGroupId::new(),
            name,
            created_at: now,
            updated_at: now,
        }
    }

    pub fn restore(
        id: TargetGroupId,
        name: TargetGroupName,
        created_at: OffsetDateTime,
        updated_at: OffsetDateTime,
    ) -> Result<Self, TargetError> {
        if updated_at < created_at {
            return Err(TargetError::UpdatedBeforeCreated);
        }
        Ok(Self {
            id,
            name,
            created_at,
            updated_at,
        })
    }

    pub fn id(&self) -> TargetGroupId {
        self.id
    }

    pub fn name(&self) -> &TargetGroupName {
        &self.name
    }

    pub const fn created_at(&self) -> OffsetDateTime {
        self.created_at
    }

    pub const fn updated_at(&self) -> OffsetDateTime {
        self.updated_at
    }

    pub fn rename(
        &mut self,
        name: TargetGroupName,
        now: OffsetDateTime,
    ) -> Result<(), TargetError> {
        if now < self.created_at {
            return Err(TargetError::UpdatedBeforeCreated);
        }
        self.name = name;
        self.updated_at = now;
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TargetGroupWithCounts {
    group: TargetGroup,
    target_count: u32,
    rule_count: u32,
}

impl TargetGroupWithCounts {
    pub fn new(group: TargetGroup, target_count: u32, rule_count: u32) -> Self {
        Self {
            group,
            target_count,
            rule_count,
        }
    }

    pub fn group(&self) -> &TargetGroup {
        &self.group
    }

    pub const fn target_count(&self) -> u32 {
        self.target_count
    }

    pub const fn rule_count(&self) -> u32 {
        self.rule_count
    }
}

pub fn normalize_group(value: Option<&str>) -> Result<Option<TargetGroupName>, TargetError> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(TargetGroupName::new)
        .transpose()
}

pub fn target_name_is_grouped(name: &TargetName) -> bool {
    name.as_str().starts_with('[') && name.as_str().contains(']')
}

pub fn group_from_target_name(name: &TargetName) -> Option<TargetGroupName> {
    let value = name.as_str();
    let end = value.find(']')?;
    if !value.starts_with('[') || end <= 1 {
        return None;
    }
    TargetGroupName::new(&value[1..end]).ok()
}
