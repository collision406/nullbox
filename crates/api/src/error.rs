use axum::Json;
use axum::extract::rejection::{JsonRejection, PathRejection, QueryRejection};
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde::Serialize;
use ts_rs::TS;

use crate::repo::RepoError;

/// Stable error codes. The frontend maps each one to copy; never rename a variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum ErrorCode {
    NotFound,
    MissingTicketToken,
    MissingActor,
    Forbidden,
    ValidationFailed,
    InvalidStatusTransition,
    TicketClosed,
    ServiceUnavailable,
    Internal,
    ImATeapot,
    RateLimited,
    PayloadTooLarge,
    InvalidCredentials,
    UsernameTaken,
    PasswordTooShort,
    PasswordTooWeak,
    PasswordLeaked,
}

/// The one error shape: `{ code, message }`. `message` is English and for developers only.
#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct ErrorBody {
    pub code: ErrorCode,
    pub message: String,
}

#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error("not found")]
    NotFound,
    #[error("missing X-Ticket-Token header")]
    MissingTicketToken,
    #[error("missing or invalid X-Actor-Name header")]
    MissingActor,
    #[error("forbidden")]
    Forbidden,
    #[error("validation failed: {0}")]
    Validation(String),
    #[error(transparent)]
    InvalidTransition(#[from] nullbox_domain::InvalidTransition),
    #[error("ticket is closed")]
    TicketClosed,
    /// The database is unreachable. It's not DNS. There's no way it's DNS. It was DNS.
    #[error("storage unavailable: {0}")]
    ProbablyDns(String),
    /// Wrong username or password, or an unknown user: deliberately the same answer.
    #[error("invalid username or password")]
    InvalidCredentials,
    #[error("username already taken")]
    UsernameTaken,
    #[error("password too short")]
    PasswordTooShort,
    #[error("password too easy to guess")]
    PasswordTooWeak,
    #[error("password found in a data breach")]
    PasswordLeaked,
    /// The payment provider is not configured or didn't answer. The details are logged, never sent.
    #[error("payments unavailable: {0}")]
    PaymentsUnavailable(String),
    /// Anything unexpected. The details are logged, never sent.
    #[error("internal error: {0}")]
    WorksOnMyMachine(String),
    #[error("I'm a teapot")]
    Teapot,
    #[error("too many requests, retry in {retry_after_secs}s")]
    RateLimited { retry_after_secs: u64 },
    #[error("request body too large")]
    PayloadTooLarge,
    /// The request ran longer than `REQUEST_TIMEOUT_SECS`.
    #[error("request timed out")]
    Timeout,
}

impl ApiError {
    pub fn code(&self) -> ErrorCode {
        match self {
            Self::NotFound => ErrorCode::NotFound,
            Self::MissingTicketToken => ErrorCode::MissingTicketToken,
            Self::MissingActor => ErrorCode::MissingActor,
            Self::Forbidden => ErrorCode::Forbidden,
            Self::Validation(_) => ErrorCode::ValidationFailed,
            Self::InvalidTransition(_) => ErrorCode::InvalidStatusTransition,
            Self::TicketClosed => ErrorCode::TicketClosed,
            Self::ProbablyDns(_) | Self::PaymentsUnavailable(_) => ErrorCode::ServiceUnavailable,
            Self::WorksOnMyMachine(_) => ErrorCode::Internal,
            Self::Teapot => ErrorCode::ImATeapot,
            Self::RateLimited { .. } => ErrorCode::RateLimited,
            Self::InvalidCredentials => ErrorCode::InvalidCredentials,
            Self::UsernameTaken => ErrorCode::UsernameTaken,
            Self::PasswordTooShort => ErrorCode::PasswordTooShort,
            Self::PasswordTooWeak => ErrorCode::PasswordTooWeak,
            Self::PasswordLeaked => ErrorCode::PasswordLeaked,
            Self::PayloadTooLarge => ErrorCode::PayloadTooLarge,
            Self::Timeout => ErrorCode::ServiceUnavailable,
        }
    }

    pub fn status(&self) -> StatusCode {
        match self {
            Self::NotFound => StatusCode::NOT_FOUND,
            Self::MissingTicketToken | Self::MissingActor => StatusCode::UNAUTHORIZED,
            Self::Forbidden => StatusCode::FORBIDDEN,
            Self::Validation(_) => StatusCode::UNPROCESSABLE_ENTITY,
            Self::InvalidTransition(_) | Self::TicketClosed => StatusCode::CONFLICT,
            Self::ProbablyDns(_) | Self::PaymentsUnavailable(_) => StatusCode::SERVICE_UNAVAILABLE,
            Self::WorksOnMyMachine(_) => StatusCode::INTERNAL_SERVER_ERROR,
            Self::Teapot => StatusCode::IM_A_TEAPOT,
            Self::RateLimited { .. } => StatusCode::TOO_MANY_REQUESTS,
            Self::InvalidCredentials => StatusCode::UNAUTHORIZED,
            Self::UsernameTaken => StatusCode::CONFLICT,
            Self::PasswordTooShort | Self::PasswordTooWeak | Self::PasswordLeaked => StatusCode::UNPROCESSABLE_ENTITY,
            Self::PayloadTooLarge => StatusCode::PAYLOAD_TOO_LARGE,
            Self::Timeout => StatusCode::SERVICE_UNAVAILABLE,
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        // Details of unexpected failures are logged, never sent.
        let message = match &self {
            Self::ProbablyDns(detail) | Self::PaymentsUnavailable(detail) | Self::WorksOnMyMachine(detail) => {
                tracing::error!(error = %detail, code = ?self.code(), "request failed");
                format!("{:?}", self.code())
            }
            other => other.to_string(),
        };
        let retry_after = match &self {
            Self::RateLimited { retry_after_secs } => Some(*retry_after_secs),
            _ => None,
        };
        let mut response = (self.status(), Json(ErrorBody { code: self.code(), message })).into_response();
        if let Some(secs) = retry_after {
            response.headers_mut().insert(header::RETRY_AFTER, HeaderValue::from(secs));
        }
        response
    }
}

impl From<crate::passwords::PasswordProblem> for ApiError {
    fn from(problem: crate::passwords::PasswordProblem) -> Self {
        use crate::passwords::PasswordProblem as Problem;
        match problem {
            Problem::TooShort => Self::PasswordTooShort,
            Problem::TooLong => Self::Validation("password: too long".into()),
            Problem::TooWeak => Self::PasswordTooWeak,
            Problem::Leaked => Self::PasswordLeaked,
        }
    }
}

impl From<RepoError> for ApiError {
    fn from(err: RepoError) -> Self {
        match err {
            RepoError::Database(e @ (sqlx::Error::PoolTimedOut | sqlx::Error::PoolClosed | sqlx::Error::Io(_))) => {
                Self::ProbablyDns(e.to_string())
            }
            other => Self::WorksOnMyMachine(other.to_string()),
        }
    }
}

impl From<JsonRejection> for ApiError {
    fn from(rejection: JsonRejection) -> Self {
        // Bodies over the limit set in `http::router` are cut off before they are parsed.
        if rejection.status() == StatusCode::PAYLOAD_TOO_LARGE {
            return Self::PayloadTooLarge;
        }
        Self::Validation(rejection.body_text())
    }
}

impl From<PathRejection> for ApiError {
    fn from(_: PathRejection) -> Self {
        Self::NotFound
    }
}

impl From<QueryRejection> for ApiError {
    fn from(rejection: QueryRejection) -> Self {
        Self::Validation(rejection.body_text())
    }
}
