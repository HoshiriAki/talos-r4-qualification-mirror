use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::Serialize;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("未登录")]
    #[allow(dead_code)]
    Unauthorized,

    #[error("仅管理员可操作")]
    #[allow(dead_code)]
    Forbidden,

    #[error("{message}")]
    CodedForbidden { code: String, message: String },

    #[error("{0}")]
    BadRequest(String),

    #[error("{message}")]
    CodedBadRequest { code: String, message: String },

    #[error("{0}")]
    NotFound(String),

    #[error("{message}")]
    CodedNotFound { code: String, message: String },

    #[error("{0}")]
    Conflict(String),

    #[error("{message}")]
    Gone { code: String, message: String },

    #[error("{message}")]
    CodedConflict { code: String, message: String },

    #[error("请求过于频繁，请稍后再试")]
    #[allow(dead_code)]
    RateLimited { retry_after_secs: u64 },

    #[error("{message}")]
    CodedRateLimited { code: String, message: String },

    #[error("{0}")]
    Internal(String),

    #[error("[{code}] {message}")]
    ServiceError { code: String, message: String },

    #[error("storage operation failed")]
    Storage,

    #[error(transparent)]
    Anyhow(#[from] anyhow::Error),

    #[error(transparent)]
    Serde(#[from] serde_json::Error),

    #[error("Excel error: {0}")]
    Xlsx(#[from] rust_xlsxwriter::XlsxError),

    #[error("Excel parse error: {0}")]
    Calamine(#[from] calamine::Error),
}

#[derive(Serialize)]
pub struct ErrorBody {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    pub error: String,
}

impl AppError {
    /// Parse ErrorPayload JSON string into typed AppError.
    /// Falls back to ServiceError on parse failure.
    pub fn from_error_payload(raw: String) -> Self {
        let payload: serde_json::Value = match serde_json::from_str(&raw) {
            Ok(v) => v,
            Err(_) => {
                return AppError::ServiceError {
                    code: "SYS_PARSE".into(),
                    message: raw,
                };
            }
        };

        let code = payload
            .get("code")
            .and_then(|v| v.as_str())
            .unwrap_or("SYS_UNKNOWN");
        let message = payload
            .get("message")
            .and_then(|v| v.as_str())
            .unwrap_or(&raw)
            .to_string();

        match code {
            // Integration commands expose stable client-actionable errors;
            // keep their public codes rather than collapsing them into 500.
            "VAL_INTEGRATION_INPUT" => AppError::CodedBadRequest {
                code: code.to_string(),
                message,
            },
            "BIZ_INTEGRATION_NOT_FOUND" => AppError::CodedNotFound {
                code: code.to_string(),
                message,
            },
            "BIZ_WORK_TASK_NOT_FOUND" => AppError::NotFound(message),
            "BIZ_INTEGRATION_CONFLICT" | "BIZ_INTEGRATION_REJECTED" => AppError::CodedConflict {
                code: code.to_string(),
                message,
            },
            "BIZ_WORK_TASK_CONFLICT" => AppError::Conflict(message),
            "BIZ_INTEGRATION_RATE_LIMITED" => AppError::CodedRateLimited {
                code: code.to_string(),
                message,
            },
            "AUTH_INTEGRATION_FORBIDDEN" => AppError::CodedForbidden {
                code: code.to_string(),
                message,
            },
            "AUTH_WORK_TASK_FORBIDDEN" => AppError::Forbidden,
            "AUTH_USER_SETTINGS_SELF_ONLY" => AppError::Forbidden,
            // Not found
            "BIZ_ORDER_NOT_FOUND" | "BIZ_DEVICE_NOT_FOUND" | "VAL_NOT_FOUND" => {
                AppError::NotFound(message)
            }
            // Validation
            "VAL_REQUIRED" | "VAL_DATE_FORMAT" | "VAL_DATE_RANGE" | "VAL_FORMAT" => {
                AppError::BadRequest(message)
            }
            "VAL_WORK_TASK_INPUT" | "VAL_WORK_TASK_ACTION" | "BIZ_WORK_TASK_TERMINAL" => {
                AppError::BadRequest(message)
            }
            "VAL_USER_SETTINGS_INPUT" => AppError::BadRequest(message),
            // Everything else → 500
            _ => AppError::ServiceError {
                code: code.to_string(),
                message,
            },
        }
    }

    /// Stable low-cardinality classification for internal logs. Never include
    /// the error Display string here: lower layers can contain filesystem
    /// paths, provider text, SQL details, credentials or other sensitive data.
    fn internal_log_class(&self) -> &'static str {
        match self {
            AppError::ServiceError { .. } => "service",
            AppError::Internal(_) => "internal",
            AppError::Storage => "storage",
            AppError::Anyhow(_) => "anyhow",
            AppError::Serde(_) => "serde",
            AppError::Xlsx(_) => "xlsx",
            AppError::Calamine(_) => "calamine",
            _ => "client",
        }
    }
}

impl From<rusqlite::Error> for AppError {
    fn from(_: rusqlite::Error) -> Self {
        AppError::Storage
    }
}

impl From<r2d2::Error> for AppError {
    fn from(_: r2d2::Error) -> Self {
        AppError::Storage
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        // R4-P4 public API contract: every error response carries a stable code.
        // Domain-specific variants retain their precise codes while generic
        // transport/application variants use bounded HTTP-level fallbacks.
        let (status, code) = match &self {
            AppError::Unauthorized => (StatusCode::UNAUTHORIZED, "AUTH_UNAUTHORIZED"),
            AppError::Forbidden => (StatusCode::FORBIDDEN, "AUTH_FORBIDDEN"),
            AppError::CodedForbidden { code, .. } => (StatusCode::FORBIDDEN, code.as_str()),
            AppError::BadRequest(_) => (StatusCode::BAD_REQUEST, "HTTP_BAD_REQUEST"),
            AppError::CodedBadRequest { code, .. } => (StatusCode::BAD_REQUEST, code.as_str()),
            AppError::NotFound(_) => (StatusCode::NOT_FOUND, "HTTP_NOT_FOUND"),
            AppError::CodedNotFound { code, .. } => (StatusCode::NOT_FOUND, code.as_str()),
            AppError::Conflict(_) => (StatusCode::CONFLICT, "HTTP_CONFLICT"),
            AppError::Gone { code, .. } => (StatusCode::GONE, code.as_str()),
            AppError::CodedConflict { code, .. } => (StatusCode::CONFLICT, code.as_str()),
            AppError::RateLimited { .. } => (StatusCode::TOO_MANY_REQUESTS, "AUTH_RATE_LIMITED"),
            AppError::CodedRateLimited { code, .. } => {
                (StatusCode::TOO_MANY_REQUESTS, code.as_str())
            }
            AppError::ServiceError { .. }
            | AppError::Storage
            | AppError::Anyhow(_)
            | AppError::Serde(_)
            | AppError::Xlsx(_)
            | AppError::Calamine(_)
            | AppError::Internal(_) => (StatusCode::INTERNAL_SERVER_ERROR, "HTTP_INTERNAL_ERROR"),
        };

        let error_message = match &self {
            AppError::ServiceError { .. }
            | AppError::Internal(_)
            | AppError::Storage
            | AppError::Anyhow(_)
            | AppError::Serde(_)
            | AppError::Xlsx(_)
            | AppError::Calamine(_) => {
                tracing::error!(
                    error_class = self.internal_log_class(),
                    "internal request error"
                );
                "服务器内部错误".to_string()
            }
            _ => self.to_string(),
        };

        let body = ErrorBody {
            ok: false,
            code: Some(code.to_string()),
            error: error_message,
        };

        (status, Json(body)).into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;

    #[tokio::test]
    async fn internal_service_details_are_not_returned_to_clients() {
        let response = AppError::ServiceError {
            code: "SYS_DB_QUERY".into(),
            message: "no such table: tenant_preview_sessions at C:\\secret\\talos.db".into(),
        }
        .into_response();

        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let text = String::from_utf8(body.to_vec()).unwrap();
        assert!(!text.contains("tenant_preview_sessions"));
        assert!(!text.contains("secret"));
        assert!(!text.contains("SYS_DB_QUERY"));
        assert!(text.contains("HTTP_INTERNAL_ERROR"));
    }

    #[test]
    fn internal_log_class_never_contains_internal_message_material() {
        let error = AppError::ServiceError {
            code: "SECRET_CODE_SHOULD_NOT_LOG".into(),
            message: "Bearer super-secret-token".into(),
        };
        assert_eq!(error.internal_log_class(), "service");
        assert!(!error.internal_log_class().contains("SECRET"));
        assert!(!error.internal_log_class().contains("Bearer"));
    }

    #[test]
    fn sqlite_and_pool_errors_convert_into_backend_neutral_storage_surface() {
        fn assert_from<T>()
        where
            AppError: From<T>,
        {
        }

        assert_from::<rusqlite::Error>();
        assert_from::<r2d2::Error>();
        let storage = AppError::Storage;
        assert_eq!(storage.internal_log_class(), "storage");
    }

    #[tokio::test]
    async fn generic_client_errors_receive_stable_public_codes() {
        for (error, expected) in [
            (AppError::BadRequest("bad".into()), "HTTP_BAD_REQUEST"),
            (AppError::NotFound("missing".into()), "HTTP_NOT_FOUND"),
            (AppError::Conflict("stale".into()), "HTTP_CONFLICT"),
        ] {
            let response = error.into_response();
            let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
            let value: serde_json::Value = serde_json::from_slice(&body).unwrap();
            assert_eq!(value["code"], expected);
        }
    }

    #[tokio::test]
    async fn coded_not_found_preserves_the_preview_session_error_code() {
        let response = AppError::CodedNotFound {
            code: "PREVIEW_SESSION_NOT_FOUND".into(),
            message: "preview session not found".into(),
        }
        .into_response();
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let value: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(value["code"], "PREVIEW_SESSION_NOT_FOUND");
    }

    #[test]
    fn integration_error_payload_maps_to_stable_client_statuses() {
        let validation = AppError::from_error_payload(
            r#"{"category":"val","code":"VAL_INTEGRATION_INPUT","message":"invalid"}"#.into(),
        )
        .into_response();
        assert_eq!(validation.status(), StatusCode::BAD_REQUEST);

        let stale = AppError::from_error_payload(
            r#"{"category":"biz","code":"BIZ_INTEGRATION_CONFLICT","message":"stale"}"#.into(),
        )
        .into_response();
        assert_eq!(stale.status(), StatusCode::CONFLICT);
    }
}
