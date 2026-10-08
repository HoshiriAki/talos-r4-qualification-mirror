//! Provider-independent deposit and refund authority.
//!
//! This module models money in minor units only. It never starts an HTTP call:
//! a refund can be linked only to a durable `ExternalOperation`, leaving the
//! actual provider effect to the operation runtime and reconciliation boundary.

use serde::{Deserialize, Serialize};

use super::types::IntegrationError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DepositState {
    Expected,
    Recorded,
    Held,
    PartiallyDeducted,
    Released,
    ManualResolutionRequired,
}

impl DepositState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Expected => "expected",
            Self::Recorded => "recorded",
            Self::Held => "held",
            Self::PartiallyDeducted => "partially_deducted",
            Self::Released => "released",
            Self::ManualResolutionRequired => "manual_resolution_required",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DepositLedgerEntryKind {
    Expected,
    Received,
    Held,
    Deducted,
    Released,
    RefundCompleted,
    ManualAdjustment,
}

impl DepositLedgerEntryKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Expected => "expected",
            Self::Received => "received",
            Self::Held => "held",
            Self::Deducted => "deducted",
            Self::Released => "released",
            Self::RefundCompleted => "refund_completed",
            Self::ManualAdjustment => "manual_adjustment",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RefundState {
    Requested,
    Approved,
    Dispatching,
    UnknownOutcome,
    Completed,
    Failed,
    ManualResolutionRequired,
}

impl RefundState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Requested => "requested",
            Self::Approved => "approved",
            Self::Dispatching => "dispatching",
            Self::UnknownOutcome => "unknown_outcome",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::ManualResolutionRequired => "manual_resolution_required",
        }
    }

    pub fn reserves_refundable_balance(self) -> bool {
        matches!(
            self,
            Self::Requested | Self::Approved | Self::Dispatching | Self::UnknownOutcome
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DepositReconciliationOutcome {
    Pending,
    Matched,
    Unknown,
    ManualRequired,
}

impl DepositReconciliationOutcome {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Matched => "matched",
            Self::Unknown => "unknown",
            Self::ManualRequired => "manual_required",
        }
    }
}

pub fn require_positive_minor_units(amount: i64) -> Result<(), IntegrationError> {
    if amount <= 0 {
        return Err(IntegrationError::InvalidOperationTransition);
    }
    Ok(())
}
