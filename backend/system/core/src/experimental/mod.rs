//! Non-default contracts retained for bounded experimentation.
//!
//! These types do not imply an operational workflow runtime, durable state,
//! replay, idempotency, or crash-safe compensation.

#[cfg(feature = "experimental-orchestration")]
pub mod orchestration;

#[cfg(feature = "experimental-orchestration")]
pub use orchestration::{CompensationLog, ModuleOp, OrchestrationError, SagaStep};
