use crate::utils::time::shanghai_now_iso;
use rusqlite::Connection;

pub fn get_settings(conn: &Connection, user_id: &str) -> serde_json::Value {
    let mut stmt = conn
        .prepare("SELECT settingsJson FROM user_settings WHERE userId = ?1")
        .unwrap();
    let result: Result<String, _> = stmt.query_row([user_id], |row| row.get(0));
    match result {
        Ok(json) => serde_json::from_str(&json).unwrap_or(serde_json::json!({})),
        Err(_) => serde_json::json!({}),
    }
}

pub fn save_settings(
    conn: &Connection,
    user_id: &str,
    settings: &serde_json::Value,
) -> anyhow::Result<()> {
    let json = serde_json::to_string(settings)?;
    let now = shanghai_now_iso();
    conn.execute(
        "INSERT INTO user_settings (userId, settingsJson, updatedAt) VALUES (?1, ?2, ?3)
         ON CONFLICT(userId) DO UPDATE SET settingsJson = excluded.settingsJson, updatedAt = excluded.updatedAt",
        rusqlite::params![user_id, json, now],
    )?;
    Ok(())
}
