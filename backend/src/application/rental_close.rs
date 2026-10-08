use std::sync::Arc;

use system_core::ExecutionContext;

use crate::repositories::{LifecycleOperationalView, RepositoryProvider};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CloseTerminalSnapshot {
    pub deposit_terminal: bool,
    pub shipments_terminal: bool,
    pub damage_repair_terminal: bool,
    pub overdue_terminal: bool,
    pub invoice_terminal: bool,
    pub audit_reconciliation_clear: bool,
}

pub trait CloseTerminalAuthority: Send + Sync {
    fn terminal_snapshot(&self, order_id: &str, ctx: &ExecutionContext) -> CloseTerminalSnapshot;
}

pub struct UnavailableProductionCloseAuthority;

impl CloseTerminalAuthority for UnavailableProductionCloseAuthority {
    fn terminal_snapshot(&self, _order_id: &str, _ctx: &ExecutionContext) -> CloseTerminalSnapshot {
        CloseTerminalSnapshot {
            deposit_terminal: false,
            shipments_terminal: false,
            damage_repair_terminal: false,
            overdue_terminal: false,
            invoice_terminal: false,
            audit_reconciliation_clear: false,
        }
    }
}

pub struct RentalCloseCoordinator {
    repositories: Arc<dyn RepositoryProvider>,
    authority: Arc<dyn CloseTerminalAuthority>,
}

impl RentalCloseCoordinator {
    pub fn new(
        repositories: Arc<dyn RepositoryProvider>,
        authority: Arc<dyn CloseTerminalAuthority>,
    ) -> Self {
        Self {
            repositories,
            authority,
        }
    }

    pub fn close_order(
        &self,
        order_id: &str,
        expected_version: i64,
        actor_user_id: &str,
        ctx: &ExecutionContext,
    ) -> Result<LifecycleOperationalView, String> {
        let scoped = self
            .repositories
            .bind(ctx)
            .map_err(|error| error.to_string())?;
        let snapshot = self.authority.terminal_snapshot(order_id, ctx);
        let mut missing = Vec::new();
        if !snapshot.deposit_terminal {
            missing.push("deposit_terminal");
        }
        if !snapshot.shipments_terminal {
            missing.push("shipments_terminal");
        }
        if !snapshot.damage_repair_terminal {
            missing.push("damage_repair_terminal");
        }
        if !snapshot.overdue_terminal {
            missing.push("overdue_terminal");
        }
        if !snapshot.invoice_terminal {
            missing.push("invoice_terminal");
        }
        if !snapshot.audit_reconciliation_clear {
            missing.push("audit_reconciliation_clear");
        }
        if scoped
            .workflows()
            .order_has_open_blockers(order_id)
            .map_err(|error| error.to_string())?
        {
            missing.push("workflow_blockers_clear");
        }
        if !missing.is_empty() {
            return Err(format!(
                "CLOSE_PREREQUISITES_UNAVAILABLE: {}",
                missing.join(",")
            ));
        }
        scoped
            .lifecycles()
            .apply_action(
                order_id,
                "close_order",
                expected_version,
                actor_user_id,
                "R1-P7 strict close prerequisites verified",
            )
            .map_err(|error| error.to_string())
    }
}
