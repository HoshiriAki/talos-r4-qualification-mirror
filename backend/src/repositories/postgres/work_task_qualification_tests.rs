use std::sync::Arc;

use sqlx::postgres::{PgConnection, PgPool, PgPoolOptions};
use sqlx::{Connection, Executor};
use system_core::{
    ActorIdentity, DataScope, ExecutionContext, ExecutionMode, NoopHttpClient, RequestId, Revision,
    TenantId, TenantScope,
};

use crate::repositories::{PostgresRepositoryProvider, RepositoryProvider, WorkTaskListRequest};

struct LiveWorkTaskFixture {
    database_url: String,
    schema: String,
    pool: PgPool,
}

impl LiveWorkTaskFixture {
    async fn create() -> anyhow::Result<Self> {
        let database_url = std::env::var("TALOS_TEST_POSTGRES_URL")?;
        let schema = format!("r4_p8_work_task_{}", uuid::Uuid::new_v4().simple());
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
             ('tenant-task-a','Task A','tenant-task-a','active','test','2026-09-24T00:00:00Z','2026-09-24T00:00:00Z'),
             ('tenant-task-b','Task B','tenant-task-b','active','test','2026-09-24T00:00:00Z','2026-09-24T00:00:00Z')",
        )
        .execute(&pool)
        .await?;

        sqlx::query(
            "INSERT INTO work_tasks
             (id,kind,status,risk,source_type,source_id,title,summary,reason,due_at,
              capabilities_json,work_route_json,assignee_id,version,created_at,updated_at,tenant_id)
             VALUES
             ('task-a','general','queued','low','system','','A','','',NULL,
              '[\"acknowledge\",\"send_to_pc\"]','{\"name\":\"dashboard\"}',NULL,1,
              '2026-09-24T01:00:00Z','2026-09-24T01:00:00Z','tenant-task-a'),
             ('task-a-2','general','in_progress','high','system','','A2','','',NULL,
              '[\"defer\",\"open_work\",\"send_to_pc\"]','{\"name\":\"dashboard\"}',NULL,1,
              '2026-09-24T02:00:00Z','2026-09-24T02:00:00Z','tenant-task-a'),
             ('task-b','general','queued','low','system','','B','','',NULL,
              '[\"acknowledge\"]','{\"name\":\"dashboard\"}',NULL,1,
              '2026-09-24T03:00:00Z','2026-09-24T03:00:00Z','tenant-task-b')",
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
        ActorIdentity::authenticated("task-actor", "staff").unwrap(),
        TenantScope::tenant(tenant_id.clone()),
        DataScope::production(tenant_id, Revision::new("task-revision").unwrap()).unwrap(),
        ExecutionMode::Normal,
        RequestId::new(request).unwrap(),
        None,
        Arc::new(NoopHttpClient),
    )
    .unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires TALOS_TEST_POSTGRES_URL for a disposable PostgreSQL 18 database"]
async fn live_pg18_work_task_cutover_preserves_tenant_scope_version_and_recomposition()
-> anyhow::Result<()> {
    let fixture = LiveWorkTaskFixture::create().await?;
    let provider = PostgresRepositoryProvider::new(fixture.pool.clone());

    let tenant_a = provider.bind(&normal_context("tenant-task-a", "task-request-a"))?;
    let tenant_b = provider.bind(&normal_context("tenant-task-b", "task-request-b"))?;
    let request = WorkTaskListRequest {
        statuses: vec!["queued".into(), "in_progress".into()],
        limit: 50,
    };

    let tasks_a = tenant_a.work_tasks().list(&request)?;
    assert_eq!(
        tasks_a
            .iter()
            .map(|task| task.id.as_str())
            .collect::<Vec<_>>(),
        vec!["task-a-2", "task-a"]
    );
    let tasks_b = tenant_b.work_tasks().list(&request)?;
    assert_eq!(tasks_b.len(), 1);
    assert_eq!(tasks_b[0].id, "task-b");
    assert!(tenant_a.work_tasks().get_state("task-b")?.is_none());
    assert!(tenant_b.work_tasks().get_state("task-a")?.is_none());

    let updated = tenant_a
        .work_tasks()
        .update_status("task-a", 1, "completed", "2026-09-24T12:00:00+08:00")?
        .expect("tenant-a optimistic update");
    assert_eq!(updated.version, 2);
    assert!(
        tenant_a
            .work_tasks()
            .update_status("task-a", 1, "queued", "2026-09-24T12:01:00+08:00",)?
            .is_none()
    );
    assert!(
        tenant_b
            .work_tasks()
            .update_status("task-a", 2, "queued", "2026-09-24T12:02:00+08:00",)?
            .is_none()
    );

    let sent = tenant_a
        .work_tasks()
        .send_to_pc("task-a-2", 1, "2026-09-24T12:03:00+08:00")?
        .expect("tenant-a send-to-pc update");
    assert_eq!(sent.version, 2);

    let recomposed = PostgresRepositoryProvider::new(fixture.pool.clone());
    let tenant_a_after = recomposed.bind(&normal_context(
        "tenant-task-a",
        "task-request-a-recomposed",
    ))?;
    let state_a = tenant_a_after
        .work_tasks()
        .get_state("task-a")?
        .expect("recomposed completed task");
    assert_eq!(state_a.status, "completed");
    assert_eq!(state_a.version, 2);
    let state_a2 = tenant_a_after
        .work_tasks()
        .get_state("task-a-2")?
        .expect("recomposed sent task");
    assert_eq!(state_a2.status, "in_progress");
    assert_eq!(state_a2.version, 2);

    let assignee: Option<String> =
        sqlx::query_scalar("SELECT assignee_id FROM work_tasks WHERE tenant_id=$1 AND id=$2")
            .bind("tenant-task-a")
            .bind("task-a-2")
            .fetch_one(&fixture.pool)
            .await?;
    assert_eq!(assignee.as_deref(), Some("pc_queue"));

    fixture.cleanup().await
}
