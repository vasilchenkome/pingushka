use std::{error::Error, fmt, sync::Arc, time::Duration};

use pingushka_domain::{
    Timestamp,
    agent::{Agent, AgentError, AgentName},
    enrollment::AgentEnrollment,
    identity::AgentId,
};

use crate::{
    error::PortError,
    ports::{
        AgentControlGateway, AgentEnrollmentRepository, AgentRepository, Clock, SecurityError,
        TokenDigester, TokenGenerator,
    },
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NewAgentCredentials {
    agent: Agent,
    enrollment_token: String,
    expires_at: Timestamp,
}

impl NewAgentCredentials {
    pub fn agent(&self) -> &Agent {
        &self.agent
    }

    pub fn enrollment_token(&self) -> &str {
        &self.enrollment_token
    }

    pub const fn expires_at(&self) -> Timestamp {
        self.expires_at
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RegisterAgentError {
    InvalidName(AgentError),
    Security(SecurityError),
    Repository(PortError),
}

impl fmt::Display for RegisterAgentError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidName(error) => write!(formatter, "{error}"),
            Self::Security(error) => write!(formatter, "{error}"),
            Self::Repository(error) => write!(formatter, "{error}"),
        }
    }
}

impl Error for RegisterAgentError {}

pub struct RegisterAgent {
    agents: Arc<dyn AgentRepository>,
    enrollments: Arc<dyn AgentEnrollmentRepository>,
    generator: Arc<dyn TokenGenerator>,
    digester: Arc<dyn TokenDigester>,
    clock: Arc<dyn Clock>,
    enrollment_ttl: Duration,
}

impl RegisterAgent {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        agents: Arc<dyn AgentRepository>,
        enrollments: Arc<dyn AgentEnrollmentRepository>,
        generator: Arc<dyn TokenGenerator>,
        digester: Arc<dyn TokenDigester>,
        clock: Arc<dyn Clock>,
        enrollment_ttl: Duration,
    ) -> Self {
        Self {
            agents,
            enrollments,
            generator,
            digester,
            clock,
            enrollment_ttl,
        }
    }

    pub async fn execute(&self, name: &str) -> Result<NewAgentCredentials, RegisterAgentError> {
        let name = AgentName::new(name).map_err(RegisterAgentError::InvalidName)?;
        let now = self.clock.now();
        let agent = Agent::new(AgentId::new(), name, now);
        self.agents
            .save(&agent)
            .await
            .map_err(RegisterAgentError::Repository)?;
        issue_enrollment(
            &agent,
            self.enrollments.as_ref(),
            self.generator.as_ref(),
            self.digester.as_ref(),
            now,
            self.enrollment_ttl,
        )
        .await
        .map_err(|error| match error {
            ProvisionError::Security(error) => RegisterAgentError::Security(error),
            ProvisionError::Enrollment => RegisterAgentError::Repository(PortError::InvalidData),
            ProvisionError::Repository(error) => RegisterAgentError::Repository(error),
        })
    }
}

enum ProvisionError {
    Security(SecurityError),
    Enrollment,
    Repository(PortError),
}

async fn issue_enrollment(
    agent: &Agent,
    enrollments: &dyn AgentEnrollmentRepository,
    generator: &dyn TokenGenerator,
    digester: &dyn TokenDigester,
    now: Timestamp,
    ttl: Duration,
) -> Result<NewAgentCredentials, ProvisionError> {
    let token = generator.generate().map_err(ProvisionError::Security)?;
    let digest = digester.digest(&token);
    let expires_at = now + ttl;
    let enrollment = AgentEnrollment::new(agent.id(), digest, now, expires_at)
        .map_err(|_| ProvisionError::Enrollment)?;
    enrollments
        .save(&enrollment)
        .await
        .map_err(ProvisionError::Repository)?;
    Ok(NewAgentCredentials {
        agent: agent.clone(),
        enrollment_token: token,
        expires_at,
    })
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RotateAgentTokenError {
    NotFound,
    Security(SecurityError),
    Repository(PortError),
}

impl fmt::Display for RotateAgentTokenError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound => formatter.write_str("agent was not found"),
            Self::Security(error) => write!(formatter, "{error}"),
            Self::Repository(error) => write!(formatter, "{error}"),
        }
    }
}

impl Error for RotateAgentTokenError {}

pub struct RotateAgentToken {
    agents: Arc<dyn AgentRepository>,
    enrollments: Arc<dyn AgentEnrollmentRepository>,
    control: Arc<dyn AgentControlGateway>,
    generator: Arc<dyn TokenGenerator>,
    digester: Arc<dyn TokenDigester>,
    clock: Arc<dyn Clock>,
    enrollment_ttl: Duration,
}

impl RotateAgentToken {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        agents: Arc<dyn AgentRepository>,
        enrollments: Arc<dyn AgentEnrollmentRepository>,
        control: Arc<dyn AgentControlGateway>,
        generator: Arc<dyn TokenGenerator>,
        digester: Arc<dyn TokenDigester>,
        clock: Arc<dyn Clock>,
        enrollment_ttl: Duration,
    ) -> Self {
        Self {
            agents,
            enrollments,
            control,
            generator,
            digester,
            clock,
            enrollment_ttl,
        }
    }

    pub async fn execute(
        &self,
        agent_id: AgentId,
    ) -> Result<NewAgentCredentials, RotateAgentTokenError> {
        let mut agent = self
            .agents
            .find(agent_id)
            .await
            .map_err(RotateAgentTokenError::Repository)?
            .ok_or(RotateAgentTokenError::NotFound)?;
        agent.replace_token_digest(None);
        agent.mark_disconnected();
        self.agents
            .save(&agent)
            .await
            .map_err(RotateAgentTokenError::Repository)?;
        let _ = self.control.disconnect(agent_id).await;
        let now = self.clock.now();
        issue_enrollment(
            &agent,
            self.enrollments.as_ref(),
            self.generator.as_ref(),
            self.digester.as_ref(),
            now,
            self.enrollment_ttl,
        )
        .await
        .map_err(|error| match error {
            ProvisionError::Security(error) => RotateAgentTokenError::Security(error),
            ProvisionError::Enrollment | ProvisionError::Repository(_) => {
                RotateAgentTokenError::Repository(PortError::InvalidData)
            }
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EnrolledAgent {
    agent_id: AgentId,
    credential: String,
}

impl EnrolledAgent {
    pub const fn agent_id(&self) -> AgentId {
        self.agent_id
    }

    pub fn credential(&self) -> &str {
        &self.credential
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EnrollAgentError {
    InvalidEnrollment,
    Security(SecurityError),
    Repository(PortError),
}

impl fmt::Display for EnrollAgentError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidEnrollment => {
                formatter.write_str("enrollment token is invalid, expired, or already used")
            }
            Self::Security(error) => write!(formatter, "{error}"),
            Self::Repository(error) => write!(formatter, "{error}"),
        }
    }
}

impl Error for EnrollAgentError {}

pub struct EnrollAgent {
    agents: Arc<dyn AgentRepository>,
    enrollments: Arc<dyn AgentEnrollmentRepository>,
    generator: Arc<dyn TokenGenerator>,
    digester: Arc<dyn TokenDigester>,
    clock: Arc<dyn Clock>,
}

impl EnrollAgent {
    pub fn new(
        agents: Arc<dyn AgentRepository>,
        enrollments: Arc<dyn AgentEnrollmentRepository>,
        generator: Arc<dyn TokenGenerator>,
        digester: Arc<dyn TokenDigester>,
        clock: Arc<dyn Clock>,
    ) -> Self {
        Self {
            agents,
            enrollments,
            generator,
            digester,
            clock,
        }
    }

    pub async fn execute(&self, enrollment_token: &str) -> Result<EnrolledAgent, EnrollAgentError> {
        let enrollment_digest = self.digester.digest(enrollment_token);
        let Some(agent_id) = self
            .enrollments
            .consume(enrollment_digest, self.clock.now())
            .await
            .map_err(EnrollAgentError::Repository)?
        else {
            return Err(EnrollAgentError::InvalidEnrollment);
        };
        let mut agent = self
            .agents
            .find(agent_id)
            .await
            .map_err(EnrollAgentError::Repository)?
            .ok_or(EnrollAgentError::InvalidEnrollment)?;
        let credential = self
            .generator
            .generate()
            .map_err(EnrollAgentError::Security)?;
        agent.replace_token_digest(Some(self.digester.digest(&credential)));
        self.agents
            .save(&agent)
            .await
            .map_err(EnrollAgentError::Repository)?;
        Ok(EnrolledAgent {
            agent_id,
            credential,
        })
    }
}
