use std::{error::Error, fmt, sync::Arc};

use pingushka_domain::check::CheckResult;

use crate::{error::PortError, ports::CheckResultRepository};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RecordCheckBatchError {
    InvalidBatchSize,
    Repository(PortError),
}

impl fmt::Display for RecordCheckBatchError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidBatchSize => {
                formatter.write_str("batch must contain between 1 and 100 results")
            }
            Self::Repository(error) => write!(formatter, "{error}"),
        }
    }
}

impl Error for RecordCheckBatchError {}

impl From<PortError> for RecordCheckBatchError {
    fn from(error: PortError) -> Self {
        Self::Repository(error)
    }
}

pub struct RecordCheckBatch {
    repository: Arc<dyn CheckResultRepository>,
}

impl RecordCheckBatch {
    pub fn new(repository: Arc<dyn CheckResultRepository>) -> Self {
        Self { repository }
    }

    pub async fn execute(&self, results: Vec<CheckResult>) -> Result<usize, RecordCheckBatchError> {
        if !(1..=100).contains(&results.len()) {
            return Err(RecordCheckBatchError::InvalidBatchSize);
        }
        self.repository
            .record_batch(&results)
            .await
            .map_err(Into::into)
    }
}
