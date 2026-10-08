#![cfg(feature = "sqlite")]

use std::{
    net::TcpListener,
    process::{Child, Command},
    time::Duration,
};

use reqwest::{
    Client, StatusCode,
    header::{COOKIE, HeaderMap, ORIGIN, SET_COOKIE},
};
use rusqlite::Connection;

const PLATFORM_USERNAME: &str = "root";
const PLATFORM_PASSWORD: &str = "TestAdmin123!";

struct TestServer {
    child: Child,
    db_path: std::path::PathBuf,
    base_url: String,
}

impl TestServer {
    async fn start() -> Self {
        let db_path =
            std::env::temp_dir().join(format!("talos-tenant-crud-{}.db", uuid::Uuid::new_v4()));
        let listener = TcpListener::bind("127.0.0.1:0").expect("reserve test port");
        let port = listener.local_addr().unwrap().port();
        drop(listener);

        let child = Command::new(env!("CARGO_BIN_EXE_talos-backend"))
            .env("DB_PATH", &db_path)
            .env("PORT", port.to_string())
            .env("HOST", "127.0.0.1")
            .env("AUTH_BOOTSTRAP_ON_START", "true")
            .env("AUTH_BOOTSTRAP_ADMIN_USERNAME", PLATFORM_USERNAME)
            .env("AUTH_BOOTSTRAP_ADMIN_PASSWORD", PLATFORM_PASSWORD)
            .spawn()
            .expect("start backend");
        let mut server = Self {
            child,
            db_path,
            base_url: format!("http://127.0.0.1:{port}"),
        };
        let client = Client::new();
        for _ in 0..150 {
            if let Ok(response) = client
                .get(format!("{}/health", server.base_url))
                .send()
                .await
                && response.status().is_success()
            {
                return server;
            }
            if let Some(status) = server.child.try_wait().expect("poll backend child status") {
                panic!("backend exited before becoming healthy: {status}");
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        panic!("backend did not become healthy within 15 seconds");
    }

    async fn platform_client(&self) -> Client {
        let login = Client::new()
            .post(format!("{}/auth/platform/login", self.base_url))
            .header(ORIGIN, &self.base_url)
            .json(&serde_json::json!({
                "username": PLATFORM_USERNAME,
                "password": PLATFORM_PASSWORD,
            }))
            .send()
            .await
            .expect("platform login request");
        assert_eq!(login.status(), StatusCode::OK);
        let cookie = login
            .headers()
            .get(SET_COOKIE)
            .expect("platform login session cookie")
            .to_str()
            .expect("valid session cookie")
            .split(';')
            .next()
            .expect("cookie pair")
            .to_string();

        let mut headers = HeaderMap::new();
        headers.insert(COOKIE, cookie.parse().unwrap());
        headers.insert(ORIGIN, self.base_url.parse().unwrap());
        headers.insert("x-talos-authority", "platform".parse().unwrap());
        Client::builder().default_headers(headers).build().unwrap()
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_file(&self.db_path);
        let _ = std::fs::remove_file(self.db_path.with_extension("db-wal"));
        let _ = std::fs::remove_file(self.db_path.with_extension("db-shm"));
    }
}

#[tokio::test]
async fn tenant_lifecycle_uses_platform_authority_and_soft_closure() {
    let server = TestServer::start().await;
    let client = server.platform_client().await;

    let unauthenticated = Client::new()
        .get(format!("{}/api/tenants", server.base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(unauthenticated.status(), StatusCode::UNAUTHORIZED);

    let create = client
        .post(format!("{}/api/tenants", server.base_url))
        .json(&serde_json::json!({ "name": "Acme", "slug": "acme" }))
        .send()
        .await
        .unwrap();
    assert_eq!(create.status(), StatusCode::OK);
    let created: serde_json::Value = create.json().await.unwrap();
    assert_eq!(created["plan"], "free");
    assert!(created.get("createdAt").and_then(|v| v.as_str()).is_some());
    assert!(created.get("updatedAt").and_then(|v| v.as_str()).is_some());
    assert!(created.get("created_at").is_none());
    let id = created["id"].as_str().unwrap();

    let update = client
        .put(format!("{}/api/tenants/{id}", server.base_url))
        .json(&serde_json::json!({ "name": "Acme Updated" }))
        .send()
        .await
        .unwrap();
    assert_eq!(update.status(), StatusCode::OK);
    let updated: serde_json::Value = update.json().await.unwrap();
    assert_eq!(updated["name"], "Acme Updated");
    assert_eq!(updated["plan"], "free");

    let forbidden_plan_update = client
        .put(format!("{}/api/tenants/{id}", server.base_url))
        .json(&serde_json::json!({ "plan": "enterprise" }))
        .send()
        .await
        .unwrap();
    assert_eq!(
        forbidden_plan_update.status(),
        StatusCode::UNPROCESSABLE_ENTITY
    );

    let suspended = client
        .patch(format!("{}/api/tenants/{id}/status", server.base_url))
        .json(&serde_json::json!({ "status": "suspended" }))
        .send()
        .await
        .unwrap();
    assert_eq!(suspended.status(), StatusCode::OK);
    assert_eq!(
        suspended.json::<serde_json::Value>().await.unwrap()["status"],
        "suspended"
    );

    let deleted = client
        .delete(format!("{}/api/tenants/{id}", server.base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(deleted.status(), StatusCode::OK);
    assert_eq!(
        deleted.json::<serde_json::Value>().await.unwrap()["success"],
        true
    );

    let conn = Connection::open(&server.db_path).unwrap();
    let (status, plan): (String, String) = conn
        .query_row(
            "SELECT status, plan FROM tenants WHERE slug = 'acme'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(status, "deleted");
    assert_eq!(plan, "free");
    let owner_count: i64 = conn
        .query_row(
            "SELECT COUNT(1) FROM tenant_memberships
             WHERE tenant_id = ?1 AND role = 'owner' AND status = 'active'",
            [id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(owner_count, 1);
}
