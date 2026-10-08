//! R4-P4 public Web/API governance primitives.
//!
//! This module is intentionally transport/route focused. It does not create a
//! second authorization model; it classifies ingress surfaces before requests
//! reach the existing session, machine, webhook and Registry authorities.

use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::Json;
use axum::extract::{DefaultBodyLimit, Request, State};
use axum::http::{HeaderName, HeaderValue, Method, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

use crate::config::AppConfig;
use crate::error::ErrorBody;

const API_PREFIXES: &[&str] = &[
    "/users",
    "/devices",
    "/audit-logs",
    "/tenant-memberships",
    "/api",
    "/meta",
    "/auth",
    "/metrics",
];

pub const JSON_REQUEST_BODY_LIMIT_BYTES: usize = 1024 * 1024;
pub const UPLOAD_REQUEST_BODY_LIMIT_BYTES: usize = 25 * 1024 * 1024;
pub const WEBHOOK_REQUEST_BODY_LIMIT_BYTES: usize = 256 * 1024;

pub const JSON_REQUEST_TIMEOUT_SECS: u64 = 60;
pub const EXTENDED_REQUEST_TIMEOUT_SECS: u64 = 180;
pub const WEBHOOK_REQUEST_TIMEOUT_SECS: u64 = 15;
pub const NO_BODY_REQUEST_TIMEOUT_SECS: u64 = 15;

const REQUEST_ID_HEADER: HeaderName = HeaderName::from_static("x-request-id");
const NOSNIFF_HEADER: HeaderName = HeaderName::from_static("x-content-type-options");
const REFERRER_POLICY_HEADER: HeaderName = HeaderName::from_static("referrer-policy");
const CSP_HEADER: HeaderName = HeaderName::from_static("content-security-policy");
const X_FRAME_OPTIONS_HEADER: HeaderName = HeaderName::from_static("x-frame-options");
const HSTS_HEADER: HeaderName = HeaderName::from_static("strict-transport-security");

// The production UI intentionally keeps only its existing Google Fonts fetches
// outside the TALOS origin. Script, API, frame and form authority remain same-origin.
const WEB_UI_CSP: &str = "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline' https://fonts.googleapis.com; font-src 'self' https://fonts.gstatic.com data:; img-src 'self' data: blob:; connect-src 'self'; frame-src 'self'; frame-ancestors 'self'; object-src 'none'; base-uri 'self'; form-action 'self'";
const HSTS_VALUE: &str = "max-age=31536000; includeSubDomains";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IngressSurface {
    BrowserSession,
    MachineBearer,
    ProviderWebhook,
    PublicDiagnostic,
    WebUi,
}

impl IngressSurface {
    pub fn classify(path: &str) -> Self {
        if path.starts_with("/api/machine/") {
            // `/api/machine-clients` deliberately does not match: provisioning
            // remains an administrator browser/session operation.
            Self::MachineBearer
        } else if path.starts_with("/api/integrations/webhooks/") {
            Self::ProviderWebhook
        } else if path == "/health" || path == "/ready" {
            Self::PublicDiagnostic
        } else if is_api_path(path) {
            Self::BrowserSession
        } else {
            Self::WebUi
        }
    }

    pub const fn csrf_exempt(self) -> bool {
        matches!(self, Self::MachineBearer | Self::ProviderWebhook)
    }

    pub const fn no_store(self) -> bool {
        matches!(
            self,
            Self::BrowserSession
                | Self::MachineBearer
                | Self::ProviderWebhook
                | Self::PublicDiagnostic
        )
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::BrowserSession => "browser-session",
            Self::MachineBearer => "machine-bearer",
            Self::ProviderWebhook => "provider-webhook",
            Self::PublicDiagnostic => "public-diagnostic",
            Self::WebUi => "web-ui",
        }
    }
}

/// HTTP resource profile is separate from authority classification. It is used
/// only to select bounded transport resources; it never grants permissions.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ApiResourceProfile {
    Json,
    Extended,
    Upload,
    Webhook,
    NoBody,
}

impl ApiResourceProfile {
    pub fn classify(path: &str) -> Self {
        match path {
            "/users/import-orders"
            | "/users/import-devices-by-orderno"
            | "/users/import-notes-by-orderno"
            | "/devices/import-excel" => Self::Upload,
            "/users/export" | "/devices/export" => Self::Extended,
            "/health" | "/ready" => Self::NoBody,
            _ if path.starts_with("/api/integrations/webhooks/") => Self::Webhook,
            _ if is_api_path(path) => Self::Json,
            _ => Self::NoBody,
        }
    }

    pub const fn request_body_limit_bytes(self) -> usize {
        match self {
            Self::Json | Self::Extended => JSON_REQUEST_BODY_LIMIT_BYTES,
            Self::Upload => UPLOAD_REQUEST_BODY_LIMIT_BYTES,
            Self::Webhook => WEBHOOK_REQUEST_BODY_LIMIT_BYTES,
            Self::NoBody => 0,
        }
    }

    /// Budget published to downstream handlers. Only safe/idempotent HTTP
    /// methods may be hard-cancelled by this middleware; mutation methods must
    /// not have their outcome erased by an outer transport timeout.
    pub const fn timeout_secs(self) -> u64 {
        match self {
            Self::Json => JSON_REQUEST_TIMEOUT_SECS,
            Self::Extended | Self::Upload => EXTENDED_REQUEST_TIMEOUT_SECS,
            Self::Webhook => WEBHOOK_REQUEST_TIMEOUT_SECS,
            Self::NoBody => NO_BODY_REQUEST_TIMEOUT_SECS,
        }
    }
}

#[derive(Clone, Debug)]
pub struct ApiGovernancePolicy {
    public_https: bool,
}

impl ApiGovernancePolicy {
    pub fn from_config(config: &AppConfig) -> Self {
        Self {
            public_https: config.public_https,
        }
    }

    #[cfg(test)]
    fn new(public_https: bool) -> Self {
        Self { public_https }
    }

    pub const fn hsts_enabled(&self) -> bool {
        self.public_https
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RequestId(String);

impl RequestId {
    fn generate() -> Self {
        Self(uuid::Uuid::new_v4().to_string())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Cooperative deadline made visible to application/extractor code.
///
/// For mutation methods this is intentionally *not* implemented by dropping the
/// handler future. A write may already have crossed its durable commit point;
/// returning 504 after cancelling that future would manufacture an Unknown
/// Outcome and invite unsafe retries. Downstream code may use `remaining()` for
/// admission or provider-call budgets without changing that outcome rule.
#[derive(Clone, Copy, Debug)]
pub struct ApiDeadline {
    expires_at: Instant,
}

impl ApiDeadline {
    fn after(duration: Duration) -> Self {
        Self {
            expires_at: Instant::now() + duration,
        }
    }

    pub fn remaining(&self) -> Duration {
        self.expires_at.saturating_duration_since(Instant::now())
    }
}

pub fn is_api_path(path: &str) -> bool {
    API_PREFIXES.iter().any(|prefix| path.starts_with(prefix))
}

fn hard_timeout_allowed(method: &Method) -> bool {
    matches!(*method, Method::GET | Method::HEAD | Method::OPTIONS)
}

fn response_is_json(response: &Response) -> bool {
    response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.to_ascii_lowercase().starts_with("application/json"))
}

fn framework_rejection(status: StatusCode) -> Option<(&'static str, &'static str)> {
    match status {
        StatusCode::BAD_REQUEST => Some(("HTTP_BAD_REQUEST", "Malformed HTTP request")),
        StatusCode::UNAUTHORIZED => Some(("AUTH_UNAUTHORIZED", "Authentication is required")),
        StatusCode::FORBIDDEN => Some(("AUTH_FORBIDDEN", "Request is forbidden")),
        StatusCode::METHOD_NOT_ALLOWED => Some((
            "HTTP_METHOD_NOT_ALLOWED",
            "HTTP method is not allowed for this resource",
        )),
        StatusCode::PAYLOAD_TOO_LARGE => Some((
            "HTTP_PAYLOAD_TOO_LARGE",
            "Request body exceeds the allowed API resource profile",
        )),
        StatusCode::UNSUPPORTED_MEDIA_TYPE => Some((
            "HTTP_UNSUPPORTED_MEDIA_TYPE",
            "Request Content-Type is not supported by this resource",
        )),
        StatusCode::UNPROCESSABLE_ENTITY => Some((
            "HTTP_UNPROCESSABLE_CONTENT",
            "Request representation does not satisfy the endpoint schema",
        )),
        StatusCode::TOO_MANY_REQUESTS => Some(("HTTP_RATE_LIMITED", "Request rate limit exceeded")),
        StatusCode::REQUEST_HEADER_FIELDS_TOO_LARGE => Some((
            "HTTP_REQUEST_HEADERS_TOO_LARGE",
            "Request headers exceed the allowed transport budget",
        )),
        StatusCode::INTERNAL_SERVER_ERROR => Some(("HTTP_INTERNAL_ERROR", "Internal server error")),
        _ => None,
    }
}

/// Framework/extractor rejections can occur before an application handler and
/// historically used Axum text/plain bodies. Preserve existing domain JSON
/// errors, but normalize non-JSON framework failures into the same stable API
/// envelope. Semantically useful transport headers survive normalization.
fn normalize_framework_rejection(response: Response) -> Response {
    if response_is_json(&response) {
        return response;
    }
    let status = response.status();
    let Some((code, message)) = framework_rejection(status) else {
        return response;
    };

    let allow = response.headers().get(header::ALLOW).cloned();
    let authenticate = response.headers().get(header::WWW_AUTHENTICATE).cloned();
    let retry_after = response.headers().get(header::RETRY_AFTER).cloned();
    let mut normalized = (
        status,
        Json(ErrorBody {
            ok: false,
            code: Some(code.to_string()),
            error: message.to_string(),
        }),
    )
        .into_response();
    if let Some(value) = allow {
        normalized.headers_mut().insert(header::ALLOW, value);
    }
    if let Some(value) = authenticate {
        normalized
            .headers_mut()
            .insert(header::WWW_AUTHENTICATE, value);
    }
    if let Some(value) = retry_after {
        normalized.headers_mut().insert(header::RETRY_AFTER, value);
    }
    normalized
}

fn timeout_response() -> Response {
    (
        StatusCode::GATEWAY_TIMEOUT,
        Json(ErrorBody {
            ok: false,
            code: Some("HTTP_DEADLINE_EXCEEDED".to_string()),
            error: "Safe request exceeded the allowed API execution deadline".to_string(),
        }),
    )
        .into_response()
}

/// Establishes server-owned request identity, ingress classification, bounded
/// body consumption and an explicit execution deadline budget.
///
/// Client supplied `X-Request-Id` is deliberately ignored as authority. A new
/// server request ID is generated for every accepted HTTP request and echoed in
/// the response. `DefaultBodyLimit::apply` preserves the original Axum body type,
/// so machine/webhook route-local admission still executes before extractors
/// consume the request body.
///
/// Hard timeout is deliberately limited to GET/HEAD/OPTIONS. Mutation methods
/// receive `ApiDeadline` but are allowed to complete so the HTTP layer cannot
/// turn an already-committed command into a false 504/unknown-outcome retry path.
pub async fn api_governance(
    State(policy): State<Arc<ApiGovernancePolicy>>,
    mut request: Request,
    next: Next,
) -> Response {
    let surface = IngressSurface::classify(request.uri().path());
    let profile = ApiResourceProfile::classify(request.uri().path());
    let request_id = RequestId::generate();
    let method = request.method().clone();
    let budget = Duration::from_secs(profile.timeout_secs());

    DefaultBodyLimit::max(profile.request_body_limit_bytes()).apply(&mut request);
    request.extensions_mut().insert(surface);
    request.extensions_mut().insert(profile);
    request.extensions_mut().insert(request_id.clone());
    request.extensions_mut().insert(ApiDeadline::after(budget));

    let mut response = if hard_timeout_allowed(&method) {
        match tokio::time::timeout(budget, next.run(request)).await {
            Ok(response) => response,
            Err(_) => timeout_response(),
        }
    } else {
        // Do not drop state-changing handler futures at an arbitrary outer
        // deadline. Registry/idempotency/provider-specific code owns outcome
        // semantics for mutations.
        next.run(request).await
    };

    if is_api_path_from_surface(surface) {
        response = normalize_framework_rejection(response);
    }

    let headers = response.headers_mut();

    if let Ok(value) = HeaderValue::from_str(request_id.as_str()) {
        headers.insert(REQUEST_ID_HEADER, value);
    }
    if !headers.contains_key(&NOSNIFF_HEADER) {
        headers.insert(NOSNIFF_HEADER, HeaderValue::from_static("nosniff"));
    }
    if !headers.contains_key(&REFERRER_POLICY_HEADER) {
        headers.insert(
            REFERRER_POLICY_HEADER,
            HeaderValue::from_static("no-referrer"),
        );
    }
    if surface.no_store() && !headers.contains_key(header::CACHE_CONTROL) {
        headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    }
    if matches!(surface, IngressSurface::WebUi) {
        if !headers.contains_key(&CSP_HEADER) {
            headers.insert(CSP_HEADER, HeaderValue::from_static(WEB_UI_CSP));
        }
        if !headers.contains_key(&X_FRAME_OPTIONS_HEADER) {
            headers.insert(
                X_FRAME_OPTIONS_HEADER,
                HeaderValue::from_static("SAMEORIGIN"),
            );
        }
    }
    if policy.hsts_enabled() && !headers.contains_key(&HSTS_HEADER) {
        headers.insert(HSTS_HEADER, HeaderValue::from_static(HSTS_VALUE));
    }

    response
}

const fn is_api_path_from_surface(surface: IngressSurface) -> bool {
    matches!(
        surface,
        IngressSurface::BrowserSession
            | IngressSurface::MachineBearer
            | IngressSurface::ProviderWebhook
            | IngressSurface::PublicDiagnostic
    )
}

#[cfg(test)]
mod tests {
    use super::{
        ApiGovernancePolicy, ApiResourceProfile, IngressSurface, framework_rejection,
        hard_timeout_allowed, is_api_path, is_api_path_from_surface,
    };
    use axum::http::{Method, StatusCode};

    #[test]
    fn classifies_browser_machine_webhook_and_public_surfaces_without_overlap() {
        assert_eq!(
            IngressSurface::classify("/api/machine/v1/tenants/t1/execute"),
            IngressSurface::MachineBearer
        );
        assert_eq!(
            IngressSurface::classify("/api/machine-clients"),
            IngressSurface::BrowserSession
        );
        assert_eq!(
            IngressSurface::classify("/api/integrations/webhooks/token"),
            IngressSurface::ProviderWebhook
        );
        assert_eq!(
            IngressSurface::classify("/health"),
            IngressSurface::PublicDiagnostic
        );
        assert_eq!(
            IngressSurface::classify("/ready"),
            IngressSurface::PublicDiagnostic
        );
        assert_eq!(
            IngressSurface::classify("/metrics"),
            IngressSurface::BrowserSession
        );
        assert_eq!(
            IngressSurface::classify("/app/orders"),
            IngressSurface::WebUi
        );
    }

    #[test]
    fn only_non_cookie_ingress_is_csrf_exempt() {
        assert!(IngressSurface::MachineBearer.csrf_exempt());
        assert!(IngressSurface::ProviderWebhook.csrf_exempt());
        assert!(!IngressSurface::BrowserSession.csrf_exempt());
        assert!(!IngressSurface::PublicDiagnostic.csrf_exempt());
        assert!(!IngressSurface::WebUi.csrf_exempt());
    }

    #[test]
    fn public_diagnostic_is_non_cacheable_and_uses_api_transport_errors() {
        assert!(IngressSurface::PublicDiagnostic.no_store());
        assert!(is_api_path_from_surface(IngressSurface::PublicDiagnostic));
    }

    #[test]
    fn resource_profiles_keep_uploads_webhooks_and_exports_explicit() {
        assert_eq!(
            ApiResourceProfile::classify("/users/import-orders"),
            ApiResourceProfile::Upload
        );
        assert_eq!(
            ApiResourceProfile::classify("/devices/import-excel"),
            ApiResourceProfile::Upload
        );
        assert_eq!(
            ApiResourceProfile::classify("/api/integrations/webhooks/token"),
            ApiResourceProfile::Webhook
        );
        assert_eq!(
            ApiResourceProfile::classify("/users/export"),
            ApiResourceProfile::Extended
        );
        assert_eq!(
            ApiResourceProfile::classify("/api/v2/orders"),
            ApiResourceProfile::Json
        );
        assert_eq!(
            ApiResourceProfile::classify("/metrics"),
            ApiResourceProfile::Json
        );
        assert_eq!(
            ApiResourceProfile::classify("/ready"),
            ApiResourceProfile::NoBody
        );
        assert!(
            ApiResourceProfile::Upload.request_body_limit_bytes()
                > ApiResourceProfile::Json.request_body_limit_bytes()
        );
        assert!(
            ApiResourceProfile::Webhook.request_body_limit_bytes()
                < ApiResourceProfile::Json.request_body_limit_bytes()
        );
        assert!(
            ApiResourceProfile::Extended.timeout_secs() > ApiResourceProfile::Json.timeout_secs()
        );
    }

    #[test]
    fn hard_timeout_never_cancels_mutation_methods() {
        assert!(hard_timeout_allowed(&Method::GET));
        assert!(hard_timeout_allowed(&Method::HEAD));
        assert!(hard_timeout_allowed(&Method::OPTIONS));
        assert!(!hard_timeout_allowed(&Method::POST));
        assert!(!hard_timeout_allowed(&Method::PUT));
        assert!(!hard_timeout_allowed(&Method::PATCH));
        assert!(!hard_timeout_allowed(&Method::DELETE));
    }

    #[test]
    fn framework_transport_errors_have_stable_codes() {
        assert_eq!(
            framework_rejection(StatusCode::UNSUPPORTED_MEDIA_TYPE).map(|value| value.0),
            Some("HTTP_UNSUPPORTED_MEDIA_TYPE")
        );
        assert_eq!(
            framework_rejection(StatusCode::METHOD_NOT_ALLOWED).map(|value| value.0),
            Some("HTTP_METHOD_NOT_ALLOWED")
        );
        assert_eq!(
            framework_rejection(StatusCode::PAYLOAD_TOO_LARGE).map(|value| value.0),
            Some("HTTP_PAYLOAD_TOO_LARGE")
        );
    }

    #[test]
    fn hsts_is_bound_to_explicit_public_https_profile() {
        assert!(ApiGovernancePolicy::new(true).hsts_enabled());
        assert!(!ApiGovernancePolicy::new(false).hsts_enabled());
    }

    #[test]
    fn api_not_found_classification_uses_the_same_prefix_contract() {
        assert!(is_api_path("/api/v2/orders/missing"));
        assert!(is_api_path("/auth/login"));
        assert!(is_api_path("/meta/modules"));
        assert!(is_api_path("/metrics"));
        assert!(!is_api_path("/app/orders"));
    }
}
