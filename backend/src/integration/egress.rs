//! R4-P5 governed outbound network transport.
//!
//! The transport binds one validated Core `EgressGrant` to a caller/capability
//! identity. It validates URL authority, resolves and classifies destination
//! addresses before dispatch, pins DNS to the validated address, enforces
//! concurrency/rate/timeout/response budgets, and emits secret-safe evidence.

use std::collections::VecDeque;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use ipnet::IpNet;
use system_core::transport::network::{
    EgressGrant, EgressMethodClass, EgressProtocol, EgressRedirectPolicy,
};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use url::Url;

use super::transport::{
    ExternalCallTransport, ExternalRequest, ExternalResponse, TransportCancellation,
    TransportErrorClass, TransportFailure,
};

// IANA IPv6 Global Unicast allocation snapshot, last updated 2025-10-10.
// P5 intentionally fails closed for unallocated/future IPv6 space: new IANA
// allocations require a deliberate source update before becoming Provider-public.
static IANA_ALLOCATED_PROVIDER_PUBLIC_IPV6: LazyLock<Vec<IpNet>> = LazyLock::new(|| {
    [
        "2001:200::/23",
        "2001:400::/23",
        "2001:600::/23",
        "2001:800::/22",
        "2001:c00::/23",
        "2001:e00::/23",
        "2001:1200::/23",
        "2001:1400::/22",
        "2001:1800::/23",
        "2001:1a00::/23",
        "2001:1c00::/22",
        "2001:2000::/19",
        "2001:4000::/23",
        "2001:4200::/23",
        "2001:4400::/23",
        "2001:4600::/23",
        "2001:4800::/23",
        "2001:4a00::/23",
        "2001:4c00::/23",
        "2001:5000::/20",
        "2001:8000::/19",
        "2001:a000::/20",
        "2001:b000::/20",
        "2003::/18",
        "2400::/12",
        "2410::/12",
        "2600::/12",
        "2610::/23",
        "2620::/23",
        "2630::/12",
        "2800::/12",
        "2a00::/12",
        "2a10::/12",
        "2c00::/12",
    ]
    .into_iter()
    .map(|prefix| prefix.parse::<IpNet>().expect("valid IANA IPv6 allocation"))
    .collect()
});

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DestinationClass {
    Public,
    Private,
    Loopback,
    LinkLocal,
    Metadata,
    Special,
}

impl DestinationClass {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Public => "public",
            Self::Private => "private",
            Self::Loopback => "loopback",
            Self::LinkLocal => "link_local",
            Self::Metadata => "metadata",
            Self::Special => "special",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EgressIdentity {
    pub actor_ref: String,
    pub capability: String,
    pub secret_purpose: Option<String>,
}

impl EgressIdentity {
    fn valid(&self) -> bool {
        valid_evidence_ref(&self.actor_ref)
            && valid_evidence_ref(&self.capability)
            && self
                .secret_purpose
                .as_deref()
                .map(valid_evidence_ref)
                .unwrap_or(true)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EgressEvidence {
    pub actor_ref: String,
    pub capability: String,
    pub grant_id: String,
    pub destination_host: String,
    pub destination_port: u16,
    pub destination_class: DestinationClass,
    pub policy_decision: &'static str,
    pub outcome: &'static str,
    pub correlation_id: String,
    pub latency_ms: u64,
    pub response_bytes: usize,
    pub status_class: Option<u16>,
}

pub trait EgressEvidenceSink: Send + Sync {
    fn record(&self, evidence: EgressEvidence);
}

#[derive(Default)]
pub struct NoopEgressEvidence;

impl EgressEvidenceSink for NoopEgressEvidence {
    fn record(&self, _evidence: EgressEvidence) {}
}

#[async_trait]
pub trait DestinationResolver: Send + Sync {
    async fn resolve(&self, host: &str, port: u16) -> Result<Vec<IpAddr>, TransportFailure>;
}

#[derive(Default)]
pub struct SystemDestinationResolver;

#[async_trait]
impl DestinationResolver for SystemDestinationResolver {
    async fn resolve(&self, host: &str, port: u16) -> Result<Vec<IpAddr>, TransportFailure> {
        if let Ok(address) = host.parse::<IpAddr>() {
            return Ok(vec![address]);
        }
        let resolved = tokio::net::lookup_host((host, port))
            .await
            .map_err(|_| TransportFailure::before_dispatch(TransportErrorClass::Dns))?;
        let mut addresses = Vec::new();
        for socket in resolved {
            if !addresses.contains(&socket.ip()) {
                addresses.push(socket.ip());
            }
        }
        if addresses.is_empty() {
            return Err(TransportFailure::before_dispatch(TransportErrorClass::Dns));
        }
        Ok(addresses)
    }
}

/// Deployment/profile-specific exceptions for exact named internal services.
///
/// The stable grant still fixes the host/port/protocol/path. This profile only
/// permits that exact host to resolve into one of the explicitly configured
/// internal CIDRs. Loopback, link-local, metadata and special-use addresses are
/// never enabled by production constructors.
#[derive(Debug, Clone, Default)]
pub struct DestinationPolicy {
    allowed_internal_cidrs: Vec<IpNet>,
    #[cfg(test)]
    allow_test_loopback: bool,
}

impl DestinationPolicy {
    pub fn public_only() -> Self {
        Self::default()
    }

    pub fn with_internal_cidrs(cidrs: impl IntoIterator<Item = IpNet>) -> Self {
        Self {
            allowed_internal_cidrs: cidrs.into_iter().collect(),
            #[cfg(test)]
            allow_test_loopback: false,
        }
    }

    #[cfg(test)]
    fn test_loopback_only() -> Self {
        Self {
            allowed_internal_cidrs: Vec::new(),
            allow_test_loopback: true,
        }
    }

    fn permits(&self, address: IpAddr) -> bool {
        let class = classify_destination(address);
        #[cfg(test)]
        if self.allow_test_loopback && class == DestinationClass::Loopback {
            return true;
        }
        match class {
            DestinationClass::Public => true,
            DestinationClass::Private => self
                .allowed_internal_cidrs
                .iter()
                .any(|cidr| cidr.contains(&address)),
            DestinationClass::Loopback
            | DestinationClass::LinkLocal
            | DestinationClass::Metadata
            | DestinationClass::Special => false,
        }
    }
}

pub struct GovernedEgressTransport {
    grant: EgressGrant,
    identity: EgressIdentity,
    destination_policy: DestinationPolicy,
    evidence: Arc<dyn EgressEvidenceSink>,
    resolver: Arc<dyn DestinationResolver>,
    concurrency: Arc<Semaphore>,
    rate_window: Mutex<VecDeque<Instant>>,
}

impl GovernedEgressTransport {
    pub fn new(
        grant: EgressGrant,
        identity: EgressIdentity,
        destination_policy: DestinationPolicy,
        evidence: Arc<dyn EgressEvidenceSink>,
    ) -> Result<Self, String> {
        Self::new_with_resolver(
            grant,
            identity,
            destination_policy,
            evidence,
            Arc::new(SystemDestinationResolver),
        )
    }

    pub fn new_with_resolver(
        grant: EgressGrant,
        identity: EgressIdentity,
        destination_policy: DestinationPolicy,
        evidence: Arc<dyn EgressEvidenceSink>,
        resolver: Arc<dyn DestinationResolver>,
    ) -> Result<Self, String> {
        grant
            .validate()
            .map_err(|error| format!("invalid egress grant: {error:?}"))?;
        if grant.redirect_policy != EgressRedirectPolicy::Deny {
            return Err(
                "redirect revalidation is not enabled in the R4-P5 stable transport".into(),
            );
        }
        if !identity.valid() || !grant.allows_secret_purpose(identity.secret_purpose.as_deref()) {
            return Err("egress identity/secret purpose is not granted".into());
        }
        Ok(Self {
            concurrency: Arc::new(Semaphore::new(grant.max_concurrency as usize)),
            grant,
            identity,
            destination_policy,
            evidence,
            resolver,
            rate_window: Mutex::new(VecDeque::new()),
        })
    }

    fn static_authority(&self, request: &ExternalRequest, url: &Url) -> Result<(String, u16), ()> {
        if !url.username().is_empty() || url.password().is_some() || url.fragment().is_some() {
            return Err(());
        }
        let protocol = EgressProtocol::from_scheme(url.scheme()).ok_or(())?;
        if !self.grant.allows_protocol(protocol) {
            return Err(());
        }
        // Secret-bearing application/provider traffic may never downgrade to
        // plaintext HTTP even when a broad grant also lists Http for non-secret use.
        if self.identity.secret_purpose.is_some() && protocol != EgressProtocol::Https {
            return Err(());
        }
        let host = url.host_str().ok_or(())?.to_ascii_lowercase();
        if host != self.grant.destination_host {
            return Err(());
        }
        let port = url
            .port_or_known_default()
            .unwrap_or_else(|| protocol.default_port());
        if !self.grant.allows_port(port) {
            return Err(());
        }
        let method_class = EgressMethodClass::from_http_method(&request.method).ok_or(())?;
        let path = url.path();
        if !self.grant.allows_method_class(method_class)
            || ambiguous_authority_path(path)
            || !self.grant.allows_path(path)
        {
            return Err(());
        }
        if request
            .headers
            .keys()
            .any(|name| forbidden_authority_header(name) || routing_override_header(name))
        {
            return Err(());
        }
        // `secret_purpose` is the trusted capability binding, but do not let a
        // caller accidentally leak common credential-bearing headers over HTTP
        // merely because that metadata was omitted. P6 will bind secret injection
        // to the shared plugin lifecycle; this is a transport-level backstop.
        if protocol != EgressProtocol::Https
            && request
                .headers
                .keys()
                .any(|name| sensitive_credential_header(name))
        {
            return Err(());
        }
        let connect_ms = duration_ms(request.timeouts.connect);
        let read_ms = duration_ms(request.timeouts.read);
        let overall_ms = duration_ms(request.timeouts.overall);
        if connect_ms == 0
            || read_ms == 0
            || overall_ms == 0
            || connect_ms > self.grant.max_connect_timeout_ms
            || read_ms > self.grant.max_read_timeout_ms
            || overall_ms > self.grant.max_overall_timeout_ms
        {
            return Err(());
        }
        Ok((host, port))
    }

    async fn resolve_and_validate_with_budget(
        &self,
        host: &str,
        port: u16,
        cancellation: Option<&TransportCancellation>,
        connect_timeout: Duration,
        overall_remaining: Duration,
    ) -> Result<(IpAddr, DestinationClass), (TransportFailure, DestinationClass)> {
        let timeout = connect_timeout.min(overall_remaining);
        let timeout_class = if overall_remaining <= connect_timeout {
            TransportErrorClass::DeadlineExceeded
        } else {
            TransportErrorClass::ConnectTimeout
        };
        let resolution = self.resolver.resolve(host, port);
        tokio::pin!(resolution);
        let deadline = tokio::time::sleep(timeout);
        tokio::pin!(deadline);
        let addresses = loop {
            tokio::select! {
                result = &mut resolution => {
                    break result.map_err(|failure| (failure, DestinationClass::Special))?;
                }
                _ = &mut deadline => {
                    return Err((
                        TransportFailure::before_dispatch(timeout_class),
                        DestinationClass::Special,
                    ));
                }
                _ = tokio::time::sleep(Duration::from_millis(10)), if cancellation.is_some() => {
                    if cancellation.is_some_and(TransportCancellation::is_cancelled) {
                        return Err((
                            TransportFailure::before_dispatch(TransportErrorClass::Cancelled),
                            DestinationClass::Special,
                        ));
                    }
                }
            }
        };
        if addresses.is_empty() {
            return Err((
                TransportFailure::before_dispatch(TransportErrorClass::Dns),
                DestinationClass::Special,
            ));
        }
        // Mixed public/private or otherwise mixed safe/unsafe answers fail
        // closed. The actual request is pinned to the first validated address,
        // preventing a second DNS lookup between validation and connect.
        if let Some(address) = addresses
            .iter()
            .copied()
            .find(|address| !self.destination_policy.permits(*address))
        {
            return Err((
                TransportFailure::before_dispatch(TransportErrorClass::EgressDenied),
                classify_destination(address),
            ));
        }
        let address = addresses[0];
        Ok((address, classify_destination(address)))
    }

    fn acquire_budget(&self) -> Result<OwnedSemaphorePermit, &'static str> {
        let permit = self
            .concurrency
            .clone()
            .try_acquire_owned()
            .map_err(|_| "concurrency_exhausted")?;
        let now = Instant::now();
        let mut window = self
            .rate_window
            .lock()
            .map_err(|_| "budget_state_failure")?;
        while window
            .front()
            .is_some_and(|started| now.duration_since(*started) >= Duration::from_secs(60))
        {
            window.pop_front();
        }
        if window.len() >= self.grant.rate_limit_per_minute as usize {
            drop(permit);
            return Err("rate_exhausted");
        }
        window.push_back(now);
        Ok(permit)
    }

    fn record_evidence(
        &self,
        host: &str,
        port: u16,
        class: DestinationClass,
        decision: &'static str,
        outcome: &'static str,
        request: &ExternalRequest,
        started: Instant,
        response_bytes: usize,
        status: Option<u16>,
    ) {
        self.evidence.record(EgressEvidence {
            actor_ref: self.identity.actor_ref.clone(),
            capability: self.identity.capability.clone(),
            grant_id: self.grant.grant_id.clone(),
            destination_host: host.to_owned(),
            destination_port: port,
            destination_class: class,
            policy_decision: decision,
            outcome,
            correlation_id: request.correlation_id.clone(),
            latency_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
            response_bytes,
            status_class: status.map(|value| (value / 100) * 100),
        });
    }

    fn record_static_denial(
        &self,
        request: &ExternalRequest,
        url: Option<&Url>,
        started: Instant,
        outcome: &'static str,
    ) {
        let host = url
            .and_then(Url::host_str)
            .unwrap_or(self.grant.destination_host.as_str());
        let port = url
            .and_then(Url::port_or_known_default)
            .or_else(|| self.grant.ports.first().copied())
            .unwrap_or(0);
        let class = host
            .parse::<IpAddr>()
            .ok()
            .map(classify_destination)
            .unwrap_or(DestinationClass::Special);
        self.record_evidence(
            host, port, class, "deny", outcome, request, started, 0, None,
        );
    }
}

#[async_trait]
impl ExternalCallTransport for GovernedEgressTransport {
    async fn send(&self, request: ExternalRequest) -> Result<ExternalResponse, TransportFailure> {
        let started = Instant::now();
        let url = match Url::parse(&request.url) {
            Ok(url) => url,
            Err(_) => {
                self.record_static_denial(&request, None, started, "malformed_url");
                return Err(TransportFailure::before_dispatch(
                    TransportErrorClass::EgressDenied,
                ));
            }
        };
        let (host, port) = match self.static_authority(&request, &url) {
            Ok(authority) => authority,
            Err(()) => {
                self.record_static_denial(&request, Some(&url), started, "authority_rejected");
                return Err(TransportFailure::before_dispatch(
                    TransportErrorClass::EgressDenied,
                ));
            }
        };
        // Rate/concurrency authority covers the complete network attempt,
        // beginning before DNS resolution. An exhausted grant must not be able
        // to consume resolver or socket resources outside its budget.
        let _permit = match self.acquire_budget() {
            Ok(permit) => permit,
            Err(outcome) => {
                self.record_evidence(
                    &host,
                    port,
                    DestinationClass::Special,
                    "deny",
                    outcome,
                    &request,
                    started,
                    0,
                    None,
                );
                return Err(TransportFailure::before_dispatch(
                    TransportErrorClass::EgressDenied,
                ));
            }
        };
        let Some(overall_remaining) = request.timeouts.overall.checked_sub(started.elapsed())
        else {
            self.record_evidence(
                &host,
                port,
                DestinationClass::Special,
                "allow",
                "deadline_exceeded",
                &request,
                started,
                0,
                None,
            );
            return Err(TransportFailure::before_dispatch(
                TransportErrorClass::DeadlineExceeded,
            ));
        };
        let (address, class) = match self
            .resolve_and_validate_with_budget(
                &host,
                port,
                request.cancellation.as_ref(),
                request.timeouts.connect,
                overall_remaining,
            )
            .await
        {
            Ok(value) => value,
            Err((failure, rejected_class)) => {
                self.record_evidence(
                    &host,
                    port,
                    rejected_class,
                    if failure.class == TransportErrorClass::EgressDenied {
                        "deny"
                    } else {
                        "allow"
                    },
                    transport_outcome(failure.class),
                    &request,
                    started,
                    0,
                    None,
                );
                return Err(failure);
            }
        };
        if request
            .cancellation
            .as_ref()
            .is_some_and(TransportCancellation::is_cancelled)
        {
            self.record_evidence(
                &host,
                port,
                class,
                "allow",
                "cancelled_before_dispatch",
                &request,
                started,
                0,
                None,
            );
            return Err(TransportFailure::before_dispatch(
                TransportErrorClass::Cancelled,
            ));
        }

        let Some(connect_remaining) = request.timeouts.connect.checked_sub(started.elapsed())
        else {
            self.record_evidence(
                &host,
                port,
                class,
                "allow",
                "connect_timeout",
                &request,
                started,
                0,
                None,
            );
            return Err(TransportFailure::before_dispatch(
                TransportErrorClass::ConnectTimeout,
            ));
        };

        let method = match reqwest::Method::from_bytes(request.method.as_bytes()) {
            Ok(method) => method,
            Err(_) => {
                self.record_static_denial(&request, Some(&url), started, "method_rejected");
                return Err(TransportFailure::before_dispatch(
                    TransportErrorClass::EgressDenied,
                ));
            }
        };
        let mut builder = reqwest::Client::builder()
            .connect_timeout(connect_remaining)
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy();
        if host.parse::<IpAddr>().is_err() {
            builder = builder.resolve(&host, SocketAddr::new(address, port));
        }
        let client = match builder.build() {
            Ok(client) => client,
            Err(_) => {
                self.record_evidence(
                    &host,
                    port,
                    class,
                    "allow",
                    "client_build_failure",
                    &request,
                    started,
                    0,
                    None,
                );
                return Err(TransportFailure::before_dispatch(
                    TransportErrorClass::ConnectFailure,
                ));
            }
        };
        let mut outbound = client.request(method, url).body(request.body.clone());
        for (name, value) in &request.headers {
            outbound = outbound.header(name, value);
        }
        if !request.correlation_id.trim().is_empty() {
            outbound = outbound.header("x-correlation-id", &request.correlation_id);
        }

        let Some(send_remaining) = request.timeouts.overall.checked_sub(started.elapsed()) else {
            self.record_evidence(
                &host,
                port,
                class,
                "allow",
                "deadline_exceeded",
                &request,
                started,
                0,
                None,
            );
            return Err(TransportFailure::before_dispatch(
                TransportErrorClass::DeadlineExceeded,
            ));
        };
        let response = match await_network_future(
            outbound.send(),
            request.cancellation.as_ref(),
            send_remaining,
            TransportErrorClass::DeadlineExceeded,
        )
        .await
        {
            Ok(Ok(response)) => response,
            Ok(Err(error)) => {
                let failure = classify_reqwest_error(error);
                self.record_evidence(
                    &host,
                    port,
                    class,
                    "allow",
                    transport_outcome(failure.class),
                    &request,
                    started,
                    0,
                    None,
                );
                return Err(failure);
            }
            Err(failure) => {
                self.record_evidence(
                    &host,
                    port,
                    class,
                    "allow",
                    transport_outcome(failure.class),
                    &request,
                    started,
                    0,
                    None,
                );
                return Err(failure);
            }
        };
        let status = response.status().as_u16();
        if response
            .content_length()
            .is_some_and(|length| length > self.grant.max_response_bytes as u64)
        {
            self.record_evidence(
                &host,
                port,
                class,
                "allow",
                "response_too_large",
                &request,
                started,
                0,
                Some(status),
            );
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
                    .map(|value| (name.to_string(), value.to_owned()))
            })
            .collect();
        let body = match read_limited_body(
            response,
            request.cancellation.as_ref(),
            request.timeouts.read,
            request.timeouts.overall,
            started,
            self.grant.max_response_bytes,
        )
        .await
        {
            Ok(body) => body,
            Err(failure) => {
                self.record_evidence(
                    &host,
                    port,
                    class,
                    "allow",
                    transport_outcome(failure.class),
                    &request,
                    started,
                    0,
                    Some(status),
                );
                return Err(failure);
            }
        };
        let result = classify_status(status).map_or_else(
            || {
                Ok(ExternalResponse {
                    status,
                    headers,
                    body: body.clone(),
                })
            },
            Err,
        );
        self.record_evidence(
            &host,
            port,
            class,
            "allow",
            match &result {
                Ok(_) => "success",
                Err(failure) => transport_outcome(failure.class),
            },
            &request,
            started,
            body.len(),
            Some(status),
        );
        result
    }
}

async fn await_network_future<T>(
    future: impl std::future::Future<Output = T>,
    cancellation: Option<&TransportCancellation>,
    timeout: Duration,
    timeout_class: TransportErrorClass,
) -> Result<T, TransportFailure> {
    let deadline = tokio::time::sleep(timeout);
    tokio::pin!(deadline);
    tokio::pin!(future);
    loop {
        tokio::select! {
            value = &mut future => return Ok(value),
            _ = &mut deadline => return Err(TransportFailure::after_dispatch(timeout_class)),
            _ = tokio::time::sleep(Duration::from_millis(10)), if cancellation.is_some() => {
                if cancellation.is_some_and(TransportCancellation::is_cancelled) {
                    return Err(TransportFailure::after_dispatch(TransportErrorClass::Cancelled));
                }
            }
        }
    }
}

async fn read_limited_body(
    mut response: reqwest::Response,
    cancellation: Option<&TransportCancellation>,
    read_timeout: Duration,
    overall_timeout: Duration,
    started: Instant,
    maximum: usize,
) -> Result<Vec<u8>, TransportFailure> {
    let mut body = Vec::new();
    loop {
        let elapsed = started.elapsed();
        let Some(remaining) = overall_timeout.checked_sub(elapsed) else {
            return Err(TransportFailure::after_dispatch(
                TransportErrorClass::DeadlineExceeded,
            ));
        };
        let timeout = read_timeout.min(remaining);
        let chunk = await_network_future(
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
        300..=399 => Some(TransportFailure::after_dispatch(
            TransportErrorClass::EgressDenied,
        )),
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
        return TransportFailure::before_dispatch(TransportErrorClass::EgressDenied);
    }
    TransportFailure::after_dispatch(TransportErrorClass::WriteFailure)
}

fn transport_outcome(class: TransportErrorClass) -> &'static str {
    match class {
        TransportErrorClass::Dns => "dns_failure",
        TransportErrorClass::ConnectTimeout => "connect_timeout",
        TransportErrorClass::ConnectFailure => "connect_failure",
        TransportErrorClass::Tls => "tls_failure",
        TransportErrorClass::WriteFailure => "write_failure",
        TransportErrorClass::ResponseTimeout => "response_timeout",
        TransportErrorClass::DeadlineExceeded => "deadline_exceeded",
        TransportErrorClass::Remote4xx => "remote_4xx",
        TransportErrorClass::Remote429 => "remote_429",
        TransportErrorClass::Remote5xx => "remote_5xx",
        TransportErrorClass::MalformedResponse => "malformed_response",
        TransportErrorClass::ResponseTooLarge => "response_too_large",
        TransportErrorClass::Cancelled => "cancelled",
        TransportErrorClass::EgressDenied => "egress_denied",
    }
}

/// Reject path spellings whose downstream decode/normalization could change
/// the authority-relevant segment structure after this gateway has admitted it.
/// Ordinary percent-encoding remains available; only structural delimiters,
/// dot traversal, percent re-encoding and control bytes fail closed.
fn ambiguous_authority_path(path: &str) -> bool {
    let bytes = path.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'\\' {
            return true;
        }
        if bytes[index] != b'%' {
            index += 1;
            continue;
        }
        if index + 2 >= bytes.len() {
            return true;
        }
        let Some(high) = hex_nibble(bytes[index + 1]) else {
            return true;
        };
        let Some(low) = hex_nibble(bytes[index + 2]) else {
            return true;
        };
        let decoded = (high << 4) | low;
        if matches!(
            decoded,
            b'.' | b'/' | b'\\' | b'%' | b'?' | b'#' | 0..=0x1f | 0x7f
        ) {
            return true;
        }
        index += 3;
    }
    false
}

fn hex_nibble(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

/// Transport framing, hop-by-hop routing and runtime correlation are gateway
/// authority. Provider/application callers may only supply end-to-end headers.
fn forbidden_authority_header(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "host"
            | "connection"
            | "keep-alive"
            | "proxy-authorization"
            | "proxy-connection"
            | "te"
            | "trailer"
            | "transfer-encoding"
            | "upgrade"
            | "content-length"
            | "x-correlation-id"
    )
}

/// Headers that can make a downstream proxy/framework reinterpret host,
/// scheme, path, client identity or HTTP method are gateway authority. A grant
/// authorizes the URL/method seen here, not a second target encoded in metadata.
fn routing_override_header(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    name == "forwarded"
        || name.starts_with("x-forwarded-")
        || name.starts_with("x-original-")
        || name.starts_with("x-rewrite-")
        || matches!(
            name.as_str(),
            "x-http-method-override"
                | "x-http-method"
                | "x-method-override"
                | "x-envoy-original-path"
                | "x-envoy-original-dst-host"
        )
}

/// Common credential-bearing headers get an HTTPS backstop even when the
/// caller failed to declare a secret purpose. This is deliberately a narrow
/// defense-in-depth classifier; P6 remains responsible for trusted secret
/// injection and purpose binding.
fn sensitive_credential_header(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    matches!(
        name.as_str(),
        "authorization"
            | "cookie"
            | "x-api-key"
            | "api-key"
            | "x-auth-token"
            | "x-amz-security-token"
            | "x-goog-api-key"
    ) || name.contains("authorization")
        || name.contains("api-key")
        || name.contains("apikey")
        || name.contains("auth-token")
        || name.contains("security-token")
}

fn duration_ms(value: Duration) -> u64 {
    u64::try_from(value.as_millis()).unwrap_or(u64::MAX)
}

fn valid_evidence_ref(value: &str) -> bool {
    !value.trim().is_empty() && value.len() <= 256 && !value.chars().any(char::is_control)
}

pub fn classify_destination(address: IpAddr) -> DestinationClass {
    match address {
        IpAddr::V4(address) => classify_v4(address),
        IpAddr::V6(address) => {
            if let Some(mapped) = address.to_ipv4_mapped() {
                classify_v4(mapped)
            } else {
                classify_v6(address)
            }
        }
    }
}

fn classify_v4(address: Ipv4Addr) -> DestinationClass {
    let octets = address.octets();
    if address == Ipv4Addr::new(169, 254, 169, 254) {
        return DestinationClass::Metadata;
    }
    if address.is_loopback() {
        return DestinationClass::Loopback;
    }
    if address.is_link_local() {
        return DestinationClass::LinkLocal;
    }
    if address.is_private() {
        return DestinationClass::Private;
    }
    if address.is_unspecified()
        || address.is_multicast()
        || address == Ipv4Addr::BROADCAST
        || octets[0] == 0
        || in_v4_prefix(octets, [100, 64, 0, 0], 10)
        || in_v4_prefix(octets, [192, 0, 0, 0], 24)
        || in_v4_prefix(octets, [192, 0, 2, 0], 24)
        || in_v4_prefix(octets, [192, 31, 196, 0], 24)
        || in_v4_prefix(octets, [192, 52, 193, 0], 24)
        || in_v4_prefix(octets, [192, 88, 99, 0], 24)
        || in_v4_prefix(octets, [192, 175, 48, 0], 24)
        || in_v4_prefix(octets, [198, 18, 0, 0], 15)
        || in_v4_prefix(octets, [198, 51, 100, 0], 24)
        || in_v4_prefix(octets, [203, 0, 113, 0], 24)
        || octets[0] >= 240
    {
        return DestinationClass::Special;
    }
    DestinationClass::Public
}

fn classify_v6(address: Ipv6Addr) -> DestinationClass {
    let segments = address.segments();
    if address.is_loopback() {
        return DestinationClass::Loopback;
    }
    if address.is_unicast_link_local() {
        return DestinationClass::LinkLocal;
    }
    if address.is_unique_local() {
        return DestinationClass::Private;
    }
    if address.is_unspecified() || address.is_multicast() {
        return DestinationClass::Special;
    }

    // RFC 6052 Well-Known Prefix. Decode the embedded IPv4 identity rather
    // than trusting the IPv6 shell: a malicious DNS answer must not smuggle
    // loopback/private/link-local/metadata IPv4 through NAT64 syntax.
    if segments[..6] == [0x0064, 0xff9b, 0, 0, 0, 0] {
        return classify_v4(embedded_v4_tail(address));
    }

    // Deprecated IPv4-compatible ::/96 forms have no role in the R4 stable
    // public profile. :: and ::1 were classified above; all other forms stay
    // special rather than gaining a second IPv4 normalization path.
    if segments[..6] == [0, 0, 0, 0, 0, 0] {
        return DestinationClass::Special;
    }

    // IANA special-purpose IPv6 space that is not appropriate as an ambient
    // public Provider destination. More-specific globally reachable protocol
    // anycasts are intentionally not generalized into the Provider egress
    // profile; a future named profile can admit them explicitly if required.
    if (segments[0] == 0x0064 && segments[1] == 0xff9b && segments[2] == 0x0001) // 64:ff9b:1::/48
        || (segments[0] == 0x0100
            && segments[1] == 0
            && segments[2] == 0
            && segments[3] == 0) // 100::/64
        || (segments[0] == 0x0100
            && segments[1] == 0
            && segments[2] == 0
            && segments[3] == 1) // 100:0:0:1::/64
        || (segments[0] == 0x2001 && segments[1] == 0) // Teredo 2001::/32
        || (segments[0] == 0x2001 && segments[1] == 0x0002 && segments[2] == 0) // benchmark 2001:2::/48
        || (segments[0] == 0x2001 && (segments[1] & 0xfff0) == 0x0010) // deprecated ORCHID 2001:10::/28
        || (segments[0] == 0x2001 && segments[1] == 0x0db8) // documentation 2001:db8::/32
        || segments[0] == 0x2002 // 6to4 2002::/16
        || (segments[0] == 0x2620 && segments[1] == 0x004f && segments[2] == 0x8000) // AS112 2620:4f:8000::/48
        || (segments[0] == 0x3fff && (segments[1] & 0xf000) == 0) // documentation 3fff::/20
        || segments[0] == 0x5f00 // SRv6 SIDs 5f00::/16
        || (segments[0] & 0xffc0) == 0xfec0
    // deprecated site-local fec0::/10
    {
        return DestinationClass::Special;
    }

    if IANA_ALLOCATED_PROVIDER_PUBLIC_IPV6
        .iter()
        .any(|network| network.contains(&IpAddr::V6(address)))
    {
        DestinationClass::Public
    } else {
        DestinationClass::Special
    }
}

fn embedded_v4_tail(address: Ipv6Addr) -> Ipv4Addr {
    let octets = address.octets();
    Ipv4Addr::new(octets[12], octets[13], octets[14], octets[15])
}

fn in_v4_prefix(address: [u8; 4], network: [u8; 4], prefix: u8) -> bool {
    let address = u32::from_be_bytes(address);
    let network = u32::from_be_bytes(network);
    let mask = if prefix == 0 {
        0
    } else {
        u32::MAX << (32 - u32::from(prefix))
    };
    address & mask == network & mask
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::net::{IpAddr, Ipv4Addr};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use async_trait::async_trait;
    use system_core::transport::network::{
        EgressGrant, EgressMethodClass, EgressProtocol, EgressRedirectPolicy,
    };
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    use super::{
        DestinationClass, DestinationPolicy, DestinationResolver, EgressEvidence,
        EgressEvidenceSink, EgressIdentity, GovernedEgressTransport, classify_destination,
    };
    use crate::integration::transport::{
        ExternalCallTransport, ExternalRequest, TransportCancellation, TransportErrorClass,
        TransportFailure, TransportTimeouts,
    };

    #[derive(Default)]
    struct Evidence(Mutex<Vec<EgressEvidence>>);

    impl EgressEvidenceSink for Evidence {
        fn record(&self, evidence: EgressEvidence) {
            self.0.lock().unwrap().push(evidence);
        }
    }

    struct Resolver {
        answers: Vec<IpAddr>,
        calls: Mutex<u32>,
    }

    #[async_trait]
    impl DestinationResolver for Resolver {
        async fn resolve(&self, _host: &str, _port: u16) -> Result<Vec<IpAddr>, TransportFailure> {
            *self.calls.lock().unwrap() += 1;
            Ok(self.answers.clone())
        }
    }

    struct SlowResolver {
        answer: IpAddr,
        delay: Duration,
    }

    #[async_trait]
    impl DestinationResolver for SlowResolver {
        async fn resolve(&self, _host: &str, _port: u16) -> Result<Vec<IpAddr>, TransportFailure> {
            tokio::time::sleep(self.delay).await;
            Ok(vec![self.answer])
        }
    }

    fn grant(host: &str, port: u16) -> EgressGrant {
        EgressGrant {
            grant_id: "fixture.egress".into(),
            destination_host: host.into(),
            ports: vec![port],
            protocols: vec![EgressProtocol::Https],
            method_classes: vec![EgressMethodClass::Mutation],
            path_prefixes: vec!["/v1".into()],
            redirect_policy: EgressRedirectPolicy::Deny,
            max_connect_timeout_ms: 1_000,
            max_read_timeout_ms: 1_000,
            max_overall_timeout_ms: 2_000,
            max_response_bytes: 64,
            max_concurrency: 1,
            rate_limit_per_minute: 2,
            secret_purposes: vec!["fixture.secret".into()],
        }
    }

    // single_authority_fixture_grant: alternate protocol or method
    // authority requires a distinct grant rather than a Cartesian product.
    fn identity() -> EgressIdentity {
        EgressIdentity {
            actor_ref: "fixture-worker".into(),
            capability: "fixture.network".into(),
            secret_purpose: Some("fixture.secret".into()),
        }
    }

    fn identity_without_secret() -> EgressIdentity {
        let mut value = identity();
        value.secret_purpose = None;
        value
    }

    fn request(url: String) -> ExternalRequest {
        ExternalRequest {
            method: "POST".into(),
            url,
            headers: BTreeMap::new(),
            body: b"fixture".to_vec(),
            timeouts: TransportTimeouts {
                connect: Duration::from_millis(500),
                read: Duration::from_millis(500),
                overall: Duration::from_millis(1_000),
            },
            correlation_id: "corr-a".into(),
            cancellation: None,
        }
    }

    #[test]
    fn destination_classification_blocks_ssrf_classes() {
        for (address, expected) in [
            ("127.0.0.1", DestinationClass::Loopback),
            ("10.1.2.3", DestinationClass::Private),
            ("0.0.0.1", DestinationClass::Special),
            ("169.254.169.254", DestinationClass::Metadata),
            ("169.254.1.2", DestinationClass::LinkLocal),
            ("::1", DestinationClass::Loopback),
            ("fc00::1", DestinationClass::Private),
            ("fe80::1", DestinationClass::LinkLocal),
            ("fec0::1", DestinationClass::Special),
            ("::ffff:127.0.0.1", DestinationClass::Loopback),
            ("::ffff:169.254.169.254", DestinationClass::Metadata),
            ("64:ff9b::169.254.169.254", DestinationClass::Metadata),
            ("64:ff9b::10.0.0.1", DestinationClass::Private),
            ("64:ff9b::8.8.8.8", DestinationClass::Public),
            ("64:ff9b:1::1", DestinationClass::Special),
            ("100::1", DestinationClass::Special),
            ("100:0:0:1::1", DestinationClass::Special),
            ("2001::1", DestinationClass::Special),
            ("2001:2::1", DestinationClass::Special),
            ("2001:10::1", DestinationClass::Special),
            ("2001:db8::1", DestinationClass::Special),
            ("2002::1", DestinationClass::Special),
            ("3fff::1", DestinationClass::Special),
            ("5f00::1", DestinationClass::Special),
            ("::192.0.2.1", DestinationClass::Special),
            ("8.8.8.8", DestinationClass::Public),
            ("192.31.196.1", DestinationClass::Special),
            ("192.52.193.1", DestinationClass::Special),
            ("192.88.99.2", DestinationClass::Special),
            ("192.175.48.1", DestinationClass::Special),
            ("2620:4f:8000::1", DestinationClass::Special),
            ("2606:4700:4700::1111", DestinationClass::Public),
            ("2001:4860:4860::8888", DestinationClass::Public),
            ("2d00::1", DestinationClass::Special),
            ("3000::1", DestinationClass::Special),
            ("4000::1", DestinationClass::Special),
        ] {
            assert_eq!(
                classify_destination(address.parse().unwrap()),
                expected,
                "unexpected classification for {address}"
            );
        }
    }

    #[tokio::test]
    async fn loopback_and_encoded_ipv4_literals_are_denied_before_dispatch_and_audited() {
        for url in [
            "https://127.0.0.1/v1/test",
            "https://2130706433/v1/test",
            "https://0x7f000001/v1/test",
        ] {
            let parsed = url::Url::parse(url).unwrap();
            let canonical_host = parsed.host_str().unwrap().to_owned();
            let evidence = Arc::new(Evidence::default());
            let transport = GovernedEgressTransport::new(
                grant(&canonical_host, 443),
                identity(),
                DestinationPolicy::public_only(),
                evidence.clone(),
            )
            .unwrap();
            let failure = transport.send(request(url.into())).await.unwrap_err();
            assert_eq!(failure.class, TransportErrorClass::EgressDenied);
            assert!(!failure.may_have_dispatched);
            let events = evidence.0.lock().unwrap();
            assert_eq!(events.len(), 1);
            assert_eq!(events[0].policy_decision, "deny");
            assert_eq!(events[0].outcome, "egress_denied");
        }
    }

    #[tokio::test]
    async fn mixed_dns_answer_fails_closed_and_resolver_is_called_once() {
        let resolver = Arc::new(Resolver {
            answers: vec!["8.8.8.8".parse().unwrap(), "10.0.0.8".parse().unwrap()],
            calls: Mutex::new(0),
        });
        let evidence = Arc::new(Evidence::default());
        let transport = GovernedEgressTransport::new_with_resolver(
            grant("api.example.test", 443),
            identity(),
            DestinationPolicy::public_only(),
            evidence.clone(),
            resolver.clone(),
        )
        .unwrap();
        let failure = transport
            .send(request("https://api.example.test/v1/test".into()))
            .await
            .unwrap_err();
        assert_eq!(failure.class, TransportErrorClass::EgressDenied);
        assert!(!failure.may_have_dispatched);
        assert_eq!(*resolver.calls.lock().unwrap(), 1);
        let events = evidence.0.lock().unwrap();
        assert_eq!(events[0].destination_class, DestinationClass::Private);
    }

    #[tokio::test]
    async fn translation_prefix_cannot_smuggle_metadata_ipv4_through_dns() {
        let resolver = Arc::new(Resolver {
            answers: vec!["64:ff9b::169.254.169.254".parse().unwrap()],
            calls: Mutex::new(0),
        });
        let evidence = Arc::new(Evidence::default());
        let transport = GovernedEgressTransport::new_with_resolver(
            grant("api.example.test", 443),
            identity(),
            DestinationPolicy::public_only(),
            evidence.clone(),
            resolver.clone(),
        )
        .unwrap();
        let failure = transport
            .send(request("https://api.example.test/v1/test".into()))
            .await
            .unwrap_err();
        assert_eq!(failure.class, TransportErrorClass::EgressDenied);
        assert!(!failure.may_have_dispatched);
        assert_eq!(*resolver.calls.lock().unwrap(), 1);
        let events = evidence.0.lock().unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].destination_class, DestinationClass::Metadata);
        assert_eq!(events[0].outcome, "egress_denied");
    }

    #[tokio::test]
    async fn dns_resolution_obeys_connect_and_overall_deadlines_before_dispatch() {
        for (connect, overall, expected) in [
            (
                Duration::from_millis(30),
                Duration::from_millis(500),
                TransportErrorClass::ConnectTimeout,
            ),
            (
                Duration::from_millis(500),
                Duration::from_millis(30),
                TransportErrorClass::DeadlineExceeded,
            ),
        ] {
            let evidence = Arc::new(Evidence::default());
            let resolver = Arc::new(SlowResolver {
                answer: "8.8.8.8".parse().unwrap(),
                delay: Duration::from_millis(150),
            });
            let transport = GovernedEgressTransport::new_with_resolver(
                grant("api.example.test", 443),
                identity(),
                DestinationPolicy::public_only(),
                evidence.clone(),
                resolver,
            )
            .unwrap();
            let mut outbound = request("https://api.example.test/v1/test".into());
            outbound.timeouts.connect = connect;
            outbound.timeouts.overall = overall;
            let failure = transport.send(outbound).await.unwrap_err();
            assert_eq!(failure.class, expected);
            assert!(!failure.may_have_dispatched);
            assert_eq!(evidence.0.lock().unwrap().len(), 1);
        }
    }

    #[tokio::test]
    async fn cancellation_during_dns_is_before_dispatch() {
        let evidence = Arc::new(Evidence::default());
        let resolver = Arc::new(SlowResolver {
            answer: "8.8.8.8".parse().unwrap(),
            delay: Duration::from_millis(250),
        });
        let transport = GovernedEgressTransport::new_with_resolver(
            grant("api.example.test", 443),
            identity(),
            DestinationPolicy::public_only(),
            evidence.clone(),
            resolver,
        )
        .unwrap();
        let cancellation = TransportCancellation::new();
        let trigger = cancellation.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(20)).await;
            trigger.cancel();
        });
        let mut outbound = request("https://api.example.test/v1/test".into());
        outbound.cancellation = Some(cancellation);
        let failure = transport.send(outbound).await.unwrap_err();
        assert_eq!(failure.class, TransportErrorClass::Cancelled);
        assert!(!failure.may_have_dispatched);
        assert_eq!(evidence.0.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn ambiguous_authority_path_is_denied_before_dns() {
        let resolver = Arc::new(Resolver {
            answers: vec!["8.8.8.8".parse().unwrap()],
            calls: Mutex::new(0),
        });
        let transport = GovernedEgressTransport::new_with_resolver(
            grant("api.invalid", 443),
            identity(),
            DestinationPolicy::public_only(),
            Arc::new(Evidence::default()),
            resolver.clone(),
        )
        .unwrap();

        for url in [
            "https://api.invalid/v1/%2e%2e/admin",
            "https://api.invalid/v1/safe%2fadmin",
            "https://api.invalid/v1/safe%5cadmin",
            "https://api.invalid/v1/%252e%252e/admin",
            "https://api.invalid/v1/%00admin",
        ] {
            let failure = transport.send(request(url.into())).await.unwrap_err();
            assert_eq!(
                failure.class,
                TransportErrorClass::EgressDenied,
                "accepted {url}"
            );
            assert!(!failure.may_have_dispatched);
        }
        assert_eq!(*resolver.calls.lock().unwrap(), 0);
    }

    #[tokio::test]
    async fn exact_method_path_port_timeout_and_reserved_headers_are_enforced_before_dns() {
        let resolver = Arc::new(Resolver {
            answers: vec!["8.8.8.8".parse().unwrap()],
            calls: Mutex::new(0),
        });
        let evidence = Arc::new(Evidence::default());
        let transport = GovernedEgressTransport::new_with_resolver(
            grant("api.invalid", 443),
            identity(),
            DestinationPolicy::public_only(),
            evidence,
            resolver.clone(),
        )
        .unwrap();

        let mut denied = request("https://api.invalid/v2/test".into());
        assert_eq!(
            transport.send(denied.clone()).await.unwrap_err().class,
            TransportErrorClass::EgressDenied
        );
        denied.url = "https://api.invalid:8443/v1/test".into();
        assert_eq!(
            transport.send(denied.clone()).await.unwrap_err().class,
            TransportErrorClass::EgressDenied
        );
        denied.url = "https://api.invalid/v1/test".into();
        denied.method = "CONNECT".into();
        assert_eq!(
            transport.send(denied.clone()).await.unwrap_err().class,
            TransportErrorClass::EgressDenied
        );
        denied.method = "POST".into();
        denied.timeouts.overall = Duration::from_secs(3);
        assert_eq!(
            transport.send(denied.clone()).await.unwrap_err().class,
            TransportErrorClass::EgressDenied
        );

        for header in [
            "Host",
            "Connection",
            "Keep-Alive",
            "Proxy-Authorization",
            "Proxy-Connection",
            "TE",
            "Trailer",
            "Transfer-Encoding",
            "Upgrade",
            "Content-Length",
            "X-Correlation-Id",
        ] {
            let mut denied = request("https://api.invalid/v1/test".into());
            denied.headers.insert(header.into(), "forbidden".into());
            assert_eq!(
                transport.send(denied).await.unwrap_err().class,
                TransportErrorClass::EgressDenied,
                "reserved transport header was accepted: {header}"
            );
        }
        assert_eq!(*resolver.calls.lock().unwrap(), 0);
    }

    #[tokio::test]
    async fn authority_override_and_plaintext_credential_headers_are_denied_before_dns() {
        let resolver = Arc::new(Resolver {
            answers: vec!["8.8.8.8".parse().unwrap()],
            calls: Mutex::new(0),
        });
        let transport = GovernedEgressTransport::new_with_resolver(
            grant("api.example.test", 443),
            identity(),
            DestinationPolicy::public_only(),
            Arc::new(Evidence::default()),
            resolver.clone(),
        )
        .unwrap();
        for header in [
            "Forwarded",
            "X-Forwarded-Host",
            "X-Original-URL",
            "X-Rewrite-URL",
            "X-HTTP-Method-Override",
            "X-Envoy-Original-Path",
        ] {
            let mut denied = request("https://api.example.test/v1/test".into());
            denied.headers.insert(header.into(), "/admin".into());
            let failure = transport.send(denied).await.unwrap_err();
            assert_eq!(
                failure.class,
                TransportErrorClass::EgressDenied,
                "accepted routing override {header}"
            );
            assert!(!failure.may_have_dispatched);
        }

        let mut http_grant = grant("api.example.test", 80);
        http_grant.protocols = vec![EgressProtocol::Http];
        let plaintext = GovernedEgressTransport::new_with_resolver(
            http_grant,
            identity_without_secret(),
            DestinationPolicy::public_only(),
            Arc::new(Evidence::default()),
            resolver.clone(),
        )
        .unwrap();
        let mut denied = request("http://api.example.test/v1/test".into());
        denied
            .headers
            .insert("Authorization".into(), "Bearer fixture".into());
        let failure = plaintext.send(denied).await.unwrap_err();
        assert_eq!(failure.class, TransportErrorClass::EgressDenied);
        assert!(!failure.may_have_dispatched);
        assert_eq!(*resolver.calls.lock().unwrap(), 0);
    }

    #[tokio::test]
    async fn secret_bearing_egress_requires_https_before_dns() {
        let resolver = Arc::new(Resolver {
            answers: vec!["8.8.8.8".parse().unwrap()],
            calls: Mutex::new(0),
        });
        let evidence = Arc::new(Evidence::default());
        let mut secret_grant = grant("api.example.test", 80);
        secret_grant.protocols = vec![EgressProtocol::Http];
        let transport = GovernedEgressTransport::new_with_resolver(
            secret_grant,
            identity(),
            DestinationPolicy::public_only(),
            evidence.clone(),
            resolver.clone(),
        )
        .unwrap();
        let failure = transport
            .send(request("http://api.example.test/v1/test".into()))
            .await
            .unwrap_err();
        assert_eq!(failure.class, TransportErrorClass::EgressDenied);
        assert!(!failure.may_have_dispatched);
        assert_eq!(*resolver.calls.lock().unwrap(), 0);
        let events = evidence.0.lock().unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].policy_decision, "deny");
        assert_eq!(events[0].outcome, "authority_rejected");
    }

    #[test]
    fn secret_purpose_and_redirect_revalidation_fail_closed_at_construction() {
        let mut wrong_identity = identity();
        wrong_identity.secret_purpose = Some("wrong.secret".into());
        assert!(
            GovernedEgressTransport::new(
                grant("api.example.test", 443),
                wrong_identity,
                DestinationPolicy::public_only(),
                Arc::new(Evidence::default()),
            )
            .is_err()
        );

        let mut redirect = grant("api.example.test", 443);
        redirect.redirect_policy = EgressRedirectPolicy::Revalidate;
        assert!(
            GovernedEgressTransport::new(
                redirect,
                identity(),
                DestinationPolicy::public_only(),
                Arc::new(Evidence::default()),
            )
            .is_err()
        );
    }

    #[tokio::test]
    async fn budget_exhaustion_prevents_dns_resolution() {
        let resolver = Arc::new(Resolver {
            answers: vec!["8.8.8.8".parse().unwrap()],
            calls: Mutex::new(0),
        });
        let evidence = Arc::new(Evidence::default());
        let transport = GovernedEgressTransport::new_with_resolver(
            grant("api.example.test", 443),
            identity(),
            DestinationPolicy::public_only(),
            evidence.clone(),
            resolver.clone(),
        )
        .unwrap();

        let held = transport.acquire_budget().unwrap();
        let failure = transport
            .send(request("https://api.example.test/v1/test".into()))
            .await
            .unwrap_err();
        assert_eq!(failure.class, TransportErrorClass::EgressDenied);
        assert!(!failure.may_have_dispatched);
        assert_eq!(*resolver.calls.lock().unwrap(), 0);
        let events = evidence.0.lock().unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].policy_decision, "deny");
        assert_eq!(events[0].outcome, "concurrency_exhausted");
        drop(held);
    }

    #[test]
    fn concurrency_and_rate_budgets_fail_closed() {
        let transport = GovernedEgressTransport::new(
            grant("api.example.test", 443),
            identity(),
            DestinationPolicy::public_only(),
            Arc::new(Evidence::default()),
        )
        .unwrap();
        let first = transport.acquire_budget().unwrap();
        assert_eq!(
            transport.acquire_budget().unwrap_err(),
            "concurrency_exhausted"
        );
        drop(first);
        let second = transport.acquire_budget().unwrap();
        drop(second);
        assert_eq!(transport.acquire_budget().unwrap_err(), "rate_exhausted");
    }

    #[test]
    fn named_internal_access_requires_exact_profile_cidr() {
        let policy = DestinationPolicy::with_internal_cidrs(["10.10.0.0/16".parse().unwrap()]);
        assert!(policy.permits("10.10.1.9".parse().unwrap()));
        assert!(!policy.permits("10.11.1.9".parse().unwrap()));
        assert!(!policy.permits(IpAddr::V4(Ipv4Addr::LOCALHOST)));
        assert!(!policy.permits("169.254.169.254".parse().unwrap()));
    }

    async fn response_server(response: String, delay: Option<Duration>) -> (String, u16) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut buffer = [0_u8; 4096];
            let _ = stream.read(&mut buffer).await;
            let boundary = response
                .find("\r\n\r\n")
                .map(|index| index + 4)
                .unwrap_or(response.len());
            let (headers, body) = response.split_at(boundary);
            stream.write_all(headers.as_bytes()).await.unwrap();
            if let Some(delay) = delay {
                tokio::time::sleep(delay).await;
            }
            if !body.is_empty() {
                let _ = stream.write_all(body.as_bytes()).await;
            }
        });
        ("127.0.0.1".into(), address.port())
    }

    #[tokio::test]
    async fn redirect_oversize_and_slow_responses_are_terminal_and_audited() {
        for (response, delay, expected) in [
            (
                "HTTP/1.1 302 Found\r\nLocation: http://169.254.169.254/latest\r\nContent-Length: 0\r\n\r\n".to_string(),
                None,
                TransportErrorClass::EgressDenied,
            ),
            (
                format!("HTTP/1.1 200 OK\r\nContent-Length: 100\r\n\r\n{}", "x".repeat(100)),
                None,
                TransportErrorClass::ResponseTooLarge,
            ),
            (
                "HTTP/1.1 200 OK\r\nContent-Length: 1\r\n\r\nx".to_string(),
                Some(Duration::from_millis(150)),
                TransportErrorClass::ResponseTimeout,
            ),
        ] {
            let (host, port) = response_server(response, delay).await;
            let evidence = Arc::new(Evidence::default());
            let mut local_grant = grant(&host, port);
            local_grant.protocols = vec![EgressProtocol::Http];
            local_grant.max_read_timeout_ms = 50;
            let transport = GovernedEgressTransport::new(
                local_grant,
                identity_without_secret(),
                DestinationPolicy::test_loopback_only(),
                evidence.clone(),
            )
            .unwrap();
            let mut outbound = request(format!("http://{host}:{port}/v1/test"));
            outbound.timeouts.read = Duration::from_millis(50);
            let failure = transport.send(outbound).await.unwrap_err();
            assert_eq!(failure.class, expected);
            assert!(failure.may_have_dispatched);
            let events = evidence.0.lock().unwrap();
            assert_eq!(events.len(), 1);
            assert_eq!(events[0].policy_decision, "allow");
        }
    }
}
