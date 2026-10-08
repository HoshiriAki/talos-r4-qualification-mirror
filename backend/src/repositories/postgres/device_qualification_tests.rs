use std::sync::Arc;

use sqlx::postgres::{PgConnection, PgPool, PgPoolOptions};
use sqlx::{Connection, Executor};
use system_core::{
    ActorIdentity, DataScope, ExecutionContext, ExecutionMode, NoopHttpClient, RequestId, Revision,
    SystemModule, TenantId, TenantScope,
};

use crate::application::ExcelImportCompatibilityModule;
use crate::repositories::{
    DeviceCompatibilityReadRequest, DeviceListRequest, DevicePagedRequest, DevicePatch,
    DeviceSortDirection, DeviceSortField, ImportedDeviceDraft, NewDevice,
    PostgresRepositoryProvider, RepositoryProvider,
};

struct LiveDeviceAuthorityFixture {
    database_url: String,
    schema: String,
    pool: PgPool,
}

impl LiveDeviceAuthorityFixture {
    async fn create() -> anyhow::Result<Self> {
        let database_url = std::env::var("TALOS_TEST_POSTGRES_URL")?;
        let schema = format!("r4_p8_device_authority_{}", uuid::Uuid::new_v4().simple());
        let mut admin = PgConnection::connect(&database_url).await?;
        admin
            .execute(format!("CREATE SCHEMA {schema}").as_str())
            .await?;
        drop(admin);

        let schema_for_pool = schema.clone();
        let pool = PgPoolOptions::new()
            .max_connections(4)
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

        sqlx::query(
            "INSERT INTO tenants (id,name,slug,status,plan,created_at,updated_at)
             VALUES
             ('tenant-device-core-a','Device Core A','tenant-device-core-a','active','test','2026-09-25T00:00:00Z','2026-09-25T00:00:00Z'),
             ('tenant-device-core-b','Device Core B','tenant-device-core-b','active','test','2026-09-25T00:00:00Z','2026-09-25T00:00:00Z')",
        )
        .execute(&pool)
        .await?;

        sqlx::query(
            "INSERT INTO device_models
             (id,name,category,prefix,enabled,createdAt,updatedAt,tenant_id)
             VALUES
             ('model-core-a','Model Core A','camera','A',true,'2026-09-25T00:00:00Z','2026-09-25T00:00:00Z','tenant-device-core-a'),
             ('model-core-b','Model Core B','camera','B',true,'2026-09-25T00:00:00Z','2026-09-25T00:00:00Z','tenant-device-core-b')",
        )
        .execute(&pool)
        .await?;

        sqlx::query(
            "INSERT INTO warehouses
             (id,name,type,enabled,createdAt,updatedAt,tenant_id)
             VALUES
             ('warehouse-core-a','Warehouse Core A','owned',true,'2026-09-25T00:00:00Z','2026-09-25T00:00:00Z','tenant-device-core-a'),
             ('warehouse-core-b','Warehouse Core B','owned',true,'2026-09-25T00:00:00Z','2026-09-25T00:00:00Z','tenant-device-core-b')",
        )
        .execute(&pool)
        .await?;

        sqlx::query(
            "INSERT INTO devices
             (id,serialNo,rentalStatus,notes,modelId,currentWarehouseId,createdAt,tenant_id)
             VALUES
             ('device-core-a1','COREA001','available','alpha note','model-core-a','warehouse-core-a','2026-09-25T00:00:00Z','tenant-device-core-a'),
             ('device-core-a2','COREA002','rented','beta note','model-core-a','warehouse-core-a','2026-09-25T00:01:00Z','tenant-device-core-a'),
             ('device-core-b1','COREB001','available','foreign note','model-core-b','warehouse-core-b','2026-09-25T00:00:00Z','tenant-device-core-b')",
        )
        .execute(&pool)
        .await?;

        sqlx::query(
            "INSERT INTO orders
             (id,orderNo,startDate,endDate,deliveryDate,pickupMethods,address,notes,status,createdAt,tenant_id)
             VALUES
             ('legacy-order-a','LEGACY-A','2026-09-20','2026-09-25','2026-09-20','[]','','','active','2026-09-20T00:00:00Z','tenant-device-core-a'),
             ('legacy-order-b','LEGACY-B','2026-09-20','2026-09-25','2026-09-20','[]','','','active','2026-09-20T00:00:00Z','tenant-device-core-b')",
        )
        .execute(&pool)
        .await?;

        sqlx::query(
            "INSERT INTO order_devices (id,orderId,serialNo,createdAt,tenant_id)
             VALUES
             ('legacy-link-a','legacy-order-a','COREA001','2026-09-20T00:00:00Z','tenant-device-core-a'),
             ('legacy-link-b','legacy-order-b','COREB001','2026-09-20T00:00:00Z','tenant-device-core-b')",
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

fn context(tenant: &str, request: &str) -> ExecutionContext {
    let tenant_id = TenantId::new(tenant).unwrap();
    ExecutionContext::new(
        ActorIdentity::authenticated("device-authority-actor", "staff").unwrap(),
        TenantScope::tenant(tenant_id.clone()),
        DataScope::production(
            tenant_id,
            Revision::new("device-authority-revision").unwrap(),
        )
        .unwrap(),
        ExecutionMode::Normal,
        RequestId::new(request).unwrap(),
        None,
        Arc::new(NoopHttpClient),
    )
    .unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires TALOS_TEST_POSTGRES_URL for a disposable PostgreSQL 18 database"]
async fn live_pg18_device_authority_preserves_scope_crud_paging_and_recomposition()
-> anyhow::Result<()> {
    let fixture = LiveDeviceAuthorityFixture::create().await?;
    let provider = PostgresRepositoryProvider::new(fixture.pool.clone());
    let ctx_a = context("tenant-device-core-a", "device-core-request-a");
    let ctx_b = context("tenant-device-core-b", "device-core-request-b");

    {
        let scoped_a = provider.bind(&ctx_a)?;
        let scoped_b = provider.bind(&ctx_b)?;

        let list_a = scoped_a.devices().list(&DeviceListRequest::default())?;
        assert_eq!(list_a.len(), 2);
        assert!(
            list_a
                .iter()
                .all(|device| device.serial_no.starts_with("COREA"))
        );
        assert_eq!(
            scoped_b
                .devices()
                .list(&DeviceListRequest::default())?
                .into_iter()
                .map(|device| device.serial_no)
                .collect::<Vec<_>>(),
            vec!["COREB001".to_string()]
        );
        assert!(scoped_b.devices().get("COREA001")?.is_none());

        let page = scoped_a.devices().list_paged(&DevicePagedRequest {
            page: 1,
            page_size: 1,
            keyword: Some("COREA".into()),
            rental_status: None,
            notes: None,
            warning_status: None,
            sort_by: DeviceSortField::SerialNo,
            sort_direction: DeviceSortDirection::Asc,
        })?;
        assert_eq!(page.devices.len(), 1);
        assert_eq!(page.devices[0].serial_no, "COREA001");
        assert_eq!(page.pagination.total, 2);
        assert_eq!(page.pagination.total_pages, 2);

        let compatibility_request = DeviceCompatibilityReadRequest {
            keyword: Some("COREA".into()),
            rental_status: None,
            notes: None,
        };
        assert_eq!(
            scoped_a
                .devices()
                .compatibility_count(&compatibility_request)?,
            2
        );
        let compatibility_rows =
            scoped_a
                .devices()
                .compatibility_rows(&compatibility_request, Some(10), Some(0))?;
        assert_eq!(compatibility_rows.len(), 2);
        assert!(
            compatibility_rows
                .iter()
                .all(|device| device.serial_no.starts_with("COREA"))
        );
        assert_eq!(
            scoped_b
                .devices()
                .compatibility_count(&compatibility_request)?,
            0
        );
        let by_serials = scoped_a
            .devices()
            .compatibility_rows_by_serials(&["COREA001".into(), "COREB001".into()])?;
        assert_eq!(
            by_serials
                .into_iter()
                .map(|device| device.serial_no)
                .collect::<Vec<_>>(),
            vec!["COREA001".to_string()]
        );

        assert!(
            scoped_b
                .devices()
                .checkin_status("COREA001", "已入库")?
                .is_none()
        );
        let checkin = scoped_a
            .devices()
            .checkin_status("COREA001", "已入库")?
            .expect("tenant A can check in its device");
        assert_eq!(checkin.before_status, "available");
        assert_eq!(checkin.after_status, "已入库");
        assert!(!checkin.already_checked_in);
        assert!(
            scoped_b
                .devices()
                .complete_legacy_orders_after_checkin("COREA001", "已入库")?
                .is_empty()
        );
        assert_eq!(
            scoped_a
                .devices()
                .complete_legacy_orders_after_checkin("COREA001", "已入库")?,
            vec!["legacy-order-a".to_string()]
        );
        let legacy_status_a: String =
            sqlx::query_scalar("SELECT status FROM orders WHERE tenant_id=$1 AND id=$2")
                .bind("tenant-device-core-a")
                .bind("legacy-order-a")
                .fetch_one(&fixture.pool)
                .await?;
        let legacy_status_b: String =
            sqlx::query_scalar("SELECT status FROM orders WHERE tenant_id=$1 AND id=$2")
                .bind("tenant-device-core-b")
                .bind("legacy-order-b")
                .fetch_one(&fixture.pool)
                .await?;
        assert_eq!(legacy_status_a, "completed");
        assert_eq!(legacy_status_b, "active");
        let repeated_checkin = scoped_a
            .devices()
            .checkin_status("COREA001", "已入库")?
            .expect("repeat check-in remains visible to tenant A");
        assert!(repeated_checkin.already_checked_in);
        assert!(scoped_a.devices().restore_status("COREA001", "available")?);
        assert!(!scoped_b.devices().restore_status("COREA001", "rented")?);

        assert!(scoped_a.devices().update_status_and_notes(
            "COREA002",
            "available",
            "bulk-updated note"
        )?);
        let bulk_updated = scoped_a
            .devices()
            .get("COREA002")?
            .expect("bulk-updated device remains visible");
        assert_eq!(bulk_updated.status, "available");
        assert_eq!(bulk_updated.notes.as_deref(), Some("bulk-updated note"));
        assert!(!scoped_b.devices().update_status_and_notes(
            "COREA002",
            "rented",
            "foreign mutation"
        )?);

        assert!(scoped_a.devices().import_device(&ImportedDeviceDraft {
            id: "device-import-a".into(),
            serial_no: "SHAREDIMPORT001".into(),
            status: "已入库".into(),
            notes: "imported by tenant A".into(),
            created_at: "2026-09-25T12:30:00+08:00".into(),
        })?);
        assert!(!scoped_a.devices().import_device(&ImportedDeviceDraft {
            id: "device-import-a-duplicate".into(),
            serial_no: "SHAREDIMPORT001".into(),
            status: "已入库".into(),
            notes: "duplicate".into(),
            created_at: "2026-09-25T12:31:00+08:00".into(),
        })?);
        assert!(scoped_b.devices().import_device(&ImportedDeviceDraft {
            id: "device-import-b".into(),
            serial_no: "SHAREDIMPORT001".into(),
            status: "已入库".into(),
            notes: "imported by tenant B".into(),
            created_at: "2026-09-25T12:32:00+08:00".into(),
        })?);
        assert_eq!(
            scoped_a
                .devices()
                .get("SHAREDIMPORT001")?
                .expect("tenant A imported device is visible")
                .notes
                .as_deref(),
            Some("imported by tenant A")
        );
        assert_eq!(
            scoped_b
                .devices()
                .get("SHAREDIMPORT001")?
                .expect("tenant B same-serial import is independently visible")
                .notes
                .as_deref(),
            Some("imported by tenant B")
        );

        let created = scoped_a.devices().create(&NewDevice {
            id: "device-core-a3".into(),
            serial_no: "COREA003".into(),
            model_id: "model-core-a".into(),
            warehouse_id: "warehouse-core-a".into(),
            status: "available".into(),
            created_at: "2026-09-25 12:00:00".into(),
        })?;
        assert_eq!(created.serial_no, "COREA003");
        assert_eq!(created.status, "available");
        assert_eq!(created.model_name.as_deref(), Some("Model Core A"));
        assert_eq!(created.warehouse_name.as_deref(), Some("Warehouse Core A"));

        assert!(
            scoped_b
                .devices()
                .update(
                    "COREA003",
                    &DevicePatch {
                        status: Some("rented".into()),
                        ..DevicePatch::default()
                    },
                )?
                .is_none()
        );
        assert!(!scoped_b.devices().delete("COREA003")?);

        let updated = scoped_a
            .devices()
            .update(
                "COREA003",
                &DevicePatch {
                    status: Some("rented".into()),
                    warehouse_id: Some("warehouse-core-a".into()),
                    ..DevicePatch::default()
                },
            )?
            .expect("tenant A can update its device");
        assert_eq!(updated.status, "rented");
    }

    let recomposed = PostgresRepositoryProvider::new(fixture.pool.clone());
    let scoped_a_after = recomposed.bind(&context(
        "tenant-device-core-a",
        "device-core-request-recomposed",
    ))?;
    assert_eq!(
        scoped_a_after
            .devices()
            .get("COREA003")?
            .expect("created device survives provider recomposition")
            .status,
        "rented"
    );
    assert!(scoped_a_after.devices().delete("COREA003")?);
    assert!(scoped_a_after.devices().get("COREA003")?.is_none());

    fixture.cleanup().await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires TALOS_TEST_POSTGRES_URL for a disposable PostgreSQL 18 database"]
async fn live_pg18_excel_import_registry_preserves_tenant_lookup_and_device_write_authority()
-> anyhow::Result<()> {
    let fixture = LiveDeviceAuthorityFixture::create().await?;
    let provider: Arc<dyn RepositoryProvider> =
        Arc::new(PostgresRepositoryProvider::new(fixture.pool.clone()));
    let module = ExcelImportCompatibilityModule::new(provider.clone());
    let ctx_a = context("tenant-device-core-a", "excel-import-registry-a");
    let ctx_b = context("tenant-device-core-b", "excel-import-registry-b");

    let imported_a = module
        .execute(
            "import_devices",
            serde_json::json!({
                "rows": [{
                    "serialNo": "REGIMPORT001",
                    "modelName": "Model Core A",
                    "warehouseName": "Warehouse Core A",
                    "status": "available",
                    "notes": "legacy Registry notes are intentionally not forwarded"
                }]
            }),
            &ctx_a,
        )
        .map_err(anyhow::Error::msg)?;
    assert_eq!(imported_a["created"], 1);
    assert_eq!(imported_a["skipped"], 0);

    let imported_b = module
        .execute(
            "import_devices",
            serde_json::json!({
                "rows": [{
                    "serialNo": "REGIMPORT001",
                    "modelName": "Model Core B",
                    "warehouseName": "Warehouse Core B",
                    "status": "available"
                }]
            }),
            &ctx_b,
        )
        .map_err(anyhow::Error::msg)?;
    assert_eq!(imported_b["created"], 1);
    assert_eq!(imported_b["skipped"], 0);

    let scoped_a = provider.bind(&ctx_a)?;
    let scoped_b = provider.bind(&ctx_b)?;
    let device_a = scoped_a
        .devices()
        .get("REGIMPORT001")?
        .expect("tenant A Registry import is visible");
    let device_b = scoped_b
        .devices()
        .get("REGIMPORT001")?
        .expect("tenant B same-serial Registry import is independently visible");
    assert_eq!(device_a.model_id, "model-core-a");
    assert_eq!(device_a.warehouse_id.as_deref(), Some("warehouse-core-a"));
    assert_eq!(device_b.model_id, "model-core-b");
    assert_eq!(device_b.warehouse_id.as_deref(), Some("warehouse-core-b"));
    assert_eq!(device_a.notes.as_deref(), Some(""));

    let isolated_lookup = module
        .execute(
            "import_devices",
            serde_json::json!({
                "rows": [{
                    "serialNo": "REGISOLATE001",
                    "modelName": "Model Core B",
                    "warehouseName": "Warehouse Core B",
                    "status": "available"
                }]
            }),
            &ctx_a,
        )
        .map_err(anyhow::Error::msg)?;
    assert_eq!(isolated_lookup["created"], 0);
    assert_eq!(isolated_lookup["skipped"], 1);
    assert!(scoped_a.devices().get("REGISOLATE001")?.is_none());

    let duplicate = module
        .execute(
            "import_devices",
            serde_json::json!({
                "rows": [{
                    "serialNo": "REGIMPORT001",
                    "modelName": "Model Core A",
                    "warehouseName": "Warehouse Core A",
                    "status": "available"
                }]
            }),
            &ctx_a,
        )
        .map_err(anyhow::Error::msg)?;
    assert_eq!(duplicate["created"], 0);
    assert_eq!(duplicate["skipped"], 1);
    assert_eq!(duplicate["errors"][0]["reason"], "device already exists");

    fixture.cleanup().await
}
