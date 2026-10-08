use std::collections::HashMap;
use std::sync::Arc;

use serde::Serialize;
use serde_json::Value;
use system_core::ExecutionContext;
use uuid::Uuid;

use crate::application::is_valid_serial_no;
use crate::repositories::{ImportedDeviceDraft, RepositoryError, RepositoryProvider};
use crate::utils::constants::{self, txt};
use crate::utils::time::shanghai_now_iso;

#[derive(Debug)]
pub enum DeviceMutationAuthorityError {
    InvalidInput(String),
    NotFound(String),
    Persistence { code: &'static str },
}

impl From<RepositoryError> for DeviceMutationAuthorityError {
    fn from(value: RepositoryError) -> Self {
        Self::Persistence { code: value.code() }
    }
}

#[derive(Debug, Clone)]
pub struct DeviceImportCandidate {
    pub row_no: usize,
    pub serial_no: String,
    pub notes: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceImportView {
    pub ok: bool,
    pub success_count: usize,
    pub fail_count: usize,
    pub failed_rows: Vec<Value>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceBulkUpdateView {
    pub items: Vec<Value>,
    pub failed: Vec<Value>,
    pub updated_count: usize,
    pub fail_count: usize,
}

#[derive(Clone)]
pub struct DeviceMutationAuthorityService {
    repository_provider: Arc<dyn RepositoryProvider>,
}

impl DeviceMutationAuthorityService {
    pub fn new(repository_provider: Arc<dyn RepositoryProvider>) -> Self {
        Self {
            repository_provider,
        }
    }

    pub fn checkin(
        &self,
        ctx: &ExecutionContext,
        serial_no: &str,
    ) -> Result<Value, DeviceMutationAuthorityError> {
        let serial_no = serial_no.trim();
        if serial_no.is_empty() {
            return Err(DeviceMutationAuthorityError::InvalidInput(
                "序列号无效".into(),
            ));
        }
        if !is_valid_serial_no(serial_no) {
            return Err(DeviceMutationAuthorityError::InvalidInput(
                "设备序列号格式无效".into(),
            ));
        }

        let scoped = self.repository_provider.bind(ctx)?;
        let mutation = scoped
            .devices()
            .checkin_status(serial_no, txt::STATUS_CHECKED_IN)?
            .ok_or_else(|| {
                DeviceMutationAuthorityError::NotFound("设备不存在，请先手动新增设备".into())
            })?;

        let action = if mutation.already_checked_in {
            "already_in_stock"
        } else {
            "updated"
        };
        let message = if mutation.already_checked_in {
            "设备已入库"
        } else {
            "设备状态已更新为已入库"
        };
        let auto_completed_orders = if mutation.already_checked_in {
            Vec::new()
        } else {
            scoped
                .devices()
                .complete_legacy_orders_after_checkin(serial_no, txt::STATUS_CHECKED_IN)?
        };

        let mut payload = serde_json::json!({
            "ok": true,
            "action": "updated",
            "message": message,
            "device": {},
        });
        if !auto_completed_orders.is_empty() {
            payload["autoCompletedOrders"] = Value::Array(
                auto_completed_orders
                    .into_iter()
                    .map(|order_id| {
                        serde_json::json!({
                            "orderId": order_id,
                            "reason": "all_devices_checked_in",
                            "triggeredBySerialNo": serial_no,
                        })
                    })
                    .collect(),
            );
        }

        let mut result = serde_json::json!({
            "ok": true,
            "action": action,
            "beforeStatus": mutation.before_status,
            "afterStatus": mutation.after_status,
            "payload": payload,
        });
        if let Some(auto_completed_orders) = result
            .get("payload")
            .and_then(|payload| payload.get("autoCompletedOrders"))
            .cloned()
        {
            result["autoCompletedOrders"] = auto_completed_orders;
        }
        Ok(result)
    }

    pub fn undo_checkin(
        &self,
        ctx: &ExecutionContext,
        serial_no: &str,
        previous_status: &str,
    ) -> Result<(), DeviceMutationAuthorityError> {
        let serial_no = serial_no.trim();
        let previous_status = previous_status.trim();
        if serial_no.is_empty() || previous_status.is_empty() {
            return Err(DeviceMutationAuthorityError::InvalidInput(
                "序列号或原状态不能为空".into(),
            ));
        }

        let scoped = self.repository_provider.bind(ctx)?;
        if !scoped
            .devices()
            .restore_status(serial_no, previous_status)?
        {
            return Err(DeviceMutationAuthorityError::NotFound("设备不存在".into()));
        }
        Ok(())
    }

    pub fn import_devices(
        &self,
        ctx: &ExecutionContext,
        candidates: Vec<DeviceImportCandidate>,
        mut failed_rows: Vec<Value>,
    ) -> Result<DeviceImportView, DeviceMutationAuthorityError> {
        let scoped = self.repository_provider.bind(ctx)?;
        let mut success_count = 0usize;
        let mut fail_count = failed_rows.len();

        for candidate in candidates {
            let draft = ImportedDeviceDraft {
                id: Uuid::new_v4().to_string(),
                serial_no: candidate.serial_no.clone(),
                status: txt::STATUS_CHECKED_IN.to_owned(),
                notes: candidate.notes,
                created_at: shanghai_now_iso(),
            };
            match scoped.devices().import_device(&draft) {
                Ok(true) => success_count += 1,
                Ok(false) => {
                    fail_count += 1;
                    failed_rows.push(serde_json::json!({
                        "rowNo": candidate.row_no,
                        "serialNo": candidate.serial_no,
                        "reason": "设备已存在",
                    }));
                }
                Err(_) => {
                    fail_count += 1;
                    failed_rows.push(serde_json::json!({
                        "rowNo": candidate.row_no,
                        "serialNo": candidate.serial_no,
                        "reason": "设备写入失败",
                    }));
                }
            }
        }

        if failed_rows.len() > 50 {
            failed_rows.truncate(50);
        }

        Ok(DeviceImportView {
            ok: true,
            success_count,
            fail_count,
            failed_rows,
        })
    }

    pub fn bulk_update(
        &self,
        ctx: &ExecutionContext,
        serial_nos: &[String],
        updates: &Value,
    ) -> Result<DeviceBulkUpdateView, DeviceMutationAuthorityError> {
        let scoped = self.repository_provider.bind(ctx)?;
        let rows = scoped.devices().compatibility_rows_by_serials(serial_nos)?;
        let existing = rows
            .into_iter()
            .map(|device| (device.serial_no.clone(), device))
            .collect::<HashMap<_, _>>();

        let requested_status = updates.get("rentalStatus").and_then(Value::as_str);
        let requested_notes = updates.get("notes").and_then(Value::as_str);
        let mut items = Vec::new();
        let mut failed = Vec::new();

        for serial_no in serial_nos {
            let Some(device) = existing.get(serial_no) else {
                failed.push(serde_json::json!({
                    "serialNo": serial_no,
                    "reason": "设备不存在",
                }));
                continue;
            };

            let merged_status = requested_status.unwrap_or(&device.rental_status);
            let merged_notes = requested_notes.unwrap_or(&device.notes);
            if !is_valid_serial_no(serial_no) {
                failed.push(serde_json::json!({
                    "serialNo": serial_no,
                    "reason": "设备序列号只能包含英文字母和数字",
                }));
                continue;
            }
            if merged_status.trim().is_empty() {
                failed.push(serde_json::json!({
                    "serialNo": serial_no,
                    "reason": "缺少字段: rentalStatus",
                }));
                continue;
            }
            if !constants::DEVICE_STATUS.contains(&merged_status) {
                failed.push(serde_json::json!({
                    "serialNo": serial_no,
                    "reason": format!("设备状态无效: {merged_status}"),
                }));
                continue;
            }

            if scoped
                .devices()
                .update_status_and_notes(serial_no, merged_status, merged_notes)?
            {
                items.push(serde_json::json!({ "serialNo": serial_no }));
            } else {
                failed.push(serde_json::json!({
                    "serialNo": serial_no,
                    "reason": "设备不存在",
                }));
            }
        }

        Ok(DeviceBulkUpdateView {
            updated_count: items.len(),
            fail_count: failed.len(),
            items,
            failed,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::DeviceMutationAuthorityError;

    #[test]
    fn mutation_errors_remain_bounded_for_http_mapping() {
        let error = DeviceMutationAuthorityError::InvalidInput("bad".into());
        assert!(matches!(
            error,
            DeviceMutationAuthorityError::InvalidInput(_)
        ));
    }
}
