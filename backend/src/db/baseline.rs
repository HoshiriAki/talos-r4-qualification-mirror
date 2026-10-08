pub const SQLITE_IDENTITY_AUTHORITY_BASELINE: &str =
    include_str!("baseline/sqlite/001_identity_authority.sql");

#[cfg(feature = "postgres")]
pub const POSTGRES_IDENTITY_AUTHORITY_BASELINE: &str =
    include_str!("baseline/postgres/001_identity_authority.sql");

#[cfg(all(test, feature = "sqlite"))]
mod tests {
    use rusqlite::Connection;

    use super::SQLITE_IDENTITY_AUTHORITY_BASELINE;

    fn fresh_database() -> Connection {
        let conn = Connection::open_in_memory().expect("open in-memory database");
        conn.execute_batch(SQLITE_IDENTITY_AUTHORITY_BASELINE)
            .expect("apply identity baseline to an empty database");
        conn
    }

    #[test]
    fn baseline_creates_only_the_new_identity_subject_model() {
        let conn = fresh_database();
        for table in [
            "identities",
            "tenant_memberships",
            "platform_memberships",
            "platform_role_grants",
            "auth_sessions",
            "audit_events",
        ] {
            let exists: bool = conn
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE type = 'table' AND name = ?1)",
                    [table],
                    |row| row.get(0),
                )
                .unwrap();
            assert!(exists, "missing {table}");
        }

        let legacy_table = ["staff", "users"].join("_");
        let legacy_exists: bool = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE name = ?1)",
                [legacy_table],
                |row| row.get(0),
            )
            .unwrap();
        assert!(!legacy_exists);
    }

    #[test]
    fn memberships_enforce_identity_tenant_and_role_boundaries() {
        let conn = fresh_database();
        conn.execute(
            "INSERT INTO tenants VALUES ('tenant-a', 'A', 'a', 'active', 'free', NULL, 'now', 'now')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO identities
             (id, username, password_hash, display_name, status, created_at, updated_at)
             VALUES ('identity-a', 'owner', 'hash', 'Owner', 'active', 'now', 'now')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO tenants VALUES ('tenant-b', 'B', 'b', 'active', 'free', NULL, 'now', 'now')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO identities
             (id, username, password_hash, display_name, status, created_at, updated_at)
             VALUES ('identity-b', 'other', 'hash', 'Other', 'active', 'now', 'now')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO tenant_memberships
             (id, identity_id, tenant_id, role, status, created_at, updated_at)
             VALUES ('membership-b', 'identity-b', 'tenant-b', 'staff', 'active', 'now', 'now')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO tenant_memberships
             (id, identity_id, tenant_id, role, status, created_at, updated_at)
             VALUES ('membership-a', 'identity-a', 'tenant-a', 'owner', 'active', 'now', 'now')",
            [],
        )
        .unwrap();

        assert!(
            conn.execute(
                "INSERT INTO tenant_memberships
             (id, identity_id, tenant_id, role, status, created_at, updated_at)
             VALUES ('membership-b', 'identity-a', 'tenant-a', 'staff', 'active', 'now', 'now')",
                [],
            )
            .is_err()
        );
        assert!(
            conn.execute(
                "INSERT INTO audit_events
                 (id, actor_identity_id, authority_kind, tenant_id, tenant_membership_id,
                  roles_snapshot, capabilities_snapshot, action, resource_type,
                  correlation_id, occurred_at)
                 VALUES ('event-b', 'identity-a', 'tenant', 'tenant-a', 'membership-b',
                         '[]', '[]', 'read', 'tenant', 'request-b', 'now')",
                [],
            )
            .is_err()
        );
        assert!(
            conn.execute(
                "INSERT INTO platform_role_grants
             (id, platform_membership_id, role, granted_at)
             VALUES ('grant-a', 'missing', 'root_admin', 'now')",
                [],
            )
            .is_err()
        );
    }

    #[test]
    fn sessions_and_audit_require_non_ambiguous_security_state() {
        let conn = fresh_database();
        conn.execute(
            "INSERT INTO tenants VALUES ('tenant-a', 'A', 'a', 'active', 'free', NULL, 'now', 'now')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO identities
             (id, username, password_hash, display_name, status, created_at, updated_at)
             VALUES ('identity-a', 'owner', 'hash', 'Owner', 'active', 'now', 'now')",
            [],
        )
        .unwrap();

        assert!(
            conn.execute(
                "INSERT INTO auth_sessions
             (id, identity_id, auth_strength, created_at, last_seen_at, expires_at)
             VALUES ('session-a', 'identity-a', 'password', 'now', 'now', 'later')",
                [],
            )
            .is_err()
        );

        assert!(
            conn.execute(
                "INSERT INTO audit_events
             (id, actor_identity_id, authority_kind, tenant_id, roles_snapshot,
              capabilities_snapshot, action, resource_type, correlation_id, occurred_at)
             VALUES ('event-a', 'identity-a', 'tenant', 'tenant-a', '[]', '[]',
                     'read', 'tenant', 'request-a', 'now')",
                [],
            )
            .is_err()
        );
    }
}
