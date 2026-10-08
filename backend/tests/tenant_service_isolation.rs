#![cfg(feature = "sqlite")]

use std::{
    net::TcpListener,
    process::{Child, Command},
    time::Duration,
};

use reqwest::{Client, StatusCode};
use rusqlite::{Connection, params};
use sha2::{Digest, Sha256};

struct TestServer {
    child: Child,
    db_path: std::path::PathBuf,
    base_url: String,
}

impl TestServer {
    async fn start() -> Self {
        let db_path = std::env::temp_dir().join(format!(
            "talos-service-isolation-{}.db",
            uuid::Uuid::new_v4()
        ));
        let listener = TcpListener::bind("127.0.0.1:0").expect("reserve test port");
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        let child = Command::new(env!("CARGO_BIN_EXE_talos-backend"))
            .env("DB_PATH", &db_path)
            .env("PORT", port.to_string())
            .env("HOST", "127.0.0.1")
            .spawn()
            .expect("start backend");
        let server = Self {
            child,
            db_path,
            base_url: format!("http://127.0.0.1:{port}"),
        };
        let health = Client::new();
        for _ in 0..50 {
            if let Ok(response) = health
                .get(format!("{}/health", server.base_url))
                .send()
                .await
                && response.status().is_success()
            {
                server.seed();
                return server;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        panic!("backend did not become healthy");
    }

    fn seed(&self) {
        let conn = Connection::open(&self.db_path).expect("open DB");
        let now = "2026-01-01T00:00:00+08:00";
        for (id, slug) in [("tenant-a", "tenant-a"), ("tenant-b", "tenant-b")] {
            conn.execute("INSERT INTO tenants (id, name, slug, status, plan, created_at, updated_at) VALUES (?1, ?1, ?2, 'active', 'free', ?3, ?3)", params![id, slug, now]).unwrap();
            conn.execute(
                "INSERT INTO identities
                 (id, username, password_hash, display_name, status, created_at, updated_at)
                 VALUES (?1, ?1, 'unused-test-hash', ?1, 'active', ?2, ?2)",
                params![id, now],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO tenant_memberships
                 (id, identity_id, tenant_id, role, status, created_at, updated_at)
                 VALUES (?1, ?2, ?2, 'admin', 'active', ?3, ?3)",
                params![format!("membership-{id}"), id, now],
            )
            .unwrap();
            let token = format!("session-{id}");
            let token_hash = hex::encode(Sha256::digest(token.as_bytes()));
            conn.execute(
                "INSERT INTO auth_sessions
                 (id, token_hash, identity_id, auth_strength, created_at, last_seen_at, expires_at)
                 VALUES (?1, ?2, ?3, 'password', ?4, ?4, '2099-01-01T00:00:00+08:00')",
                params![format!("auth-session-{id}"), token_hash, id, now],
            )
            .unwrap();
        }
        for (id, serial, tenant) in [
            ("device-a", "DEVICEA", "tenant-a"),
            ("device-b", "DEVICEB", "tenant-b"),
        ] {
            conn.execute("INSERT INTO devices (id, serialNo, rentalStatus, notes, modelId, currentWarehouseId, expectedWarehouseId, expectedAvailableDate, createdAt, tenant_id) VALUES (?1, ?2, '已入库', '', '', '', '', '', ?3, ?4)", params![id, serial, now, tenant]).unwrap();
        }
        for (id, tenant) in [("warehouse-a", "tenant-a"), ("warehouse-b", "tenant-b")] {
            conn.execute("INSERT INTO warehouses (id, name, type, enabled, createdAt, updatedAt, tenant_id) VALUES (?1, ?1, 'owned', 1, ?2, ?2, ?3)", params![id, now, tenant]).unwrap();
        }
        conn.execute(
            "UPDATE devices SET currentWarehouseId = 'warehouse-b' WHERE id = 'device-b'",
            [],
        )
        .unwrap();
        conn.execute("INSERT INTO warehouse_region_rules (id, warehouseId, province, shippingDays, returnDays, isPrimary, createdAt, updatedAt) VALUES ('rule-b', 'warehouse-b', 'Zhejiang', 1, 1, 1, ?1, ?1)", [now]).unwrap();
    }

    fn tenant_a_client(&self) -> Client {
        Client::builder()
            .default_headers({
                let mut h = reqwest::header::HeaderMap::new();
                h.insert(
                    reqwest::header::COOKIE,
                    "talos_session=session-tenant-a".parse().unwrap(),
                );
                h.insert(reqwest::header::HOST, "tenant-a.talos.app".parse().unwrap());
                h.insert(
                    reqwest::header::ORIGIN,
                    "http://tenant-a.talos.app".parse().unwrap(),
                );
                h
            })
            .build()
            .unwrap()
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
async fn tenant_a_cannot_read_or_mutate_tenant_b_device_through_direct_services() {
    let server = TestServer::start().await;
    let client = server.tenant_a_client();
    let list = client
        .get(format!("{}/devices?page=1&pageSize=100", server.base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(list.status(), StatusCode::OK);
    let listed: serde_json::Value = list.json().await.unwrap();
    assert_eq!(
        listed["pagination"]["total"], 1,
        "tenant B device must not be listed"
    );

    let checkin = client
        .post(format!("{}/devices/checkin-scan", server.base_url))
        .json(&serde_json::json!({"serialNo":"DEVICEB"}))
        .send()
        .await
        .unwrap();
    assert_eq!(
        checkin.status(),
        StatusCode::NOT_FOUND,
        "cross-tenant check-in must not reveal existence"
    );

    let bulk = client
        .post(format!("{}/devices/bulk-update", server.base_url))
        .json(&serde_json::json!({"serialNos":["DEVICEB"],"updates":{"notes":"pwned"}}))
        .send()
        .await
        .unwrap();
    assert_eq!(bulk.status(), StatusCode::OK);
    let body: serde_json::Value = bulk.json().await.unwrap();
    assert_eq!(body["updatedCount"], 0);
    let conn = Connection::open(&server.db_path).unwrap();
    assert_eq!(
        conn.query_row::<String, _, _>(
            "SELECT notes FROM devices WHERE serialNo = 'DEVICEB'",
            [],
            |r| r.get(0)
        )
        .unwrap(),
        ""
    );

    let stats = client
        .get(format!("{}/api/warehouses/stats", server.base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(stats.status(), StatusCode::OK);
    let stats: serde_json::Value = stats.json().await.unwrap();
    assert_eq!(
        stats["warehouses"].as_array().unwrap().len(),
        1,
        "tenant B warehouse must not be in stats"
    );
    let regions = client
        .get(format!(
            "{}/api/warehouses/warehouse-b/regions",
            server.base_url
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(
        regions.status(),
        StatusCode::NOT_FOUND,
        "cross-tenant warehouse lookup must not disclose an empty resource"
    );
    let warehouse_devices = client
        .get(format!(
            "{}/api/warehouses/warehouse-b/devices",
            server.base_url
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(warehouse_devices.status(), StatusCode::NOT_FOUND);

    let cross_upsert = client
        .put(format!(
            "{}/api/warehouses/warehouse-b/regions",
            server.base_url
        ))
        .json(&serde_json::json!({"province":"Jiangsu","shippingDays":1,"returnDays":1}))
        .send()
        .await
        .unwrap();
    assert_eq!(cross_upsert.status(), StatusCode::NOT_FOUND);
    let cross_delete = client
        .delete(format!(
            "{}/api/warehouses/warehouse-b/regions/Zhejiang",
            server.base_url
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(cross_delete.status(), StatusCode::NOT_FOUND);
    assert_eq!(
        conn.query_row::<i64, _, _>(
            "SELECT COUNT(*) FROM warehouse_region_rules WHERE id = 'rule-b'",
            [],
            |r| r.get(0)
        )
        .unwrap(),
        1,
        "cross-tenant region mutations must leave data unchanged"
    );

    let bulk_delete = client
        .post(format!("{}/devices/bulk-delete", server.base_url))
        .json(&serde_json::json!({"serialNos":["DEVICEB"]}))
        .send()
        .await
        .unwrap();
    assert_eq!(bulk_delete.status(), StatusCode::OK);
    assert_eq!(
        bulk_delete.json::<serde_json::Value>().await.unwrap()["successCount"],
        0
    );
    assert_eq!(
        conn.query_row::<i64, _, _>(
            "SELECT COUNT(*) FROM devices WHERE id = 'device-b'",
            [],
            |r| r.get(0)
        )
        .unwrap(),
        1
    );

    let own_devices = client
        .get(format!(
            "{}/api/warehouses/warehouse-a/devices",
            server.base_url
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(
        own_devices.status(),
        StatusCode::OK,
        "same-tenant warehouse access remains available"
    );

    let mut workbook = rust_xlsxwriter::Workbook::new();
    workbook
        .add_worksheet()
        .write(0, 0, "serialNo")
        .unwrap()
        .write(1, 0, "IMPORTA")
        .unwrap();
    let boundary = "talos-test-boundary";
    let mut body = format!("--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"devices.xlsx\"\r\nContent-Type: application/vnd.openxmlformats-officedocument.spreadsheetml.sheet\r\n\r\n").into_bytes();
    body.extend(workbook.save_to_buffer().unwrap());
    body.extend(format!("\r\n--{boundary}--\r\n").as_bytes());
    let import = client
        .post(format!("{}/devices/import-excel", server.base_url))
        .header(
            reqwest::header::CONTENT_TYPE,
            format!("multipart/form-data; boundary={boundary}"),
        )
        .body(body)
        .send()
        .await
        .unwrap();
    assert_eq!(import.status(), StatusCode::OK);
    assert_eq!(
        conn.query_row::<String, _, _>(
            "SELECT tenant_id FROM devices WHERE serialNo = 'IMPORTA'",
            [],
            |r| r.get(0)
        )
        .unwrap(),
        "tenant-a",
        "Excel import must write the authenticated tenant ID"
    );
}
