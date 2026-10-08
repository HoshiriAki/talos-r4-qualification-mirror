//! Small, dependency-free runtime metrics boundary.
//!
//! Metrics are process-local operational aggregates, not an audit replacement.
//! Every label is selected from a closed set in this module: callers cannot put
//! tenant, actor, request, order, operation, secret, payload, or raw error data
//! into an exported metric.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use system_core::ExecutionMode;

const REGISTRY_MODULES: &[&str] = &[
    "r3_settlement",
    "integration",
    "tenant_governance",
    "tenant_preview",
    "tenant_simulation",
    "other",
];
const REGISTRY_COMMANDS: &[&str] = &[
    "get_business_timezone",
    "get_closure_facts",
    "configure_business_timezone",
    "begin_inspection",
    "complete_inspection",
    "create_damage_finding",
    "decide_liability",
    "decide_repair",
    "transition_repair",
    "propose_settlement",
    "accept_settlement",
    "deduct_deposit",
    "admit_additional_charge",
    "open_dispute",
    "resolve_dispute",
    "complete_settlement",
    "close_order",
    "other",
];
const EXECUTION_MODES: &[&str] = &["normal", "preview", "simulation"];
const RESULT_CLASSES: &[&str] = &[
    "succeeded",
    "access_denied",
    "preview_blocked",
    "simulation_blocked",
    "validation_error",
    "business_error",
    "system_error",
];
const HTTP_METHODS: &[&str] = &["get", "post", "put", "patch", "delete", "other"];
const HTTP_ROUTES: &[&str] = &[
    "metrics",
    "health",
    "r3_settlement_command",
    "integration_webhook",
    "integrations",
    "api_other",
    "other",
];
const STATUS_CLASSES: &[&str] = &["1xx", "2xx", "3xx", "4xx", "5xx"];
const INTEGRATION_EVENTS: &[&str] = &[
    "admitted",
    "claimed",
    "dispatch_started",
    "attempted",
    "succeeded",
    "rejected",
    "retry_scheduled",
    "deferred",
    "blocked",
    "blocked_configuration",
    "system_failure",
    "non_retryable_failure",
    "unknown_outcome",
    "reconciliation_started",
    "reconciliation_resolved",
    "manual_resolution_required",
    "compensation_scheduled",
];
const OPERATION_STATES: &[&str] = &[
    "planned",
    "ready",
    "dispatching",
    "succeeded",
    "rejected",
    "retryable_failure",
    "non_retryable_failure",
    "unknown_outcome",
    "reconciling",
    "resolved",
    "manual_resolution_required",
];
const WEBHOOK_EVENTS: &[&str] = &[
    "received",
    "verified",
    "processing_started",
    "processed",
    "retry_scheduled",
    "rejected",
    "conflict",
    "dead_lettered",
    "replayed",
    "duplicate",
    "system_failure",
];
const WEBHOOK_OUTCOMES: &[&str] = &[
    "pending",
    "accepted",
    "rejected",
    "retryable",
    "permanent_failure",
    "system_failure",
];
const RECOVERY_SOURCES: &[&str] = &[
    "startup_snapshot",
    "tenant_page",
    "unknown_outcome",
    "webhook",
    "scheduled_page",
];
const RECOVERY_OUTCOMES: &[&str] = &["started", "recovered", "empty", "failed"];
const WORKFLOW_EVENTS: &[&str] = &[
    "step_processed",
    "step_completed",
    "retry_scheduled",
    "blocked",
    "manual_resolution_required",
    "compensation_scheduled",
    "outbox_delivered",
    "inbox_consumed",
];
const WORKFLOW_OUTCOMES: &[&str] = &["succeeded", "retryable", "blocked", "failed"];
const FINANCIAL_EVENTS: &[&str] = &[
    "deposit_recorded",
    "deposit_received_ledger",
    "deposit_deduction_admitted",
    "refund_intent_admitted",
    "refund_operation_planned",
    "refund_reconciliation",
];
const SETTLEMENT_COMMANDS: &[&str] = &[
    "complete_inspection",
    "create_damage_finding",
    "decide_liability",
    "decide_repair",
    "transition_repair",
    "propose_settlement",
    "accept_settlement",
    "deduct_deposit",
    "admit_additional_charge",
    "open_dispute",
    "resolve_dispute",
    "complete_settlement",
    "close_order",
    "other",
];
const SETTLEMENT_OUTCOMES: &[&str] = &["succeeded", "blocked", "failed"];
const SETTLEMENT_BLOCKERS: &[&str] = &[
    "none",
    "return_incomplete",
    "inspection_incomplete",
    "damage_unresolved",
    "liability_unresolved",
    "repair_unresolved",
    "dispute_active",
    "effect_unresolved",
    "overdue_active",
    "financial_unbalanced",
    "settlement_not_terminal",
    "settlement_not_accepted",
    "terminal_sealed",
    "other_blocked",
];
const ATTEMPT_OUTCOMES: &[&str] = &[
    "succeeded",
    "rejected",
    "retryable_failure",
    "non_retryable_failure",
    "unknown_outcome",
    "persistence_failure",
];
const ATTEMPT_DISPATCH_CLASSES: &[&str] = &["dispatched", "may_have_dispatched", "not_dispatched"];

const REGISTRY_ATTEMPTS: usize =
    REGISTRY_MODULES.len() * REGISTRY_COMMANDS.len() * EXECUTION_MODES.len();
const REGISTRY_RESULTS: usize = REGISTRY_ATTEMPTS * RESULT_CLASSES.len();
const HTTP_METRICS: usize = HTTP_METHODS.len() * HTTP_ROUTES.len() * STATUS_CLASSES.len();
const INTEGRATION_METRICS: usize = INTEGRATION_EVENTS.len() * OPERATION_STATES.len();
const WEBHOOK_METRICS: usize = WEBHOOK_EVENTS.len() * WEBHOOK_OUTCOMES.len();
const RECOVERY_METRICS: usize = RECOVERY_SOURCES.len() * RECOVERY_OUTCOMES.len();
const WORKFLOW_METRICS: usize = WORKFLOW_EVENTS.len() * WORKFLOW_OUTCOMES.len();
const SETTLEMENT_METRICS: usize =
    SETTLEMENT_COMMANDS.len() * SETTLEMENT_OUTCOMES.len() * SETTLEMENT_BLOCKERS.len();
const ATTEMPT_METRICS: usize = ATTEMPT_OUTCOMES.len() * ATTEMPT_DISPATCH_CLASSES.len();

/// Injectable operational observation port. Implementations must preserve the
/// bounded-label rule documented above.
pub(crate) trait MetricsSink: Send + Sync {
    fn registry_attempt(&self, _module: &str, _command: &str, _mode: &ExecutionMode) {}
    fn registry_result(
        &self,
        _module: &str,
        _command: &str,
        _mode: &ExecutionMode,
        _result: ResultClass,
        _elapsed: Duration,
    ) {
    }
    fn http_request(&self, _method: &str, _route: &str, _status: u16, _elapsed: Duration) {}
    fn integration_operation(&self, _event: IntegrationEvent, _state: OperationStateClass) {}
    fn integration_attempt(
        &self,
        _outcome: AttemptOutcome,
        _dispatch_class: AttemptDispatchClass,
        _elapsed: Duration,
    ) {
    }
    fn webhook(&self, _event: WebhookEvent, _outcome: WebhookOutcome) {}
    fn recovery(&self, _source: RecoverySource, _outcome: RecoveryOutcome) {}
    fn workflow(&self, _event: WorkflowEvent, _outcome: WorkflowOutcome) {}
    fn financial(&self, _event: FinancialEvent) {}
    fn settlement(&self, _command: &str, _outcome: SettlementOutcome, _blocker: SettlementBlocker) {
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ResultClass {
    Succeeded,
    AccessDenied,
    PreviewBlocked,
    SimulationBlocked,
    ValidationError,
    BusinessError,
    SystemError,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum IntegrationEvent {
    Admitted,
    Claimed,
    DispatchStarted,
    Attempted,
    Succeeded,
    Rejected,
    RetryScheduled,
    Deferred,
    Blocked,
    BlockedConfiguration,
    SystemFailure,
    NonRetryableFailure,
    UnknownOutcome,
    ReconciliationStarted,
    ReconciliationResolved,
    ManualResolutionRequired,
    CompensationScheduled,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum OperationStateClass {
    Planned,
    Ready,
    Dispatching,
    Succeeded,
    Rejected,
    RetryableFailure,
    NonRetryableFailure,
    UnknownOutcome,
    Reconciling,
    Resolved,
    ManualResolutionRequired,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum WebhookEvent {
    Received,
    Verified,
    ProcessingStarted,
    Processed,
    RetryScheduled,
    Rejected,
    Conflict,
    DeadLettered,
    Replayed,
    Duplicate,
    SystemFailure,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum WebhookOutcome {
    Pending,
    Accepted,
    Rejected,
    Retryable,
    PermanentFailure,
    SystemFailure,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RecoverySource {
    StartupSnapshot,
    TenantPage,
    UnknownOutcome,
    Webhook,
    ScheduledPage,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RecoveryOutcome {
    Started,
    Recovered,
    Empty,
    Failed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum WorkflowEvent {
    StepProcessed,
    StepCompleted,
    RetryScheduled,
    Blocked,
    ManualResolutionRequired,
    CompensationScheduled,
    OutboxDelivered,
    InboxConsumed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum WorkflowOutcome {
    Succeeded,
    Retryable,
    Blocked,
    Failed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SettlementOutcome {
    Succeeded,
    Blocked,
    Failed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SettlementBlocker {
    None,
    ReturnIncomplete,
    InspectionIncomplete,
    DamageUnresolved,
    LiabilityUnresolved,
    RepairUnresolved,
    DisputeActive,
    EffectUnresolved,
    OverdueActive,
    FinancialUnbalanced,
    SettlementNotTerminal,
    SettlementNotAccepted,
    TerminalSealed,
    OtherBlocked,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AttemptOutcome {
    Succeeded,
    Rejected,
    RetryableFailure,
    NonRetryableFailure,
    UnknownOutcome,
    PersistenceFailure,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AttemptDispatchClass {
    Dispatched,
    MayHaveDispatched,
    NotDispatched,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum FinancialEvent {
    DepositRecorded,
    DepositReceivedLedger,
    DepositDeductionAdmitted,
    RefundIntentAdmitted,
    RefundOperationPlanned,
    RefundReconciliation,
}

/// Production in-process exporter. It has no business state and is safe to
/// replace with a no-op or a deployment-owned exporter at composition time.
pub(crate) struct RuntimeMetrics {
    started_at: Instant,
    registry_attempts: [AtomicU64; REGISTRY_ATTEMPTS],
    registry_results: [AtomicU64; REGISTRY_RESULTS],
    registry_latency_sum: [AtomicU64; REGISTRY_RESULTS],
    registry_latency_count: [AtomicU64; REGISTRY_RESULTS],
    http_requests: [AtomicU64; HTTP_METRICS],
    http_latency_sum: [AtomicU64; HTTP_METRICS],
    http_latency_count: [AtomicU64; HTTP_METRICS],
    integration: [AtomicU64; INTEGRATION_METRICS],
    integration_attempts: [AtomicU64; ATTEMPT_METRICS],
    integration_attempt_latency_sum: [AtomicU64; ATTEMPT_METRICS],
    integration_attempt_latency_count: [AtomicU64; ATTEMPT_METRICS],
    webhook: [AtomicU64; WEBHOOK_METRICS],
    recovery: [AtomicU64; RECOVERY_METRICS],
    workflow: [AtomicU64; WORKFLOW_METRICS],
    financial: [AtomicU64; FINANCIAL_EVENTS.len()],
    settlement: [AtomicU64; SETTLEMENT_METRICS],
}

impl Default for RuntimeMetrics {
    fn default() -> Self {
        Self {
            started_at: Instant::now(),
            registry_attempts: std::array::from_fn(|_| AtomicU64::new(0)),
            registry_results: std::array::from_fn(|_| AtomicU64::new(0)),
            registry_latency_sum: std::array::from_fn(|_| AtomicU64::new(0)),
            registry_latency_count: std::array::from_fn(|_| AtomicU64::new(0)),
            http_requests: std::array::from_fn(|_| AtomicU64::new(0)),
            http_latency_sum: std::array::from_fn(|_| AtomicU64::new(0)),
            http_latency_count: std::array::from_fn(|_| AtomicU64::new(0)),
            integration: std::array::from_fn(|_| AtomicU64::new(0)),
            integration_attempts: std::array::from_fn(|_| AtomicU64::new(0)),
            integration_attempt_latency_sum: std::array::from_fn(|_| AtomicU64::new(0)),
            integration_attempt_latency_count: std::array::from_fn(|_| AtomicU64::new(0)),
            webhook: std::array::from_fn(|_| AtomicU64::new(0)),
            recovery: std::array::from_fn(|_| AtomicU64::new(0)),
            workflow: std::array::from_fn(|_| AtomicU64::new(0)),
            financial: std::array::from_fn(|_| AtomicU64::new(0)),
            settlement: std::array::from_fn(|_| AtomicU64::new(0)),
        }
    }
}

impl RuntimeMetrics {
    fn increment(counter: &AtomicU64) {
        counter.fetch_add(1, Ordering::Relaxed);
    }

    fn observe_latency(sum: &AtomicU64, count: &AtomicU64, elapsed: Duration) {
        let millis = u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX);
        sum.fetch_add(millis, Ordering::Relaxed);
        count.fetch_add(1, Ordering::Relaxed);
    }

    /// Prometheus text exposition. Values only originate in the closed
    /// classifiers below, so rendering cannot disclose caller-provided values.
    pub(crate) fn render(&self) -> String {
        let mut output = String::new();
        output.push_str(
            "# HELP talos_uptime_seconds Process uptime\n# TYPE talos_uptime_seconds gauge\n",
        );
        output.push_str(&format!(
            "talos_uptime_seconds {}\n",
            self.started_at.elapsed().as_secs()
        ));
        self.render_registry(&mut output);
        self.render_http(&mut output);
        self.render_pair_family(
            &mut output,
            "talos_integration_operation_events_total",
            "event",
            INTEGRATION_EVENTS,
            "state",
            OPERATION_STATES,
            &self.integration,
        );
        self.render_integration_attempts(&mut output);
        self.render_pair_family(
            &mut output,
            "talos_webhook_events_total",
            "event",
            WEBHOOK_EVENTS,
            "outcome",
            WEBHOOK_OUTCOMES,
            &self.webhook,
        );
        self.render_pair_family(
            &mut output,
            "talos_recovery_events_total",
            "source",
            RECOVERY_SOURCES,
            "outcome",
            RECOVERY_OUTCOMES,
            &self.recovery,
        );
        self.render_pair_family(
            &mut output,
            "talos_workflow_events_total",
            "event",
            WORKFLOW_EVENTS,
            "outcome",
            WORKFLOW_OUTCOMES,
            &self.workflow,
        );
        for (index, event) in FINANCIAL_EVENTS.iter().enumerate() {
            push_metric(
                &mut output,
                "talos_financial_foundation_events_total",
                &[("event", event)],
                self.financial[index].load(Ordering::Relaxed),
            );
        }
        self.render_settlement(&mut output);
        output
    }

    fn render_registry(&self, output: &mut String) {
        for (module_index, module) in REGISTRY_MODULES.iter().enumerate() {
            for (command_index, command) in REGISTRY_COMMANDS.iter().enumerate() {
                for (mode_index, mode) in EXECUTION_MODES.iter().enumerate() {
                    let attempt_index = index3(
                        module_index,
                        REGISTRY_COMMANDS.len(),
                        command_index,
                        EXECUTION_MODES.len(),
                        mode_index,
                    );
                    push_metric(
                        output,
                        "talos_registry_commands_total",
                        &[
                            ("phase", "attempt"),
                            ("module", module),
                            ("command", command),
                            ("mode", mode),
                        ],
                        self.registry_attempts[attempt_index].load(Ordering::Relaxed),
                    );
                    for (result_index, result) in RESULT_CLASSES.iter().enumerate() {
                        let index = attempt_index * RESULT_CLASSES.len() + result_index;
                        let labels = [
                            ("phase", "result"),
                            ("module", *module),
                            ("command", *command),
                            ("mode", *mode),
                            ("result", *result),
                        ];
                        push_metric(
                            output,
                            "talos_registry_commands_total",
                            &labels,
                            self.registry_results[index].load(Ordering::Relaxed),
                        );
                        push_metric(
                            output,
                            "talos_registry_command_latency_milliseconds_sum",
                            &labels,
                            self.registry_latency_sum[index].load(Ordering::Relaxed),
                        );
                        push_metric(
                            output,
                            "talos_registry_command_latency_milliseconds_count",
                            &labels,
                            self.registry_latency_count[index].load(Ordering::Relaxed),
                        );
                    }
                }
            }
        }
    }

    fn render_http(&self, output: &mut String) {
        for (method_index, method) in HTTP_METHODS.iter().enumerate() {
            for (route_index, route) in HTTP_ROUTES.iter().enumerate() {
                for (status_index, status) in STATUS_CLASSES.iter().enumerate() {
                    let index = index3(
                        method_index,
                        HTTP_ROUTES.len(),
                        route_index,
                        STATUS_CLASSES.len(),
                        status_index,
                    );
                    let labels = [("method", *method), ("route", *route), ("status", *status)];
                    push_metric(
                        output,
                        "talos_http_requests_total",
                        &labels,
                        self.http_requests[index].load(Ordering::Relaxed),
                    );
                    push_metric(
                        output,
                        "talos_http_request_latency_milliseconds_sum",
                        &labels,
                        self.http_latency_sum[index].load(Ordering::Relaxed),
                    );
                    push_metric(
                        output,
                        "talos_http_request_latency_milliseconds_count",
                        &labels,
                        self.http_latency_count[index].load(Ordering::Relaxed),
                    );
                }
            }
        }
    }

    fn render_integration_attempts(&self, output: &mut String) {
        for (outcome_index, outcome) in ATTEMPT_OUTCOMES.iter().enumerate() {
            for (dispatch_index, dispatch_class) in ATTEMPT_DISPATCH_CLASSES.iter().enumerate() {
                let index = index2(
                    outcome_index,
                    ATTEMPT_DISPATCH_CLASSES.len(),
                    dispatch_index,
                );
                let labels = [("outcome", *outcome), ("dispatch_class", *dispatch_class)];
                push_metric(
                    output,
                    "talos_integration_attempts_total",
                    &labels,
                    self.integration_attempts[index].load(Ordering::Relaxed),
                );
                push_metric(
                    output,
                    "talos_integration_attempt_latency_milliseconds_sum",
                    &labels,
                    self.integration_attempt_latency_sum[index].load(Ordering::Relaxed),
                );
                push_metric(
                    output,
                    "talos_integration_attempt_latency_milliseconds_count",
                    &labels,
                    self.integration_attempt_latency_count[index].load(Ordering::Relaxed),
                );
            }
        }
    }

    fn render_settlement(&self, output: &mut String) {
        for (command_index, command) in SETTLEMENT_COMMANDS.iter().enumerate() {
            for (outcome_index, outcome) in SETTLEMENT_OUTCOMES.iter().enumerate() {
                for (blocker_index, blocker) in SETTLEMENT_BLOCKERS.iter().enumerate() {
                    let index = index3(
                        command_index,
                        SETTLEMENT_OUTCOMES.len(),
                        outcome_index,
                        SETTLEMENT_BLOCKERS.len(),
                        blocker_index,
                    );
                    push_metric(
                        output,
                        "talos_r3_settlement_commands_total",
                        &[
                            ("command", *command),
                            ("outcome", *outcome),
                            ("blocker", *blocker),
                        ],
                        self.settlement[index].load(Ordering::Relaxed),
                    );
                }
            }
        }
    }

    fn render_pair_family(
        &self,
        output: &mut String,
        name: &'static str,
        first_key: &'static str,
        first_values: &[&'static str],
        second_key: &'static str,
        second_values: &[&'static str],
        counters: &[AtomicU64],
    ) {
        for (first_index, first) in first_values.iter().enumerate() {
            for (second_index, second) in second_values.iter().enumerate() {
                let index = first_index * second_values.len() + second_index;
                push_metric(
                    output,
                    name,
                    &[(first_key, first), (second_key, second)],
                    counters[index].load(Ordering::Relaxed),
                );
            }
        }
    }
}

impl MetricsSink for RuntimeMetrics {
    fn registry_attempt(&self, module: &str, command: &str, mode: &ExecutionMode) {
        let index = index3(
            closed_index(REGISTRY_MODULES, registry_metric_module(module)),
            REGISTRY_COMMANDS.len(),
            closed_index(REGISTRY_COMMANDS, registry_metric_command(module, command)),
            EXECUTION_MODES.len(),
            closed_index(EXECUTION_MODES, execution_mode(mode)),
        );
        Self::increment(&self.registry_attempts[index]);
    }

    fn registry_result(
        &self,
        module: &str,
        command: &str,
        mode: &ExecutionMode,
        result: ResultClass,
        elapsed: Duration,
    ) {
        let attempt_index = index3(
            closed_index(REGISTRY_MODULES, registry_metric_module(module)),
            REGISTRY_COMMANDS.len(),
            closed_index(REGISTRY_COMMANDS, registry_metric_command(module, command)),
            EXECUTION_MODES.len(),
            closed_index(EXECUTION_MODES, execution_mode(mode)),
        );
        let index = attempt_index * RESULT_CLASSES.len()
            + closed_index(RESULT_CLASSES, result_class(result));
        Self::increment(&self.registry_results[index]);
        Self::observe_latency(
            &self.registry_latency_sum[index],
            &self.registry_latency_count[index],
            elapsed,
        );
    }

    fn http_request(&self, method: &str, route: &str, status: u16, elapsed: Duration) {
        let index = index3(
            closed_index(HTTP_METHODS, http_method(method)),
            HTTP_ROUTES.len(),
            closed_index(HTTP_ROUTES, http_route(route)),
            STATUS_CLASSES.len(),
            closed_index(STATUS_CLASSES, status_class(status)),
        );
        Self::increment(&self.http_requests[index]);
        Self::observe_latency(
            &self.http_latency_sum[index],
            &self.http_latency_count[index],
            elapsed,
        );
    }

    fn integration_operation(&self, event: IntegrationEvent, state: OperationStateClass) {
        let index = index2(
            closed_index(INTEGRATION_EVENTS, integration_event(event)),
            OPERATION_STATES.len(),
            closed_index(OPERATION_STATES, operation_state(state)),
        );
        Self::increment(&self.integration[index]);
    }

    fn integration_attempt(
        &self,
        outcome: AttemptOutcome,
        dispatch_class: AttemptDispatchClass,
        elapsed: Duration,
    ) {
        let index = index2(
            closed_index(ATTEMPT_OUTCOMES, attempt_outcome(outcome)),
            ATTEMPT_DISPATCH_CLASSES.len(),
            closed_index(
                ATTEMPT_DISPATCH_CLASSES,
                attempt_dispatch_class(dispatch_class),
            ),
        );
        Self::increment(&self.integration_attempts[index]);
        Self::observe_latency(
            &self.integration_attempt_latency_sum[index],
            &self.integration_attempt_latency_count[index],
            elapsed,
        );
    }

    fn webhook(&self, event: WebhookEvent, outcome: WebhookOutcome) {
        let index = index2(
            closed_index(WEBHOOK_EVENTS, webhook_event(event)),
            WEBHOOK_OUTCOMES.len(),
            closed_index(WEBHOOK_OUTCOMES, webhook_outcome(outcome)),
        );
        Self::increment(&self.webhook[index]);
    }

    fn recovery(&self, source: RecoverySource, outcome: RecoveryOutcome) {
        let index = index2(
            closed_index(RECOVERY_SOURCES, recovery_source(source)),
            RECOVERY_OUTCOMES.len(),
            closed_index(RECOVERY_OUTCOMES, recovery_outcome(outcome)),
        );
        Self::increment(&self.recovery[index]);
    }

    fn workflow(&self, event: WorkflowEvent, outcome: WorkflowOutcome) {
        let index = index2(
            closed_index(WORKFLOW_EVENTS, workflow_event(event)),
            WORKFLOW_OUTCOMES.len(),
            closed_index(WORKFLOW_OUTCOMES, workflow_outcome(outcome)),
        );
        Self::increment(&self.workflow[index]);
    }

    fn settlement(&self, command: &str, outcome: SettlementOutcome, blocker: SettlementBlocker) {
        let index = index3(
            closed_index(SETTLEMENT_COMMANDS, settlement_command(command)),
            SETTLEMENT_OUTCOMES.len(),
            closed_index(SETTLEMENT_OUTCOMES, settlement_outcome(outcome)),
            SETTLEMENT_BLOCKERS.len(),
            closed_index(SETTLEMENT_BLOCKERS, settlement_blocker(blocker)),
        );
        Self::increment(&self.settlement[index]);
    }

    fn financial(&self, event: FinancialEvent) {
        let index = closed_index(FINANCIAL_EVENTS, financial_event(event));
        Self::increment(&self.financial[index]);
    }
}

#[derive(Default)]
pub(crate) struct NoopMetrics;
impl MetricsSink for NoopMetrics {}

pub(crate) fn classify_registry_error(error: &str) -> ResultClass {
    let payload = serde_json::from_str::<serde_json::Value>(error).ok();
    let category = payload
        .as_ref()
        .and_then(|value| value.get("category"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    match category {
        "auth" => return ResultClass::AccessDenied,
        "val" => return ResultClass::ValidationError,
        "biz" => return ResultClass::BusinessError,
        _ => {}
    }
    let code = payload
        .and_then(|value| {
            value
                .get("code")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
        })
        .unwrap_or_default();
    match code {
        value if value.starts_with("EXEC_PREVIEW_") => ResultClass::PreviewBlocked,
        value if value.starts_with("EXEC_SIMULATION_") => ResultClass::SimulationBlocked,
        value if value.starts_with("EXEC_ACCESS_") || value.starts_with("AUTH_") => {
            ResultClass::AccessDenied
        }
        value if value.starts_with("VALIDATION_") || value.starts_with("INPUT_") => {
            ResultClass::ValidationError
        }
        value if value.starts_with("BUSINESS_") || value.starts_with("R3_") => {
            ResultClass::BusinessError
        }
        _ => ResultClass::SystemError,
    }
}

fn index2(first: usize, second_width: usize, second: usize) -> usize {
    first * second_width + second
}

fn index3(
    first: usize,
    second_width: usize,
    second: usize,
    third_width: usize,
    third: usize,
) -> usize {
    (first * second_width + second) * third_width + third
}

fn closed_index(labels: &[&str], value: &str) -> usize {
    labels
        .iter()
        .position(|candidate| *candidate == value)
        .expect("metric classifiers only return closed label values")
}

/// Rendering accepts only compile-time label names and values. Callers must
/// classify runtime strings before they reach this final exporter boundary.
fn push_metric(
    output: &mut String,
    name: &'static str,
    labels: &[(&'static str, &'static str)],
    value: u64,
) {
    if value == 0 {
        return;
    }
    let labels = labels
        .iter()
        .map(|(key, value)| format!("{key}=\"{value}\""))
        .collect::<Vec<_>>()
        .join(",");
    output.push_str(&format!("{name}{{{labels}}} {value}\n"));
}

pub(crate) fn registry_metric_module(module: &str) -> &'static str {
    match module {
        "r3_settlement" => "r3_settlement",
        "integration" => "integration",
        "tenant_governance" => "tenant_governance",
        "tenant_preview" => "tenant_preview",
        "tenant_simulation" => "tenant_simulation",
        _ => "other",
    }
}

pub(crate) fn registry_metric_command(module: &str, command: &str) -> &'static str {
    match (module, command) {
        ("r3_settlement", "get_business_timezone") => "get_business_timezone",
        ("r3_settlement", "get_closure_facts") => "get_closure_facts",
        ("r3_settlement", "configure_business_timezone") => "configure_business_timezone",
        ("r3_settlement", "begin_inspection") => "begin_inspection",
        ("r3_settlement", "complete_inspection") => "complete_inspection",
        ("r3_settlement", "create_damage_finding") => "create_damage_finding",
        ("r3_settlement", "decide_liability") => "decide_liability",
        ("r3_settlement", "decide_repair") => "decide_repair",
        ("r3_settlement", "transition_repair") => "transition_repair",
        ("r3_settlement", "propose_settlement") => "propose_settlement",
        ("r3_settlement", "accept_settlement") => "accept_settlement",
        ("r3_settlement", "deduct_deposit") => "deduct_deposit",
        ("r3_settlement", "admit_additional_charge") => "admit_additional_charge",
        ("r3_settlement", "open_dispute") => "open_dispute",
        ("r3_settlement", "resolve_dispute") => "resolve_dispute",
        ("r3_settlement", "complete_settlement") => "complete_settlement",
        ("r3_settlement", "close_order") => "close_order",
        _ => "other",
    }
}

fn execution_mode(mode: &ExecutionMode) -> &'static str {
    match mode {
        ExecutionMode::Normal => "normal",
        ExecutionMode::ReadOnlyPreview(_) => "preview",
        ExecutionMode::Simulation(_) => "simulation",
    }
}

fn result_class(value: ResultClass) -> &'static str {
    match value {
        ResultClass::Succeeded => "succeeded",
        ResultClass::AccessDenied => "access_denied",
        ResultClass::PreviewBlocked => "preview_blocked",
        ResultClass::SimulationBlocked => "simulation_blocked",
        ResultClass::ValidationError => "validation_error",
        ResultClass::BusinessError => "business_error",
        ResultClass::SystemError => "system_error",
    }
}

fn http_method(value: &str) -> &'static str {
    match value {
        "GET" => "get",
        "POST" => "post",
        "PUT" => "put",
        "PATCH" => "patch",
        "DELETE" => "delete",
        _ => "other",
    }
}

fn http_route(value: &str) -> &'static str {
    match value {
        "/metrics" => "metrics",
        "/health" => "health",
        "/api/v3/rental-settlement/commands/[COMMAND]" => "r3_settlement_command",
        "/api/integrations/webhooks/[REDACTED]" => "integration_webhook",
        "/api/integrations" => "integrations",
        _ if value.starts_with("/api/") => "api_other",
        _ => "other",
    }
}

fn status_class(status: u16) -> &'static str {
    match status {
        100..=199 => "1xx",
        200..=299 => "2xx",
        300..=399 => "3xx",
        400..=499 => "4xx",
        _ => "5xx",
    }
}

macro_rules! enum_labels {
    ($function:ident, $type:ty, { $($variant:ident => $label:literal),+ $(,)? }) => {
        fn $function(value: $type) -> &'static str {
            match value { $(<$type>::$variant => $label,)+ }
        }
    };
}

enum_labels!(integration_event, IntegrationEvent, {
    Admitted => "admitted", Claimed => "claimed", DispatchStarted => "dispatch_started", Attempted => "attempted",
    Succeeded => "succeeded", Rejected => "rejected", RetryScheduled => "retry_scheduled", Deferred => "deferred",
    Blocked => "blocked", BlockedConfiguration => "blocked_configuration", SystemFailure => "system_failure",
    NonRetryableFailure => "non_retryable_failure",
    UnknownOutcome => "unknown_outcome", ReconciliationStarted => "reconciliation_started",
    ReconciliationResolved => "reconciliation_resolved",
    ManualResolutionRequired => "manual_resolution_required",
    CompensationScheduled => "compensation_scheduled"
});
enum_labels!(operation_state, OperationStateClass, {
    Planned => "planned", Ready => "ready", Dispatching => "dispatching", Succeeded => "succeeded",
    Rejected => "rejected", RetryableFailure => "retryable_failure",
    NonRetryableFailure => "non_retryable_failure", UnknownOutcome => "unknown_outcome",
    Reconciling => "reconciling", Resolved => "resolved",
    ManualResolutionRequired => "manual_resolution_required"
});
enum_labels!(webhook_event, WebhookEvent, {
    Received => "received", Verified => "verified", ProcessingStarted => "processing_started",
    Processed => "processed", RetryScheduled => "retry_scheduled", Rejected => "rejected",
    Conflict => "conflict", DeadLettered => "dead_lettered", Replayed => "replayed", Duplicate => "duplicate",
    SystemFailure => "system_failure"
});
enum_labels!(webhook_outcome, WebhookOutcome, {
    Pending => "pending", Accepted => "accepted", Rejected => "rejected", Retryable => "retryable",
    PermanentFailure => "permanent_failure", SystemFailure => "system_failure"
});
enum_labels!(recovery_source, RecoverySource, {
    StartupSnapshot => "startup_snapshot", TenantPage => "tenant_page", UnknownOutcome => "unknown_outcome",
    Webhook => "webhook", ScheduledPage => "scheduled_page"
});
enum_labels!(recovery_outcome, RecoveryOutcome, {
    Started => "started", Recovered => "recovered", Empty => "empty", Failed => "failed"
});
enum_labels!(workflow_event, WorkflowEvent, {
    StepProcessed => "step_processed", StepCompleted => "step_completed", RetryScheduled => "retry_scheduled",
    Blocked => "blocked", ManualResolutionRequired => "manual_resolution_required",
    CompensationScheduled => "compensation_scheduled", OutboxDelivered => "outbox_delivered", InboxConsumed => "inbox_consumed"
});
enum_labels!(workflow_outcome, WorkflowOutcome, {
    Succeeded => "succeeded", Retryable => "retryable", Blocked => "blocked", Failed => "failed"
});
enum_labels!(settlement_outcome, SettlementOutcome, {
    Succeeded => "succeeded", Blocked => "blocked", Failed => "failed"
});
enum_labels!(settlement_blocker, SettlementBlocker, {
    None => "none", ReturnIncomplete => "return_incomplete",
    InspectionIncomplete => "inspection_incomplete", DamageUnresolved => "damage_unresolved",
    LiabilityUnresolved => "liability_unresolved", RepairUnresolved => "repair_unresolved",
    DisputeActive => "dispute_active", EffectUnresolved => "effect_unresolved",
    OverdueActive => "overdue_active", FinancialUnbalanced => "financial_unbalanced",
    SettlementNotTerminal => "settlement_not_terminal",
    SettlementNotAccepted => "settlement_not_accepted", TerminalSealed => "terminal_sealed",
    OtherBlocked => "other_blocked"
});
enum_labels!(attempt_outcome, AttemptOutcome, {
    Succeeded => "succeeded", Rejected => "rejected", RetryableFailure => "retryable_failure",
    NonRetryableFailure => "non_retryable_failure", UnknownOutcome => "unknown_outcome",
    PersistenceFailure => "persistence_failure"
});
enum_labels!(attempt_dispatch_class, AttemptDispatchClass, {
    Dispatched => "dispatched", MayHaveDispatched => "may_have_dispatched",
    NotDispatched => "not_dispatched"
});
enum_labels!(financial_event, FinancialEvent, {
    DepositRecorded => "deposit_recorded", DepositReceivedLedger => "deposit_received_ledger",
    DepositDeductionAdmitted => "deposit_deduction_admitted",
    RefundIntentAdmitted => "refund_intent_admitted", RefundOperationPlanned => "refund_operation_planned",
    RefundReconciliation => "refund_reconciliation"
});

fn settlement_command(command: &str) -> &'static str {
    match command {
        "complete_inspection" => "complete_inspection",
        "create_damage_finding" => "create_damage_finding",
        "decide_liability" => "decide_liability",
        "decide_repair" => "decide_repair",
        "transition_repair" => "transition_repair",
        "propose_settlement" => "propose_settlement",
        "accept_settlement" => "accept_settlement",
        "deduct_deposit" => "deduct_deposit",
        "admit_additional_charge" => "admit_additional_charge",
        "open_dispute" => "open_dispute",
        "resolve_dispute" => "resolve_dispute",
        "complete_settlement" => "complete_settlement",
        "close_order" => "close_order",
        _ => "other",
    }
}

/// Converts only known R3 public error detail into a closed operational class.
/// The original error remains in the Registry audit/trace path; no free-form
/// detail leaves this classifier as a metric label.
pub(crate) fn classify_settlement_error(error: &str) -> (SettlementOutcome, SettlementBlocker) {
    let detail = serde_json::from_str::<serde_json::Value>(error)
        .ok()
        .and_then(|value| {
            value
                .get("message")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
        })
        .unwrap_or_else(|| error.to_owned())
        .to_ascii_lowercase();

    let blocker = if detail.contains("return_received=false") {
        Some(SettlementBlocker::ReturnIncomplete)
    } else if detail.contains("inspection_complete=false") {
        Some(SettlementBlocker::InspectionIncomplete)
    } else if nonzero_closure_count(&detail, "unresolved_damage_findings") {
        Some(SettlementBlocker::DamageUnresolved)
    } else if nonzero_closure_count(&detail, "unresolved_liability_decisions")
        || detail.contains("unresolved liability")
    {
        Some(SettlementBlocker::LiabilityUnresolved)
    } else if nonzero_closure_count(&detail, "unresolved_repair_cases") {
        Some(SettlementBlocker::RepairUnresolved)
    } else if nonzero_closure_count(&detail, "active_disputes") {
        Some(SettlementBlocker::DisputeActive)
    } else if nonzero_closure_count(&detail, "unresolved_effect_intents") {
        Some(SettlementBlocker::EffectUnresolved)
    } else if detail.contains("overdue_open=true") {
        Some(SettlementBlocker::OverdueActive)
    } else if detail.contains("financial_balance_fully_accounted=false")
        || detail.contains("exact accounting")
        || detail.contains("authorized settlement total")
    {
        Some(SettlementBlocker::FinancialUnbalanced)
    } else if detail.contains("settlement_terminal=false") {
        Some(SettlementBlocker::SettlementNotTerminal)
    } else if detail.contains("settlement must be accepted") {
        Some(SettlementBlocker::SettlementNotAccepted)
    } else if detail.contains("terminal r3 settlement is sealed") {
        Some(SettlementBlocker::TerminalSealed)
    } else if detail.contains("settlement requires") || detail.contains("blocked") {
        Some(SettlementBlocker::OtherBlocked)
    } else {
        None
    };

    match blocker {
        Some(blocker) => (SettlementOutcome::Blocked, blocker),
        None => (SettlementOutcome::Failed, SettlementBlocker::None),
    }
}

fn nonzero_closure_count(detail: &str, name: &str) -> bool {
    detail
        .split_once(&format!("{name}="))
        .and_then(|(_, suffix)| suffix.split([',', ';', ' ']).next())
        .is_some_and(|value| value != "0")
}

#[cfg(test)]
mod tests {
    use super::{
        FinancialEvent, IntegrationEvent, MetricsSink, OperationStateClass, ResultClass,
        RuntimeMetrics, SettlementBlocker, SettlementOutcome, WorkflowEvent, WorkflowOutcome,
        classify_registry_error, classify_settlement_error,
    };
    use std::time::Duration;
    use system_core::ExecutionMode;

    #[test]
    fn metrics_use_only_bounded_labels_and_never_render_secret_or_ids() {
        let metrics = RuntimeMetrics::default();
        metrics.registry_attempt(
            "tenant-id-raw",
            "Bearer secret-token",
            &ExecutionMode::Normal,
        );
        metrics.registry_result(
            "tenant-id-raw",
            "Bearer secret-token",
            &ExecutionMode::Normal,
            ResultClass::SystemError,
            Duration::from_millis(7),
        );
        metrics.http_request(
            "GET",
            "/api/webhook-token?session=session-cookie&secret=keystore://deployment/secret-id&provider_event_id=event-raw&order_id=order-raw",
            500,
            Duration::from_millis(2),
        );
        let output = metrics.render();
        assert!(output.contains("\n"));
        assert!(output.contains("talos_registry_commands_total"));
        assert!(output.contains("module=\"other\""));
        assert!(output.contains("command=\"other\""));
        assert!(output.contains("route=\"api_other\""));
        assert_eq!(super::registry_metric_module("tenant-id-raw"), "other");
        assert_eq!(
            super::registry_metric_command("tenant-id-raw", "Bearer secret-token"),
            "other"
        );
        assert_eq!(super::http_route("/api/opaque/session-cookie"), "api_other");
        assert_eq!(super::http_route("/opaque/session-cookie"), "other");
        assert!(!output.contains("\\\""));
        for secret_or_id in [
            "Bearer secret-token",
            "keystore://deployment/secret-id",
            "session-cookie",
            "webhook-token",
            "provider_event_id",
            "event-raw",
            "order-raw",
            "tenant-id-raw",
        ] {
            assert!(!output.contains(secret_or_id));
        }
    }

    #[test]
    fn settlement_metric_keeps_success_and_blocked_outcomes_bounded() {
        let metrics = RuntimeMetrics::default();
        metrics.settlement(
            "close_order",
            SettlementOutcome::Succeeded,
            SettlementBlocker::None,
        );
        metrics.settlement(
            "raw-command-value",
            SettlementOutcome::Blocked,
            SettlementBlocker::ReturnIncomplete,
        );
        let output = metrics.render();
        assert!(output.contains("talos_r3_settlement_commands_total"));
        assert!(output.contains("command=\"close_order\",outcome=\"succeeded\",blocker=\"none\""));
        assert!(
            output.contains("command=\"other\",outcome=\"blocked\",blocker=\"return_incomplete\"")
        );
        assert!(!output.contains("raw-command-value"));
    }

    #[test]
    fn fixed_atomic_lanes_render_financial_integration_and_workflow_events_without_ids() {
        let metrics = RuntimeMetrics::default();
        metrics.integration_operation(IntegrationEvent::Admitted, OperationStateClass::Planned);
        metrics.financial(FinancialEvent::RefundReconciliation);
        metrics.workflow(WorkflowEvent::OutboxDelivered, WorkflowOutcome::Succeeded);
        let output = metrics.render();
        assert!(output.contains("event=\"admitted\",state=\"planned\""));
        assert!(output.contains("event=\"refund_reconciliation\""));
        assert!(output.contains("event=\"outbox_delivered\",outcome=\"succeeded\""));
        assert!(!output.contains("tenant-a"));
        assert!(!output.contains("operation-123"));
    }

    #[test]
    fn r3_terminal_blockers_and_registry_business_errors_are_closed_classes() {
        let (outcome, blocker) = classify_settlement_error(
            r#"{"category":"biz","code":"REPOSITORY_CONTRACT_VIOLATION","message":"R3 terminal closure blocked: return_received=true; inspection_complete=true; unresolved_damage_findings=0; unresolved_liability_decisions=0; unresolved_repair_cases=0; active_disputes=0; unresolved_effect_intents=1; overdue_open=false; settlement_terminal=true; financial_balance_fully_accounted=true"}"#,
        );
        assert_eq!(outcome, SettlementOutcome::Blocked);
        assert_eq!(blocker, SettlementBlocker::EffectUnresolved);
        assert_eq!(
            classify_settlement_error(
                r#"{"category":"biz","message":"settlement requires an accepted proposal"}"#
            ),
            (SettlementOutcome::Blocked, SettlementBlocker::OtherBlocked),
        );
        assert_eq!(
            classify_registry_error(
                r#"{"category":"biz","code":"REPOSITORY_CONTRACT_VIOLATION","message":"untrusted raw detail"}"#
            ),
            ResultClass::BusinessError,
        );
    }
}
