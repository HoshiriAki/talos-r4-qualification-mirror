//! In-memory orchestration value types retained behind an explicit feature.
//!
//! This module provides data contracts only. It deliberately provides no Saga
//! executor, persistence, recovery, replay, outbox, or compensation runtime.

use serde_json::Value;

use crate::ErrorPayload;

/// A single module invocation description.
pub struct ModuleOp {
    pub module_name: String,
    pub command: String,
    pub payload: Value,
    pub timeout_ms: Option<u64>,
}

/// An in-memory forward operation and its optional compensation description.
pub struct SagaStep {
    pub forward: ModuleOp,
    pub compensate: Option<ModuleOp>,
}

/// Structured failure information for a sequence of in-memory operations.
pub struct OrchestrationError {
    pub step: usize,
    pub module: String,
    pub command: String,
    pub error: ErrorPayload,
    pub compensated: Vec<CompensationLog>,
}

/// The observed result of an attempted in-memory compensation operation.
pub struct CompensationLog {
    pub step: usize,
    pub module: String,
    pub command: String,
    pub result: Result<Value, String>,
}
