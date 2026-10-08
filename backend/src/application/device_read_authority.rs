use std::collections::HashSet;
use std::sync::Arc;

use chrono::{Datelike, Timelike};
use rust_xlsxwriter::{Workbook, XlsxError};
use serde::Serialize;
use system_core::ExecutionContext;

use crate::repositories::{
    DeviceCompatibilityReadRequest, DeviceCompatibilityReadRow, DeviceListRequest,
    DeviceProjection, RepositoryError, RepositoryProvider,
};
use crate::utils::constants::{self, txt};
use crate::utils::time;

pub const DEVICE_LEGACY_LIST_ROWS_MAX: usize = 500;
pub const DEVICE_EXPORT_ROWS_MAX: usize = 500;
pub const DEVICE_WARNING_SCAN_ROWS_MAX: usize = 5_000;

#[derive(Debug)]
pub enum DeviceReadAuthorityError {
    InvalidInput(String),
    Persistence { code: &'static str },
    Export(String),
}

impl From<RepositoryError> for DeviceReadAuthorityError {
    fn from(value: RepositoryError) -> Self {
        Self::Persistence { code: value.code() }
    }
}

impl From<XlsxError> for DeviceReadAuthorityError {
    fn from(value: XlsxError) -> Self {
        Self::Export(value.to_string())
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceReadView {
    pub id: String,
    pub serial_no: String,
    pub rental_status: String,
    pub notes: String,
    pub fallback_return_node: String,
    pub model_id: String,
    pub current_warehouse_id: String,
    pub expected_warehouse_id: String,
    pub expected_available_date: String,
    pub created_at: String,
    pub return_node: String,
    pub return_node_source: String,
    pub warning_status: String,
    pub warning_reason: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceReadPaginationView {
    pub page: u32,
    pub page_size: u32,
    pub total: u32,
    pub total_pages: u32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceReadPageView {
    pub devices: Vec<DeviceReadView>,
    pub pagination: DeviceReadPaginationView,
}

#[derive(Clone)]
pub struct DeviceReadAuthorityService {
    repository_provider: Arc<dyn RepositoryProvider>,
}

impl DeviceReadAuthorityService {
    pub fn new(repository_provider: Arc<dyn RepositoryProvider>) -> Self {
        Self {
            repository_provider,
        }
    }

    pub fn legacy_unpaged(
        &self,
        ctx: &ExecutionContext,
        rental_status: Option<&str>,
        legacy_model_id_from_keyword: Option<&str>,
        legacy_warehouse_id_from_notes: Option<&str>,
        legacy_serial_no_from_warning_status: Option<&str>,
    ) -> Result<Vec<DeviceProjection>, DeviceReadAuthorityError> {
        let scoped = self.repository_provider.bind(ctx)?;
        let devices = scoped.devices().list(&DeviceListRequest {
            status: normalized_owned(rental_status),
            model_id: normalized_owned(legacy_model_id_from_keyword),
            warehouse_id: normalized_owned(legacy_warehouse_id_from_notes),
            serial_no: normalized_owned(legacy_serial_no_from_warning_status),
        })?;
        if devices.len() > DEVICE_LEGACY_LIST_ROWS_MAX {
            return Err(DeviceReadAuthorityError::InvalidInput(format!(
                "无分页设备列表超过 {DEVICE_LEGACY_LIST_ROWS_MAX} 行；请使用 page/pageSize 分页查询"
            )));
        }
        Ok(devices)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn paged(
        &self,
        ctx: &ExecutionContext,
        keyword: Option<&str>,
        rental_status: Option<&str>,
        notes: Option<&str>,
        warning_status: Option<&str>,
        page: Option<u32>,
        page_size: Option<u32>,
    ) -> Result<DeviceReadPageView, DeviceReadAuthorityError> {
        let request = compatibility_request(keyword, rental_status, notes);
        let warning_status = normalized_owned(warning_status);
        let safe_page_size = page_size.filter(|value| *value > 0).unwrap_or(20).min(100);
        let requested_page = page.filter(|value| *value > 0).unwrap_or(1);
        let scoped = self.repository_provider.bind(ctx)?;

        if is_dynamic_warning_filter(&warning_status) {
            let base_count = scoped.devices().compatibility_count(&request)?;
            if usize::try_from(base_count).unwrap_or(usize::MAX) > DEVICE_WARNING_SCAN_ROWS_MAX {
                return Err(DeviceReadAuthorityError::InvalidInput(format!(
                    "预警状态查询需要扫描 {base_count} 台设备，超过上限 {DEVICE_WARNING_SCAN_ROWS_MAX}；请增加筛选条件"
                )));
            }
            let rows = scoped.devices().compatibility_rows(&request, None, None)?;
            let filtered = filter_warning(enrich_rows(rows), &warning_status);
            let total = u32::try_from(filtered.len())
                .map_err(|_| DeviceReadAuthorityError::InvalidInput("设备分页总数溢出".into()))?;
            let total_pages = total.div_ceil(safe_page_size).max(1);
            let safe_page = requested_page.min(total_pages);
            let offset = (safe_page - 1) * safe_page_size;
            let devices = filtered
                .into_iter()
                .skip(offset as usize)
                .take(safe_page_size as usize)
                .collect();
            return Ok(DeviceReadPageView {
                devices,
                pagination: DeviceReadPaginationView {
                    page: safe_page,
                    page_size: safe_page_size,
                    total,
                    total_pages,
                },
            });
        }

        let total = scoped.devices().compatibility_count(&request)?;
        let total_pages = total.div_ceil(safe_page_size).max(1);
        let safe_page = requested_page.min(total_pages);
        let offset = (safe_page - 1) * safe_page_size;
        let rows =
            scoped
                .devices()
                .compatibility_rows(&request, Some(safe_page_size), Some(offset))?;

        Ok(DeviceReadPageView {
            devices: enrich_rows(rows),
            pagination: DeviceReadPaginationView {
                page: safe_page,
                page_size: safe_page_size,
                total,
                total_pages,
            },
        })
    }

    pub fn export_filtered(
        &self,
        ctx: &ExecutionContext,
        keyword: Option<&str>,
        rental_status: Option<&str>,
        notes: Option<&str>,
        warning_status: Option<&str>,
    ) -> Result<Vec<DeviceReadView>, DeviceReadAuthorityError> {
        let request = compatibility_request(keyword, rental_status, notes);
        let scoped = self.repository_provider.bind(ctx)?;
        let base_count = scoped.devices().compatibility_count(&request)?;
        if usize::try_from(base_count).unwrap_or(usize::MAX) > DEVICE_EXPORT_ROWS_MAX {
            return Err(DeviceReadAuthorityError::InvalidInput(format!(
                "设备导出候选 {base_count} 行，超过上限 {DEVICE_EXPORT_ROWS_MAX}；请缩小筛选范围"
            )));
        }

        let rows = scoped.devices().compatibility_rows(
            &request,
            Some(u32::try_from(DEVICE_EXPORT_ROWS_MAX + 1).unwrap_or(u32::MAX)),
            None,
        )?;
        if rows.len() > DEVICE_EXPORT_ROWS_MAX {
            return Err(DeviceReadAuthorityError::InvalidInput(format!(
                "设备导出候选超过上限 {DEVICE_EXPORT_ROWS_MAX}；请缩小筛选范围"
            )));
        }
        Ok(filter_warning(
            enrich_rows(rows),
            &normalized_owned(warning_status),
        ))
    }

    pub fn export_by_serials(
        &self,
        ctx: &ExecutionContext,
        serial_nos: &[String],
        max_items: usize,
    ) -> Result<Vec<DeviceReadView>, DeviceReadAuthorityError> {
        if serial_nos.len() > max_items {
            return Err(DeviceReadAuthorityError::InvalidInput(format!(
                "serialNos 数量超过上限 {max_items}"
            )));
        }
        let mut seen = HashSet::new();
        let unique = serial_nos
            .iter()
            .map(|serial| serial.trim().to_owned())
            .filter(|serial| !serial.is_empty() && seen.insert(serial.clone()))
            .collect::<Vec<_>>();
        if unique.is_empty() {
            return Ok(Vec::new());
        }
        let scoped = self.repository_provider.bind(ctx)?;
        Ok(enrich_rows(
            scoped.devices().compatibility_rows_by_serials(&unique)?,
        ))
    }

    pub fn build_export_workbook(
        &self,
        devices: &[DeviceReadView],
    ) -> Result<Vec<u8>, DeviceReadAuthorityError> {
        let mut workbook = Workbook::new();
        let worksheet = workbook.add_worksheet().set_name("devices")?;
        let headers = ["序列号", "状态", "预警状态", "归还节点", "备注", "创建时间"];
        for (column, header) in headers.iter().enumerate() {
            worksheet.write(0, column as u16, *header)?;
        }
        for (index, device) in devices.iter().enumerate() {
            let row = (index + 1) as u32;
            worksheet.write(row, 0, device.serial_no.as_str())?;
            worksheet.write(row, 1, device.rental_status.as_str())?;
            worksheet.write(row, 2, device.warning_status.as_str())?;
            worksheet.write(row, 3, device.return_node.as_str())?;
            worksheet.write(row, 4, device.notes.as_str())?;
            worksheet.write(row, 5, device.created_at.as_str())?;
        }
        workbook.save_to_buffer().map_err(Into::into)
    }

    pub fn export_file_name(&self) -> String {
        let now = chrono::Utc::now().with_timezone(&chrono_tz::Asia::Shanghai);
        format!(
            "devices-{}-{:02}-{:02}-{:02}{:02}{:02}.xlsx",
            now.year(),
            now.month(),
            now.day(),
            now.hour(),
            now.minute(),
            now.second()
        )
    }
}

fn compatibility_request(
    keyword: Option<&str>,
    rental_status: Option<&str>,
    notes: Option<&str>,
) -> DeviceCompatibilityReadRequest {
    let rental_status = normalized_owned(rental_status)
        .filter(|value| constants::DEVICE_STATUS.contains(&value.as_str()));
    DeviceCompatibilityReadRequest {
        keyword: normalized_owned(keyword),
        rental_status,
        notes: normalized_owned(notes),
    }
}

fn normalized_owned(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

fn enrich_rows(rows: Vec<DeviceCompatibilityReadRow>) -> Vec<DeviceReadView> {
    let now_epoch_ms = time::shanghai_now_epoch_ms();
    rows.into_iter()
        .map(|row| enrich_row(row, now_epoch_ms))
        .collect()
}

fn enrich_row(row: DeviceCompatibilityReadRow, now_epoch_ms: i64) -> DeviceReadView {
    let order_return_node = row.order_return_node.trim();
    let fallback_return_node = row.fallback_return_node.trim();
    let (return_node, return_node_source) = if !order_return_node.is_empty() {
        (order_return_node.to_owned(), "order".to_owned())
    } else if !fallback_return_node.is_empty() {
        (fallback_return_node.to_owned(), "fallback".to_owned())
    } else {
        match time::parse_date_time_to_epoch_ms(&row.created_at) {
            Some(epoch_ms) => (
                time::format_epoch_ms_to_shanghai_iso(epoch_ms),
                "createdAt".to_owned(),
            ),
            None => (row.created_at.clone(), "createdAt".to_owned()),
        }
    };
    let (warning_status, warning_reason) =
        calculate_warning(&row.rental_status, &return_node, now_epoch_ms);

    DeviceReadView {
        id: row.id,
        serial_no: row.serial_no,
        rental_status: row.rental_status,
        notes: row.notes,
        fallback_return_node: row.fallback_return_node,
        model_id: row.model_id,
        current_warehouse_id: row.current_warehouse_id,
        expected_warehouse_id: row.expected_warehouse_id,
        expected_available_date: row.expected_available_date,
        created_at: row.created_at,
        return_node,
        return_node_source,
        warning_status,
        warning_reason,
    }
}

fn calculate_warning(
    rental_status: &str,
    return_node: &str,
    now_epoch_ms: i64,
) -> (String, String) {
    if rental_status == txt::STATUS_REPAIR
        || rental_status == txt::STATUS_LOST
        || rental_status == txt::STATUS_SCRAPPED
    {
        return (txt::WARNING_NORMAL.to_owned(), String::new());
    }
    let Some(base_epoch_ms) = time::parse_date_time_to_epoch_ms(return_node) else {
        return (txt::WARNING_NORMAL.to_owned(), String::new());
    };
    let deadline_epoch_ms = base_epoch_ms + 3 * 24 * 60 * 60 * 1000;
    if now_epoch_ms > deadline_epoch_ms && rental_status != txt::STATUS_CHECKED_IN {
        return (
            txt::WARNING_LOST.to_owned(),
            "超过归还节点3天仍未入库".to_owned(),
        );
    }
    (txt::WARNING_NORMAL.to_owned(), String::new())
}

fn is_dynamic_warning_filter(value: &Option<String>) -> bool {
    value
        .as_deref()
        .is_some_and(|value| value == txt::WARNING_NORMAL || value == txt::WARNING_LOST)
}

fn filter_warning(
    devices: Vec<DeviceReadView>,
    warning_status: &Option<String>,
) -> Vec<DeviceReadView> {
    if is_dynamic_warning_filter(warning_status) {
        let expected = warning_status.as_deref().unwrap_or_default();
        devices
            .into_iter()
            .filter(|device| device.warning_status == expected)
            .collect()
    } else {
        devices
    }
}

#[cfg(test)]
mod tests {
    use super::{
        DEVICE_EXPORT_ROWS_MAX, DEVICE_LEGACY_LIST_ROWS_MAX, DEVICE_WARNING_SCAN_ROWS_MAX,
        DeviceListRequest, calculate_warning, compatibility_request,
    };
    use crate::utils::constants::txt;

    #[test]
    fn compatibility_read_budgets_remain_bounded() {
        assert!((1..=500).contains(&DEVICE_LEGACY_LIST_ROWS_MAX));
        assert!((1..=500).contains(&DEVICE_EXPORT_ROWS_MAX));
        assert!((1..=5_000).contains(&DEVICE_WARNING_SCAN_ROWS_MAX));
    }

    #[test]
    fn legacy_unpaged_parameter_mapping_is_intentionally_compatibility_shaped() {
        let request = DeviceListRequest {
            status: Some("status".into()),
            model_id: Some("keyword".into()),
            warehouse_id: Some("notes".into()),
            serial_no: Some("warning".into()),
        };
        assert_eq!(request.status.as_deref(), Some("status"));
        assert_eq!(request.model_id.as_deref(), Some("keyword"));
        assert_eq!(request.warehouse_id.as_deref(), Some("notes"));
        assert_eq!(request.serial_no.as_deref(), Some("warning"));
    }

    #[test]
    fn invalid_rental_status_preserves_legacy_ignore_semantics() {
        assert!(
            compatibility_request(None, Some("not-a-status"), None)
                .rental_status
                .is_none()
        );
    }

    #[test]
    fn dynamic_warning_marks_overdue_unchecked_device_lost() {
        let base = 1_700_000_000_000_i64;
        let return_node = crate::utils::time::format_epoch_ms_to_shanghai_iso(base);
        let (status, reason) =
            calculate_warning("租赁中", &return_node, base + 4 * 24 * 60 * 60 * 1000);
        assert_eq!(status, txt::WARNING_LOST);
        assert!(!reason.is_empty());
    }
}
