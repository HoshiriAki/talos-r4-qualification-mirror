use std::{collections::HashMap, sync::Arc};

use sqlx::postgres::{PgConnection, PgPoolOptions};
use sqlx::{Connection, Executor};
use system_core::{
    ActorIdentity, DataScope, ExecutionContext, ExecutionMode, NoopHttpClient, RequestId, Revision,
    TenantId, TenantScope,
};

use crate::repositories::{PostgresRepositoryProvider, RepositoryProvider};

fn context(tenant: &str, request: &str) -> ExecutionContext {
    let tenant_id = TenantId::new(tenant).unwrap();
    ExecutionContext::new(
        ActorIdentity::authenticated("pricing-actor", "staff").unwrap(),
        TenantScope::tenant(tenant_id.clone()),
        DataScope::production(tenant_id, Revision::new("pricing-revision").unwrap()).unwrap(),
        ExecutionMode::Normal,
        RequestId::new(request).unwrap(),
        None,
        Arc::new(NoopHttpClient),
    )
    .unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires TALOS_TEST_POSTGRES_URL"]
async fn live_pg18_pricing_authority_preserves_scope_atomic_config_and_recomposition()
-> anyhow::Result<()> {
    let database_url = std::env::var("TALOS_TEST_POSTGRES_URL")?;
    let schema = format!("r4_p8_pricing_{}", uuid::Uuid::new_v4().simple());
    let mut admin = PgConnection::connect(&database_url).await?;
    admin
        .execute(format!("CREATE SCHEMA {schema}").as_str())
        .await?;
    drop(admin);

    let search_path = schema.clone();
    let pool = PgPoolOptions::new()
        .max_connections(4)
        .after_connect(move |connection, _| {
            let schema = search_path.clone();
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
         VALUES ('pricing-a','A','pricing-a','active','test','now','now'),
                ('pricing-b','B','pricing-b','active','test','now','now')",
    )
    .execute(&pool)
    .await?;
    sqlx::query(
        "INSERT INTO device_models
         (id,name,category,prefix,enabled,createdat,updatedat,tenant_id)
         VALUES ('model-a','A','camera','A',true,'now','now','pricing-a'),
                ('model-b','B','camera','B',true,'now','now','pricing-b')",
    )
    .execute(&pool)
    .await?;
    sqlx::query(
        "INSERT INTO model_base_prices
         (modelid,weekdayprice,weekendprice,updatedby,createdat,updatedat,tenant_id)
         VALUES ('model-a',31,41,'','now','now','pricing-a'),
                ('model-b',32,42,'','now','now','pricing-b')",
    )
    .execute(&pool)
    .await?;

    let provider = PostgresRepositoryProvider::new(pool.clone());
    let ctx_a = context("pricing-a", "pricing-a-request");
    let ctx_b = context("pricing-b", "pricing-b-request");
    let now = "2026-10-01T13:10:00+08:00";
    {
        let a = provider.bind(&ctx_a)?;
        let b = provider.bind(&ctx_b)?;

        a.pricing()
            .update_config(11.0, 21.0, "[]", "a", "{}", now)?;
        b.pricing()
            .update_config(12.0, 22.0, "[]", "b", "{}", now)?;
        a.pricing()
            .upsert_dynamic_price("2026-10-05", 111.0, "a", now)?;
        b.pricing()
            .upsert_dynamic_price("2026-10-05", 222.0, "b", now)?;

        assert_eq!(a.pricing().list_dynamic_prices()?[0].price, 111.0);
        assert_eq!(b.pricing().list_dynamic_prices()?[0].price, 222.0);
        assert!(a.pricing().get_model_base_price("model-b")?.is_none());

        let replacement = HashMap::from([
            ("2026-10-06".to_owned(), 131.0),
            ("2026-10-07".to_owned(), 141.0),
        ]);
        a.pricing()
            .save_config(13.0, 23.0, "[]", "{}", &replacement, "a2", now)?;
        assert_eq!(a.pricing().list_dynamic_prices()?.len(), 2);
        assert_eq!(b.pricing().list_dynamic_prices()?.len(), 1);
    }

    let recomposed = PostgresRepositoryProvider::new(pool.clone());
    let a = recomposed.bind(&context("pricing-a", "pricing-recomposed"))?;
    let persisted = a.pricing().get_or_create_config(8.5, 14.0, "{}", now)?;
    assert_eq!(persisted.base_weekday_price, 13.0);
    assert_eq!(a.pricing().list_dynamic_prices()?.len(), 2);

    pool.close().await;
    let mut admin = PgConnection::connect(&database_url).await?;
    admin
        .execute(format!("DROP SCHEMA {schema} CASCADE").as_str())
        .await?;
    Ok(())
}
