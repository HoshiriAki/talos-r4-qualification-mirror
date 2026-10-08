use std::collections::HashMap;
use std::sync::Arc;

use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use system_core::{
    ActorIdentity, AuthorityContext, DataScope, ExecutionContext, ExecutionMode, Namespace,
    NoopHttpClient, PlatformMembershipId, PlatformRole, PreviewSessionId, RequestId, Revision,
    SimulationId, SystemModule, TenantId, TenantMembershipId, TenantRole, TenantScope,
};

use crate::integration::operation::OperationState;
use crate::integration::store::IntegrationStore;
use crate::integration::types::ExternalOperationId;
use crate::observability::RuntimeMetrics;
use crate::registry::ModuleRegistry;
use crate::repositories::{
    AdditionalChargeInput, DamageFindingInput, DepositDeductionInput, InspectionCompletionInput,
    LiabilityDecisionInput, OpenDisputeInput, RepairDecisionInput, RepairTransitionInput,
    RepositoryProvider, ResolveDisputeInput, SettlementLineInput, SettlementProposalInput,
    SqliteRepositoryProvider,
};
use crate::utils::constants::txt;

use super::R3SettlementModule;

fn pool() -> Pool<SqliteConnectionManager> {
    let pool = Pool::builder()
        .max_size(1)
        .build(SqliteConnectionManager::memory())
        .unwrap();
    let connection = pool.get().unwrap();
    connection.execute_batch(
        "PRAGMA foreign_keys=ON;
         CREATE TABLE tenants (id TEXT PRIMARY KEY, name TEXT, slug TEXT, status TEXT, plan TEXT, settings TEXT, created_at TEXT, updated_at TEXT);
         INSERT INTO tenants VALUES ('tenant-a','A','a','active','test',NULL,'2026-01-01T00:00:00Z','2026-01-01T00:00:00Z');
         INSERT INTO tenants VALUES ('tenant-b','B','b','active','test',NULL,'2026-01-01T00:00:00Z','2026-01-01T00:00:00Z');
         CREATE TABLE orders(id TEXT NOT NULL,tenant_id TEXT NOT NULL,startDate TEXT NOT NULL,endDate TEXT NOT NULL,status TEXT NOT NULL,createdAt TEXT NOT NULL,PRIMARY KEY(id,tenant_id));
         CREATE TABLE rental_reservations(id TEXT NOT NULL,tenant_id TEXT NOT NULL,order_id TEXT,status TEXT,created_at TEXT,updated_at TEXT,PRIMARY KEY(tenant_id,id));
         CREATE TABLE reservation_requirements(id TEXT PRIMARY KEY,tenant_id TEXT NOT NULL,reservation_id TEXT NOT NULL,model_id TEXT NOT NULL,quantity INTEGER NOT NULL);
         CREATE TABLE allocations(id TEXT NOT NULL,tenant_id TEXT NOT NULL,reservation_id TEXT NOT NULL,device_serial_no TEXT NOT NULL,model_id TEXT,status TEXT NOT NULL,PRIMARY KEY(tenant_id,id));
         CREATE TABLE devices(id TEXT PRIMARY KEY,serialNo TEXT NOT NULL,tenant_id TEXT NOT NULL,rentalStatus TEXT NOT NULL,createdAt TEXT NOT NULL,UNIQUE(tenant_id,serialNo));
         CREATE TABLE overdue_records(id INTEGER PRIMARY KEY,tenant_id TEXT NOT NULL,order_id TEXT NOT NULL,status TEXT NOT NULL);",
    ).unwrap();
    for migration in [
        include_str!("../db/migrations/056_order_lifecycle_v2.sql"),
        include_str!("../db/migrations/057_durable_rental_workflow.sql"),
        include_str!("../db/migrations/058_return_inspection_settlement.sql"),
        include_str!("../db/migrations/059_integration_fabric.sql"),
        include_str!("../db/migrations/063_r3_damage_settlement_clock.sql"),
        include_str!("../db/migrations/064_r3_settlement_integration_authority.sql"),
        include_str!("../db/migrations/065_r3_terminal_settlement_seal.sql"),
    ] {
        connection.execute_batch(migration).unwrap();
    }
    pool
}

fn context(tenant: &str, mode: ExecutionMode, namespace: Namespace) -> ExecutionContext {
    let tenant_id = TenantId::new(tenant).unwrap();
    let actor = match mode {
        ExecutionMode::Normal => ActorIdentity::with_authority(
            format!("actor-{tenant}"),
            AuthorityContext::Tenant {
                membership_id: TenantMembershipId::new(format!("member-{tenant}")).unwrap(),
                tenant_id: tenant_id.clone(),
                role: TenantRole::Admin,
            },
        )
        .unwrap(),
        _ => ActorIdentity::with_authority(
            "platform-owner",
            AuthorityContext::Platform {
                membership_id: PlatformMembershipId::new("platform-member").unwrap(),
                roles: vec![PlatformRole::Owner],
            },
        )
        .unwrap(),
    };
    ExecutionContext::new(
        actor,
        TenantScope::tenant(tenant_id.clone()),
        DataScope::new(tenant_id, namespace, Revision::new("r3-test").unwrap()).unwrap(),
        mode,
        RequestId::new(format!("request-{tenant}")).unwrap(),
        None,
        Arc::new(NoopHttpClient),
    )
    .unwrap()
}

fn normal(tenant: &str) -> ExecutionContext {
    context(tenant, ExecutionMode::Normal, Namespace::Production)
}

fn staff(tenant: &str) -> ExecutionContext {
    let tenant_id = TenantId::new(tenant).unwrap();
    ExecutionContext::new(
        ActorIdentity::with_authority(
            format!("staff-{tenant}"),
            AuthorityContext::Tenant {
                membership_id: TenantMembershipId::new(format!("staff-member-{tenant}")).unwrap(),
                tenant_id: tenant_id.clone(),
                role: TenantRole::Staff,
            },
        )
        .unwrap(),
        TenantScope::tenant(tenant_id.clone()),
        DataScope::new(
            tenant_id,
            Namespace::Production,
            Revision::new("r3-staff-test").unwrap(),
        )
        .unwrap(),
        ExecutionMode::Normal,
        RequestId::new(format!("staff-request-{tenant}")).unwrap(),
        None,
        Arc::new(NoopHttpClient),
    )
    .unwrap()
}

fn seed_order(pool: &Pool<SqliteConnectionManager>, tenant: &str, order: &str, serial: &str) {
    let connection = pool.get().unwrap();
    connection.execute("INSERT INTO orders(id,tenant_id,startDate,endDate,status,createdAt) VALUES (?1,?2,'2026-01-01','2026-01-02','draft','2026-01-01T00:00:00Z')",rusqlite::params![order,tenant]).unwrap();
    connection.execute("INSERT INTO rental_reservations(id,tenant_id,order_id,status,created_at,updated_at) VALUES (?1,?2,?3,'confirmed','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z')",rusqlite::params![format!("reservation-{order}"),tenant,order]).unwrap();
    connection.execute("INSERT INTO allocations(id,tenant_id,reservation_id,device_serial_no,status) VALUES (?1,?2,?3,?4,'allocated')",rusqlite::params![format!("allocation-{order}"),tenant,format!("reservation-{order}"),serial]).unwrap();
    connection.execute("INSERT INTO devices(id,serialNo,tenant_id,rentalStatus,createdAt) VALUES (?1,?2,?3,?4,'2026-01-01T00:00:00Z')",rusqlite::params![format!("device-{order}"),serial,tenant,txt::STATUS_CHECKED_IN]).unwrap();
}

fn seed_additional_allocation(
    pool: &Pool<SqliteConnectionManager>,
    order: &str,
    allocation_id: &str,
    serial: &str,
) {
    let connection = pool.get().unwrap();
    connection.execute(
        "INSERT INTO allocations(id,tenant_id,reservation_id,device_serial_no,status) VALUES (?1,'tenant-a',?2,?3,'allocated')",
        rusqlite::params![allocation_id, format!("reservation-{order}"), serial],
    ).unwrap();
    connection.execute(
        "INSERT INTO devices(id,serialNo,tenant_id,rentalStatus,createdAt) VALUES (?1,?2,'tenant-a',?3,'2026-01-01T00:00:00Z')",
        rusqlite::params![format!("device-{order}-{allocation_id}"), serial, txt::STATUS_CHECKED_IN],
    ).unwrap();
}

fn inspection_id(pool: &Pool<SqliteConnectionManager>, allocation_id: &str) -> String {
    pool.get()
        .unwrap()
        .query_row(
            "SELECT id FROM rental_inspections WHERE tenant_id='tenant-a' AND allocation_id=?1",
            [allocation_id],
            |row| row.get(0),
        )
        .unwrap()
}

fn receive_allocation(
    pool: &Pool<SqliteConnectionManager>,
    provider: &SqliteRepositoryProvider,
    order: &str,
    allocation_id: &str,
    receive_identity: &str,
) -> String {
    provider
        .bind(&normal("tenant-a"))
        .unwrap()
        .rental_closure()
        .receive_allocation(order, allocation_id, receive_identity)
        .unwrap();
    inspection_id(pool, allocation_id)
}

fn complete_inspection(
    provider: &SqliteRepositoryProvider,
    inspection_id: &str,
    condition_code: &str,
    missing: bool,
    normal_wear: bool,
) {
    let scoped = provider.bind(&normal("tenant-a")).unwrap();
    scoped
        .rental_closure()
        .transition_inspection(inspection_id, "in_progress", 1)
        .unwrap();
    scoped
        .r3_settlements()
        .complete_inspection(
            InspectionCompletionInput {
                inspection_id: inspection_id.into(),
                expected_version: 2,
                condition_code: condition_code.into(),
                missing,
                normal_wear,
                evidence_refs: vec![format!("evidence://inspection/{inspection_id}")],
            },
            "actor-a",
        )
        .unwrap();
}

fn create_open_damage_finding(
    pool: &Pool<SqliteConnectionManager>,
    provider: &SqliteRepositoryProvider,
    order: &str,
    allocation_id: &str,
) -> String {
    let inspection_id = receive_allocation(
        pool,
        provider,
        order,
        allocation_id,
        &format!("scan-{order}-{allocation_id}"),
    );
    complete_inspection(provider, &inspection_id, "damaged", false, false);
    let scoped = provider.bind(&normal("tenant-a")).unwrap();
    let finding = scoped
        .r3_settlements()
        .create_damage_finding(
            DamageFindingInput {
                inspection_id,
                finding_code: format!("lens-crack-{allocation_id}"),
                condition_code: "damaged".into(),
                description: "front lens cracked".into(),
                evidence_refs: vec!["evidence://damage/1".into()],
                idempotency_key: format!("finding-{order}-{allocation_id}"),
            },
            "actor-a",
        )
        .unwrap();
    finding.id
}

fn create_decided_damage_finding(
    pool: &Pool<SqliteConnectionManager>,
    provider: &SqliteRepositoryProvider,
    order: &str,
    allocation_id: &str,
    amount_minor: i64,
) -> String {
    let finding_id = create_open_damage_finding(pool, provider, order, allocation_id);
    let scoped = provider.bind(&normal("tenant-a")).unwrap();
    let decision = scoped
        .r3_settlements()
        .decide_liability(
            LiabilityDecisionInput {
                finding_id: finding_id.clone(),
                decision: "liable".into(),
                apportioned_amount_minor: amount_minor,
                currency: "CNY".into(),
                rationale: "inspection evidence confirms renter-caused damage".into(),
                evidence_refs: vec!["evidence://liability/1".into()],
                idempotency_key: format!("liability-{order}-{allocation_id}"),
            },
            "actor-a",
        )
        .unwrap();
    assert_eq!(decision.status, "decided");
    finding_id
}

fn seed_deposit(pool: &Pool<SqliteConnectionManager>, id: &str, order: &str, amount_minor: i64) {
    let connection = pool.get().unwrap();
    connection.execute(
        "INSERT INTO integration_deposits (id,tenant_id,authority_kind,authority_id,expected_amount_minor,currency,state,created_at,updated_at) VALUES (?1,'tenant-a','order',?2,?3,'CNY','recorded','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z')",
        rusqlite::params![id, order, amount_minor],
    ).unwrap();
    connection.execute(
        "INSERT INTO integration_deposit_ledger (id,tenant_id,deposit_id,entry_type,amount_minor,currency,audit_ref,created_at) VALUES (?1,'tenant-a',?2,'received',?3,'CNY','fixture','2026-01-01T00:00:00Z')",
        rusqlite::params![format!("ledger-{id}"), id, amount_minor],
    ).unwrap();
}

fn accepted_settlement_with_fundable_deposit(
    pool: &Pool<SqliteConnectionManager>,
    provider: &SqliteRepositoryProvider,
    order: &str,
    total_minor: i64,
) -> String {
    let (settlement_id, _) = create_damage_case(pool, provider, order, total_minor);
    provider
        .bind(&normal("tenant-a"))
        .unwrap()
        .r3_settlements()
        .accept_settlement(&settlement_id)
        .unwrap();
    seed_fixture_charge_binding(pool);
    seed_deposit(pool, &format!("deposit-{order}"), order, total_minor);
    settlement_id
}

fn device_status(pool: &Pool<SqliteConnectionManager>, serial: &str) -> String {
    pool.get()
        .unwrap()
        .query_row(
            "SELECT rentalStatus FROM devices WHERE tenant_id='tenant-a' AND serialNo=?1",
            [serial],
            |row| row.get(0),
        )
        .unwrap()
}

fn seed_fixture_charge_binding(pool: &Pool<SqliteConnectionManager>) {
    let connection = pool.get().unwrap();
    connection.execute("INSERT INTO provider_manifests (provider_id,version,capabilities_json,config_schema_json,secret_schema_json,api_versions_json,webhook_types_json,simulation_capabilities_json,readiness,compatibility_json,created_at) VALUES ('fixture','1','[\"payment.charge\"]','[]','[]','{}','[]','[]','fixture','{}','2026-01-01T00:00:00Z')", []).unwrap();
    connection.execute("INSERT INTO provider_instances (id,tenant_id,provider_id,manifest_version,config_revision,config_json,secret_refs_json,lifecycle,readiness,health,created_at,updated_at) VALUES ('instance-a','tenant-a','fixture','1','1','{}','{}','active','fixture','ready','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z')", []).unwrap();
    connection.execute("INSERT INTO provider_bindings (id,tenant_id,provider_instance_id,capability_id,config_revision,enabled,created_at,updated_at) VALUES ('binding-a','tenant-a','instance-a','payment.charge','1',1,'2026-01-01T00:00:00Z','2026-01-01T00:00:00Z')", []).unwrap();
}

fn complete_r2_operation(
    pool: &Pool<SqliteConnectionManager>,
    operation_id: &str,
    state: OperationState,
) {
    let store = IntegrationStore::new(pool.clone());
    let operation_id = ExternalOperationId::new(operation_id).unwrap();
    let attempt_id = store.start_attempt("tenant-a", &operation_id).unwrap();
    store
        .complete_attempt("tenant-a", &operation_id, &attempt_id, state, None, None)
        .unwrap();
}

fn create_damage_case(
    pool: &Pool<SqliteConnectionManager>,
    provider: &SqliteRepositoryProvider,
    order: &str,
    amount_minor: i64,
) -> (String, String) {
    seed_order(pool, "tenant-a", order, &format!("serial-{order}"));
    let finding_id = create_decided_damage_finding(
        pool,
        provider,
        order,
        &format!("allocation-{order}"),
        amount_minor,
    );
    let scoped = provider.bind(&normal("tenant-a")).unwrap();
    let repair = scoped
        .r3_settlements()
        .decide_repair(
            RepairDecisionInput {
                finding_id: finding_id.clone(),
                decision: "repair_required".into(),
                idempotency_key: format!("repair-{order}"),
            },
            "actor-a",
        )
        .unwrap();
    scoped
        .r3_settlements()
        .transition_repair(RepairTransitionInput {
            repair_case_id: repair.id.clone(),
            target_status: "under_repair".into(),
            expected_version: 1,
        })
        .unwrap();
    scoped
        .r3_settlements()
        .transition_repair(RepairTransitionInput {
            repair_case_id: repair.id,
            target_status: "repaired".into(),
            expected_version: 2,
        })
        .unwrap();
    let settlement = scoped
        .r3_settlements()
        .propose_settlement(
            SettlementProposalInput {
                order_id: order.into(),
                currency: "CNY".into(),
                idempotency_key: format!("settlement-{order}"),
                lines: vec![SettlementLineInput {
                    finding_id: finding_id.clone(),
                    line_kind: "damage".into(),
                    amount_minor,
                    description: "lens replacement".into(),
                }],
            },
            "actor-a",
        )
        .unwrap();
    (settlement.id, finding_id)
}

#[test]
fn settlement_phase_requires_a_scoped_order_return_and_complete_inspection_before_zero_value_terminal_settlement()
 {
    let pool = pool();
    let provider = SqliteRepositoryProvider::new(pool.clone());
    let scoped = provider.bind(&normal("tenant-a")).unwrap();

    assert!(
        scoped
            .r3_settlements()
            .propose_settlement(
                SettlementProposalInput {
                    order_id: "missing-order".into(),
                    currency: "CNY".into(),
                    idempotency_key: "missing-order-settlement".into(),
                    lines: vec![],
                },
                "actor-a"
            )
            .is_err()
    );

    seed_order(&pool, "tenant-a", "order-no-return", "serial-no-return");
    let no_return = scoped
        .r3_settlements()
        .propose_settlement(
            SettlementProposalInput {
                order_id: "order-no-return".into(),
                currency: "CNY".into(),
                idempotency_key: "no-return-settlement".into(),
                lines: vec![],
            },
            "actor-a",
        )
        .unwrap();
    scoped
        .r3_settlements()
        .accept_settlement(&no_return.id)
        .unwrap();
    assert!(
        scoped
            .r3_settlements()
            .complete_settlement(&no_return.id)
            .is_err()
    );

    seed_order(
        &pool,
        "tenant-a",
        "order-partial-inspection",
        "serial-partial-primary",
    );
    seed_additional_allocation(
        &pool,
        "order-partial-inspection",
        "allocation-partial-secondary",
        "serial-partial-secondary",
    );
    let primary_inspection = receive_allocation(
        &pool,
        &provider,
        "order-partial-inspection",
        "allocation-order-partial-inspection",
        "scan-partial-primary",
    );
    receive_allocation(
        &pool,
        &provider,
        "order-partial-inspection",
        "allocation-partial-secondary",
        "scan-partial-secondary",
    );
    complete_inspection(&provider, &primary_inspection, "good", false, false);
    let partial = scoped
        .r3_settlements()
        .propose_settlement(
            SettlementProposalInput {
                order_id: "order-partial-inspection".into(),
                currency: "CNY".into(),
                idempotency_key: "partial-inspection-settlement".into(),
                lines: vec![],
            },
            "actor-a",
        )
        .unwrap();
    scoped
        .r3_settlements()
        .accept_settlement(&partial.id)
        .unwrap();
    assert!(
        scoped
            .r3_settlements()
            .complete_settlement(&partial.id)
            .is_err()
    );

    seed_order(&pool, "tenant-a", "order-normal-zero", "serial-normal-zero");
    let normal_inspection = receive_allocation(
        &pool,
        &provider,
        "order-normal-zero",
        "allocation-order-normal-zero",
        "scan-normal-zero",
    );
    complete_inspection(&provider, &normal_inspection, "good", false, false);
    let normal_zero = scoped
        .r3_settlements()
        .propose_settlement(
            SettlementProposalInput {
                order_id: "order-normal-zero".into(),
                currency: "CNY".into(),
                idempotency_key: "normal-zero-settlement".into(),
                lines: vec![],
            },
            "actor-a",
        )
        .unwrap();
    assert_eq!(normal_zero.total_amount_minor, 0);
    scoped
        .r3_settlements()
        .accept_settlement(&normal_zero.id)
        .unwrap();
    assert_eq!(
        scoped
            .r3_settlements()
            .complete_settlement(&normal_zero.id)
            .unwrap()
            .status,
        "settled"
    );
}

#[test]
fn settlement_lines_require_exact_complete_liability_accounting() {
    let pool = pool();
    let provider = SqliteRepositoryProvider::new(pool.clone());
    let scoped = provider.bind(&normal("tenant-a")).unwrap();

    seed_order(
        &pool,
        "tenant-a",
        "order-under-accounted",
        "serial-under-accounted",
    );
    let under = create_decided_damage_finding(
        &pool,
        &provider,
        "order-under-accounted",
        "allocation-order-under-accounted",
        500,
    );
    assert!(
        scoped
            .r3_settlements()
            .propose_settlement(
                SettlementProposalInput {
                    order_id: "order-under-accounted".into(),
                    currency: "CNY".into(),
                    idempotency_key: "under-accounted".into(),
                    lines: vec![SettlementLineInput {
                        finding_id: under,
                        line_kind: "damage".into(),
                        amount_minor: 1,
                        description: "under-accounted liability".into(),
                    }],
                },
                "actor-a"
            )
            .is_err()
    );

    seed_order(
        &pool,
        "tenant-a",
        "order-zero-accounted",
        "serial-zero-accounted",
    );
    let zero = create_decided_damage_finding(
        &pool,
        &provider,
        "order-zero-accounted",
        "allocation-order-zero-accounted",
        500,
    );
    assert!(
        scoped
            .r3_settlements()
            .propose_settlement(
                SettlementProposalInput {
                    order_id: "order-zero-accounted".into(),
                    currency: "CNY".into(),
                    idempotency_key: "zero-accounted".into(),
                    lines: vec![SettlementLineInput {
                        finding_id: zero,
                        line_kind: "damage".into(),
                        amount_minor: 0,
                        description: "zeroed liability without adjustment".into(),
                    }],
                },
                "actor-a"
            )
            .is_err()
    );

    seed_order(
        &pool,
        "tenant-a",
        "order-currency-accounting",
        "serial-currency-accounting",
    );
    let currency = create_decided_damage_finding(
        &pool,
        &provider,
        "order-currency-accounting",
        "allocation-order-currency-accounting",
        500,
    );
    assert!(
        scoped
            .r3_settlements()
            .propose_settlement(
                SettlementProposalInput {
                    order_id: "order-currency-accounting".into(),
                    currency: "USD".into(),
                    idempotency_key: "currency-mismatch".into(),
                    lines: vec![SettlementLineInput {
                        finding_id: currency,
                        line_kind: "damage".into(),
                        amount_minor: 500,
                        description: "currency mismatch".into(),
                    }],
                },
                "actor-a"
            )
            .is_err()
    );

    seed_order(
        &pool,
        "tenant-a",
        "order-omitted-liability",
        "serial-omitted-primary",
    );
    seed_additional_allocation(
        &pool,
        "order-omitted-liability",
        "allocation-omitted-secondary",
        "serial-omitted-secondary",
    );
    let first = create_decided_damage_finding(
        &pool,
        &provider,
        "order-omitted-liability",
        "allocation-order-omitted-liability",
        500,
    );
    create_decided_damage_finding(
        &pool,
        &provider,
        "order-omitted-liability",
        "allocation-omitted-secondary",
        200,
    );
    assert!(
        scoped
            .r3_settlements()
            .propose_settlement(
                SettlementProposalInput {
                    order_id: "order-omitted-liability".into(),
                    currency: "CNY".into(),
                    idempotency_key: "omitted-liability".into(),
                    lines: vec![SettlementLineInput {
                        finding_id: first,
                        line_kind: "damage".into(),
                        amount_minor: 500,
                        description: "only one of two liable findings".into(),
                    }],
                },
                "actor-a"
            )
            .is_err()
    );

    seed_order(
        &pool,
        "tenant-a",
        "order-exact-accounting",
        "serial-exact-accounting",
    );
    let exact = create_decided_damage_finding(
        &pool,
        &provider,
        "order-exact-accounting",
        "allocation-order-exact-accounting",
        500,
    );
    assert_eq!(
        scoped
            .r3_settlements()
            .propose_settlement(
                SettlementProposalInput {
                    order_id: "order-exact-accounting".into(),
                    currency: "CNY".into(),
                    idempotency_key: "exact-accounting".into(),
                    lines: vec![SettlementLineInput {
                        finding_id: exact,
                        line_kind: "damage".into(),
                        amount_minor: 500,
                        description: "exact authorized liability".into(),
                    }],
                },
                "actor-a"
            )
            .unwrap()
            .total_amount_minor,
        500
    );
}

#[test]
fn damaged_return_deducts_authorized_deposit_atomically_and_is_idempotent() {
    let pool = pool();
    let provider = SqliteRepositoryProvider::new(pool.clone());
    let (settlement_id, finding_id) = create_damage_case(&pool, &provider, "order-damaged", 500);
    let scoped = provider.bind(&normal("tenant-a")).unwrap();
    scoped
        .r3_settlements()
        .accept_settlement(&settlement_id)
        .unwrap();
    {
        let connection = pool.get().unwrap();
        connection.execute("INSERT INTO integration_deposits (id,tenant_id,authority_kind,authority_id,expected_amount_minor,currency,state,created_at,updated_at) VALUES ('deposit-a','tenant-a','order','order-damaged',1000,'CNY','recorded','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z')", []).unwrap();
        connection.execute("INSERT INTO integration_deposit_ledger (id,tenant_id,deposit_id,entry_type,amount_minor,currency,audit_ref,created_at) VALUES ('ledger-a','tenant-a','deposit-a','received',1000,'CNY','fixture','2026-01-01T00:00:00Z')", []).unwrap();
    }
    let first = scoped
        .r3_settlements()
        .deduct_deposit(
            DepositDeductionInput {
                settlement_case_id: settlement_id.clone(),
                deposit_id: "deposit-a".into(),
                amount_minor: 500,
                idempotency_key: "deduct-1".into(),
            },
            "actor-a",
        )
        .unwrap();
    let repeat = scoped
        .r3_settlements()
        .deduct_deposit(
            DepositDeductionInput {
                settlement_case_id: settlement_id.clone(),
                deposit_id: "deposit-a".into(),
                amount_minor: 500,
                idempotency_key: "deduct-1".into(),
            },
            "actor-a",
        )
        .unwrap();
    assert_eq!(first.id, repeat.id);
    assert_eq!(first.external_operation_state, None);
    assert!(
        scoped
            .r3_settlements()
            .deduct_deposit(
                DepositDeductionInput {
                    settlement_case_id: settlement_id.clone(),
                    deposit_id: "deposit-a".into(),
                    amount_minor: 501,
                    idempotency_key: "deduct-too-much".into()
                },
                "actor-a"
            )
            .is_err()
    );
    assert!(
        provider
            .bind(&normal("tenant-b"))
            .unwrap()
            .r3_settlements()
            .decide_liability(
                LiabilityDecisionInput {
                    finding_id,
                    decision: "liable".into(),
                    apportioned_amount_minor: 500,
                    currency: "CNY".into(),
                    rationale: "foreign".into(),
                    evidence_refs: vec![],
                    idempotency_key: "foreign".into()
                },
                "actor-b"
            )
            .is_err()
    );
    let terminal = scoped
        .r3_settlements()
        .complete_settlement(&settlement_id)
        .unwrap();
    assert_eq!(terminal.status, "settled");
    let deductions: i64 = pool.get().unwrap().query_row("SELECT COUNT(*) FROM integration_deposit_ledger WHERE tenant_id='tenant-a' AND deposit_id='deposit-a' AND entry_type='deducted'", [], |row| row.get(0)).unwrap();
    assert_eq!(deductions, 1);
}

#[test]
fn dispute_blocks_closure_until_manual_resolution() {
    let pool = pool();
    let provider = SqliteRepositoryProvider::new(pool.clone());
    let (settlement_id, _) = create_damage_case(&pool, &provider, "order-dispute", 200);
    let scoped = provider.bind(&normal("tenant-a")).unwrap();
    scoped
        .r3_settlements()
        .accept_settlement(&settlement_id)
        .unwrap();
    let dispute = scoped
        .r3_settlements()
        .open_dispute(
            OpenDisputeInput {
                settlement_case_id: settlement_id.clone(),
                reason: "renter contests inspection".into(),
                evidence_refs: vec!["evidence://dispute/1".into()],
                idempotency_key: "dispute-1".into(),
            },
            "actor-a",
        )
        .unwrap();
    assert!(
        scoped
            .r3_settlements()
            .complete_settlement(&settlement_id)
            .is_err()
    );
    scoped
        .r3_settlements()
        .resolve_dispute(
            ResolveDisputeInput {
                dispute_id: dispute.id,
                resolution_note: "manual review completed".into(),
            },
            "actor-a",
        )
        .unwrap();
    // The unresolved balance is still blocked until a governed effect is admitted.
    assert!(
        scoped
            .r3_settlements()
            .complete_settlement(&settlement_id)
            .is_err()
    );
}

#[test]
fn r2_external_operation_is_the_only_charge_outcome_and_reconciliation_authority() {
    let pool = pool();
    let provider = SqliteRepositoryProvider::new(pool.clone());
    let (settlement_id, _) = create_damage_case(&pool, &provider, "order-unknown", 300);
    let scoped = provider.bind(&normal("tenant-a")).unwrap();
    scoped
        .r3_settlements()
        .accept_settlement(&settlement_id)
        .unwrap();
    seed_fixture_charge_binding(&pool);
    let effect = scoped
        .r3_settlements()
        .admit_additional_charge(
            AdditionalChargeInput {
                settlement_case_id: settlement_id.clone(),
                amount_minor: 300,
                idempotency_key: "charge-1".into(),
            },
            "actor-a",
        )
        .unwrap();
    let operation_id = effect
        .external_operation_id
        .clone()
        .expect("R2 operation is atomically linked");
    complete_r2_operation(&pool, &operation_id, OperationState::UnknownOutcome);
    assert!(
        scoped
            .r3_settlements()
            .complete_settlement(&settlement_id)
            .is_err()
    );
    let store = IntegrationStore::new(pool.clone());
    let operation_id = ExternalOperationId::new(operation_id).unwrap();
    store
        .begin_reconciliation(
            "tenant-a",
            &operation_id,
            "provider-reconciliation://operation-a",
        )
        .unwrap();
    store
        .resolve_reconciliation("tenant-a", &operation_id, true, "integration-admin")
        .unwrap();
    assert_eq!(
        scoped
            .r3_settlements()
            .complete_settlement(&settlement_id)
            .unwrap()
            .status,
        "settled"
    );
}

#[test]
fn additional_charge_emits_one_post_commit_r2_admission_metric_and_idempotent_replay_does_not_double_count()
 {
    let pool = pool();
    let metrics = Arc::new(RuntimeMetrics::default());
    let provider = SqliteRepositoryProvider::new_with_metrics(pool.clone(), metrics.clone());
    let (settlement_id, _) = create_damage_case(&pool, &provider, "order-charge-metrics", 300);
    let scoped = provider.bind(&normal("tenant-a")).unwrap();
    scoped
        .r3_settlements()
        .accept_settlement(&settlement_id)
        .unwrap();
    seed_fixture_charge_binding(&pool);
    let input = AdditionalChargeInput {
        settlement_case_id: settlement_id,
        amount_minor: 300,
        idempotency_key: "charge-metrics".into(),
    };

    let first = scoped
        .r3_settlements()
        .admit_additional_charge(input.clone(), "actor-a")
        .unwrap();
    let replay = scoped
        .r3_settlements()
        .admit_additional_charge(input, "actor-a")
        .unwrap();

    assert_eq!(first.external_operation_id, replay.external_operation_id);
    let output = metrics.render();
    assert!(output.contains("event=\"admitted\",state=\"ready\"} 1"));
    assert!(!output.contains("order-charge-metrics"));
}

#[test]
fn rolled_back_additional_charge_emits_no_r2_admission_metric() {
    let pool = pool();
    let metrics = Arc::new(RuntimeMetrics::default());
    let provider = SqliteRepositoryProvider::new_with_metrics(pool.clone(), metrics.clone());
    let (settlement_id, _) = create_damage_case(&pool, &provider, "order-charge-rollback", 300);
    let scoped = provider.bind(&normal("tenant-a")).unwrap();
    scoped
        .r3_settlements()
        .accept_settlement(&settlement_id)
        .unwrap();
    seed_fixture_charge_binding(&pool);
    pool.get()
        .unwrap()
        .execute_batch(
            "CREATE TRIGGER force_r3_charge_rollback
             BEFORE INSERT ON rental_settlement_effect_admissions
             WHEN NEW.effect_kind = 'additional_charge'
             BEGIN SELECT RAISE(ABORT, 'forced rollback'); END;",
        )
        .unwrap();

    assert!(
        scoped
            .r3_settlements()
            .admit_additional_charge(
                AdditionalChargeInput {
                    settlement_case_id: settlement_id,
                    amount_minor: 300,
                    idempotency_key: "charge-rollback".into(),
                },
                "actor-a",
            )
            .is_err()
    );

    assert!(
        !metrics
            .render()
            .contains("event=\"admitted\",state=\"ready\"")
    );
    let operation_count: i64 = pool
        .get()
        .unwrap()
        .query_row(
            "SELECT COUNT(*) FROM external_operations WHERE tenant_id='tenant-a'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(operation_count, 0);
}

#[test]
fn financial_reservation_is_shared_by_deposits_and_non_rejected_r2_charges() {
    let pool = pool();
    let provider = SqliteRepositoryProvider::new(pool.clone());
    let (settlement_id, _) = create_damage_case(&pool, &provider, "order-reservation", 500);
    let scoped = provider.bind(&normal("tenant-a")).unwrap();
    scoped
        .r3_settlements()
        .accept_settlement(&settlement_id)
        .unwrap();
    seed_fixture_charge_binding(&pool);
    let connection = pool.get().unwrap();
    connection.execute("INSERT INTO integration_deposits (id,tenant_id,authority_kind,authority_id,expected_amount_minor,currency,state,created_at,updated_at) VALUES ('deposit-reservation','tenant-a','order','order-reservation',500,'CNY','recorded','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z')", []).unwrap();
    connection.execute("INSERT INTO integration_deposit_ledger (id,tenant_id,deposit_id,entry_type,amount_minor,currency,audit_ref,created_at) VALUES ('ledger-reservation','tenant-a','deposit-reservation','received',500,'CNY','fixture','2026-01-01T00:00:00Z')", []).unwrap();
    drop(connection);

    let charge = scoped
        .r3_settlements()
        .admit_additional_charge(
            AdditionalChargeInput {
                settlement_case_id: settlement_id.clone(),
                amount_minor: 300,
                idempotency_key: "charge-reservation".into(),
            },
            "actor-a",
        )
        .unwrap();
    assert!(charge.external_operation_id.is_some());
    assert!(
        scoped
            .r3_settlements()
            .deduct_deposit(
                DepositDeductionInput {
                    settlement_case_id: settlement_id.clone(),
                    deposit_id: "deposit-reservation".into(),
                    amount_minor: 201,
                    idempotency_key: "deduct-over".into()
                },
                "actor-a",
            )
            .is_err()
    );

    complete_r2_operation(
        &pool,
        charge.external_operation_id.as_deref().unwrap(),
        OperationState::Rejected,
    );
    scoped
        .r3_settlements()
        .deduct_deposit(
            DepositDeductionInput {
                settlement_case_id: settlement_id.clone(),
                deposit_id: "deposit-reservation".into(),
                amount_minor: 500,
                idempotency_key: "deduct-after-rejection".into(),
            },
            "actor-a",
        )
        .unwrap();
    assert!(
        scoped
            .r3_settlements()
            .admit_additional_charge(
                AdditionalChargeInput {
                    settlement_case_id: settlement_id,
                    amount_minor: 1,
                    idempotency_key: "charge-over-after-deposit".into()
                },
                "actor-a",
            )
            .is_err()
    );
    let availability: String = pool.get().unwrap().query_row(
        "SELECT rentalStatus FROM devices WHERE tenant_id='tenant-a' AND serialNo='serial-order-reservation'", [], |row| row.get(0),
    ).unwrap();
    assert_eq!(availability, "已入库");
}

#[test]
fn financial_reservation_enforces_both_orderings_and_idempotent_charge_retries() {
    let charge_first_pool = pool();
    let provider = SqliteRepositoryProvider::new(charge_first_pool.clone());
    let charge_first = accepted_settlement_with_fundable_deposit(
        &charge_first_pool,
        &provider,
        "order-charge-first-100",
        100,
    );
    let scoped = provider.bind(&normal("tenant-a")).unwrap();
    let first_charge = scoped
        .r3_settlements()
        .admit_additional_charge(
            AdditionalChargeInput {
                settlement_case_id: charge_first.clone(),
                amount_minor: 100,
                idempotency_key: "charge-first-100".into(),
            },
            "actor-a",
        )
        .unwrap();
    assert!(
        scoped
            .r3_settlements()
            .deduct_deposit(
                DepositDeductionInput {
                    settlement_case_id: charge_first.clone(),
                    deposit_id: "deposit-order-charge-first-100".into(),
                    amount_minor: 1,
                    idempotency_key: "charge-first-over".into(),
                },
                "actor-a",
            )
            .is_err()
    );
    assert_eq!(
        first_charge.id,
        scoped
            .r3_settlements()
            .admit_additional_charge(
                AdditionalChargeInput {
                    settlement_case_id: charge_first,
                    amount_minor: 100,
                    idempotency_key: "charge-first-100".into(),
                },
                "actor-a",
            )
            .unwrap()
            .id
    );

    let deposit_first_pool = pool();
    let provider = SqliteRepositoryProvider::new(deposit_first_pool.clone());
    let deposit_first = accepted_settlement_with_fundable_deposit(
        &deposit_first_pool,
        &provider,
        "order-deposit-first-100",
        100,
    );
    let scoped = provider.bind(&normal("tenant-a")).unwrap();
    scoped
        .r3_settlements()
        .deduct_deposit(
            DepositDeductionInput {
                settlement_case_id: deposit_first.clone(),
                deposit_id: "deposit-order-deposit-first-100".into(),
                amount_minor: 100,
                idempotency_key: "deposit-first-100".into(),
            },
            "actor-a",
        )
        .unwrap();
    assert!(
        scoped
            .r3_settlements()
            .admit_additional_charge(
                AdditionalChargeInput {
                    settlement_case_id: deposit_first,
                    amount_minor: 1,
                    idempotency_key: "deposit-first-over".into(),
                },
                "actor-a",
            )
            .is_err()
    );

    let deposit_then_charge_pool = pool();
    let provider = SqliteRepositoryProvider::new(deposit_then_charge_pool.clone());
    let deposit_then_charge = accepted_settlement_with_fundable_deposit(
        &deposit_then_charge_pool,
        &provider,
        "order-deposit-40-charge-60",
        100,
    );
    let scoped = provider.bind(&normal("tenant-a")).unwrap();
    scoped
        .r3_settlements()
        .deduct_deposit(
            DepositDeductionInput {
                settlement_case_id: deposit_then_charge.clone(),
                deposit_id: "deposit-order-deposit-40-charge-60".into(),
                amount_minor: 40,
                idempotency_key: "deposit-40".into(),
            },
            "actor-a",
        )
        .unwrap();
    scoped
        .r3_settlements()
        .admit_additional_charge(
            AdditionalChargeInput {
                settlement_case_id: deposit_then_charge,
                amount_minor: 60,
                idempotency_key: "charge-60".into(),
            },
            "actor-a",
        )
        .unwrap();

    let charge_then_deposit_pool = pool();
    let provider = SqliteRepositoryProvider::new(charge_then_deposit_pool.clone());
    let charge_then_deposit = accepted_settlement_with_fundable_deposit(
        &charge_then_deposit_pool,
        &provider,
        "order-charge-60-deposit-40",
        100,
    );
    let scoped = provider.bind(&normal("tenant-a")).unwrap();
    let charge = scoped
        .r3_settlements()
        .admit_additional_charge(
            AdditionalChargeInput {
                settlement_case_id: charge_then_deposit.clone(),
                amount_minor: 60,
                idempotency_key: "charge-60-retry".into(),
            },
            "actor-a",
        )
        .unwrap();
    scoped
        .r3_settlements()
        .deduct_deposit(
            DepositDeductionInput {
                settlement_case_id: charge_then_deposit.clone(),
                deposit_id: "deposit-order-charge-60-deposit-40".into(),
                amount_minor: 40,
                idempotency_key: "deposit-40-after-charge".into(),
            },
            "actor-a",
        )
        .unwrap();
    assert_eq!(
        charge.id,
        scoped
            .r3_settlements()
            .admit_additional_charge(
                AdditionalChargeInput {
                    settlement_case_id: charge_then_deposit.clone(),
                    amount_minor: 60,
                    idempotency_key: "charge-60-retry".into(),
                },
                "actor-a",
            )
            .unwrap()
            .id
    );
    let admissions: i64 = charge_then_deposit_pool
        .get()
        .unwrap()
        .query_row(
            "SELECT COUNT(*) FROM rental_settlement_effect_admissions WHERE tenant_id='tenant-a' AND settlement_case_id=?1",
            [charge_then_deposit],
            |row| row.get::<_, i64>(0),
        )
        .unwrap();
    assert_eq!(admissions, 2);
}

#[test]
fn multi_admission_financial_aggregation_accepts_exact_splits_and_rejects_under_accounting() {
    let two_deposit_pool = pool();
    let provider = SqliteRepositoryProvider::new(two_deposit_pool.clone());
    let settlement_id = accepted_settlement_with_fundable_deposit(
        &two_deposit_pool,
        &provider,
        "order-two-deposit-admissions",
        100,
    );
    let scoped = provider.bind(&normal("tenant-a")).unwrap();
    for (amount_minor, idempotency_key) in [(40, "deposit-split-40"), (60, "deposit-split-60")] {
        scoped
            .r3_settlements()
            .deduct_deposit(
                DepositDeductionInput {
                    settlement_case_id: settlement_id.clone(),
                    deposit_id: "deposit-order-two-deposit-admissions".into(),
                    amount_minor,
                    idempotency_key: idempotency_key.into(),
                },
                "actor-a",
            )
            .unwrap();
    }
    assert_eq!(
        scoped
            .r3_settlements()
            .complete_settlement(&settlement_id)
            .unwrap()
            .status,
        "settled"
    );

    let split_pool = pool();
    let provider = SqliteRepositoryProvider::new(split_pool.clone());
    let split_id = accepted_settlement_with_fundable_deposit(
        &split_pool,
        &provider,
        "order-deposit-charge-exact",
        100,
    );
    let scoped = provider.bind(&normal("tenant-a")).unwrap();
    scoped
        .r3_settlements()
        .deduct_deposit(
            DepositDeductionInput {
                settlement_case_id: split_id.clone(),
                deposit_id: "deposit-order-deposit-charge-exact".into(),
                amount_minor: 40,
                idempotency_key: "deposit-exact-40".into(),
            },
            "actor-a",
        )
        .unwrap();
    let charge = scoped
        .r3_settlements()
        .admit_additional_charge(
            AdditionalChargeInput {
                settlement_case_id: split_id.clone(),
                amount_minor: 60,
                idempotency_key: "charge-exact-60".into(),
            },
            "actor-a",
        )
        .unwrap();
    complete_r2_operation(
        &split_pool,
        charge.external_operation_id.as_deref().unwrap(),
        OperationState::Succeeded,
    );
    assert_eq!(
        scoped
            .r3_settlements()
            .complete_settlement(&split_id)
            .unwrap()
            .status,
        "settled"
    );

    let under_pool = pool();
    let provider = SqliteRepositoryProvider::new(under_pool.clone());
    let under_id = accepted_settlement_with_fundable_deposit(
        &under_pool,
        &provider,
        "order-under-accounted-admissions",
        100,
    );
    let scoped = provider.bind(&normal("tenant-a")).unwrap();
    scoped
        .r3_settlements()
        .deduct_deposit(
            DepositDeductionInput {
                settlement_case_id: under_id.clone(),
                deposit_id: "deposit-order-under-accounted-admissions".into(),
                amount_minor: 40,
                idempotency_key: "deposit-under-40".into(),
            },
            "actor-a",
        )
        .unwrap();
    let charge = scoped
        .r3_settlements()
        .admit_additional_charge(
            AdditionalChargeInput {
                settlement_case_id: under_id.clone(),
                amount_minor: 50,
                idempotency_key: "charge-under-50".into(),
            },
            "actor-a",
        )
        .unwrap();
    complete_r2_operation(
        &under_pool,
        charge.external_operation_id.as_deref().unwrap(),
        OperationState::Succeeded,
    );
    assert!(
        scoped
            .r3_settlements()
            .complete_settlement(&under_id)
            .is_err()
    );
}

#[test]
fn r2_financial_state_classification_releases_only_known_no_effect_reservations() {
    for (order, state) in [
        ("order-known-rejected", OperationState::Rejected),
        (
            "order-known-non-retryable",
            OperationState::NonRetryableFailure,
        ),
    ] {
        let pool = pool();
        let provider = SqliteRepositoryProvider::new(pool.clone());
        let settlement_id = accepted_settlement_with_fundable_deposit(&pool, &provider, order, 100);
        let scoped = provider.bind(&normal("tenant-a")).unwrap();
        let charge = scoped
            .r3_settlements()
            .admit_additional_charge(
                AdditionalChargeInput {
                    settlement_case_id: settlement_id.clone(),
                    amount_minor: 100,
                    idempotency_key: format!("charge-{order}"),
                },
                "actor-a",
            )
            .unwrap();
        complete_r2_operation(
            &pool,
            charge.external_operation_id.as_deref().unwrap(),
            state,
        );
        scoped
            .r3_settlements()
            .deduct_deposit(
                DepositDeductionInput {
                    settlement_case_id: settlement_id,
                    deposit_id: format!("deposit-{order}"),
                    amount_minor: 100,
                    idempotency_key: format!("deduct-{order}"),
                },
                "actor-a",
            )
            .unwrap();
    }

    let retry_pool = pool();
    let provider = SqliteRepositoryProvider::new(retry_pool.clone());
    let retry_id = accepted_settlement_with_fundable_deposit(
        &retry_pool,
        &provider,
        "order-retryable-held",
        100,
    );
    let scoped = provider.bind(&normal("tenant-a")).unwrap();
    let charge = scoped
        .r3_settlements()
        .admit_additional_charge(
            AdditionalChargeInput {
                settlement_case_id: retry_id.clone(),
                amount_minor: 100,
                idempotency_key: "charge-retryable-held".into(),
            },
            "actor-a",
        )
        .unwrap();
    complete_r2_operation(
        &retry_pool,
        charge.external_operation_id.as_deref().unwrap(),
        OperationState::RetryableFailure,
    );
    assert!(
        scoped
            .r3_settlements()
            .deduct_deposit(
                DepositDeductionInput {
                    settlement_case_id: retry_id,
                    deposit_id: "deposit-order-retryable-held".into(),
                    amount_minor: 1,
                    idempotency_key: "deduct-retryable-held".into(),
                },
                "actor-a"
            )
            .is_err()
    );

    let unknown_pool = pool();
    let provider = SqliteRepositoryProvider::new(unknown_pool.clone());
    let unknown_id = accepted_settlement_with_fundable_deposit(
        &unknown_pool,
        &provider,
        "order-unknown-held",
        100,
    );
    let scoped = provider.bind(&normal("tenant-a")).unwrap();
    let charge = scoped
        .r3_settlements()
        .admit_additional_charge(
            AdditionalChargeInput {
                settlement_case_id: unknown_id.clone(),
                amount_minor: 100,
                idempotency_key: "charge-unknown-held".into(),
            },
            "actor-a",
        )
        .unwrap();
    complete_r2_operation(
        &unknown_pool,
        charge.external_operation_id.as_deref().unwrap(),
        OperationState::UnknownOutcome,
    );
    assert!(
        scoped
            .r3_settlements()
            .deduct_deposit(
                DepositDeductionInput {
                    settlement_case_id: unknown_id,
                    deposit_id: "deposit-order-unknown-held".into(),
                    amount_minor: 1,
                    idempotency_key: "deduct-unknown-held".into(),
                },
                "actor-a"
            )
            .is_err()
    );

    let manual_pool = pool();
    let provider = SqliteRepositoryProvider::new(manual_pool.clone());
    let manual_id = accepted_settlement_with_fundable_deposit(
        &manual_pool,
        &provider,
        "order-manual-held",
        100,
    );
    let scoped = provider.bind(&normal("tenant-a")).unwrap();
    let charge = scoped
        .r3_settlements()
        .admit_additional_charge(
            AdditionalChargeInput {
                settlement_case_id: manual_id.clone(),
                amount_minor: 100,
                idempotency_key: "charge-manual-held".into(),
            },
            "actor-a",
        )
        .unwrap();
    complete_r2_operation(
        &manual_pool,
        charge.external_operation_id.as_deref().unwrap(),
        OperationState::ManualResolutionRequired,
    );
    assert!(
        scoped
            .r3_settlements()
            .deduct_deposit(
                DepositDeductionInput {
                    settlement_case_id: manual_id,
                    deposit_id: "deposit-order-manual-held".into(),
                    amount_minor: 1,
                    idempotency_key: "deduct-manual-held".into(),
                },
                "actor-a"
            )
            .is_err()
    );
    assert_eq!(charge.external_operation_state.as_deref(), Some("ready"));
}

#[test]
fn settlement_charge_admission_requires_the_r2_manifest_secret_references() {
    let pool = pool();
    let provider = SqliteRepositoryProvider::new(pool.clone());
    let settlement_id = accepted_settlement_with_fundable_deposit(
        &pool,
        &provider,
        "order-required-secret-reference",
        100,
    );
    let connection = pool.get().unwrap();
    connection.execute(
        "UPDATE provider_manifests SET secret_schema_json='[{\"name\":\"apiKey\",\"required\":true,\"valueType\":\"opaque_credential\",\"purpose\":\"provider_authentication\"}]' WHERE provider_id='fixture' AND version='1'",
        [],
    ).unwrap();
    drop(connection);
    let scoped = provider.bind(&normal("tenant-a")).unwrap();
    assert!(
        scoped
            .r3_settlements()
            .admit_additional_charge(
                AdditionalChargeInput {
                    settlement_case_id: settlement_id.clone(),
                    amount_minor: 100,
                    idempotency_key: "charge-missing-secret-reference".into(),
                },
                "actor-a"
            )
            .is_err()
    );
    // Test fixture setup persists a reference only; no raw credential is ever present.
    pool.get().unwrap().execute(
        "UPDATE provider_instances SET secret_refs_json='{\"apiKey\":\"keystore://fixture/api-key\"}',config_revision='2' WHERE tenant_id='tenant-a' AND id='instance-a'",
        [],
    ).unwrap();
    pool.get().unwrap().execute(
        "UPDATE provider_bindings SET config_revision='2' WHERE tenant_id='tenant-a' AND id='binding-a'",
        [],
    ).unwrap();
    assert!(
        scoped
            .r3_settlements()
            .admit_additional_charge(
                AdditionalChargeInput {
                    settlement_case_id: settlement_id,
                    amount_minor: 100,
                    idempotency_key: "charge-with-secret-reference".into(),
                },
                "actor-a"
            )
            .is_ok()
    );
}

#[test]
fn repair_inventory_transitions_use_canonical_device_statuses_and_rejoin_checked_in_stock() {
    let pool = pool();
    let provider = SqliteRepositoryProvider::new(pool.clone());
    let scoped = provider.bind(&normal("tenant-a")).unwrap();

    seed_order(&pool, "tenant-a", "order-repaired", "serial-repaired");
    let repaired_finding = create_decided_damage_finding(
        &pool,
        &provider,
        "order-repaired",
        "allocation-order-repaired",
        1,
    );
    let repaired = scoped
        .r3_settlements()
        .decide_repair(
            RepairDecisionInput {
                finding_id: repaired_finding,
                decision: "repair_required".into(),
                idempotency_key: "repair-required".into(),
            },
            "actor-a",
        )
        .unwrap();
    assert_eq!(device_status(&pool, "serial-repaired"), txt::STATUS_REPAIR);
    scoped
        .r3_settlements()
        .transition_repair(RepairTransitionInput {
            repair_case_id: repaired.id.clone(),
            target_status: "under_repair".into(),
            expected_version: 1,
        })
        .unwrap();
    assert_eq!(device_status(&pool, "serial-repaired"), txt::STATUS_REPAIR);
    scoped
        .r3_settlements()
        .transition_repair(RepairTransitionInput {
            repair_case_id: repaired.id,
            target_status: "repaired".into(),
            expected_version: 2,
        })
        .unwrap();
    assert_eq!(
        device_status(&pool, "serial-repaired"),
        txt::STATUS_CHECKED_IN
    );
    let checked_in_count: i64 = pool
        .get()
        .unwrap()
        .query_row(
            "SELECT COUNT(*) FROM devices WHERE tenant_id='tenant-a' AND rentalStatus=?1",
            [txt::STATUS_CHECKED_IN],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(checked_in_count, 1);

    seed_order(&pool, "tenant-a", "order-retired", "serial-retired");
    let retired_finding = create_decided_damage_finding(
        &pool,
        &provider,
        "order-retired",
        "allocation-order-retired",
        1,
    );
    let retired = scoped
        .r3_settlements()
        .decide_repair(
            RepairDecisionInput {
                finding_id: retired_finding,
                decision: "repair_required".into(),
                idempotency_key: "retire-required".into(),
            },
            "actor-a",
        )
        .unwrap();
    scoped
        .r3_settlements()
        .transition_repair(RepairTransitionInput {
            repair_case_id: retired.id,
            target_status: "retired".into(),
            expected_version: 1,
        })
        .unwrap();
    assert_eq!(device_status(&pool, "serial-retired"), txt::STATUS_SCRAPPED);

    seed_order(&pool, "tenant-a", "order-lost", "serial-lost");
    let lost_finding =
        create_decided_damage_finding(&pool, &provider, "order-lost", "allocation-order-lost", 1);
    let lost = scoped
        .r3_settlements()
        .decide_repair(
            RepairDecisionInput {
                finding_id: lost_finding,
                decision: "repair_required".into(),
                idempotency_key: "lost-required".into(),
            },
            "actor-a",
        )
        .unwrap();
    scoped
        .r3_settlements()
        .transition_repair(RepairTransitionInput {
            repair_case_id: lost.id,
            target_status: "lost".into(),
            expected_version: 1,
        })
        .unwrap();
    assert_eq!(device_status(&pool, "serial-lost"), txt::STATUS_LOST);

    seed_order(&pool, "tenant-a", "order-no-action", "serial-no-action");
    let no_action_finding = create_decided_damage_finding(
        &pool,
        &provider,
        "order-no-action",
        "allocation-order-no-action",
        1,
    );
    scoped
        .r3_settlements()
        .decide_repair(
            RepairDecisionInput {
                finding_id: no_action_finding,
                decision: "no_action".into(),
                idempotency_key: "no-action".into(),
            },
            "actor-a",
        )
        .unwrap();
    assert_eq!(
        device_status(&pool, "serial-no-action"),
        txt::STATUS_CHECKED_IN
    );
}

#[test]
fn inspection_missing_false_cannot_pass_and_terminal_retries_require_identical_facts() {
    let pool = pool();
    let provider = SqliteRepositoryProvider::new(pool.clone());
    seed_order(
        &pool,
        "tenant-a",
        "order-inspection",
        "serial-order-inspection",
    );
    let scoped = provider.bind(&normal("tenant-a")).unwrap();
    scoped
        .rental_closure()
        .receive_allocation(
            "order-inspection",
            "allocation-order-inspection",
            "scan-inspection",
        )
        .unwrap();
    let inspection_id: String = pool.get().unwrap().query_row(
        "SELECT id FROM rental_inspections WHERE tenant_id='tenant-a' AND allocation_id='allocation-order-inspection'", [], |row| row.get(0),
    ).unwrap();
    scoped
        .rental_closure()
        .transition_inspection(&inspection_id, "in_progress", 1)
        .unwrap();
    assert!(
        scoped
            .r3_settlements()
            .complete_inspection(
                InspectionCompletionInput {
                    inspection_id: inspection_id.clone(),
                    expected_version: 2,
                    condition_code: "missing".into(),
                    missing: false,
                    normal_wear: false,
                    evidence_refs: vec!["evidence://missing".into()]
                },
                "actor-a",
            )
            .is_err()
    );
    let first = scoped
        .r3_settlements()
        .complete_inspection(
            InspectionCompletionInput {
                inspection_id: inspection_id.clone(),
                expected_version: 2,
                condition_code: "good".into(),
                missing: false,
                normal_wear: false,
                evidence_refs: vec!["evidence://good".into()],
            },
            "actor-a",
        )
        .unwrap();
    let repeated = scoped
        .r3_settlements()
        .complete_inspection(
            InspectionCompletionInput {
                inspection_id: inspection_id.clone(),
                expected_version: 2,
                condition_code: "good".into(),
                missing: false,
                normal_wear: false,
                evidence_refs: vec!["evidence://good".into()],
            },
            "actor-a",
        )
        .unwrap();
    assert_eq!(first.status, repeated.status);
    assert!(
        scoped
            .r3_settlements()
            .complete_inspection(
                InspectionCompletionInput {
                    inspection_id,
                    expected_version: 2,
                    condition_code: "good".into(),
                    missing: false,
                    normal_wear: false,
                    evidence_refs: vec!["evidence://different".into()]
                },
                "actor-a",
            )
            .is_err()
    );
}

#[test]
fn preview_and_simulation_cannot_mutate_r3_production_authority() {
    let pool = pool();
    let provider = SqliteRepositoryProvider::new(pool);
    let preview = context(
        "tenant-a",
        ExecutionMode::ReadOnlyPreview(PreviewSessionId::new("preview-r3").unwrap()),
        Namespace::Production,
    );
    assert!(
        provider
            .bind(&preview)
            .unwrap()
            .r3_settlements()
            .configure_business_timezone("America/New_York", "platform")
            .is_err()
    );
    let simulation = SimulationId::new("simulation-r3").unwrap();
    let simulation = context(
        "tenant-a",
        ExecutionMode::Simulation(simulation.clone()),
        Namespace::Simulation(simulation),
    );
    assert!(provider.bind(&simulation).is_err());
}

#[test]
fn registry_blocks_r3_preview_writes_but_allows_production_scoped_read() {
    let pool = pool();
    let provider: Arc<dyn RepositoryProvider> = Arc::new(SqliteRepositoryProvider::new(pool));
    let module: Arc<dyn SystemModule> = Arc::new(R3SettlementModule::new(provider));
    let registry = ModuleRegistry::new(HashMap::from([("r3_settlement".into(), module)])).unwrap();
    let preview = context(
        "tenant-a",
        ExecutionMode::ReadOnlyPreview(PreviewSessionId::new("preview-registry-r3").unwrap()),
        Namespace::Production,
    );

    assert!(
        registry
            .execute(
                "r3_settlement",
                "get_business_timezone",
                serde_json::json!({}),
                &preview,
            )
            .is_ok()
    );
    let blocked = registry
        .execute(
            "r3_settlement",
            "configure_business_timezone",
            serde_json::json!({ "timeZoneId": "America/New_York" }),
            &preview,
        )
        .unwrap_err();
    assert!(blocked.contains("EXEC_PREVIEW_WRITE_BLOCKED"));
}

#[test]
fn registry_denies_every_r3_financial_or_terminal_command_to_non_admin_staff() {
    let provider: Arc<dyn RepositoryProvider> = Arc::new(SqliteRepositoryProvider::new(pool()));
    let module: Arc<dyn SystemModule> = Arc::new(R3SettlementModule::new(provider));
    let registry = ModuleRegistry::new(HashMap::from([("r3_settlement".into(), module)])).unwrap();
    for command in [
        "decide_liability",
        "accept_settlement",
        "deduct_deposit",
        "admit_additional_charge",
        "resolve_dispute",
        "complete_settlement",
        "close_order",
    ] {
        let error = registry
            .execute(
                "r3_settlement",
                command,
                serde_json::json!({}),
                &staff("tenant-a"),
            )
            .unwrap_err();
        assert!(error.contains("EXEC_ACCESS_DENIED"), "{command}: {error}");
    }
}

#[test]
fn terminal_settlement_sqlite_seal_rejects_old_authority_retargeting() {
    let pool = pool();
    let provider = SqliteRepositoryProvider::new(pool.clone());
    let (settled_case_id, settled_finding_id) =
        create_damage_case(&pool, &provider, "order-retarget-settled", 500);
    let (unsettled_case_id, _) =
        create_damage_case(&pool, &provider, "order-retarget-unsettled", 100);
    seed_order(
        &pool,
        "tenant-a",
        "order-retarget-open",
        "serial-order-retarget-open",
    );
    let open_finding_id = create_open_damage_finding(
        &pool,
        &provider,
        "order-retarget-open",
        "allocation-order-retarget-open",
    );
    let scoped = provider.bind(&normal("tenant-a")).unwrap();

    let dispute = scoped
        .r3_settlements()
        .open_dispute(
            OpenDisputeInput {
                settlement_case_id: settled_case_id.clone(),
                reason: "record an already-resolved dispute before terminal settlement".into(),
                evidence_refs: vec!["evidence://dispute/retarget".into()],
                idempotency_key: "retarget-dispute".into(),
            },
            "actor-a",
        )
        .unwrap();
    scoped
        .r3_settlements()
        .resolve_dispute(
            ResolveDisputeInput {
                dispute_id: dispute.id.clone(),
                resolution_note: "resolved before terminal settlement".into(),
            },
            "actor-a",
        )
        .unwrap();
    scoped
        .r3_settlements()
        .accept_settlement(&settled_case_id)
        .unwrap();
    seed_deposit(
        &pool,
        "deposit-retarget-settled",
        "order-retarget-settled",
        500,
    );
    scoped
        .r3_settlements()
        .deduct_deposit(
            DepositDeductionInput {
                settlement_case_id: settled_case_id.clone(),
                deposit_id: "deposit-retarget-settled".into(),
                amount_minor: 500,
                idempotency_key: "deduct-retarget-settled".into(),
            },
            "actor-a",
        )
        .unwrap();
    {
        let connection = pool.get().unwrap();
        connection
            .execute(
                "INSERT INTO overdue_records (tenant_id,order_id,status) VALUES ('tenant-a','order-retarget-settled','cleared')",
                [],
            )
            .unwrap();
    }
    assert_eq!(
        scoped
            .r3_settlements()
            .complete_settlement(&settled_case_id)
            .unwrap()
            .status,
        "settled"
    );
    // Terminal completion remains idempotent; the OLD-authority guard is a
    // database backstop for direct/internal SQL, not a second application flow.
    assert_eq!(
        scoped
            .r3_settlements()
            .complete_settlement(&settled_case_id)
            .unwrap()
            .status,
        "settled"
    );

    let connection = pool.get().unwrap();
    let liability_id: String = connection
        .query_row(
            "SELECT id FROM rental_liability_decisions WHERE tenant_id='tenant-a' AND finding_id=?1",
            [&settled_finding_id],
            |row| row.get(0),
        )
        .unwrap();
    let repair_id: String = connection
        .query_row(
            "SELECT id FROM rental_repair_cases WHERE tenant_id='tenant-a' AND finding_id=?1",
            [&settled_finding_id],
            |row| row.get(0),
        )
        .unwrap();
    let line_id: String = connection
        .query_row(
            "SELECT id FROM rental_settlement_lines WHERE tenant_id='tenant-a' AND settlement_case_id=?1",
            [&settled_case_id],
            |row| row.get(0),
        )
        .unwrap();
    let overdue_id: i64 = connection
        .query_row(
            "SELECT id FROM overdue_records WHERE tenant_id='tenant-a' AND order_id='order-retarget-settled'",
            [],
            |row| row.get(0),
        )
        .unwrap();

    // Every destination is unsealed.  These failures therefore prove the
    // trigger protects OLD authority, not merely the replacement reference.
    assert!(connection
        .execute(
            "UPDATE rental_damage_findings SET order_id='order-retarget-open' WHERE tenant_id='tenant-a' AND id=?1",
            [&settled_finding_id],
        )
        .is_err());
    assert!(connection
        .execute(
            "UPDATE rental_liability_decisions SET finding_id=?1 WHERE tenant_id='tenant-a' AND id=?2",
            rusqlite::params![open_finding_id, liability_id],
        )
        .is_err());
    assert!(connection
        .execute(
            "UPDATE rental_repair_cases SET finding_id=?1,order_id='order-retarget-open' WHERE tenant_id='tenant-a' AND id=?2",
            rusqlite::params![open_finding_id, repair_id],
        )
        .is_err());
    assert!(connection
        .execute(
            "UPDATE rental_disputes SET settlement_case_id=?1 WHERE tenant_id='tenant-a' AND id=?2",
            rusqlite::params![unsettled_case_id, dispute.id],
        )
        .is_err());
    assert!(connection
        .execute(
            "UPDATE rental_settlement_lines SET settlement_case_id=?1 WHERE tenant_id='tenant-a' AND id=?2",
            rusqlite::params![unsettled_case_id, line_id],
        )
        .is_err());
    assert!(connection
        .execute(
            "UPDATE overdue_records SET order_id='order-retarget-open',status='active' WHERE id=?1",
            [overdue_id],
        )
        .is_err());

    // Unsealed authority remains ordinarily mutable; an active overdue record
    // may still be deactivated while no terminal authority is involved.
    assert_eq!(
        connection
            .execute(
                "UPDATE rental_damage_findings SET description='unsealed correction' WHERE tenant_id='tenant-a' AND id=?1",
                [&open_finding_id],
            )
            .unwrap(),
        1
    );
    connection
        .execute(
            "INSERT INTO overdue_records (tenant_id,order_id,status) VALUES ('tenant-a','order-retarget-open','active')",
            [],
        )
        .unwrap();
    assert_eq!(
        connection
            .execute(
                "UPDATE overdue_records SET status='cleared' WHERE tenant_id='tenant-a' AND order_id='order-retarget-open'",
                [],
            )
            .unwrap(),
        1
    );
}

#[test]
fn r3_terminal_settlement_closes_only_through_the_named_lifecycle_command() {
    let pool = pool();
    let provider = SqliteRepositoryProvider::new(pool.clone());
    let (settlement_id, finding_id) = create_damage_case(&pool, &provider, "order-close", 500);
    let scoped = provider.bind(&normal("tenant-a")).unwrap();
    scoped
        .r3_settlements()
        .accept_settlement(&settlement_id)
        .unwrap();
    seed_deposit(&pool, "deposit-close", "order-close", 500);
    scoped
        .r3_settlements()
        .deduct_deposit(
            DepositDeductionInput {
                settlement_case_id: settlement_id.clone(),
                deposit_id: "deposit-close".into(),
                amount_minor: 500,
                idempotency_key: "deduct-close".into(),
            },
            "actor-a",
        )
        .unwrap();
    scoped
        .r3_settlements()
        .complete_settlement(&settlement_id)
        .unwrap();

    // Application and direct SQL paths both fail closed after the terminal
    // settlement seal; a future explicit amend/reopen authority is required.
    assert!(
        scoped
            .r3_settlements()
            .create_damage_finding(
                DamageFindingInput {
                    inspection_id: inspection_id(&pool, "allocation-order-close"),
                    finding_code: "late-case-seal-crack".into(),
                    condition_code: "damaged".into(),
                    description: "late review found an additional cracked seal".into(),
                    evidence_refs: vec!["evidence://damage/late-seal".into()],
                    idempotency_key: "late-finding-order-close".into(),
                },
                "actor-a"
            )
            .is_err()
    );
    assert!(
        scoped
            .r3_settlements()
            .deduct_deposit(
                DepositDeductionInput {
                    settlement_case_id: settlement_id.clone(),
                    deposit_id: "deposit-close".into(),
                    amount_minor: 1,
                    idempotency_key: "late-deduction-order-close".into(),
                },
                "actor-a"
            )
            .is_err()
    );
    {
        let connection = pool.get().unwrap();
        assert!(connection.execute(
            "UPDATE rental_damage_findings SET description='late mutation' WHERE tenant_id='tenant-a' AND id=?1",
            [&finding_id],
        ).is_err());
        assert!(connection.execute(
            "UPDATE rental_liability_decisions SET rationale='late mutation' WHERE tenant_id='tenant-a' AND finding_id=?1",
            [&finding_id],
        ).is_err());
        assert!(connection.execute(
            "UPDATE rental_repair_cases SET status='under_repair' WHERE tenant_id='tenant-a' AND finding_id=?1",
            [&finding_id],
        ).is_err());
        assert!(connection.execute(
            "INSERT INTO rental_disputes (id,tenant_id,settlement_case_id,status,reason,evidence_refs_json,command_idempotency_key,opened_by,opened_at,created_at,updated_at) VALUES ('late-dispute','tenant-a',?1,'open','late','[]','late-dispute','actor-a','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z')",
            [&settlement_id],
        ).is_err());
        assert!(connection.execute(
            "INSERT INTO overdue_records (tenant_id,order_id,status) VALUES ('tenant-a','order-close','active')", [],
        ).is_err());
        connection.execute(
            "INSERT INTO overdue_records (tenant_id,order_id,status) VALUES ('tenant-a','order-close','cleared')", [],
        ).unwrap();
        assert!(connection.execute(
            "UPDATE overdue_records SET status='active' WHERE tenant_id='tenant-a' AND order_id='order-close'", [],
        ).is_err());
    }
    {
        let connection = pool.get().unwrap();
        connection.execute("UPDATE order_lifecycle SET commercial_status='completed',contract_status='not_required',financial_status='settled',fulfilment_status='inspected',risk_status='clear',version=9 WHERE tenant_id='tenant-a' AND order_id='order-close'", []).unwrap();
    }

    let module: Arc<dyn SystemModule> = Arc::new(R3SettlementModule::new(Arc::new(
        SqliteRepositoryProvider::new(pool.clone()),
    )));
    let registry = ModuleRegistry::new(HashMap::from([("r3_settlement".into(), module)])).unwrap();
    registry
        .execute(
            "r3_settlement",
            "close_order",
            serde_json::json!({ "orderId": "order-close", "expectedVersion": 9 }),
            &normal("tenant-a"),
        )
        .unwrap();
    let connection = pool.get().unwrap();
    let lifecycle: (String, i64) = connection
        .query_row("SELECT commercial_status,version FROM order_lifecycle WHERE tenant_id='tenant-a' AND order_id='order-close'", [], |row| Ok((row.get(0)?,row.get(1)?)))
        .unwrap();
    assert_eq!(lifecycle, ("closed".into(), 10));
    let guard: String = connection
        .query_row("SELECT guard_name FROM order_lifecycle_history WHERE tenant_id='tenant-a' AND order_id='order-close' ORDER BY resulting_version DESC LIMIT 1", [], |row| row.get(0))
        .unwrap();
    assert_eq!(guard, "can_close_order");
}
