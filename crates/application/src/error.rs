use std::{error::Error, fmt};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PortError {
    Conflict,
    InvalidData,
    NotFound,
    Unavailable,
}

impl fmt::Display for PortError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::Conflict => "repository conflict",
            Self::InvalidData => "repository data is invalid",
            Self::NotFound => "repository item was not found",
            Self::Unavailable => "repository is unavailable",
        };
        formatter.write_str(message)
    }
}

impl Error for PortError {}
