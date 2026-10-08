use crate::{
    error::AppError,
    middleware::{auth::AdminUser, trusted_proxy::ResolvedClientIp},
    services::machine_api::{self, Provision, Scope},
    state::AppState,
};
use axum::{
    Json, Router,
    extract::{ConnectInfo, Path, State},
    http::{HeaderMap, header},
    routing::{get, post},
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    net::{IpAddr, SocketAddr},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

pub(crate) const MACHINE_INGRESS_RATE_LIMIT: u32 = 30;
const MACHINE_INGRESS_RATE_WINDOW: Duration = Duration::from_secs(60);
const MACHINE_INGRESS_RATE_BUCKETS: usize = 4096;

struct MachineIngressBucket {
    window_started: Instant,
    requests: u32,
}

/// Bounded transport-admission guard for the bearer-only execution route.
/// It is keyed by the P4-resolved client IP (falling back to the socket peer
/// only when the governance extension is unavailable), never by a bearer
/// secret. Durable per-client RPM remains the post-auth authorization policy.
pub(crate) struct MachineIngressRateLimiter {
    buckets: Mutex<BTreeMap<IpAddr, MachineIngressBucket>>,
    limit: u32,
    max_buckets: usize,
}

impl Default for MachineIngressRateLimiter {
    fn default() -> Self {
        Self::new(MACHINE_INGRESS_RATE_LIMIT, MACHINE_INGRESS_RATE_BUCKETS)
    }
}

impl MachineIngressRateLimiter {
    pub(crate) fn new(limit: u32, max_buckets: usize) -> Self {
        Self {
            buckets: Mutex::new(BTreeMap::new()),
            limit,
            max_buckets,
        }
    }

    pub(crate) fn allow(&self, peer_ip: IpAddr) -> bool {
        let now = Instant::now();
        let Ok(mut buckets) = self.buckets.lock() else {
            return false;
        };
        if let Some(bucket) = buckets.get_mut(&peer_ip) {
            if now.duration_since(bucket.window_started) >= MACHINE_INGRESS_RATE_WINDOW {
                bucket.window_started = now;
                bucket.requests = 1;
                return true;
            }
            if bucket.requests >= self.limit {
                return false;
            }
            bucket.requests += 1;
            return true;
        }
        if buckets.len() >= self.max_buckets {
            buckets.retain(|_, bucket| {
                now.duration_since(bucket.window_started) < MACHINE_INGRESS_RATE_WINDOW
            });
            if buckets.len() >= self.max_buckets {
                return false;
            }
        }
        buckets.insert(
            peer_ip,
            MachineIngressBucket {
                window_started: now,
                requests: 1,
            },
        );
        true
    }

    #[cfg(test)]
    pub(crate) fn bucket_count(&self) -> usize {
        self.buckets
            .lock()
            .map(|buckets| buckets.len())
            .unwrap_or(0)
    }
}

pub fn machine_api_routes() -> Router<Arc<AppState>> {
    let ingress_limiter = Arc::new(MachineIngressRateLimiter::default());
    Router::new()
        .route("/api/machine-clients", get(list).post(provision))
        .route("/api/machine-clients/{client}/{action}", post(lifecycle))
        .route(
            "/api/machine-clients/{client}/credentials/{credential}/{action}",
            post(credential_lifecycle),
        )
        .route(
            "/api/machine/{version}/tenants/{tenant}/execute",
            post(execute).route_layer(axum::middleware::from_fn_with_state(
                ingress_limiter,
                machine_ingress_rate_limit,
            )),
        )
}

/// This route-local limiter runs before the JSON extractor and before the
/// Registry opens its durable authorization transaction.
async fn machine_ingress_rate_limit(
    State(rate_limiter): State<Arc<MachineIngressRateLimiter>>,
    ConnectInfo(peer_addr): ConnectInfo<SocketAddr>,
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> Result<axum::response::Response, AppError> {
    let client_ip = request
        .extensions()
        .get::<ResolvedClientIp>()
        .map(|resolved| resolved.0)
        .unwrap_or_else(|| peer_addr.ip());
    if !rate_limiter.allow(client_ip) {
        return Err(AppError::CodedRateLimited {
            code: "MACHINE_INGRESS_RATE_LIMITED".into(),
            message: "Machine ingress rate limit exceeded".into(),
        });
    }
    Ok(next.run(request).await)
}

async fn list(
    State(state): State<Arc<AppState>>,
    admin: AdminUser,
) -> Result<Json<Value>, AppError> {
    Ok(Json(machine_api::list_with_repository(
        state.machine_authority_repository(),
        &admin.0,
    )?))
}
async fn provision(
    State(state): State<Arc<AppState>>,
    admin: AdminUser,
    Json(input): Json<Provision>,
) -> Result<(HeaderMap, Json<machine_api::Issued>), AppError> {
    Ok((
        no_store(),
        Json(machine_api::provision_with_repository(
            state.machine_authority_repository(),
            &state.registry,
            &admin.0,
            input,
        )?),
    ))
}
async fn lifecycle(
    State(state): State<Arc<AppState>>,
    admin: AdminUser,
    Path((client, action)): Path<(String, String)>,
) -> Result<(HeaderMap, Json<Value>), AppError> {
    Ok((
        no_store(),
        Json(json!({
            "credential": machine_api::lifecycle_with_repository(
                state.machine_authority_repository(),
                &admin.0,
                &client,
                &action,
            )?
        })),
    ))
}
fn no_store() -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(header::CACHE_CONTROL, "no-store".parse().unwrap());
    headers
}
async fn credential_lifecycle(
    State(state): State<Arc<AppState>>,
    admin: AdminUser,
    Path((client, credential, action)): Path<(String, String, String)>,
) -> Result<Json<Value>, AppError> {
    machine_api::credential_lifecycle_with_repository(
        state.machine_authority_repository(),
        &admin.0,
        &client,
        &credential,
        &action,
    )?;
    Ok(Json(json!({"ok":true})))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Invocation {
    module: String,
    command: String,
    payload: Value,
}

fn require_machine_bearer_lane(headers: &HeaderMap) -> Result<(), AppError> {
    if headers.contains_key(header::COOKIE)
        || headers.contains_key("x-talos-authority")
        || headers.contains_key("x-talos-execution-mode")
    {
        return Err(AppError::Forbidden);
    }
    Ok(())
}

async fn execute(
    State(state): State<Arc<AppState>>,
    Path((version, tenant)): Path<(String, String)>,
    headers: HeaderMap,
    Json(input): Json<Invocation>,
) -> Result<(HeaderMap, Json<Value>), AppError> {
    require_machine_bearer_lane(&headers)?;
    let token = headers
        .get(header::AUTHORIZATION)
        .and_then(|h| h.to_str().ok())
        .and_then(|h| h.strip_prefix("Bearer "))
        .unwrap_or("");
    let correlation = uuid::Uuid::new_v4().to_string();
    let result = state.registry.execute_machine_with_repository(
        state.machine_authority_repository(),
        token,
        &tenant,
        &version,
        &Scope {
            module: input.module,
            command: input.command,
        },
        input.payload,
        state.http_client.clone(),
        &correlation,
        headers
            .get("idempotency-key")
            .map(|h| h.to_str().map(str::to_owned))
            .transpose()
            .map_err(|_| AppError::BadRequest("invalid Idempotency-Key".into()))?,
    )?;
    let mut response_headers = no_store();
    response_headers.insert("x-correlation-id", correlation.parse().unwrap());
    Ok((response_headers, Json(json!({"ok":true,"result":result}))))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn machine_lane_rejects_browser_and_caller_selected_authority_headers() {
        for (name, value) in [
            (header::COOKIE.as_str(), "session=browser-session"),
            ("x-talos-authority", "platform-owner"),
            ("x-talos-execution-mode", "simulation"),
        ] {
            let mut headers = HeaderMap::new();
            headers.insert(name, value.parse().unwrap());
            assert!(matches!(
                require_machine_bearer_lane(&headers),
                Err(AppError::Forbidden)
            ));
        }
    }

    #[test]
    fn machine_lane_accepts_only_bearer_transport_metadata() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::AUTHORIZATION,
            "Bearer machine-secret".parse().unwrap(),
        );
        headers.insert("idempotency-key", "0123456789abcdef".parse().unwrap());
        assert!(require_machine_bearer_lane(&headers).is_ok());
    }
}
