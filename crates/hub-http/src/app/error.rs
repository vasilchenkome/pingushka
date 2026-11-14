use axum::{
    Json,
    extract::rejection::{JsonRejection, QueryRejection},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use pingushka_application::{
    error::PortError,
    use_cases::{
        EnrollAgentError, LoginError, RegisterAgentError, RotateAgentTokenError, SetupAdminError,
        TargetUseCaseError,
    },
};
use serde_json::json;

#[derive(Clone, Debug)]
pub struct ApiError {
    status: StatusCode,
    code: &'static str,
    message: String,
}

impl ApiError {
    pub fn new(status: StatusCode, code: &'static str, message: impl Into<String>) -> Self {
        Self {
            status,
            code,
            message: message.into(),
        }
    }

    pub fn bad_request(message: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, "bad_request", message)
    }

    pub fn unauthorized(message: impl Into<String>) -> Self {
        Self::new(StatusCode::UNAUTHORIZED, "unauthorized", message)
    }

    pub fn forbidden(message: impl Into<String>) -> Self {
        Self::new(StatusCode::FORBIDDEN, "forbidden", message)
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new(StatusCode::NOT_FOUND, "not_found", message)
    }

    pub fn conflict(message: impl Into<String>) -> Self {
        Self::new(StatusCode::CONFLICT, "conflict", message)
    }

    pub fn too_many_requests(message: impl Into<String>) -> Self {
        Self::new(StatusCode::TOO_MANY_REQUESTS, "too_many_requests", message)
    }

    pub fn unprocessable(message: impl Into<String>) -> Self {
        Self::new(StatusCode::UNPROCESSABLE_ENTITY, "invalid_input", message)
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, "internal_error", message)
    }

    pub fn from_port(error: PortError) -> Self {
        match error {
            PortError::NotFound => Self::not_found(error.to_string()),
            PortError::Conflict => Self::conflict(error.to_string()),
            PortError::InvalidData => Self::unprocessable(error.to_string()),
            PortError::Unavailable => Self::new(
                StatusCode::SERVICE_UNAVAILABLE,
                "unavailable",
                error.to_string(),
            ),
        }
    }

    pub fn from_setup(error: SetupAdminError) -> Self {
        match error {
            SetupAdminError::InvalidUsername | SetupAdminError::InvalidPassword => {
                Self::unprocessable(error.to_string())
            }
            SetupAdminError::AlreadyConfigured => Self::conflict(error.to_string()),
            SetupAdminError::Security(_) => Self::internal(error.to_string()),
            SetupAdminError::Repository(error) => Self::from_port(error),
        }
    }

    pub fn from_login(error: LoginError) -> Self {
        match error {
            LoginError::InvalidCredentials => Self::unauthorized(error.to_string()),
            LoginError::Security(_) => Self::internal(error.to_string()),
            LoginError::Repository(error) => Self::from_port(error),
        }
    }

    pub fn from_register_agent(error: RegisterAgentError) -> Self {
        match error {
            RegisterAgentError::InvalidName(_) => Self::unprocessable(error.to_string()),
            RegisterAgentError::Security(_) => Self::internal(error.to_string()),
            RegisterAgentError::Repository(error) => Self::from_port(error),
        }
    }

    pub fn from_rotate(error: RotateAgentTokenError) -> Self {
        match error {
            RotateAgentTokenError::NotFound => Self::not_found(error.to_string()),
            RotateAgentTokenError::Security(_) => Self::internal(error.to_string()),
            RotateAgentTokenError::Repository(error) => Self::from_port(error),
        }
    }

    pub fn from_enroll(error: EnrollAgentError) -> Self {
        match error {
            EnrollAgentError::InvalidEnrollment => Self::unauthorized(error.to_string()),
            EnrollAgentError::Security(_) => Self::internal(error.to_string()),
            EnrollAgentError::Repository(error) => Self::from_port(error),
        }
    }

    pub fn from_target(error: TargetUseCaseError) -> Self {
        match error {
            TargetUseCaseError::NotFound => Self::not_found(error.to_string()),
            TargetUseCaseError::Disabled | TargetUseCaseError::TooSoon => {
                Self::conflict(error.to_string())
            }
            TargetUseCaseError::Invalid(_) => Self::unprocessable(error.to_string()),
            TargetUseCaseError::Repository(error) => Self::from_port(error),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let body = Json(json!({
            "error": {
                "code": self.code,
                "message": self.message,
            }
        }));
        (self.status, body).into_response()
    }
}

impl From<JsonRejection> for ApiError {
    fn from(rejection: JsonRejection) -> Self {
        Self::bad_request(rejection.body_text())
    }
}

impl From<QueryRejection> for ApiError {
    fn from(rejection: QueryRejection) -> Self {
        Self::bad_request(rejection.body_text())
    }
}
