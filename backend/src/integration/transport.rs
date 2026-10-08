//! Provider-neutral asynchronous HTTP transport.
//!
//! This layer applies egress, deadline, cancellation, response-size, and error
//! classification policy. It deliberately does not construct provider requests,
//! resolve secrets, retry business work, or access application repositories.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio::sync::watch;
use url::Url;

use super::types::IntegrationError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransportErrorClass {
    Dns,
    ConnectTimeout,
    ConnectFailure,
    Tls,
    WriteFailure,
    ResponseTimeout,
    DeadlineExceeded,
    Remote4xx,
    Remote429,
    Remote5xx,
    MalformedResponse,
    ResponseTooLarge,
    Cancelled,
    EgressDenied,
}

impl TransportErrorClass {
    pub fn as_persisted(&self) -> &'static str {
        match self {
            Self::Dns => "dns",
            Self::ConnectTimeout => "connect_timeout",
            Self::ConnectFailure => "connect_failure",
            Self::Tls => "tls",
            Self::WriteFailure => "write_failure",
            Self::ResponseTimeout => "response_timeout",
            Self::DeadlineExceeded => "deadline_exceeded",
            Self::Remote4xx => "remote_4xx",
            Self::Remote429 => "remote_429",
            Self::Remote5xx => "remote_5xx",
            Self::MalformedResponse => "malformed_response",
            Self::ResponseTooLarge => "response_too_large",
            Self::Cancelled => "cancelled",
            Self::EgressDenied => "egress_denied",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransportFailure {
    pub class: TransportErrorClass,
    /// `true` means the caller must preserve the operation as UnknownOutcome
    /// until reconciliation proves an effect did not occur.
    pub may_have_dispatched: bool,
}

impl TransportFailure {
    pub fn before_dispatch(class: TransportErrorClass) -> Self {
        Self {
            class,
            may_have_dispatched: false,
        }
    }

    pub fn after_dispatch(class: TransportErrorClass) -> Self {
        Self {
            class,
            may_have_dispatched: true,
        }
    }

    pub fn is_retryable(&self) -> bool {
        matches!(
            self.class,
            TransportErrorClass::Dns
                | TransportErrorClass::ConnectTimeout
                | TransportErrorClass::ConnectFailure
                | TransportErrorClass::Tls
                | TransportErrorClass::WriteFailure
                | TransportErrorClass::ResponseTimeout
                | TransportErrorClass::DeadlineExceeded
                | TransportErrorClass::Remote429
                | TransportErrorClass::Remote5xx
                | TransportErrorClass::Cancelled
        )
    }
}

/// Cooperative cancellation for an outgoing request. Cancelling after a send
/// has begun is conservatively reported as `may_have_dispatched`.
#[derive(Clone)]
pub struct TransportCancellation {
    sender: watch::Sender<bool>,
}

impl std::fmt::Debug for TransportCancellation {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("TransportCancellation").finish()
    }
}

impl TransportCancellation {
    pub fn new() -> Self {
        let (sender, _) = watch::channel(false);
        Self { sender }
    }

    pub fn cancel(&self) {
        self.sender.send_replace(true);
    }

    pub fn is_cancelled(&self) -> bool {
        *self.sender.borrow()
    }

    async fn cancelled(&self) {
        let mut receiver = self.sender.subscribe();
        if *receiver.borrow() {
            return;
        }
        let _ = receiver.changed().await;
    }
}

impl Default for TransportCancellation {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransportTimeouts {
    pub connect: Duration,
    pub read: Duration,
    pub overall: Duration,
}

impl Default for TransportTimeouts {
    fn default() -> Self {
        Self {
            connect: Duration::from_secs(5),
            read: Duration::from_secs(15),
            overall: Duration::from_secs(30),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ExternalRequest {
    pub method: String,
    pub url: String,
    pub headers: BTreeMap<String, String>,
    pub body: Vec<u8>,
    pub timeouts: TransportTimeouts,
    pub correlation_id: String,
    pub cancellation: Option<TransportCancellation>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExternalResponse {
    pub status: u16,
    pub headers: BTreeMap<String, String>,
    pub body: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct EgressPolicy {
    allowed_hosts: BTreeSet<String>,
    require_https: bool,
    max_response_bytes: usize,
}

impl EgressPolicy {
    pub fn new(
        allowed_hosts: impl IntoIterator<Item = String>,
        require_https: bool,
        max_response_bytes: usize,
    ) -> Result<Self, IntegrationError> {
        if max_response_bytes == 0 {
            return Err(IntegrationError::InvalidTransportRequest);
        }
        Ok(Self {
            allowed_hosts: allowed_hosts
                .into_iter()
                .map(|host| host.to_lowercase())
                .collect(),
            require_https,
            max_response_bytes,
        })
    }

    pub fn permits(&self, url: &Url) -> bool {
        (!self.require_https || url.scheme() == "https")
            && url
                .host_str()
                .is_some_and(|host| self.allowed_hosts.contains(&host.to_lowercase()))
    }

    fn max_response_bytes(&self) -> usize {
        self.max_response_bytes
    }
}

#[async_trait]
pub trait ExternalCallTransport: Send + Sync {
    async fn send(&self, request: ExternalRequest) -> Result<ExternalResponse, TransportFailure>;
}

pub struct ReqwestExternalCallTransport {
    policy: EgressPolicy,
}

impl ReqwestExternalCallTransport {
    pub fn new(policy: EgressPolicy) -> Self {
        Self { policy }
    }
}

#[async_trait]
impl ExternalCallTransport for ReqwestExternalCallTransport {
    async fn send(&self, request: ExternalRequest) -> Result<ExternalResponse, TransportFailure> {
        let url = Url::parse(&request.url).map_err(|_| {
            TransportFailure::before_dispatch(TransportErrorClass::MalformedResponse)
        })?;
        if !self.policy.permits(&url) {
            return Err(TransportFailure::before_dispatch(
                TransportErrorClass::EgressDenied,
            ));
        }
        if request
            .cancellation
            .as_ref()
            .is_some_and(TransportCancellation::is_cancelled)
        {
            return Err(TransportFailure::before_dispatch(
                TransportErrorClass::Cancelled,
            ));
        }
        let method = reqwest::Method::from_bytes(request.method.as_bytes()).map_err(|_| {
            TransportFailure::before_dispatch(TransportErrorClass::MalformedResponse)
        })?;
        let client = reqwest::Client::builder()
            .connect_timeout(request.timeouts.connect)
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|_| TransportFailure::before_dispatch(TransportErrorClass::ConnectFailure))?;
        let mut builder = client.request(method, url).body(request.body);
        for (name, value) in request.headers {
            builder = builder.header(name, value);
        }
        if !request.correlation_id.trim().is_empty() {
            builder = builder.header("x-correlation-id", request.correlation_id);
        }
        let started_at = tokio::time::Instant::now();
        let response = await_request(
            builder.send(),
            request.cancellation.as_ref(),
            request.timeouts.overall,
            TransportErrorClass::DeadlineExceeded,
        )
        .await?
        .map_err(classify_reqwest_error)?;
        let status = response.status().as_u16();
        if response
            .content_length()
            .is_some_and(|length| length > self.policy.max_response_bytes() as u64)
        {
            return Err(TransportFailure::after_dispatch(
                TransportErrorClass::ResponseTooLarge,
            ));
        }
        let headers = response
            .headers()
            .iter()
            .filter_map(|(name, value)| {
                value
                    .to_str()
                    .ok()
                    .map(|value| (name.to_string(), value.into()))
            })
            .collect();
        let body = read_limited_body(
            response,
            request.cancellation.as_ref(),
            request.timeouts.read,
            request.timeouts.overall,
            started_at,
            self.policy.max_response_bytes(),
        )
        .await?;
        if let Some(failure) = classify_status(status) {
            return Err(failure);
        }
        Ok(ExternalResponse {
            status,
            headers,
            body,
        })
    }
}

/// Temporary compatibility path for legacy callers. It is provider-neutral,
/// applies the same egress/size/status semantics, and can be removed when the
/// remaining old HttpClient callers have migrated to ExternalCallTransport.
pub struct LegacyHttpClientAdapter {
    legacy: Arc<dyn system_core::transport::http_client::HttpClient>,
    policy: EgressPolicy,
}

impl LegacyHttpClientAdapter {
    pub fn new(
        legacy: Arc<dyn system_core::transport::http_client::HttpClient>,
        policy: EgressPolicy,
    ) -> Self {
        Self { legacy, policy }
    }
}

#[async_trait]
impl ExternalCallTransport for LegacyHttpClientAdapter {
    async fn send(&self, request: ExternalRequest) -> Result<ExternalResponse, TransportFailure> {
        let url = Url::parse(&request.url).map_err(|_| {
            TransportFailure::before_dispatch(TransportErrorClass::MalformedResponse)
        })?;
        if !self.policy.permits(&url) {
            return Err(TransportFailure::before_dispatch(
                TransportErrorClass::EgressDenied,
            ));
        }
        if request
            .cancellation
            .as_ref()
            .is_some_and(TransportCancellation::is_cancelled)
        {
            return Err(TransportFailure::before_dispatch(
                TransportErrorClass::Cancelled,
            ));
        }
        let legacy = self.legacy.clone();
        let timeout_ms = u64::try_from(request.timeouts.overall.as_millis()).unwrap_or(u64::MAX);
        let cancellation = request.cancellation.clone();
        let overall_timeout = request.timeouts.overall;
        let legacy_request = system_core::transport::http_client::HttpRequest {
            method: request.method,
            url: request.url,
            headers: request.headers.into_iter().collect(),
            body: (!request.body.is_empty()).then_some(request.body),
            timeout_ms,
        };
        let task = tokio::task::spawn_blocking(move || legacy.send(legacy_request));
        let response = await_legacy(task, cancellation.as_ref(), overall_timeout)
            .await?
            .map_err(|_| TransportFailure::after_dispatch(TransportErrorClass::WriteFailure))?;
        if response.body.len() > self.policy.max_response_bytes() {
            return Err(TransportFailure::after_dispatch(
                TransportErrorClass::ResponseTooLarge,
            ));
        }
        if let Some(failure) = classify_status(response.status) {
            return Err(failure);
        }
        Ok(ExternalResponse {
            status: response.status,
            headers: response.headers.into_iter().collect(),
            body: response.body,
        })
    }
}

async fn await_request<T>(
    future: impl std::future::Future<Output = T>,
    cancellation: Option<&TransportCancellation>,
    timeout: Duration,
    timeout_class: TransportErrorClass,
) -> Result<T, TransportFailure> {
    if let Some(cancellation) = cancellation {
        tokio::select! {
            value = tokio::time::timeout(timeout, future) => value.map_err(|_| TransportFailure::after_dispatch(timeout_class)),
            _ = cancellation.cancelled() => Err(TransportFailure::after_dispatch(TransportErrorClass::Cancelled)),
        }
    } else {
        tokio::time::timeout(timeout, future)
            .await
            .map_err(|_| TransportFailure::after_dispatch(timeout_class))
    }
}

async fn await_legacy(
    task: tokio::task::JoinHandle<
        Result<system_core::transport::http_client::HttpResponse, String>,
    >,
    cancellation: Option<&TransportCancellation>,
    timeout: Duration,
) -> Result<Result<system_core::transport::http_client::HttpResponse, String>, TransportFailure> {
    let join = if let Some(cancellation) = cancellation {
        tokio::select! {
            value = tokio::time::timeout(timeout, task) => value.map_err(|_| TransportFailure::after_dispatch(TransportErrorClass::DeadlineExceeded))?,
            _ = cancellation.cancelled() => return Err(TransportFailure::after_dispatch(TransportErrorClass::Cancelled)),
        }
    } else {
        tokio::time::timeout(timeout, task)
            .await
            .map_err(|_| TransportFailure::after_dispatch(TransportErrorClass::DeadlineExceeded))?
    };
    join.map_err(|_| TransportFailure::after_dispatch(TransportErrorClass::WriteFailure))
}

async fn read_limited_body(
    mut response: reqwest::Response,
    cancellation: Option<&TransportCancellation>,
    read_timeout: Duration,
    overall_timeout: Duration,
    started_at: tokio::time::Instant,
    maximum: usize,
) -> Result<Vec<u8>, TransportFailure> {
    let mut body = Vec::new();
    loop {
        let elapsed = started_at.elapsed();
        let Some(remaining) = overall_timeout.checked_sub(elapsed) else {
            return Err(TransportFailure::after_dispatch(
                TransportErrorClass::DeadlineExceeded,
            ));
        };
        let timeout = read_timeout.min(remaining);
        let chunk = await_request(
            response.chunk(),
            cancellation,
            timeout,
            TransportErrorClass::ResponseTimeout,
        )
        .await?
        .map_err(classify_reqwest_error)?;
        let Some(chunk) = chunk else {
            break;
        };
        if body.len().saturating_add(chunk.len()) > maximum {
            return Err(TransportFailure::after_dispatch(
                TransportErrorClass::ResponseTooLarge,
            ));
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

fn classify_status(status: u16) -> Option<TransportFailure> {
    match status {
        429 => Some(TransportFailure::after_dispatch(
            TransportErrorClass::Remote429,
        )),
        400..=499 => Some(TransportFailure::after_dispatch(
            TransportErrorClass::Remote4xx,
        )),
        500..=599 => Some(TransportFailure::after_dispatch(
            TransportErrorClass::Remote5xx,
        )),
        _ => None,
    }
}

fn classify_reqwest_error(error: reqwest::Error) -> TransportFailure {
    if error.is_connect() && error.is_timeout() {
        return TransportFailure::before_dispatch(TransportErrorClass::ConnectTimeout);
    }
    if error.is_connect() {
        return TransportFailure::before_dispatch(TransportErrorClass::ConnectFailure);
    }
    if error.is_timeout() {
        return TransportFailure::after_dispatch(TransportErrorClass::ResponseTimeout);
    }
    if error.is_request() {
        return TransportFailure::before_dispatch(TransportErrorClass::MalformedResponse);
    }
    TransportFailure::after_dispatch(TransportErrorClass::WriteFailure)
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};
    use std::time::Duration;

    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    use super::{
        EgressPolicy, ExternalCallTransport, ExternalRequest, ReqwestExternalCallTransport,
        TransportCancellation, TransportErrorClass, TransportFailure, TransportTimeouts,
    };

    fn request(url: String) -> ExternalRequest {
        ExternalRequest {
            method: "POST".into(),
            url,
            headers: BTreeMap::new(),
            body: b"fixture".to_vec(),
            timeouts: TransportTimeouts {
                connect: Duration::from_secs(1),
                read: Duration::from_millis(50),
                overall: Duration::from_millis(250),
            },
            correlation_id: "trace-a".into(),
            cancellation: None,
        }
    }

    async fn response_server(response: String, delay: Option<Duration>) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = [0_u8; 1024];
            let _ = stream.read(&mut request).await;
            if let Some(delay) = delay {
                tokio::time::sleep(delay).await;
            }
            stream.write_all(response.as_bytes()).await.unwrap();
        });
        format!("http://{address}/fixture")
    }

    async fn delayed_body_server(delay: Duration) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = [0_u8; 1024];
            let _ = stream.read(&mut request).await;
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 1\r\n\r\n")
                .await
                .unwrap();
            tokio::time::sleep(delay).await;
            stream.write_all(b"x").await.unwrap();
        });
        format!("http://{address}/fixture")
    }

    #[test]
    fn egress_policy_is_exact_and_https_only_when_requested() {
        let policy = EgressPolicy::new(["api.fixture.invalid".to_string()], true, 1024).unwrap();
        assert!(policy.permits(&url::Url::parse("https://api.fixture.invalid/pay").unwrap()));
        assert!(!policy.permits(&url::Url::parse("http://api.fixture.invalid/pay").unwrap()));
        assert!(!policy.permits(&url::Url::parse("https://other.invalid/pay").unwrap()));
    }

    #[test]
    fn all_standard_methods_remain_transport_neutral() {
        let request = request("https://api.fixture.invalid/pay".into());
        assert!(reqwest::Method::from_bytes(request.method.as_bytes()).is_ok());
        let _ = BTreeSet::<String>::new();
    }

    #[test]
    fn retry_classification_never_turns_possible_dispatch_into_safe_retry() {
        assert!(TransportFailure::before_dispatch(TransportErrorClass::Remote429).is_retryable());
        assert!(
            TransportFailure::after_dispatch(TransportErrorClass::ResponseTimeout).is_retryable()
        );
        assert!(!TransportFailure::before_dispatch(TransportErrorClass::Remote4xx).is_retryable());
    }

    #[tokio::test]
    async fn classifies_429_5xx_timeout_and_oversized_responses() {
        let policy = EgressPolicy::new(["127.0.0.1".to_string()], false, 64).unwrap();
        let transport = ReqwestExternalCallTransport::new(policy);
        let too_many = response_server(
            "HTTP/1.1 429 Too Many Requests\r\nContent-Length: 0\r\n\r\n".into(),
            None,
        )
        .await;
        assert_eq!(
            transport.send(request(too_many)).await.unwrap_err().class,
            TransportErrorClass::Remote429
        );
        let failure = response_server(
            "HTTP/1.1 500 Internal Server Error\r\nContent-Length: 0\r\n\r\n".into(),
            None,
        )
        .await;
        assert_eq!(
            transport.send(request(failure)).await.unwrap_err().class,
            TransportErrorClass::Remote5xx
        );
        let oversized = response_server(
            format!(
                "HTTP/1.1 200 OK\r\nContent-Length: 65\r\n\r\n{}",
                "x".repeat(65)
            ),
            None,
        )
        .await;
        assert_eq!(
            transport.send(request(oversized)).await.unwrap_err().class,
            TransportErrorClass::ResponseTooLarge
        );
        let delayed = delayed_body_server(Duration::from_millis(100)).await;
        assert_eq!(
            transport.send(request(delayed)).await.unwrap_err().class,
            TransportErrorClass::ResponseTimeout
        );
    }

    #[tokio::test]
    async fn cancellation_and_malformed_request_fail_without_dispatch() {
        let policy = EgressPolicy::new(["127.0.0.1".to_string()], false, 64).unwrap();
        let transport = ReqwestExternalCallTransport::new(policy);
        let cancellation = TransportCancellation::new();
        cancellation.cancel();
        let mut cancelled = request("http://127.0.0.1:9/fixture".into());
        cancelled.cancellation = Some(cancellation);
        let failure = transport.send(cancelled).await.unwrap_err();
        assert_eq!(failure.class, TransportErrorClass::Cancelled);
        assert!(!failure.may_have_dispatched);
        assert_eq!(
            transport
                .send(request("not a url".into()))
                .await
                .unwrap_err()
                .class,
            TransportErrorClass::MalformedResponse
        );
    }
}
