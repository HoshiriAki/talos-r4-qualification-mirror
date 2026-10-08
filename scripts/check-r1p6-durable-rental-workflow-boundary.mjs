#!/usr/bin/env node
import fs from 'node:fs'
import path from 'node:path'
import process from 'node:process'
const root = path.resolve(process.env.TALOS_R1P6_ROOT || process.cwd())
const failures = []
function read(rel) { const file = path.join(root, rel); if (!fs.existsSync(file)) { failures.push(`missing ${rel}`); return '' } return fs.readFileSync(file, 'utf8').replaceAll('\r\n', '\n') }
function requireTokens(rel, tokens) { const text = read(rel); for (const token of tokens) if (!text.includes(token)) failures.push(`${rel}: missing ${JSON.stringify(token)}`); return text }
function forbidTokens(rel, tokens) { const text = read(rel); for (const token of tokens) if (text.includes(token)) failures.push(`${rel}: forbidden ${JSON.stringify(token)}`); return text }
for (const rel of ['backend/src/db/migrations/057_durable_rental_workflow.sql', 'backend/src/db/migrations/postgres/057_durable_rental_workflow.sql']) requireTokens(rel, ['workflow_instances', 'workflow_steps', 'workflow_blockers', 'manual_decision_tasks', 'domain_outbox', 'domain_inbox', 'definition_id', 'definition_version', 'definition_hash', 'UNIQUE (tenant_id, message_id)', 'idx_workflow_steps_due', 'idx_domain_outbox_due'])
requireTokens('backend/src/db/migrations.rs', ['057_durable_rental_workflow', 'migrations/057_durable_rental_workflow.sql', 'Some("066_r3_machine_api")'])
requireTokens('backend/src/db/migrations_pg.rs', ['057_durable_rental_workflow', 'migrations/postgres/057_durable_rental_workflow.sql', 'ids.len(), 63'])
const repository = requireTokens('backend/src/repositories/workflow.rs', ['pub struct ScopedWorkflowRepository', 'pub const RENTAL_DEFINITION_ID', 'pub const RENTAL_DEFINITION_VERSION', 'pub const RENTAL_DEFINITION_HASH', 'write_immediate', 'claim_due', 'consume_domain_events', 'illegal workflow transition', 'append_outbox_tx', 'RetryScheduled', 'Blocked', 'Compensating', 'ManualReview'])
for (const forbidden of ['pub pool:', 'pub fn pool(', 'tenant_id: &str) -> ScopedWorkflowRepository', 'reqwest', 'HttpClient', 'WorkflowEngine', 'BpmEngine', 'GenericSagaExecutor']) if (repository.includes(forbidden)) failures.push(`backend/src/repositories/workflow.rs: forbidden ${JSON.stringify(forbidden)}`)
requireTokens('backend/src/repositories/contracts/provider.rs', ['pub fn workflows(&self)', 'ScopedWorkflowRepository::new(self)'])
const manager = requireTokens('backend/src/application/rental_workflow.rs', ['DurableRentalProcessManager', 'RentalWorkflowEffects', 'UnavailableProductionRentalEffects', 'WORKFLOW_DEFINITION_MISMATCH', 'DOWNSTREAM_AUTHORITY_UNAVAILABLE', 'lifecycles()', 'reservations()', 'allocation_complete_for_order'])
for (const forbidden of ['reqwest', 'HttpClient', 'tokio::time::interval', 'WorkflowEngine', 'BpmEngine']) if (manager.includes(forbidden)) failures.push(`backend/src/application/rental_workflow.rs: forbidden ${JSON.stringify(forbidden)}`)
requireTokens('backend/src/application/workers.rs', ['ProcessDurableRentalWorkflows', 'DurableRentalWorkflowWorker', 'tenant_context', 'consume_domain_events', 'run_batch'])
forbidTokens('backend/src/main.rs', ['tokio::time::interval'])
requireTokens('backend/src/repositories/lifecycle.rs', ['if action == "confirm_order"', 'OrderConfirmed', 'append_outbox_tx'])
requireTokens('backend/src/repositories/reservation.rs', ['ReservationConfirmed', 'AllocationComplete', 'required > 0 && allocated >= required', 'append_outbox_tx'])
requireTokens('backend/src/application/workflow_tests.rs', ['durable_identity_steps_inbox_and_restart_are_persisted_and_idempotent', 'claim_is_exclusive', 'RetryScheduled', 'REPOSITORY_PREVIEW_WRITE_DENIED', 'REPOSITORY_SIMULATION_UNSUPPORTED', 'deterministic_effects_cover_success_retry_blocker_manual_and_compensation_without_network', 'production_future_capability_is_visibly_blocked'])
const experimental = read('backend/system/core/src/experimental/orchestration.rs'); if (!experimental.includes('no Saga') || !experimental.includes('outbox')) failures.push('experimental orchestration must remain explicitly non-operational')
requireTokens('package.json', ['quality:talos-ops:durable-workflow:test', 'quality:talos-ops:durable-workflow'])
if (failures.length) { process.stderr.write('TALOS-OPS-032 R1-P6 durable rental workflow boundary check failed:\n'); for (const failure of failures) process.stderr.write(`- ${failure}\n`); process.exit(1) }
process.stdout.write('TALOS-OPS-032 R1-P6 durable rental workflow boundary passed.\n')
