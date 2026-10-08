use std::sync::Arc;

use sqlx::postgres::{PgConnection, PgPool, PgPoolOptions};
use sqlx::{Connection, Executor};
use system_core::{
    ActorIdentity, AuthorityContext, DataScope, ExecutionContext, ExecutionMode, NoopHttpClient,
    PlatformMembershipId, PlatformRole, PreviewSessionId, RequestId, Revision, TenantId,
    TenantScope,
};

use crate::domain::ReservationId;
use crate::repositories::{
    AdditionalChargeInput, DamageFindingInput, DepositDeductionInput, DeviceAllocationRequest,
    InspectionCompletionInput, LiabilityDecisionInput, PostgresRepositoryProvider,
    RepairDecisionInput, RepositoryProvider, SettlementLineInput, SettlementProposalInput,
};

struct LiveR3SettlementFixture {
    database_url: String,
    schema: String,
    pool: PgPool,
}

impl LiveR3SettlementFixture {
    async fn create() -> anyhow::Result<Self> {
        let database_url = std::env::var("TALOS_TEST_POSTGRES_URL")?;
        let schema = format!("r4_p8_r3_settlement_{}", uuid::Uuid::new_v4().simple());
        let mut admin = PgConnection::connect(&database_url).await?;
        admin
            .execute(format!("CREATE SCHEMA {schema}").as_str())
            .await?;
        drop(admin);

        let schema_for_pool = schema.clone();
        let pool = PgPoolOptions::new()
            .max_connections(6)
            .after_connect(move |connection, _| {
                let schema = schema_for_pool.clone();
                Box::pin(async move {
                    connection
                        .execute(format!("SET search_path TO {schema}").as_str())
                        .await?;
                    Ok(())
                })
            })
            .connect(&database_url)
            .await?;
        crate::db::run_all_pg_migrations(&pool).await?;

        for (id, name, slug) in [
            ("tenant-a", "Tenant A", "tenant-a"),
            ("tenant-b", "Tenant B", "tenant-b"),
        ] {
            sqlx::query(
                "INSERT INTO tenants (id,name,slug,status,plan,created_at,updated_at)                  VALUES ($1,$2,$3,'active','test','2026-09-19T00:00:00Z','2026-09-19T00:00:00Z')",
            )
            .bind(id)
            .bind(name)
            .bind(slug)
            .execute(&pool)
            .await?;
        }

        sqlx::query(
            "INSERT INTO device_models              (id,name,category,prefix,enabled,createdat,updatedat,tenant_id)              VALUES ('model-a','Model A','fixture','A',TRUE,                      '2026-09-19T00:00:00Z','2026-09-19T00:00:00Z','tenant-a')",
        )
        .execute(&pool)
        .await?;

        sqlx::query(
            "INSERT INTO devices              (id,serialno,rentalstatus,createdat,modelid,tenant_id)              VALUES ('device-a1','SER-A1','available','2026-09-19T00:00:00Z','model-a','tenant-a')",
        )
        .execute(&pool)
        .await?;

        sqlx::query(
            "INSERT INTO orders              (id,orderno,startdate,enddate,deliverydate,pickupmethods,address,notes,deviceserialno,               createdat,tenant_id,totalprice,province,sendwarehouseid,returnwarehouseid,accessories,               status,trackingno,devicemodels)              VALUES ('r3-order-a','R3-A-001','2026-10-01','2026-10-05','2026-10-01',                      '[]','','','', '2026-09-19T00:00:00Z','tenant-a',0,'','','','[]'::jsonb,                      'draft','','{}'::jsonb)",
        )
        .execute(&pool)
        .await?;

        sqlx::query(
            "INSERT INTO order_lines              (id,tenant_id,order_id,source_quote_line_id,line_kind,reference_id,description,               quantity,unit_price_minor,subtotal_minor,currency,price_snapshot_json,created_at)              VALUES ('r3-line-a','tenant-a','r3-order-a','r3-source-a','model',                      'model-a','R3 demand',1,0,0,'CNY','{}'::jsonb,'2026-09-19T00:00:00Z')",
        )
        .execute(&pool)
        .await?;

        sqlx::query(
            r#"INSERT INTO provider_manifests
             (provider_id,version,capabilities_json,config_schema_json,secret_schema_json,
              api_versions_json,webhook_types_json,simulation_capabilities_json,readiness,
              compatibility_json,created_at)
             VALUES ('fixture','1','["payment.charge"]','[]',
                     '[{"name":"apiKey","required":true,"valueType":"opaque_credential",
                        "purpose":"provider_authentication"}]',
                     '{}','[]','[]','fixture','{}','2026-09-19T00:00:00Z')"#,
        )
        .execute(&pool)
        .await?;

        sqlx::query(
            "INSERT INTO provider_instances              (id,tenant_id,provider_id,manifest_version,config_revision,config_json,secret_refs_json,               lifecycle,readiness,health,created_at,updated_at)              VALUES ('instance-a','tenant-a','fixture','1','1','{}','{}',                      'active','fixture','ready','2026-09-19T00:00:00Z','2026-09-19T00:00:00Z')",
        )
        .execute(&pool)
        .await?;

        sqlx::query(
            "INSERT INTO provider_bindings              (id,tenant_id,provider_instance_id,capability_id,config_revision,enabled,created_at,updated_at)              VALUES ('binding-a','tenant-a','instance-a','payment.charge','1',TRUE,                      '2026-09-19T00:00:00Z','2026-09-19T00:00:00Z')",
        )
        .execute(&pool)
        .await?;

        Ok(Self {
            database_url,
            schema,
            pool,
        })
    }

    async fn cleanup(self) -> anyhow::Result<()> {
        self.pool.close().await;
        let mut admin = PgConnection::connect(&self.database_url).await?;
        admin
            .execute(format!("DROP SCHEMA {} CASCADE", self.schema).as_str())
            .await?;
        Ok(())
    }
}

fn normal_context(tenant: &str, request: &str) -> ExecutionContext {
    let tenant_id = TenantId::new(tenant).unwrap();
    ExecutionContext::new(
        ActorIdentity::authenticated("tenant-actor", "staff").unwrap(),
        TenantScope::tenant(tenant_id.clone()),
        DataScope::production(tenant_id, Revision::new("r3-settlement-pg18").unwrap()).unwrap(),
        ExecutionMode::Normal,
        RequestId::new(request).unwrap(),
        None,
        Arc::new(NoopHttpClient),
    )
    .unwrap()
}

fn preview_context() -> ExecutionContext {
    let tenant = TenantId::new("tenant-a").unwrap();
    ExecutionContext::new(
        ActorIdentity::with_authority(
            "platform-r3-preview",
            AuthorityContext::Platform {
                membership_id: PlatformMembershipId::new("platform-r3-preview-member").unwrap(),
                roles: vec![PlatformRole::Owner],
            },
        )
        .unwrap(),
        TenantScope::tenant(tenant.clone()),
        DataScope::production(tenant, Revision::new("r3-preview").unwrap()).unwrap(),
        ExecutionMode::ReadOnlyPreview(PreviewSessionId::new("r3-preview").unwrap()),
        RequestId::new("r3-preview-request").unwrap(),
        None,
        Arc::new(NoopHttpClient),
    )
    .unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires TALOS_TEST_POSTGRES_URL for a disposable PostgreSQL 18 database"]
async fn live_pg18_r3_settlement_preserves_r2_authority_terminal_seal_and_atomic_close()
-> anyhow::Result<()> {
    let fixture = LiveR3SettlementFixture::create().await?;
    let provider = PostgresRepositoryProvider::new(fixture.pool.clone());
    let tenant_a = provider.bind(&normal_context("tenant-a", "r3-a"))?;
    let tenant_b = provider.bind(&normal_context("tenant-b", "r3-b"))?;

    tenant_a
        .r3_settlements()
        .configure_business_timezone("Asia/Shanghai", "platform-admin")?;
    assert_eq!(
        tenant_a.r3_settlements().business_timezone()?,
        "Asia/Shanghai"
    );

    let reservation_id = ReservationId::new();
    tenant_a
        .reservations()
        .create_from_order(reservation_id.clone(), "r3-order-a", 30)?;
    tenant_a.reservations().confirm(&reservation_id, None)?;
    let allocations =
        tenant_a
            .reservations()
            .allocate_devices_batch(&[DeviceAllocationRequest {
                order_id: "r3-order-a".into(),
                device_serial_no: "SER-A1".into(),
            }])?;
    assert_eq!(allocations.len(), 1);

    tenant_a.rental_closure().receive_allocation(
        "r3-order-a",
        allocations[0].id.as_str(),
        "r3-return-scan",
    )?;

    let inspection_id: String = sqlx::query_scalar(
        "SELECT id FROM rental_inspections          WHERE tenant_id='tenant-a' AND allocation_id=$1",
    )
    .bind(allocations[0].id.as_str())
    .fetch_one(&fixture.pool)
    .await?;

    tenant_a
        .rental_closure()
        .transition_inspection(&inspection_id, "in_progress", 1)?;

    let invalid_missing = tenant_a.r3_settlements().complete_inspection(
        InspectionCompletionInput {
            inspection_id: inspection_id.clone(),
            expected_version: 2,
            condition_code: "missing".into(),
            missing: false,
            normal_wear: false,
            evidence_refs: vec!["evidence://r3/missing-invalid".into()],
        },
        "inspector-a",
    );
    assert!(invalid_missing.is_err());

    let completed = tenant_a.r3_settlements().complete_inspection(
        InspectionCompletionInput {
            inspection_id: inspection_id.clone(),
            expected_version: 2,
            condition_code: "damaged".into(),
            missing: false,
            normal_wear: false,
            evidence_refs: vec!["evidence://r3/damage".into()],
        },
        "inspector-a",
    )?;
    assert_eq!(completed.status, "failed");

    let repeated = tenant_a.r3_settlements().complete_inspection(
        InspectionCompletionInput {
            inspection_id: inspection_id.clone(),
            expected_version: 2,
            condition_code: "damaged".into(),
            missing: false,
            normal_wear: false,
            evidence_refs: vec!["evidence://r3/damage".into()],
        },
        "inspector-a",
    )?;
    assert_eq!(repeated.status, "failed");

    let finding = tenant_a.r3_settlements().create_damage_finding(
        DamageFindingInput {
            inspection_id: inspection_id.clone(),
            finding_code: "CASE_CRACK".into(),
            condition_code: "damaged".into(),
            description: "fixture crack".into(),
            evidence_refs: vec!["evidence://r3/damage".into()],
            idempotency_key: "finding-r3-a".into(),
        },
        "inspector-a",
    )?;

    assert!(
        tenant_b
            .r3_settlements()
            .create_damage_finding(
                DamageFindingInput {
                    inspection_id: inspection_id.clone(),
                    finding_code: "FOREIGN".into(),
                    condition_code: "damaged".into(),
                    description: "foreign tenant".into(),
                    evidence_refs: vec![],
                    idempotency_key: "foreign-r3".into(),
                },
                "actor-b",
            )
            .is_err()
    );

    let liability = tenant_a.r3_settlements().decide_liability(
        LiabilityDecisionInput {
            finding_id: finding.id.clone(),
            decision: "liable".into(),
            apportioned_amount_minor: 500,
            currency: "CNY".into(),
            rationale: "fixture liability".into(),
            evidence_refs: vec!["evidence://r3/liability".into()],
            idempotency_key: "liability-r3-a".into(),
        },
        "risk-admin",
    )?;
    assert_eq!(liability.status, "decided");

    let repair = tenant_a.r3_settlements().decide_repair(
        RepairDecisionInput {
            finding_id: finding.id.clone(),
            decision: "no_action".into(),
            idempotency_key: "repair-r3-a".into(),
        },
        "repair-admin",
    )?;
    assert_eq!(repair.status, "no_action");

    let settlement = tenant_a.r3_settlements().propose_settlement(
        SettlementProposalInput {
            order_id: "r3-order-a".into(),
            currency: "CNY".into(),
            idempotency_key: "settlement-r3-a".into(),
            lines: vec![SettlementLineInput {
                finding_id: finding.id.clone(),
                line_kind: "damage".into(),
                amount_minor: 500,
                description: "fixture damage".into(),
            }],
        },
        "settlement-admin",
    )?;
    assert_eq!(settlement.total_amount_minor, 500);

    let settlement = tenant_a
        .r3_settlements()
        .accept_settlement(&settlement.id)?;
    assert_eq!(settlement.status, "accepted");

    sqlx::query(
        "INSERT INTO integration_deposits          (id,tenant_id,authority_kind,authority_id,expected_amount_minor,currency,state,created_at,updated_at)          VALUES ('deposit-r3-a','tenant-a','order','r3-order-a',200,'CNY','recorded',                  '2026-09-19T00:00:00Z','2026-09-19T00:00:00Z')",
    )
    .execute(&fixture.pool)
    .await?;
    sqlx::query(
        "INSERT INTO integration_deposit_ledger          (id,tenant_id,deposit_id,entry_type,amount_minor,currency,audit_ref,created_at)          VALUES ('deposit-ledger-r3-a','tenant-a','deposit-r3-a','received',200,'CNY',                  'fixture','2026-09-19T00:00:00Z')",
    )
    .execute(&fixture.pool)
    .await?;

    let deposit_effect = tenant_a.r3_settlements().deduct_deposit(
        DepositDeductionInput {
            settlement_case_id: settlement.id.clone(),
            deposit_id: "deposit-r3-a".into(),
            amount_minor: 200,
            idempotency_key: "deduct-r3-a".into(),
        },
        "settlement-admin",
    )?;
    let deposit_repeat = tenant_a.r3_settlements().deduct_deposit(
        DepositDeductionInput {
            settlement_case_id: settlement.id.clone(),
            deposit_id: "deposit-r3-a".into(),
            amount_minor: 200,
            idempotency_key: "deduct-r3-a".into(),
        },
        "settlement-admin",
    )?;
    assert_eq!(deposit_effect.id, deposit_repeat.id);
    assert_eq!(deposit_effect.external_operation_state, None);

    assert!(
        tenant_a
            .r3_settlements()
            .admit_additional_charge(
                AdditionalChargeInput {
                    settlement_case_id: settlement.id.clone(),
                    amount_minor: 300,
                    idempotency_key: "charge-r3-a".into(),
                },
                "settlement-admin",
            )
            .is_err()
    );

    sqlx::query(
        r#"UPDATE provider_instances
         SET config_revision='2',
             secret_refs_json='{"apiKey":"keystore://fixture/api-key"}',
             updated_at='2026-09-19T00:01:00Z'
         WHERE tenant_id='tenant-a' AND id='instance-a'"#,
    )
    .execute(&fixture.pool)
    .await?;
    sqlx::query(
        "UPDATE provider_bindings          SET config_revision='2',updated_at='2026-09-19T00:01:00Z'          WHERE tenant_id='tenant-a' AND id='binding-a'",
    )
    .execute(&fixture.pool)
    .await?;

    let charge = tenant_a.r3_settlements().admit_additional_charge(
        AdditionalChargeInput {
            settlement_case_id: settlement.id.clone(),
            amount_minor: 300,
            idempotency_key: "charge-r3-a".into(),
        },
        "settlement-admin",
    )?;
    assert_eq!(charge.effect_kind, "additional_charge");
    assert_eq!(charge.external_operation_state.as_deref(), Some("ready"));
    let operation_id = charge
        .external_operation_id
        .clone()
        .expect("R2 operation must be linked atomically");

    assert!(
        tenant_a
            .r3_settlements()
            .complete_settlement(&settlement.id)
            .is_err()
    );

    sqlx::query(
        "UPDATE external_operations          SET state='succeeded',updated_at='2026-09-19T00:02:00Z'          WHERE tenant_id='tenant-a' AND id=$1",
    )
    .bind(&operation_id)
    .execute(&fixture.pool)
    .await?;

    let settled = tenant_a
        .r3_settlements()
        .complete_settlement(&settlement.id)?;
    assert_eq!(settled.status, "settled");
    assert!(
        tenant_a
            .r3_settlements()
            .closure_facts("r3-order-a")?
            .settlement_terminal
    );

    let deduction_count = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*)::bigint FROM integration_deposit_ledger          WHERE tenant_id='tenant-a' AND deposit_id='deposit-r3-a' AND entry_type='deducted'",
    )
    .fetch_one(&fixture.pool)
    .await?;
    assert_eq!(deduction_count, 1);

    let operation_count = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*)::bigint FROM external_operations          WHERE tenant_id='tenant-a' AND id=$1 AND capability_id='payment.charge'",
    )
    .bind(&operation_id)
    .fetch_one(&fixture.pool)
    .await?;
    assert_eq!(operation_count, 1);

    let sealed_update = sqlx::query(
        "UPDATE rental_damage_findings SET description='late mutation'          WHERE tenant_id='tenant-a' AND id=$1",
    )
    .bind(&finding.id)
    .execute(&fixture.pool)
    .await;
    assert!(sealed_update.is_err());

    sqlx::query(
        "UPDATE order_lifecycle          SET commercial_status='completed',contract_status='not_required',financial_status='settled',              fulfilment_status='inspected',risk_status='clear',version=9,              updated_at='2026-09-19T00:03:00Z'          WHERE tenant_id='tenant-a' AND order_id='r3-order-a'",
    )
    .execute(&fixture.pool)
    .await?;

    tenant_a
        .r3_settlements()
        .close_order_atomic("r3-order-a", 9, "settlement-admin")?;

    let lifecycle: (String, i64) = sqlx::query_as(
        "SELECT commercial_status,version FROM order_lifecycle          WHERE tenant_id='tenant-a' AND order_id='r3-order-a'",
    )
    .fetch_one(&fixture.pool)
    .await?;
    assert_eq!(lifecycle, ("closed".into(), 10));

    let guard: String = sqlx::query_scalar(
        "SELECT guard_name FROM order_lifecycle_history          WHERE tenant_id='tenant-a' AND order_id='r3-order-a'          ORDER BY resulting_version DESC LIMIT 1",
    )
    .fetch_one(&fixture.pool)
    .await?;
    assert_eq!(guard, "can_close_order");

    let preview = provider.bind(&preview_context())?;
    assert_eq!(
        preview
            .r3_settlements()
            .configure_business_timezone("America/New_York", "preview")
            .unwrap_err()
            .code(),
        "REPOSITORY_PREVIEW_WRITE_DENIED"
    );

    fixture.cleanup().await?;
    Ok(())
}
