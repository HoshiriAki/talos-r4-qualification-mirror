//! Backup status API
//! GET /api/tenant/backup-status — bounded backup directory metadata scan (Tenant Admin/Owner)

use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};
use std::sync::Arc;

use crate::middleware::auth::AdminUser;
use crate::state::AppState;

const BACKUP_SCAN_ENTRY_LIMIT: usize = 4096;

pub fn backup_routes() -> Router<Arc<AppState>> {
    Router::new().route("/api/tenant/backup-status", get(backup_status))
}

async fn backup_status(
    State(_state): State<Arc<AppState>>,
    _admin: AdminUser,
) -> Json<serde_json::Value> {
    let backup_dir = std::path::Path::new("backups");
    let mut latest_name = String::new();
    let mut latest_modified = std::time::SystemTime::UNIX_EPOCH;
    let mut count = 0usize;
    let mut total_size = 0u64;
    let mut scan_truncated = false;
    let mut scan_error = false;

    if backup_dir.exists() {
        match std::fs::read_dir(backup_dir) {
            Ok(entries) => {
                for (index, entry) in entries.enumerate() {
                    if index >= BACKUP_SCAN_ENTRY_LIMIT {
                        scan_truncated = true;
                        break;
                    }
                    let Ok(entry) = entry else {
                        scan_error = true;
                        continue;
                    };
                    let Ok(metadata) = entry.metadata() else {
                        scan_error = true;
                        continue;
                    };
                    if !metadata.is_file() {
                        continue;
                    }

                    count += 1;
                    total_size = total_size.saturating_add(metadata.len());
                    if let Ok(modified) = metadata.modified()
                        && modified >= latest_modified
                    {
                        latest_modified = modified;
                        latest_name = entry.file_name().to_string_lossy().to_string();
                    }
                }
            }
            Err(_) => {
                scan_error = true;
            }
        }
    }

    if scan_truncated {
        // A bounded scan cannot truthfully claim the globally newest file.
        latest_name.clear();
    }

    Json(serde_json::json!({
        "ok": !scan_error,
        "backupDir": "backups",
        "backupDirExists": backup_dir.exists(),
        "latestBackup": latest_name,
        "backupCount": count,
        "totalSizeBytes": total_size,
        "totalSizeHuman": format_size(total_size),
        "retentionDays": 30,
        "scanEntryLimit": BACKUP_SCAN_ENTRY_LIMIT,
        "scanTruncated": scan_truncated,
        "scanError": scan_error,
    }))
}

fn format_size(bytes: u64) -> String {
    if bytes > 1_073_741_824 {
        format!("{:.1} GB", bytes as f64 / 1_073_741_824.0)
    } else if bytes > 1_048_576 {
        format!("{:.1} MB", bytes as f64 / 1_048_576.0)
    } else if bytes > 1_024 {
        format!("{:.1} KB", bytes as f64 / 1_024.0)
    } else {
        format!("{} B", bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scan_budget_is_explicit_and_finite() {
        assert!(BACKUP_SCAN_ENTRY_LIMIT > 0);
        assert!(BACKUP_SCAN_ENTRY_LIMIT <= 4096);
    }
}
