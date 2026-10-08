//! Crate-private P5 governed dispatch seam.
//!
//! P5 owns `GovernedEgressTransport` construction. Higher layers may supply
//! already-authorized grant/identity inputs, but they cannot instantiate the
//! transport directly. This keeps DNS/SSRF/timeout/response enforcement and
//! transport construction in the P5 Integration boundary while allowing P6 to
//! hold its longer-lived shared budget permit across the complete dispatch.

use std::sync::Arc;

use system_core::transport::network::EgressGrant;

use super::egress::{
    DestinationPolicy, DestinationResolver, EgressEvidenceSink, EgressIdentity,
    GovernedEgressTransport, SystemDestinationResolver,
};
use super::transport::{
    ExternalCallTransport, ExternalRequest, ExternalResponse, TransportFailure,
};

#[derive(Debug)]
pub(crate) enum GovernedDispatchError {
    InvalidGrant,
    Transport(TransportFailure),
}

pub(crate) struct GovernedEgressDispatcher {
    resolver: Arc<dyn DestinationResolver>,
}

impl GovernedEgressDispatcher {
    pub(crate) fn system() -> Self {
        Self {
            resolver: Arc::new(SystemDestinationResolver),
        }
    }

    #[cfg(test)]
    pub(crate) fn with_resolver(resolver: Arc<dyn DestinationResolver>) -> Self {
        Self { resolver }
    }

    pub(crate) async fn send(
        &self,
        grant: EgressGrant,
        identity: EgressIdentity,
        destination_policy: DestinationPolicy,
        evidence: Arc<dyn EgressEvidenceSink>,
        request: ExternalRequest,
    ) -> Result<ExternalResponse, GovernedDispatchError> {
        let transport = GovernedEgressTransport::new_with_resolver(
            grant,
            identity,
            destination_policy,
            evidence,
            self.resolver.clone(),
        )
        .map_err(|_| GovernedDispatchError::InvalidGrant)?;

        transport
            .send(request)
            .await
            .map_err(GovernedDispatchError::Transport)
    }
}
