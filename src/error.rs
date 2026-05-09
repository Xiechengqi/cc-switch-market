use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::Serialize;

#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error("{message}")]
    Http {
        status: StatusCode,
        error_type: &'static str,
        code: &'static str,
        message: String,
        param: Option<String>,
    },
    #[error(transparent)]
    Libsql(#[from] libsql::Error),
    #[error(transparent)]
    Anyhow(#[from] anyhow::Error),
}

#[derive(Serialize)]
struct ErrorEnvelope<'a> {
    error: ErrorBody<'a>,
}

#[derive(Serialize)]
struct ErrorBody<'a> {
    #[serde(rename = "type")]
    error_type: &'a str,
    message: String,
    code: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    param: Option<String>,
}

impl ApiError {
    pub fn bad_request(code: &'static str, message: impl Into<String>) -> Self {
        Self::Http {
            status: StatusCode::BAD_REQUEST,
            error_type: "invalid_request_error",
            code,
            message: message.into(),
            param: None,
        }
    }

    pub fn unauthorized(message: impl Into<String>) -> Self {
        Self::Http {
            status: StatusCode::UNAUTHORIZED,
            error_type: "authentication_error",
            code: "unauthorized",
            message: message.into(),
            param: None,
        }
    }

    pub fn forbidden(message: impl Into<String>) -> Self {
        Self::Http {
            status: StatusCode::FORBIDDEN,
            error_type: "permission_error",
            code: "forbidden",
            message: message.into(),
            param: None,
        }
    }

    pub fn conflict(code: &'static str, message: impl Into<String>) -> Self {
        Self::Http {
            status: StatusCode::CONFLICT,
            error_type: "invalid_request_error",
            code,
            message: message.into(),
            param: None,
        }
    }

    pub fn service_unavailable(message: impl Into<String>) -> Self {
        Self::Http {
            status: StatusCode::SERVICE_UNAVAILABLE,
            error_type: "api_error",
            code: "service_unavailable",
            message: message.into(),
            param: None,
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, error_type, code, message, param) = match self {
            ApiError::Http {
                status,
                error_type,
                code,
                message,
                param,
            } => (status, error_type, code, message, param),
            ApiError::Libsql(err) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "api_error",
                "database_error",
                err.to_string(),
                None,
            ),
            ApiError::Anyhow(err) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "api_error",
                "internal_error",
                err.to_string(),
                None,
            ),
        };

        (
            status,
            Json(ErrorEnvelope {
                error: ErrorBody {
                    error_type,
                    message,
                    code,
                    param,
                },
            }),
        )
            .into_response()
    }
}
