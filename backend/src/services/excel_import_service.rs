#![allow(dead_code)]
use calamine::{Data, Reader, open_workbook_auto};
use serde::Serialize;
use std::path::{Path, PathBuf};
use uuid::Uuid;

use crate::error::AppError;

// ── Excel type detection / P7-B import budgets ───────────────────

const EXCEL_EXTENSIONS: &[&str] = &[".xlsx", ".xls", ".xlsm", ".xlsb", ".csv"];
const EXCEL_MIME_TYPES: &[&str] = &[
    "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
    "application/vnd.ms-excel",
    "application/vnd.ms-excel.sheet.macroenabled.12",
    "application/vnd.ms-excel.sheet.binary.macroenabled.12",
    "text/csv",
    "application/csv",
    "application/octet-stream",
];

pub const IMPORT_FILE_BYTES_MAX: usize = 20 * 1024 * 1024;
pub const IMPORT_FILE_NAME_BYTES_MAX: usize = 180;
pub const IMPORT_WORKBOOK_SHEETS_MAX: usize = 32;
pub const IMPORT_WORKBOOK_ROWS_MAX: usize = 10_001; // header + 10k data rows
pub const IMPORT_WORKBOOK_COLUMNS_MAX: usize = 128;
pub const IMPORT_WORKBOOK_CELLS_MAX: usize = 500_000;
pub const IMPORT_CELL_STRING_BYTES_MAX: usize = 64 * 1024;

const ARCHIVE_ENTRIES_MAX: usize = 512;
const ARCHIVE_UNCOMPRESSED_BYTES_MAX: u64 = 128 * 1024 * 1024;
const ARCHIVE_ENTRY_UNCOMPRESSED_BYTES_MAX: u64 = 64 * 1024 * 1024;
const ARCHIVE_EXPANSION_RATIO_MAX: u64 = 80;
const ARCHIVE_RATIO_SLACK_BYTES: u64 = 16 * 1024 * 1024;
const ZIP_EOCD_SIGNATURE: &[u8] = b"PK\x05\x06";
const ZIP_CENTRAL_SIGNATURE: &[u8] = b"PK\x01\x02";

fn import_extension(file_name: &str) -> Option<String> {
    Path::new(file_name)
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_ascii_lowercase())
}

fn read_u16_le(buffer: &[u8], offset: usize) -> Option<u16> {
    let bytes: [u8; 2] = buffer
        .get(offset..offset.checked_add(2)?)?
        .try_into()
        .ok()?;
    Some(u16::from_le_bytes(bytes))
}

fn read_u32_le(buffer: &[u8], offset: usize) -> Option<u32> {
    let bytes: [u8; 4] = buffer
        .get(offset..offset.checked_add(4)?)?
        .try_into()
        .ok()?;
    Some(u32::from_le_bytes(bytes))
}

fn find_zip_eocd(buffer: &[u8]) -> Option<usize> {
    if buffer.len() < 22 {
        return None;
    }
    let last = buffer.len() - 22;
    let first = buffer.len().saturating_sub(22 + u16::MAX as usize);
    (first..=last)
        .rev()
        .find(|offset| buffer.get(*offset..*offset + 4) == Some(ZIP_EOCD_SIGNATURE))
}

fn validate_zip_expansion_budget(buffer: &[u8]) -> Result<(), AppError> {
    let eocd = find_zip_eocd(buffer)
        .ok_or_else(|| AppError::BadRequest("Excel ZIP 目录无效或缺失".to_string()))?;
    let disk_no = read_u16_le(buffer, eocd + 4)
        .ok_or_else(|| AppError::BadRequest("Excel ZIP 目录损坏".to_string()))?;
    let central_disk = read_u16_le(buffer, eocd + 6)
        .ok_or_else(|| AppError::BadRequest("Excel ZIP 目录损坏".to_string()))?;
    let entries_on_disk = read_u16_le(buffer, eocd + 8)
        .ok_or_else(|| AppError::BadRequest("Excel ZIP 目录损坏".to_string()))?;
    let total_entries = read_u16_le(buffer, eocd + 10)
        .ok_or_else(|| AppError::BadRequest("Excel ZIP 目录损坏".to_string()))?;
    let central_size = read_u32_le(buffer, eocd + 12)
        .ok_or_else(|| AppError::BadRequest("Excel ZIP 目录损坏".to_string()))?;
    let central_offset = read_u32_le(buffer, eocd + 16)
        .ok_or_else(|| AppError::BadRequest("Excel ZIP 目录损坏".to_string()))?;
    let comment_len = read_u16_le(buffer, eocd + 20)
        .ok_or_else(|| AppError::BadRequest("Excel ZIP 目录损坏".to_string()))?
        as usize;

    if disk_no != 0 || central_disk != 0 || entries_on_disk != total_entries {
        return Err(AppError::BadRequest(
            "不支持多卷 Excel ZIP 文件".to_string(),
        ));
    }
    if total_entries == u16::MAX || central_size == u32::MAX || central_offset == u32::MAX {
        return Err(AppError::BadRequest(
            "不支持 ZIP64 Excel 导入文件".to_string(),
        ));
    }
    if total_entries as usize > ARCHIVE_ENTRIES_MAX {
        return Err(AppError::BadRequest(format!(
            "Excel ZIP 条目数超过上限 {ARCHIVE_ENTRIES_MAX}"
        )));
    }

    let eocd_end = eocd
        .checked_add(22)
        .and_then(|value| value.checked_add(comment_len))
        .ok_or_else(|| AppError::BadRequest("Excel ZIP 目录长度溢出".to_string()))?;
    if eocd_end > buffer.len() {
        return Err(AppError::BadRequest("Excel ZIP 注释长度无效".to_string()));
    }

    let central_offset = central_offset as usize;
    let central_end = central_offset
        .checked_add(central_size as usize)
        .ok_or_else(|| AppError::BadRequest("Excel ZIP 中央目录长度溢出".to_string()))?;
    if central_end > eocd || central_end > buffer.len() {
        return Err(AppError::BadRequest(
            "Excel ZIP 中央目录范围无效".to_string(),
        ));
    }

    let mut cursor = central_offset;
    let mut total_compressed = 0u64;
    let mut total_uncompressed = 0u64;
    for _ in 0..total_entries {
        if buffer.get(cursor..cursor.saturating_add(4)) != Some(ZIP_CENTRAL_SIGNATURE) {
            return Err(AppError::BadRequest(
                "Excel ZIP 中央目录条目无效".to_string(),
            ));
        }
        let flags = read_u16_le(buffer, cursor + 8)
            .ok_or_else(|| AppError::BadRequest("Excel ZIP 条目损坏".to_string()))?;
        if flags & 0x0001 != 0 {
            return Err(AppError::BadRequest(
                "不支持加密 Excel ZIP 文件".to_string(),
            ));
        }
        let compressed = read_u32_le(buffer, cursor + 20)
            .ok_or_else(|| AppError::BadRequest("Excel ZIP 条目损坏".to_string()))?;
        let uncompressed = read_u32_le(buffer, cursor + 24)
            .ok_or_else(|| AppError::BadRequest("Excel ZIP 条目损坏".to_string()))?;
        if compressed == u32::MAX || uncompressed == u32::MAX {
            return Err(AppError::BadRequest("不支持 ZIP64 Excel 条目".to_string()));
        }
        if uncompressed as u64 > ARCHIVE_ENTRY_UNCOMPRESSED_BYTES_MAX {
            return Err(AppError::BadRequest(format!(
                "Excel ZIP 单条目展开后超过 {} MiB",
                ARCHIVE_ENTRY_UNCOMPRESSED_BYTES_MAX / 1024 / 1024
            )));
        }
        total_compressed = total_compressed.saturating_add(compressed as u64);
        total_uncompressed = total_uncompressed.saturating_add(uncompressed as u64);
        if total_uncompressed > ARCHIVE_UNCOMPRESSED_BYTES_MAX {
            return Err(AppError::BadRequest(format!(
                "Excel ZIP 展开后总大小超过 {} MiB",
                ARCHIVE_UNCOMPRESSED_BYTES_MAX / 1024 / 1024
            )));
        }

        let file_name_len = read_u16_le(buffer, cursor + 28)
            .ok_or_else(|| AppError::BadRequest("Excel ZIP 条目损坏".to_string()))?
            as usize;
        let extra_len = read_u16_le(buffer, cursor + 30)
            .ok_or_else(|| AppError::BadRequest("Excel ZIP 条目损坏".to_string()))?
            as usize;
        let entry_comment_len = read_u16_le(buffer, cursor + 32)
            .ok_or_else(|| AppError::BadRequest("Excel ZIP 条目损坏".to_string()))?
            as usize;
        cursor = cursor
            .checked_add(46)
            .and_then(|value| value.checked_add(file_name_len))
            .and_then(|value| value.checked_add(extra_len))
            .and_then(|value| value.checked_add(entry_comment_len))
            .ok_or_else(|| AppError::BadRequest("Excel ZIP 条目长度溢出".to_string()))?;
        if cursor > central_end {
            return Err(AppError::BadRequest(
                "Excel ZIP 中央目录条目越界".to_string(),
            ));
        }
    }

    if total_uncompressed > 0 && total_compressed == 0 {
        return Err(AppError::BadRequest(
            "Excel ZIP 压缩尺寸信息无效".to_string(),
        ));
    }
    let ratio_budget = total_compressed
        .saturating_mul(ARCHIVE_EXPANSION_RATIO_MAX)
        .saturating_add(ARCHIVE_RATIO_SLACK_BYTES);
    if total_uncompressed > ratio_budget {
        return Err(AppError::BadRequest(
            "Excel ZIP 展开倍率超过安全预算".to_string(),
        ));
    }
    Ok(())
}

pub fn validate_import_file(buffer: &[u8], file_name: &str) -> Result<(), AppError> {
    if buffer.is_empty() {
        return Err(AppError::BadRequest("上传文件为空".to_string()));
    }
    if buffer.len() > IMPORT_FILE_BYTES_MAX {
        return Err(AppError::BadRequest(format!(
            "导入文件超过 {} MiB 单文件上限",
            IMPORT_FILE_BYTES_MAX / 1024 / 1024
        )));
    }

    let file_name = file_name.trim();
    if file_name.is_empty() || file_name.len() > IMPORT_FILE_NAME_BYTES_MAX {
        return Err(AppError::BadRequest("上传文件名无效或过长".to_string()));
    }
    if file_name.chars().any(|ch| matches!(ch, '/' | '\\' | '\0'))
        || Path::new(file_name)
            .file_name()
            .and_then(|name| name.to_str())
            != Some(file_name)
    {
        return Err(AppError::BadRequest(
            "上传文件名不得包含路径信息".to_string(),
        ));
    }

    let ext = import_extension(file_name)
        .ok_or_else(|| AppError::BadRequest("导入文件缺少受支持的扩展名".to_string()))?;
    if !matches!(ext.as_str(), "xlsx" | "xls" | "xlsm" | "xlsb" | "csv") {
        return Err(AppError::BadRequest(
            "仅允许 xlsx/xls/xlsm/xlsb/csv 导入文件".to_string(),
        ));
    }
    if matches!(ext.as_str(), "xlsx" | "xlsm" | "xlsb") {
        validate_zip_expansion_budget(buffer)?;
    }
    Ok(())
}

pub fn has_excel_like_type(file_name: &str, mime_type: &str) -> bool {
    let ext = Path::new(file_name)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| {
            let with_dot = format!(".{}", e);
            with_dot.to_lowercase()
        })
        .unwrap_or_default();
    let mime_lower = mime_type.to_lowercase();

    EXCEL_EXTENSIONS.contains(&ext.as_str()) || EXCEL_MIME_TYPES.contains(&mime_lower.as_str())
}

struct TempImportFile(PathBuf);

impl TempImportFile {
    fn create(buffer: &[u8], extension: &str) -> Result<Self, AppError> {
        let path =
            std::env::temp_dir().join(format!("talos_import_{}.{}", Uuid::new_v4(), extension));
        std::fs::write(&path, buffer)
            .map_err(|e| AppError::BadRequest(format!("无法创建临时文件: {}", e)))?;
        Ok(Self(path))
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempImportFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

// ── Cell value normalization ──────────────────────────────────────

pub fn normalize_cell_value(value: &Data) -> String {
    match value {
        Data::Empty => String::new(),
        Data::String(s) => s.trim().to_string(),
        Data::Float(f) => {
            // Check if it's a whole number
            if f.fract() == 0.0 {
                format!("{}", *f as i64)
            } else {
                format!("{}", f)
            }
        }
        Data::Int(i) => format!("{}", i),
        Data::Bool(b) => {
            if *b {
                "TRUE".to_string()
            } else {
                "FALSE".to_string()
            }
        }
        Data::DateTime(d) => {
            format!("{}", d)
        }
        Data::Error(e) => {
            format!("#ERROR({})", e)
        }
        _ => String::new(),
    }
}

pub fn normalize_header_key(value: &str) -> String {
    value.trim_start_matches('\u{FEFF}').trim().to_string()
}

// ── Row helpers (for calamine Data rows) ──────────────────────────

/// Get a string value from a row (indexed by position) by trying multiple column header aliases.
pub fn get_field_value(headers: &[String], row: &[Data], keys: &[&str]) -> String {
    for key in keys {
        let normalized_key = normalize_header_key(key);
        let lower_key = normalized_key.to_lowercase();
        for (i, header) in headers.iter().enumerate() {
            let nh = normalize_header_key(header);
            if (nh == normalized_key || nh.to_lowercase() == lower_key)
                && let Some(val) = row.get(i)
            {
                return normalize_cell_value(val);
            }
        }
    }
    String::new()
}

/// Get the raw Data value from a row by multiple keys.
pub fn get_raw_field_value<'a>(
    headers: &[String],
    row: &'a [Data],
    keys: &[&str],
) -> Option<&'a Data> {
    for key in keys {
        let normalized_key = normalize_header_key(key);
        let lower_key = normalized_key.to_lowercase();
        for (i, header) in headers.iter().enumerate() {
            let nh = normalize_header_key(header);
            if nh == normalized_key || nh.to_lowercase() == lower_key {
                return row.get(i);
            }
        }
    }
    None
}

/// Get the raw Data value, or None (returns an Option).
pub fn get_raw_field_value_opt<'a>(
    headers: &[String],
    row: &'a [Data],
    keys: &[&str],
) -> Option<&'a Data> {
    get_raw_field_value(headers, row, keys)
}

// ── Parse Excel from buffer ───────────────────────────────────────

pub struct ParsedExcel {
    pub headers: Vec<String>,
    pub rows: Vec<Vec<Data>>,
}

/// Parse an Excel file from a byte buffer using calamine.
/// The input is admitted against explicit P7-B file/archive/workbook budgets
/// before rows are copied into application-owned vectors.
pub fn parse_excel_rows_from_buffer(
    buffer: &[u8],
    file_name: &str,
) -> Result<ParsedExcel, AppError> {
    validate_import_file(buffer, file_name)?;
    let ext = import_extension(file_name)
        .ok_or_else(|| AppError::BadRequest("导入文件扩展名无效".to_string()))?;

    let temp_file = TempImportFile::create(buffer, &ext)?;
    let mut workbook = open_workbook_auto(temp_file.path())
        .map_err(|e| AppError::BadRequest(format!("无法解析 Excel 文件: {}", e)))?;

    let sheet_names = workbook.sheet_names();
    if sheet_names.is_empty() {
        return Err(AppError::BadRequest("Excel 没有工作表".to_string()));
    }
    if sheet_names.len() > IMPORT_WORKBOOK_SHEETS_MAX {
        return Err(AppError::BadRequest(format!(
            "Excel 工作表数量超过上限 {IMPORT_WORKBOOK_SHEETS_MAX}"
        )));
    }

    let first_sheet = sheet_names[0].clone();
    let range = workbook
        .worksheet_range(&first_sheet)
        .map_err(|e| AppError::BadRequest(format!("无法读取工作表: {}", e)))?;

    let row_count = range.rows().count();
    let max_columns = range.rows().map(|row| row.len()).max().unwrap_or(0);
    let total_cells = row_count
        .checked_mul(max_columns)
        .ok_or_else(|| AppError::BadRequest("Excel 单元格数量溢出".to_string()))?;
    if row_count > IMPORT_WORKBOOK_ROWS_MAX {
        return Err(AppError::BadRequest(format!(
            "Excel 行数超过上限 {IMPORT_WORKBOOK_ROWS_MAX}（含表头）"
        )));
    }
    if max_columns > IMPORT_WORKBOOK_COLUMNS_MAX {
        return Err(AppError::BadRequest(format!(
            "Excel 列数超过上限 {IMPORT_WORKBOOK_COLUMNS_MAX}"
        )));
    }
    if total_cells > IMPORT_WORKBOOK_CELLS_MAX {
        return Err(AppError::BadRequest(format!(
            "Excel 单元格总量超过上限 {IMPORT_WORKBOOK_CELLS_MAX}"
        )));
    }
    if range.rows().flatten().any(
        |cell| matches!(cell, Data::String(value) if value.len() > IMPORT_CELL_STRING_BYTES_MAX),
    ) {
        return Err(AppError::BadRequest(format!(
            "Excel 单元格字符串超过 {} KiB 上限",
            IMPORT_CELL_STRING_BYTES_MAX / 1024
        )));
    }

    let mut rows_iter = range.rows();
    let header_row = match rows_iter.next() {
        Some(row) => row,
        None => {
            return Err(AppError::BadRequest("Excel 没有可导入数据".to_string()));
        }
    };

    let headers: Vec<String> = header_row.iter().map(normalize_cell_value).collect();
    let data_rows: Vec<Vec<Data>> = rows_iter.map(|row| row.to_vec()).collect();

    if data_rows.is_empty() {
        return Err(AppError::BadRequest("Excel 没有可导入数据".to_string()));
    }

    Ok(ParsedExcel {
        headers,
        rows: data_rows,
    })
}

// ── Header validation ─────────────────────────────────────────────

pub struct RequiredHeaderGroup {
    pub name: String,
    pub aliases: Vec<String>,
}

pub fn validate_header_groups(
    headers: &[String],
    required_groups: &[RequiredHeaderGroup],
) -> Result<(), AppError> {
    let header_set: std::collections::HashSet<String> =
        headers.iter().map(|h| h.trim().to_string()).collect();

    let mut missing = Vec::new();
    for group in required_groups {
        let found = group
            .aliases
            .iter()
            .any(|alias| header_set.contains(alias.as_str()));
        if !found {
            missing.push(group.name.clone());
        }
    }

    if !missing.is_empty() {
        return Err(AppError::BadRequest(format!(
            "Excel 缺少必需列: {}",
            missing.join("\u{3001}")
        )));
    }

    Ok(())
}

// ── Import summary ────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct ImportSummary {
    #[serde(alias = "successCount")]
    pub success_count: usize,
    #[serde(alias = "failCount")]
    pub fail_count: usize,
    #[serde(alias = "failedRows")]
    pub failed_rows: Vec<serde_json::Value>,
    #[serde(alias = "successRows", skip_serializing_if = "Option::is_none")]
    pub success_rows: Option<Vec<serde_json::Value>>,
}

pub fn create_import_summary(include_success_rows: bool) -> ImportSummary {
    ImportSummary {
        success_count: 0,
        fail_count: 0,
        failed_rows: Vec::new(),
        success_rows: if include_success_rows {
            Some(Vec::new())
        } else {
            None
        },
    }
}

pub fn push_import_failure(summary: &mut ImportSummary, row_info: serde_json::Value) {
    summary.fail_count += 1;
    summary.failed_rows.push(row_info);
}

pub fn push_import_success(summary: &mut ImportSummary, row_info: Option<serde_json::Value>) {
    summary.success_count += 1;
    if let Some(ref mut rows) = summary.success_rows
        && let Some(info) = row_info
    {
        rows.push(info);
    }
}

// ── Excel serial date normalization ───────────────────────────────

/// Convert an Excel serial date number (or date string) to YYYY-MM-DD.
/// Handles both Excel serial numbers and string dates like YYYY-MM-DD or YYYY/MM/DD.
pub fn normalize_business_date_from_data(value: &Data) -> Result<String, String> {
    match value {
        Data::Float(f) => {
            let n = *f;
            if n > 1.0 && n < 100000.0 {
                // Excel serial date: days since 1899-12-30
                let excel_epoch = chrono::NaiveDate::from_ymd_opt(1899, 12, 30).unwrap();
                let days = n.floor() as i64;
                let d = excel_epoch + chrono::Duration::days(days);
                return Ok(d.format("%Y-%m-%d").to_string());
            }
            Err(format!("无效 Excel 日期数字: {}", n))
        }
        Data::Int(i) => {
            let n = *i as f64;
            if n > 1.0 && n < 100000.0 {
                let excel_epoch = chrono::NaiveDate::from_ymd_opt(1899, 12, 30).unwrap();
                let days = n.floor() as i64;
                let d = excel_epoch + chrono::Duration::days(days);
                return Ok(d.format("%Y-%m-%d").to_string());
            }
            Err(format!("无效 Excel 日期数字: {}", i))
        }
        Data::String(s) => {
            let s = s.trim();
            if s.is_empty() {
                return Err("日期为空".to_string());
            }
            // Try YYYY-MM-DD or YYYY/MM/DD
            if let Ok(d) = chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d") {
                return Ok(d.format("%Y-%m-%d").to_string());
            }
            if let Ok(d) = chrono::NaiveDate::parse_from_str(s, "%Y/%m/%d") {
                return Ok(d.format("%Y-%m-%d").to_string());
            }
            // Try parsing as number
            if let Ok(n) = s.parse::<f64>()
                && n > 1.0
                && n < 100000.0
            {
                let excel_epoch = chrono::NaiveDate::from_ymd_opt(1899, 12, 30).unwrap();
                let days = n.floor() as i64;
                let d = excel_epoch + chrono::Duration::days(days);
                return Ok(d.format("%Y-%m-%d").to_string());
            }
            Err(format!("无效日期: {}", s))
        }
        Data::DateTime(d) => Ok(format!("{}", d)),
        _ => Err("日期格式不支持".to_string()),
    }
}

/// Convenience wrapper for normalizing dates from optional calamine Data.
pub fn normalize_optional_business_date(value: Option<&Data>) -> Result<String, String> {
    match value {
        Some(d) => normalize_business_date_from_data(d),
        None => Err("日期为空".to_string()),
    }
}

// ── Device Excel import parsing ───────────────────────────────────

const DEVICE_SERIAL_NO_KEYS: &[&str] = &[
    "serialNo",
    "设备序列号",
    "序列号",
    "serial",
    "sn",
    "SERIALNO",
];
const DEVICE_NOTES_KEYS: &[&str] = &["notes", "备注", "NOTES"];

fn is_valid_device_serial_no(value: &str) -> bool {
    !value.is_empty()
        && value
            .chars()
            .all(|character| character.is_ascii_alphanumeric())
}

#[derive(Debug, Clone)]
pub struct ParsedDeviceImportRow {
    pub row_no: usize,
    pub serial_no: String,
    pub notes: String,
}

#[derive(Debug, Clone)]
pub struct ParsedDeviceImportBatch {
    pub rows: Vec<ParsedDeviceImportRow>,
    pub failed_rows: Vec<serde_json::Value>,
}

/// Parse and validate the file-local portion of a device import.
///
/// Persistence ownership intentionally stays outside this parser. Tenant-scoped
/// duplicate checks and inserts are performed through the selected repository
/// provider by the application authority.
pub fn parse_device_import_rows(
    buffer: &[u8],
    file_name: &str,
) -> Result<ParsedDeviceImportBatch, AppError> {
    let parsed = parse_excel_rows_from_buffer(buffer, file_name)?;
    let required = vec![RequiredHeaderGroup {
        name: "serialNo".to_string(),
        aliases: DEVICE_SERIAL_NO_KEYS
            .iter()
            .map(|value| value.to_string())
            .collect(),
    }];
    validate_header_groups(&parsed.headers, &required)?;

    let mut file_seen = std::collections::HashSet::new();
    let mut rows = Vec::new();
    let mut failed_rows = Vec::new();

    for (index, row) in parsed.rows.iter().enumerate() {
        let row_no = index + 2;
        let serial_no = get_field_value(&parsed.headers, row, DEVICE_SERIAL_NO_KEYS);
        let notes = get_field_value(&parsed.headers, row, DEVICE_NOTES_KEYS);

        let error = if serial_no.is_empty() {
            Some("serialNo 不能为空")
        } else if !is_valid_device_serial_no(&serial_no) {
            Some("设备序列号只能包含英文字母和数字")
        } else if !file_seen.insert(serial_no.clone()) {
            Some("同一文件内 serialNo 重复")
        } else {
            None
        };

        if let Some(reason) = error {
            failed_rows.push(serde_json::json!({
                "rowNo": row_no,
                "serialNo": serial_no,
                "reason": reason,
            }));
            continue;
        }

        rows.push(ParsedDeviceImportRow {
            row_no,
            serial_no,
            notes,
        });
    }

    Ok(ParsedDeviceImportBatch { rows, failed_rows })
}

#[cfg(test)]
mod security_tests {
    use super::*;

    fn minimal_zip_central_entry(compressed: u32, uncompressed: u32) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(ZIP_CENTRAL_SIGNATURE);
        bytes.extend_from_slice(&[0; 4]); // versions
        bytes.extend_from_slice(&0u16.to_le_bytes()); // flags
        bytes.extend_from_slice(&0u16.to_le_bytes()); // compression
        bytes.extend_from_slice(&[0; 8]); // time/date/crc
        bytes.extend_from_slice(&compressed.to_le_bytes());
        bytes.extend_from_slice(&uncompressed.to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes()); // file name len
        bytes.extend_from_slice(&0u16.to_le_bytes()); // extra len
        bytes.extend_from_slice(&0u16.to_le_bytes()); // comment len
        bytes.extend_from_slice(&0u16.to_le_bytes()); // disk start
        bytes.extend_from_slice(&0u16.to_le_bytes()); // internal attrs
        bytes.extend_from_slice(&0u32.to_le_bytes()); // external attrs
        bytes.extend_from_slice(&0u32.to_le_bytes()); // local header offset
        bytes
    }

    fn zip_with_single_central_entry(compressed: u32, uncompressed: u32) -> Vec<u8> {
        let mut bytes = minimal_zip_central_entry(compressed, uncompressed);
        let central_size = bytes.len() as u32;
        bytes.extend_from_slice(ZIP_EOCD_SIGNATURE);
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&central_size.to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes
    }

    #[test]
    fn rejects_path_bearing_import_file_names() {
        assert!(validate_import_file(b"a,b\n1,2\n", "../orders.csv").is_err());
        assert!(validate_import_file(b"a,b\n1,2\n", "folder\\orders.csv").is_err());
    }

    #[test]
    fn rejects_unsupported_extensions() {
        assert!(validate_import_file(b"a,b\n1,2\n", "orders.txt").is_err());
    }

    #[test]
    fn rejects_archive_expansion_beyond_budget() {
        let zip = zip_with_single_central_entry(1024, 65 * 1024 * 1024);
        assert!(validate_import_file(&zip, "orders.xlsx").is_err());
    }

    #[test]
    fn accepts_small_csv_at_admission_layer() {
        assert!(validate_import_file(b"a,b\n1,2\n", "orders.csv").is_ok());
    }
}
