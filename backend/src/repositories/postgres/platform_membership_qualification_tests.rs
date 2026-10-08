use sqlx::postgres::{PgConnection, PgPool, PgPoolOptions};
use sqlx::{Connection, Executor};

use crate::repositories::{
    CreatePlatformMembership, NewPlatformIdentity, PlatformMembershipAuditActor,
    PlatformMembershipMutationError, PlatformMembershipRepository, RevokePlatformMembership,
    UpdatePlatformMembership,
};

struct LivePlatformMembershipFixture {
    database_url: String,
    schema: String,
    pool: PgPool,
}

impl LivePlatformMembershipFixture {
    async fn create() -> anyhow::Result<Self> {
        let database_url = std::env::var("TALOS_TEST_POSTGRES_URL")?;
        let schema = format!(
            "r4_p8_platform_membership_{}",
            uuid::Uuid::new_v4().simple()
        );
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
            "INSERT INTO identities
             (id,username,email,password_hash,display_name,phone,status,created_at,updated_at)
             VALUES
             ('identity-owner','owner','owner@example.test','hash-owner','Owner',NULL,'active','2026-09-20T00:00:00Z','2026-09-20T00:00:00Z'),
             ('identity-auditor','auditor',NULL,'hash-auditor','Auditor',NULL,'active','2026-09-19T00:00:00Z','2026-09-19T00:00:00Z')",
        )
        .execute(&pool)
        .await?;
        sqlx::query(
            "INSERT INTO platform_memberships
             (id,identity_id,status,created_at,updated_at)
             VALUES
             ('pm-owner','identity-owner','active','2026-09-20T02:00:00Z','2026-09-20T02:00:00Z'),
             ('pm-auditor','identity-auditor','suspended','2026-09-19T02:00:00Z','2026-09-19T02:00:00Z')",
        )
        .execute(&pool)
        .await?;
        sqlx::query(
            "INSERT INTO platform_role_grants
             (id,platform_membership_id,role,granted_by_identity_id,granted_at)
             VALUES
             ('grant-owner','pm-owner','platform_owner','identity-owner','2026-09-20T02:00:00Z'),
             ('grant-admin','pm-owner','platform_admin','identity-owner','2026-09-20T02:00:01Z'),
             ('grant-auditor','pm-auditor','security_auditor','identity-owner','2026-09-19T02:00:00Z')",
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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires TALOS_TEST_POSTGRES_URL for a disposable PostgreSQL 18 database"]
async fn live_pg18_platform_membership_read_preserves_projection_roles_and_recomposition()
-> anyhow::Result<()> {
    let fixture = LivePlatformMembershipFixture::create().await?;
    let repository = PlatformMembershipRepository::postgres(fixture.pool.clone());

    let memberships = repository.list()?;
    assert_eq!(memberships.len(), 2);
    assert_eq!(memberships[0].id, "pm-owner");
    assert_eq!(memberships[0].identity_id, "identity-owner");
    assert_eq!(memberships[0].username, "owner");
    assert_eq!(memberships[0].display_name, "Owner");
    assert_eq!(memberships[0].email, "owner@example.test");
    assert_eq!(memberships[0].status, "active");
    assert_eq!(
        memberships[0].roles,
        vec!["platform_admin".to_string(), "platform_owner".to_string()]
    );
    assert_eq!(memberships[1].id, "pm-auditor");
    assert_eq!(memberships[1].email, "");
    assert_eq!(memberships[1].status, "suspended");
    assert_eq!(memberships[1].roles, vec!["security_auditor".to_string()]);

    let recomposed = PlatformMembershipRepository::postgres(fixture.pool.clone());
    let after_recomposition = recomposed.list()?;
    assert_eq!(after_recomposition, memberships);

    fixture.cleanup().await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires TALOS_TEST_POSTGRES_URL for a disposable PostgreSQL 18 database"]
async fn live_pg18_platform_membership_mutations_preserve_owner_and_audit_atomicity()
-> anyhow::Result<()> {
    let fixture = LivePlatformMembershipFixture::create().await?;
    let repository = PlatformMembershipRepository::postgres(fixture.pool.clone());
    let owner_actor = PlatformMembershipAuditActor {
        identity_id: "identity-owner".into(),
        membership_id: "pm-owner".into(),
        roles_snapshot: r#"["platform_owner","platform_admin"]"#.into(),
        capabilities_snapshot: "[]".into(),
    };

    let created = repository.create(CreatePlatformMembership {
        existing_identity_id: None,
        new_identity: Some(NewPlatformIdentity {
            username: "operator-new".into(),
            email: Some("operator@example.test".into()),
            password_hash: "hash-operator".into(),
            display_name: "Operator".into(),
            phone: None,
        }),
        roles: vec!["platform_operator".into()],
        actor: owner_actor.clone(),
        actor_is_platform_owner: true,
        now: "2026-09-20T03:00:00Z".into(),
    })?;
    let created_audits: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM audit_events
         WHERE action = 'platform_membership.created' AND resource_id = $1",
    )
    .bind(&created.membership_id)
    .fetch_one(&fixture.pool)
    .await?;
    assert_eq!(created_audits, 1);

    let non_owner_actor = PlatformMembershipAuditActor {
        identity_id: "identity-auditor".into(),
        membership_id: "pm-auditor".into(),
        roles_snapshot: r#"["security_auditor"]"#.into(),
        capabilities_snapshot: "[]".into(),
    };
    let unauthorized_owner_create = repository.create(CreatePlatformMembership {
        existing_identity_id: None,
        new_identity: Some(NewPlatformIdentity {
            username: "forbidden-owner".into(),
            email: None,
            password_hash: "hash-forbidden".into(),
            display_name: "Forbidden Owner".into(),
            phone: None,
        }),
        roles: vec!["platform_owner".into()],
        actor: non_owner_actor,
        actor_is_platform_owner: true,
        now: "2026-09-20T03:05:00Z".into(),
    });
    assert!(matches!(
        unauthorized_owner_create,
        Err(PlatformMembershipMutationError::OwnerAuthorityRequired)
    ));
    let forbidden_identity_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM identities WHERE username = 'forbidden-owner'")
            .fetch_one(&fixture.pool)
            .await?;
    assert_eq!(forbidden_identity_count, 0);

    let remove_last_owner = repository.update(UpdatePlatformMembership {
        membership_id: "pm-owner".into(),
        status: None,
        roles: Some(vec!["platform_admin".into()]),
        actor: owner_actor.clone(),
        actor_is_platform_owner: true,
        now: "2026-09-20T03:10:00Z".into(),
    });
    assert!(matches!(
        remove_last_owner,
        Err(PlatformMembershipMutationError::LastActiveOwner)
    ));
    let owner_roles = sqlx::query_scalar::<_, String>(
        "SELECT role FROM platform_role_grants
         WHERE platform_membership_id = 'pm-owner' ORDER BY role",
    )
    .fetch_all(&fixture.pool)
    .await?;
    assert_eq!(
        owner_roles,
        vec!["platform_admin".to_string(), "platform_owner".to_string()]
    );
    let failed_owner_audits: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM audit_events
         WHERE action = 'platform_membership.updated' AND resource_id = 'pm-owner'",
    )
    .fetch_one(&fixture.pool)
    .await?;
    assert_eq!(failed_owner_audits, 0);

    let invalid_audit_actor = PlatformMembershipAuditActor {
        identity_id: "identity-owner".into(),
        membership_id: "missing-platform-membership".into(),
        roles_snapshot: r#"["platform_owner"]"#.into(),
        capabilities_snapshot: "[]".into(),
    };
    let audit_failure = repository.update(UpdatePlatformMembership {
        membership_id: "pm-auditor".into(),
        status: Some("active".into()),
        roles: None,
        actor: invalid_audit_actor,
        actor_is_platform_owner: true,
        now: "2026-09-20T03:15:00Z".into(),
    });
    assert!(matches!(
        audit_failure,
        Err(PlatformMembershipMutationError::Storage(_))
    ));
    let auditor_status_after_failed_audit: String =
        sqlx::query_scalar("SELECT status FROM platform_memberships WHERE id = 'pm-auditor'")
            .fetch_one(&fixture.pool)
            .await?;
    assert_eq!(auditor_status_after_failed_audit, "suspended");

    repository.update(UpdatePlatformMembership {
        membership_id: "pm-auditor".into(),
        status: Some("active".into()),
        roles: Some(vec!["business_operator".into(), "security_auditor".into()]),
        actor: owner_actor.clone(),
        actor_is_platform_owner: true,
        now: "2026-09-20T03:20:00Z".into(),
    })?;
    let auditor_roles = sqlx::query_scalar::<_, String>(
        "SELECT role FROM platform_role_grants
         WHERE platform_membership_id = 'pm-auditor' ORDER BY role",
    )
    .fetch_all(&fixture.pool)
    .await?;
    assert_eq!(
        auditor_roles,
        vec![
            "business_operator".to_string(),
            "security_auditor".to_string()
        ]
    );
    let update_audits: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM audit_events
         WHERE action = 'platform_membership.updated' AND resource_id = 'pm-auditor'",
    )
    .fetch_one(&fixture.pool)
    .await?;
    assert_eq!(update_audits, 1);

    repository.revoke(RevokePlatformMembership {
        membership_id: "pm-auditor".into(),
        actor: owner_actor,
        actor_is_platform_owner: true,
        now: "2026-09-20T03:25:00Z".into(),
    })?;
    let revoked_status: String =
        sqlx::query_scalar("SELECT status FROM platform_memberships WHERE id = 'pm-auditor'")
            .fetch_one(&fixture.pool)
            .await?;
    assert_eq!(revoked_status, "revoked");
    let revoke_audits: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM audit_events
         WHERE action = 'platform_membership.revoked' AND resource_id = 'pm-auditor'",
    )
    .fetch_one(&fixture.pool)
    .await?;
    assert_eq!(revoke_audits, 1);

    fixture.cleanup().await
}
