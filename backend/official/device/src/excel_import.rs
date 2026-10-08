//! feature-excel-import — Excel import module (Maxwell native)
//!
//! calamine parsing + device batch import
//! 4-layer pipeline: DeserializeGuard -> serde -> Sanitize -> Validate -> execute

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use system_core::*;

use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;

// ── Constants ──

const BASE64_CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

const EXCEL_EXTENSIONS: &[&str] = &[".xlsx", ".xls", ".xlsm", ".xlsb", ".csv"];

const EXCEL_MIME_TYPES: &[&str] = &[
    "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
    "application/vnd.ms-excel",
    "application/vnd.ms-excel.sheet.macroenabled.12",
    "application/vnd.ms-excel.sheet.binary.macroenabled.12",
    "text/csv",
    "application/csv",
];

const REGISTRY_EXCEL_BASE64_BYTES_MAX: usize = 8 * 1024 * 1024;
const REGISTRY_EXCEL_FILE_BYTES_MAX: usize = 6 * 1024 * 1024;
const REGISTRY_EXCEL_FILE_NAME_BYTES_MAX: usize = 180;
const REGISTRY_EXCEL_SHEETS_MAX: usize = 32;
const REGISTRY_EXCEL_ROWS_MAX: usize = 10_001;
const REGISTRY_EXCEL_COLUMNS_MAX: usize = 128;
const REGISTRY_EXCEL_CELLS_MAX: usize = 500_000;
const REGISTRY_EXCEL_CELL_STRING_BYTES_MAX: usize = 64 * 1024;
const REGISTRY_DEVICE_IMPORT_ROWS_MAX: usize = 2_000;

const ARCHIVE_ENTRIES_MAX: usize = 512;
const ARCHIVE_UNCOMPRESSED_BYTES_MAX: u64 = 128 * 1024 * 1024;
const ARCHIVE_ENTRY_UNCOMPRESSED_BYTES_MAX: u64 = 64 * 1024 * 1024;
const ARCHIVE_EXPANSION_RATIO_MAX: u64 = 80;
const ARCHIVE_RATIO_SLACK_BYTES: u64 = 16 * 1024 * 1024;
const ZIP_EOCD_SIGNATURE: &[u8] = b"PK\x05\x06";
const ZIP_CENTRAL_SIGNATURE: &[u8] = b"PK\x01\x02";

const DEVICE_SERIAL_NO_KEYS: &[&str] = &[
    "serialNo",
    "serial_no",
    "SerialNo",
    "SERIALNO",
    "serial",
    "sn",
    "SN",
];

const REQUIRED_VALIDATE_HEADERS: &[&[&str]] = &[DEVICE_SERIAL_NO_KEYS];

fn validation_error(code: &str, message: impl Into<String>, field: Option<&str>) -> String {
    serde_json::to_string(&ErrorPayload {
        category: "val".into(),
        code: code.into(),
        message: message.into(),
        field: field.map(str::to_string),
        context: None,
    })
    .unwrap_or_default()
}

fn registry_excel_guard() -> DeserializeGuard {
    DeserializeGuard {
        max_payload_bytes: REGISTRY_EXCEL_BASE64_BYTES_MAX + 4096,
        max_string_len: REGISTRY_EXCEL_BASE64_BYTES_MAX,
        ..DeserializeGuard::default()
    }
}

// ── Base64 decoder ──

pub fn base64_decode(input: &str) -> Result<Vec<u8>, String> {
    let input = input.trim();
    if input.is_empty() {
        return Ok(Vec::new());
    }
    if input.len() > REGISTRY_EXCEL_BASE64_BYTES_MAX {
        return Err(validation_error(
            "VAL_EXCEL_BASE64_TOO_LARGE",
            format!(
                "base64 Excel payload exceeds {} MiB",
                REGISTRY_EXCEL_BASE64_BYTES_MAX / 1024 / 1024
            ),
            Some("bufferBase64"),
        ));
    }

    let mut decode_table = [0xFFu8; 256];
    for (i, &c) in BASE64_CHARS.iter().enumerate() {
        decode_table[c as usize] = i as u8;
    }

    let capacity = input
        .len()
        .checked_mul(3)
        .and_then(|value| value.checked_div(4))
        .ok_or_else(|| {
            validation_error("VAL_EXCEL_TOO_LARGE", "Excel decode size overflow", None)
        })?;
    let mut output = Vec::with_capacity(capacity.min(REGISTRY_EXCEL_FILE_BYTES_MAX));
    let bytes = input.as_bytes();
    let mut i = 0;
    let mut buf = [0u8; 4];

    while i < bytes.len() {
        let mut valid = 0u8;
        for j in 0..4u8 {
            let idx = i + j as usize;
            if idx >= bytes.len() {
                break;
            }
            let c = bytes[idx];
            if c == b'=' {
                buf[j as usize] = 0;
                continue;
            }
            if c == b'\n' || c == b'\r' || c == b' ' {
                continue;
            }
            let val = decode_table[c as usize];
            if val == 0xFF {
                return Err(validation_error(
                    "VAL_BASE64",
                    format!("Base64 illegal char: '{}'", c as char),
                    Some("bufferBase64"),
                ));
            }
            buf[j as usize] = val;
            valid += 1;
        }

        if valid > 0 {
            output.push((buf[0] << 2) | (buf[1] >> 4));
            if valid > 2 {
                output.push((buf[1] << 4) | (buf[2] >> 2));
            }
            if valid > 3 {
                output.push((buf[2] << 6) | buf[3]);
            }
            if output.len() > REGISTRY_EXCEL_FILE_BYTES_MAX {
                return Err(validation_error(
                    "VAL_EXCEL_TOO_LARGE",
                    format!(
                        "decoded Excel file exceeds {} MiB",
                        REGISTRY_EXCEL_FILE_BYTES_MAX / 1024 / 1024
                    ),
                    Some("bufferBase64"),
                ));
            }
        }

        i += 4.min(bytes.len().saturating_sub(i));
    }

    Ok(output)
}

// ── Helper functions ──

pub fn has_excel_like_type(file_name: &str, mime_type: Option<&str>) -> bool {
    if let Some(ext) = file_name.rfind('.') {
        let ext_lower = file_name[ext..].to_lowercase();
        if EXCEL_EXTENSIONS.contains(&ext_lower.as_str()) {
            return true;
        }
    }
    if let Some(mt) = mime_type {
        let mt_lower = mt.to_lowercase();
        if EXCEL_MIME_TYPES.contains(&mt_lower.as_str()) {
            return true;
        }
    }
    false
}

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

fn validate_zip_expansion_budget(buffer: &[u8]) -> Result<(), String> {
    let eocd = find_zip_eocd(buffer).ok_or_else(|| {
        validation_error("VAL_EXCEL_ARCHIVE", "Excel ZIP directory missing", None)
    })?;
    let disk_no = read_u16_le(buffer, eocd + 4).ok_or_else(|| {
        validation_error("VAL_EXCEL_ARCHIVE", "Excel ZIP directory corrupt", None)
    })?;
    let central_disk = read_u16_le(buffer, eocd + 6).ok_or_else(|| {
        validation_error("VAL_EXCEL_ARCHIVE", "Excel ZIP directory corrupt", None)
    })?;
    let entries_on_disk = read_u16_le(buffer, eocd + 8).ok_or_else(|| {
        validation_error("VAL_EXCEL_ARCHIVE", "Excel ZIP directory corrupt", None)
    })?;
    let total_entries = read_u16_le(buffer, eocd + 10).ok_or_else(|| {
        validation_error("VAL_EXCEL_ARCHIVE", "Excel ZIP directory corrupt", None)
    })?;
    let central_size = read_u32_le(buffer, eocd + 12).ok_or_else(|| {
        validation_error("VAL_EXCEL_ARCHIVE", "Excel ZIP directory corrupt", None)
    })?;
    let central_offset = read_u32_le(buffer, eocd + 16).ok_or_else(|| {
        validation_error("VAL_EXCEL_ARCHIVE", "Excel ZIP directory corrupt", None)
    })?;
    let comment_len = read_u16_le(buffer, eocd + 20)
        .ok_or_else(|| validation_error("VAL_EXCEL_ARCHIVE", "Excel ZIP directory corrupt", None))?
        as usize;

    if disk_no != 0 || central_disk != 0 || entries_on_disk != total_entries {
        return Err(validation_error(
            "VAL_EXCEL_ARCHIVE",
            "multi-disk Excel ZIP is not supported",
            None,
        ));
    }
    if total_entries == u16::MAX || central_size == u32::MAX || central_offset == u32::MAX {
        return Err(validation_error(
            "VAL_EXCEL_ARCHIVE",
            "ZIP64 Excel import is not supported",
            None,
        ));
    }
    if total_entries as usize > ARCHIVE_ENTRIES_MAX {
        return Err(validation_error(
            "VAL_EXCEL_ARCHIVE_TOO_MANY_ENTRIES",
            format!("Excel ZIP entries exceed {ARCHIVE_ENTRIES_MAX}"),
            None,
        ));
    }

    let eocd_end = eocd
        .checked_add(22)
        .and_then(|value| value.checked_add(comment_len))
        .ok_or_else(|| validation_error("VAL_EXCEL_ARCHIVE", "Excel ZIP length overflow", None))?;
    if eocd_end > buffer.len() {
        return Err(validation_error(
            "VAL_EXCEL_ARCHIVE",
            "Excel ZIP comment length is invalid",
            None,
        ));
    }

    let central_offset = central_offset as usize;
    let central_end = central_offset
        .checked_add(central_size as usize)
        .ok_or_else(|| {
            validation_error("VAL_EXCEL_ARCHIVE", "Excel ZIP directory overflow", None)
        })?;
    if central_end > eocd || central_end > buffer.len() {
        return Err(validation_error(
            "VAL_EXCEL_ARCHIVE",
            "Excel ZIP directory range is invalid",
            None,
        ));
    }

    let mut cursor = central_offset;
    let mut total_compressed = 0u64;
    let mut total_uncompressed = 0u64;
    for _ in 0..total_entries {
        if buffer.get(cursor..cursor.saturating_add(4)) != Some(ZIP_CENTRAL_SIGNATURE) {
            return Err(validation_error(
                "VAL_EXCEL_ARCHIVE",
                "Excel ZIP central-directory entry is invalid",
                None,
            ));
        }
        let flags = read_u16_le(buffer, cursor + 8).ok_or_else(|| {
            validation_error("VAL_EXCEL_ARCHIVE", "Excel ZIP entry corrupt", None)
        })?;
        if flags & 0x0001 != 0 {
            return Err(validation_error(
                "VAL_EXCEL_ARCHIVE",
                "encrypted Excel ZIP is not supported",
                None,
            ));
        }
        let compressed = read_u32_le(buffer, cursor + 20).ok_or_else(|| {
            validation_error("VAL_EXCEL_ARCHIVE", "Excel ZIP entry corrupt", None)
        })?;
        let uncompressed = read_u32_le(buffer, cursor + 24).ok_or_else(|| {
            validation_error("VAL_EXCEL_ARCHIVE", "Excel ZIP entry corrupt", None)
        })?;
        if compressed == u32::MAX || uncompressed == u32::MAX {
            return Err(validation_error(
                "VAL_EXCEL_ARCHIVE",
                "ZIP64 Excel entry is not supported",
                None,
            ));
        }
        if uncompressed as u64 > ARCHIVE_ENTRY_UNCOMPRESSED_BYTES_MAX {
            return Err(validation_error(
                "VAL_EXCEL_ARCHIVE_TOO_LARGE",
                format!(
                    "Excel ZIP entry expands beyond {} MiB",
                    ARCHIVE_ENTRY_UNCOMPRESSED_BYTES_MAX / 1024 / 1024
                ),
                None,
            ));
        }
        total_compressed = total_compressed.saturating_add(compressed as u64);
        total_uncompressed = total_uncompressed.saturating_add(uncompressed as u64);
        if total_uncompressed > ARCHIVE_UNCOMPRESSED_BYTES_MAX {
            return Err(validation_error(
                "VAL_EXCEL_ARCHIVE_TOO_LARGE",
                format!(
                    "Excel ZIP expands beyond {} MiB",
                    ARCHIVE_UNCOMPRESSED_BYTES_MAX / 1024 / 1024
                ),
                None,
            ));
        }

        let file_name_len = read_u16_le(buffer, cursor + 28)
            .ok_or_else(|| validation_error("VAL_EXCEL_ARCHIVE", "Excel ZIP entry corrupt", None))?
            as usize;
        let extra_len = read_u16_le(buffer, cursor + 30)
            .ok_or_else(|| validation_error("VAL_EXCEL_ARCHIVE", "Excel ZIP entry corrupt", None))?
            as usize;
        let entry_comment_len = read_u16_le(buffer, cursor + 32)
            .ok_or_else(|| validation_error("VAL_EXCEL_ARCHIVE", "Excel ZIP entry corrupt", None))?
            as usize;
        cursor = cursor
            .checked_add(46)
            .and_then(|value| value.checked_add(file_name_len))
            .and_then(|value| value.checked_add(extra_len))
            .and_then(|value| value.checked_add(entry_comment_len))
            .ok_or_else(|| {
                validation_error("VAL_EXCEL_ARCHIVE", "Excel ZIP entry overflow", None)
            })?;
        if cursor > central_end {
            return Err(validation_error(
                "VAL_EXCEL_ARCHIVE",
                "Excel ZIP entry exceeds central directory",
                None,
            ));
        }
    }

    if total_uncompressed > 0 && total_compressed == 0 {
        return Err(validation_error(
            "VAL_EXCEL_ARCHIVE",
            "Excel ZIP compressed size is invalid",
            None,
        ));
    }
    let ratio_budget = total_compressed
        .saturating_mul(ARCHIVE_EXPANSION_RATIO_MAX)
        .saturating_add(ARCHIVE_RATIO_SLACK_BYTES);
    if total_uncompressed > ratio_budget {
        return Err(validation_error(
            "VAL_EXCEL_ARCHIVE_RATIO",
            "Excel ZIP expansion ratio exceeds security budget",
            None,
        ));
    }
    Ok(())
}

fn validate_excel_buffer(buffer: &[u8], file_name: &str) -> Result<String, String> {
    if buffer.is_empty() {
        return Err(validation_error(
            "VAL_EXCEL_EMPTY",
            "Excel content is empty",
            Some("bufferBase64"),
        ));
    }
    if buffer.len() > REGISTRY_EXCEL_FILE_BYTES_MAX {
        return Err(validation_error(
            "VAL_EXCEL_TOO_LARGE",
            format!(
                "decoded Excel file exceeds {} MiB",
                REGISTRY_EXCEL_FILE_BYTES_MAX / 1024 / 1024
            ),
            Some("bufferBase64"),
        ));
    }

    let file_name = file_name.trim();
    if file_name.is_empty() || file_name.len() > REGISTRY_EXCEL_FILE_NAME_BYTES_MAX {
        return Err(validation_error(
            "VAL_EXCEL_FILE_NAME",
            "Excel file name is empty or too long",
            Some("fileName"),
        ));
    }
    if file_name.chars().any(|ch| matches!(ch, '/' | '\\' | '\0'))
        || Path::new(file_name)
            .file_name()
            .and_then(|name| name.to_str())
            != Some(file_name)
    {
        return Err(validation_error(
            "VAL_PATH_TRAVERSAL",
            "fileName must not contain path information",
            Some("fileName"),
        ));
    }

    let extension = import_extension(file_name).ok_or_else(|| {
        validation_error(
            "VAL_EXCEL_FILE_NAME",
            "Excel file name is missing an extension",
            Some("fileName"),
        )
    })?;
    if !matches!(extension.as_str(), "xlsx" | "xls" | "xlsm" | "xlsb" | "csv") {
        return Err(validation_error(
            "VAL_FORMAT",
            "Only Excel files supported (.xlsx/.xls/.xlsm/.xlsb/.csv)",
            Some("fileName"),
        ));
    }
    if matches!(extension.as_str(), "xlsx" | "xlsm" | "xlsb") {
        validate_zip_expansion_budget(buffer)?;
    }
    Ok(extension)
}

struct TempExcelFile(PathBuf);

impl TempExcelFile {
    fn create(buffer: &[u8], extension: &str) -> Result<Self, String> {
        let path = std::env::temp_dir().join(format!(
            "talos_registry_excel_{}.{}",
            uuid::Uuid::new_v4(),
            extension
        ));
        std::fs::write(&path, buffer).map_err(|e| {
            serde_json::to_string(&ErrorPayload {
                category: "sys".into(),
                code: "SYS_FILE_WRITE".into(),
                message: format!("Failed to write temp file: {}", e),
                field: None,
                context: None,
            })
            .unwrap_or_default()
        })?;
        Ok(Self(path))
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempExcelFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn normalize_cell_value(data: &calamine::Data) -> String {
    match data {
        calamine::Data::Empty => String::new(),
        calamine::Data::String(s) => s.trim().to_string(),
        calamine::Data::Float(f) => {
            if f.fract() == 0.0_f64 {
                format!("{}", *f as i64)
            } else {
                format!("{}", f)
            }
        }
        calamine::Data::Int(i) => format!("{}", i),
        calamine::Data::Bool(b) => format!("{}", b),
        calamine::Data::Error(e) => format!("#ERROR({:?})", e),
        calamine::Data::DateTime(dt) => format!("{}", dt),
        calamine::Data::DateTimeIso(s) => s.clone(),
        calamine::Data::DurationIso(s) => s.clone(),
    }
}

fn normalize_header_key(value: &str) -> String {
    value.trim_start_matches('\u{FEFF}').trim().to_string()
}

pub fn get_field_value(headers: &[String], row: &[String], keys: &[&str]) -> String {
    let key_set: HashSet<&str> = keys.iter().copied().collect();
    for (i, header) in headers.iter().enumerate() {
        if i < row.len() {
            let normalized = normalize_header_key(header);
            if key_set.contains(normalized.as_str()) {
                return row[i].clone();
            }
            if key_set.contains(&normalized.to_lowercase().as_str()) {
                return row[i].clone();
            }
        }
    }

    let lower_keys: Vec<String> = keys.iter().map(|k| k.to_lowercase()).collect();
    for (i, header) in headers.iter().enumerate() {
        if i < row.len() {
            let normalized_lower = normalize_header_key(header).to_lowercase();
            if lower_keys.contains(&normalized_lower) {
                return row[i].clone();
            }
        }
    }

    String::new()
}

fn parse_excel_rows_from_buffer(
    buffer: &[u8],
    file_name: &str,
) -> Result<(Vec<String>, Vec<Vec<String>>), String> {
    let extension = validate_excel_buffer(buffer, file_name)?;
    let temp_file = TempExcelFile::create(buffer, &extension)?;
    parse_excel_file(temp_file.path())
}

fn parse_excel_file<P: AsRef<Path>>(path: P) -> Result<(Vec<String>, Vec<Vec<String>>), String> {
    use calamine::{Reader, open_workbook_auto};

    let mut workbook = open_workbook_auto(path).map_err(|e| {
        serde_json::to_string(&ErrorPayload {
            category: "val".into(),
            code: "VAL_EXCEL_PARSE".into(),
            message: format!("Cannot open Excel file: {}", e),
            field: None,
            context: None,
        })
        .unwrap_or_default()
    })?;

    let sheet_names = workbook.sheet_names().to_vec();
    if sheet_names.is_empty() {
        return Err(validation_error(
            "VAL_EXCEL_EMPTY",
            "Excel has no sheets",
            None,
        ));
    }
    if sheet_names.len() > REGISTRY_EXCEL_SHEETS_MAX {
        return Err(validation_error(
            "VAL_EXCEL_TOO_MANY_SHEETS",
            format!("Excel sheets exceed {REGISTRY_EXCEL_SHEETS_MAX}"),
            None,
        ));
    }

    let first_sheet = &sheet_names[0];
    let range = workbook.worksheet_range(first_sheet).map_err(|e| {
        serde_json::to_string(&ErrorPayload {
            category: "val".into(),
            code: "VAL_EXCEL_PARSE".into(),
            message: format!("Cannot read sheet '{}': {}", first_sheet, e),
            field: None,
            context: None,
        })
        .unwrap_or_default()
    })?;

    let row_count = range.rows().count();
    let max_columns = range.rows().map(|row| row.len()).max().unwrap_or(0);
    let total_cells = row_count.checked_mul(max_columns).ok_or_else(|| {
        validation_error("VAL_EXCEL_TOO_LARGE", "Excel cell count overflow", None)
    })?;
    if row_count > REGISTRY_EXCEL_ROWS_MAX {
        return Err(validation_error(
            "VAL_EXCEL_TOO_MANY_ROWS",
            format!("Excel rows exceed {REGISTRY_EXCEL_ROWS_MAX}"),
            None,
        ));
    }
    if max_columns > REGISTRY_EXCEL_COLUMNS_MAX {
        return Err(validation_error(
            "VAL_EXCEL_TOO_MANY_COLUMNS",
            format!("Excel columns exceed {REGISTRY_EXCEL_COLUMNS_MAX}"),
            None,
        ));
    }
    if total_cells > REGISTRY_EXCEL_CELLS_MAX {
        return Err(validation_error(
            "VAL_EXCEL_TOO_MANY_CELLS",
            format!("Excel cells exceed {REGISTRY_EXCEL_CELLS_MAX}"),
            None,
        ));
    }
    if range.rows().flatten().any(|cell| {
        matches!(cell, calamine::Data::String(value) if value.len() > REGISTRY_EXCEL_CELL_STRING_BYTES_MAX)
    }) {
        return Err(validation_error(
            "VAL_EXCEL_CELL_TOO_LARGE",
            format!(
                "Excel cell string exceeds {} KiB",
                REGISTRY_EXCEL_CELL_STRING_BYTES_MAX / 1024
            ),
            None,
        ));
    }

    let mut rows_iter = range.rows();
    let headers: Vec<String>;
    let mut data_rows: Vec<Vec<String>> = Vec::new();

    if let Some(header_row) = rows_iter.next() {
        headers = header_row
            .iter()
            .map(normalize_cell_value)
            .collect::<Vec<String>>();
    } else {
        return Err(validation_error(
            "VAL_EXCEL_EMPTY",
            "Excel has no data",
            None,
        ));
    }

    for row in rows_iter {
        let cells: Vec<String> = row
            .iter()
            .map(normalize_cell_value)
            .collect::<Vec<String>>();
        if cells.iter().all(|c: &String| c.is_empty()) {
            continue;
        }
        data_rows.push(cells);
    }

    Ok((headers, data_rows))
}

// ── Input types (camelCase, Validate + Sanitize) ──

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ParseExcelInput {
    pub file_name: String,
    pub buffer_base64: String,
}

impl Sanitize for ParseExcelInput {
    fn sanitize(&mut self) {
        self.file_name = self.file_name.trim().to_string();
        self.buffer_base64 = self.buffer_base64.trim().to_string();
    }
}

impl Validate for ParseExcelInput {
    fn validate(&self) -> ValidationResult {
        let mut errors = Vec::new();
        if self.file_name.is_empty() {
            errors.push(FieldError {
                field: "fileName".into(),
                code: "VAL_REQUIRED".into(),
                message: "file name is required".into(),
            });
        } else if self.file_name.len() > REGISTRY_EXCEL_FILE_NAME_BYTES_MAX
            || self
                .file_name
                .chars()
                .any(|ch| matches!(ch, '/' | '\\' | '\0'))
            || Path::new(&self.file_name)
                .file_name()
                .and_then(|name| name.to_str())
                != Some(self.file_name.as_str())
        {
            errors.push(FieldError {
                field: "fileName".into(),
                code: "VAL_PATH_TRAVERSAL".into(),
                message: "fileName must be a bounded basename without path information".into(),
            });
        }
        if self.buffer_base64.is_empty() {
            errors.push(FieldError {
                field: "bufferBase64".into(),
                code: "VAL_REQUIRED".into(),
                message: "Excel content is required".into(),
            });
        } else if self.buffer_base64.len() > REGISTRY_EXCEL_BASE64_BYTES_MAX {
            errors.push(FieldError {
                field: "bufferBase64".into(),
                code: "VAL_EXCEL_BASE64_TOO_LARGE".into(),
                message: format!(
                    "base64 Excel payload exceeds {} MiB",
                    REGISTRY_EXCEL_BASE64_BYTES_MAX / 1024 / 1024
                ),
            });
        }
        if !self.file_name.is_empty()
            && !self.buffer_base64.is_empty()
            && !has_excel_like_type(&self.file_name, None)
        {
            errors.push(FieldError {
                field: "fileName".into(),
                code: "VAL_FORMAT".into(),
                message: "Only Excel files supported (.xlsx/.xls/.xlsm/.xlsb/.csv)".into(),
            });
        }
        ValidationResult { errors }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ImportDevicesInput {
    pub rows: Vec<DeviceImportRow>,
}

impl Sanitize for ImportDevicesInput {
    fn sanitize(&mut self) {
        for row in &mut self.rows {
            row.sanitize();
        }
    }
}

impl Validate for ImportDevicesInput {
    fn validate(&self) -> ValidationResult {
        let mut errors = Vec::new();
        if self.rows.is_empty() {
            errors.push(FieldError {
                field: "rows".into(),
                code: "VAL_REQUIRED".into(),
                message: "import rows cannot be empty".into(),
            });
            return ValidationResult { errors };
        }
        if self.rows.len() > REGISTRY_DEVICE_IMPORT_ROWS_MAX {
            errors.push(FieldError {
                field: "rows".into(),
                code: "VAL_TOO_MANY_ITEMS".into(),
                message: format!("device import rows exceed {REGISTRY_DEVICE_IMPORT_ROWS_MAX}"),
            });
            return ValidationResult { errors };
        }
        for (i, row) in self.rows.iter().enumerate() {
            let row_errors = row.validate().errors;
            for e in row_errors {
                errors.push(FieldError {
                    field: format!("rows[{}].{}", i, e.field),
                    code: e.code,
                    message: e.message,
                });
            }
        }
        ValidationResult { errors }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DeviceImportRow {
    pub serial_no: String,
    pub model_name: Option<String>,
    pub warehouse_name: Option<String>,
    pub status: Option<String>,
    pub notes: Option<String>,
}

impl Sanitize for DeviceImportRow {
    fn sanitize(&mut self) {
        self.serial_no = self.serial_no.trim().to_uppercase();
        self.serial_no.retain(|c| !c.is_control());
        if let Some(ref mut s) = self.model_name {
            *s = s.trim().to_string();
        }
        if let Some(ref mut s) = self.warehouse_name {
            *s = s.trim().to_string();
        }
        if let Some(ref mut s) = self.status {
            *s = s.trim().to_string().to_lowercase();
        }
        if let Some(ref mut s) = self.notes {
            *s = s.trim().to_string();
        }
    }
}

impl Validate for DeviceImportRow {
    fn validate(&self) -> ValidationResult {
        let mut errors = Vec::new();
        if self.serial_no.is_empty() {
            errors.push(FieldError {
                field: "serialNo".into(),
                code: "VAL_REQUIRED".into(),
                message: "serialNo is required".into(),
            });
        }
        if !self.serial_no.is_empty() && !self.serial_no.chars().all(|c| c.is_ascii_alphanumeric())
        {
            errors.push(FieldError {
                field: "serialNo".into(),
                code: "VAL_FORMAT".into(),
                message: "serialNo must be alphanumeric".into(),
            });
        }
        ValidationResult { errors }
    }
}

// ── Output types (JsonSchema) ──

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ParsedExcelOutput {
    pub headers: Vec<String>,
    pub rows: Vec<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ImportResult {
    pub total: usize,
    pub created: usize,
    pub skipped: usize,
    pub errors: Vec<ImportError>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ImportError {
    pub row_index: usize,
    pub serial_no: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ValidateExcelOutput {
    pub valid: bool,
    pub row_count: usize,
    pub headers: Vec<String>,
    pub warnings: Vec<String>,
}

// ── Module ──

pub struct FeatureExcelImport {
    pub pool: Mutex<Option<Pool<SqliteConnectionManager>>>,
    pub device_module: Mutex<Option<Arc<dyn SystemModule>>>,
}

impl FeatureExcelImport {
    pub fn new() -> Self {
        Self {
            pool: Mutex::new(None),
            device_module: Mutex::new(None),
        }
    }

    fn get_conn(&self) -> Result<r2d2::PooledConnection<SqliteConnectionManager>, String> {
        let guard = self.pool.lock().map_err(|e| {
            serde_json::to_string(&ErrorPayload {
                category: "sys".into(),
                code: "SYS_LOCK".into(),
                message: format!("pool lock failed: {}", e),
                field: None,
                context: None,
            })
            .unwrap_or_default()
        })?;
        let pool = guard.as_ref().ok_or_else(|| {
            serde_json::to_string(&ErrorPayload {
                category: "sys".into(),
                code: "SYS_NO_DB".into(),
                message: "database not initialized, call init() first".into(),
                field: None,
                context: None,
            })
            .unwrap_or_default()
        })?;
        pool.get().map_err(|e| {
            serde_json::to_string(&ErrorPayload {
                category: "sys".into(),
                code: "SYS_DB_CONN".into(),
                message: format!("connection failed: {}", e),
                field: None,
                context: None,
            })
            .unwrap_or_default()
        })
    }

    // ── Business methods ──

    fn do_parse_excel(&self, input: &ParseExcelInput) -> Result<ParsedExcelOutput, String> {
        let buffer = base64_decode(&input.buffer_base64)?;
        let (headers, rows) = parse_excel_rows_from_buffer(&buffer, &input.file_name)?;

        if rows.is_empty() {
            return Err(validation_error(
                "VAL_EXCEL_EMPTY",
                "Excel has no importable data",
                None,
            ));
        }

        Ok(ParsedExcelOutput { headers, rows })
    }

    fn do_validate_excel(&self, input: &ParseExcelInput) -> Result<ValidateExcelOutput, String> {
        let parsed = self.do_parse_excel(input)?;
        let mut warnings: Vec<String> = Vec::new();

        for group in REQUIRED_VALIDATE_HEADERS {
            let found = group.iter().any(|alias| {
                parsed
                    .headers
                    .iter()
                    .any(|h| normalize_header_key(h) == *alias)
            });
            if !found {
                let names: Vec<&str> = group.to_vec();
                warnings.push(format!(
                    "Missing required column: {} (aliases: {})",
                    names[0],
                    names.join(", ")
                ));
            }
        }

        let valid = warnings.is_empty();

        Ok(ValidateExcelOutput {
            valid,
            row_count: parsed.rows.len(),
            headers: parsed.headers,
            warnings,
        })
    }

    fn do_import_devices(
        &self,
        input: &ImportDevicesInput,
        ctx: &ExecutionContext,
    ) -> Result<ImportResult, String> {
        let total = input.rows.len();
        let mut created = 0usize;
        let mut skipped = 0usize;
        let mut import_errors: Vec<ImportError> = Vec::new();
        let mut file_seen: HashSet<String> = HashSet::new();

        let device_module = {
            let guard = self.device_module.lock().map_err(|e| {
                serde_json::to_string(&ErrorPayload {
                    category: "sys".into(),
                    code: "SYS_LOCK".into(),
                    message: format!("device_module lock failed: {}", e),
                    field: None,
                    context: None,
                })
                .unwrap_or_default()
            })?;
            guard.clone().ok_or_else(|| {
                serde_json::to_string(&ErrorPayload {
                    category: "sys".into(),
                    code: "SYS_MODULE_MISSING".into(),
                    message: "device_module not injected, call set_device_module() first".into(),
                    field: None,
                    context: None,
                })
                .unwrap_or_default()
            })?
        };

        for (idx, row) in input.rows.iter().enumerate() {
            let serial_no = &row.serial_no;

            if file_seen.contains(serial_no.as_str()) {
                skipped += 1;
                import_errors.push(ImportError {
                    row_index: idx + 1,
                    serial_no: serial_no.clone(),
                    reason: "duplicate serialNo within file".into(),
                });
                continue;
            }

            let exists_payload = serde_json::json!({
                "serialNo": serial_no,
            });
            let device_result = device_module.execute("get_device", exists_payload, ctx);
            if let Ok(device_val) = device_result
                && !device_val.is_null()
            {
                skipped += 1;
                import_errors.push(ImportError {
                    row_index: idx + 1,
                    serial_no: serial_no.clone(),
                    reason: "device already exists".into(),
                });
                continue;
            }

            file_seen.insert(serial_no.clone());

            let model_id = if let Some(ref model_name) = row.model_name {
                if !model_name.is_empty() {
                    self.lookup_model_id_by_name(ctx.data_scope(), model_name)?
                } else {
                    None
                }
            } else {
                None
            };

            let warehouse_id = if let Some(ref warehouse_name) = row.warehouse_name {
                if !warehouse_name.is_empty() {
                    self.lookup_warehouse_id_by_name(ctx.data_scope(), warehouse_name)?
                } else {
                    None
                }
            } else {
                None
            };

            let create_payload = {
                let mut m = HashMap::new();
                m.insert(
                    "serialNo",
                    serde_json::to_value(serial_no).unwrap_or_default(),
                );
                m.insert(
                    "modelId",
                    serde_json::to_value(model_id.unwrap_or_default()).unwrap_or_default(),
                );
                m.insert(
                    "warehouseId",
                    serde_json::to_value(warehouse_id.unwrap_or_default()).unwrap_or_default(),
                );
                if let Some(ref status) = row.status {
                    m.insert("status", serde_json::to_value(status).unwrap_or_default());
                } else {
                    m.insert(
                        "status",
                        serde_json::to_value("available").unwrap_or_default(),
                    );
                }
                serde_json::to_value(m).unwrap_or_default()
            };

            match device_module.execute("create_device", create_payload, ctx) {
                Ok(_) => {
                    created += 1;
                }
                Err(e) => {
                    skipped += 1;
                    import_errors.push(ImportError {
                        row_index: idx + 1,
                        serial_no: serial_no.clone(),
                        reason: format!("create failed: {}", e),
                    });
                }
            }
        }

        Ok(ImportResult {
            total,
            created,
            skipped,
            errors: import_errors,
        })
    }

    fn lookup_model_id_by_name(
        &self,
        scope: &DataScope,
        model_name: &str,
    ) -> Result<Option<String>, String> {
        let conn = self.get_conn()?;
        let mut stmt = conn
            .prepare("SELECT id FROM device_models WHERE name = ?1 AND tenant_id = ?2 LIMIT 1")
            .map_err(|e| {
                serde_json::to_string(&ErrorPayload {
                    category: "sys".into(),
                    code: "SYS_DB_QUERY".into(),
                    message: format!("model lookup failed: {}", e),
                    field: None,
                    context: None,
                })
                .unwrap_or_default()
            })?;
        let model_id: Option<String> = stmt
            .query_row(
                rusqlite::params![model_name, scope.tenant_id().as_str()],
                |row| row.get(0),
            )
            .ok();
        Ok(model_id)
    }

    fn lookup_warehouse_id_by_name(
        &self,
        scope: &DataScope,
        warehouse_name: &str,
    ) -> Result<Option<String>, String> {
        let conn = self.get_conn()?;
        let mut stmt = conn
            .prepare("SELECT id FROM warehouses WHERE name = ?1 AND tenant_id = ?2 LIMIT 1")
            .map_err(|e| {
                serde_json::to_string(&ErrorPayload {
                    category: "sys".into(),
                    code: "SYS_DB_QUERY".into(),
                    message: format!("warehouse lookup failed: {}", e),
                    field: None,
                    context: None,
                })
                .unwrap_or_default()
            })?;
        let warehouse_id: Option<String> = stmt
            .query_row(
                rusqlite::params![warehouse_name, scope.tenant_id().as_str()],
                |row| row.get(0),
            )
            .ok();
        Ok(warehouse_id)
    }

    pub fn set_device_module(&self, module: Arc<dyn SystemModule>) -> Result<(), String> {
        let mut guard = self.device_module.lock().map_err(|e| {
            serde_json::to_string(&ErrorPayload {
                category: "sys".into(),
                code: "SYS_LOCK".into(),
                message: format!("device_module lock failed: {}", e),
                field: None,
                context: None,
            })
            .unwrap_or_default()
        })?;
        *guard = Some(module);
        Ok(())
    }
}

impl Default for FeatureExcelImport {
    fn default() -> Self {
        Self::new()
    }
}

impl SystemModule for FeatureExcelImport {
    fn metadata(&self) -> ModuleMetadata {
        ModuleMetadata {
            name: "feature-excel-import".into(),
            version: "0.1.0".into(),
            description: "Excel import — calamine parsing + device batch import".into(),
            author: "Maxwell".into(),
            wasm_compatible: false,
            storage: Some("required".into()),
        }
    }

    fn commands(&self) -> Vec<CommandMetadata> {
        vec![
            CommandMetadata::new(
                "parse_excel",
                AccessRequirement::Authenticated,
                &[EffectClass::DatabaseRead],
                SimulationSupport::Supported,
            ),
            CommandMetadata::new(
                "validate_excel",
                AccessRequirement::Authenticated,
                &[EffectClass::DatabaseRead],
                SimulationSupport::Supported,
            ),
            CommandMetadata::new(
                "import_devices",
                AccessRequirement::TenantAdmin,
                &[EffectClass::DatabaseWrite],
                SimulationSupport::Supported,
            ),
        ]
    }

    fn init(&mut self, config: Value) -> Result<(), String> {
        let database_url = config
            .get("databaseUrl")
            .and_then(|v| v.as_str())
            .unwrap_or(":memory:");
        let manager = SqliteConnectionManager::file(database_url);
        let pool = Pool::builder().max_size(4).build(manager).map_err(|e| {
            serde_json::to_string(&ErrorPayload {
                category: "sys".into(),
                code: "SYS_DB_POOL".into(),
                message: format!("pool creation failed: {}", e),
                field: None,
                context: None,
            })
            .unwrap_or_default()
        })?;
        let mut guard = self.pool.lock().map_err(|e| {
            serde_json::to_string(&ErrorPayload {
                category: "sys".into(),
                code: "SYS_LOCK".into(),
                message: format!("pool lock failed: {}", e),
                field: None,
                context: None,
            })
            .unwrap_or_default()
        })?;
        *guard = Some(pool);
        Ok(())
    }

    fn execute(
        &self,
        command: &str,
        payload: Value,
        ctx: &ExecutionContext,
    ) -> Result<Value, String> {
        match command {
            "parse_excel" => {
                registry_excel_guard().check_raw(&payload)?;
                let unvalidated: Unvalidated<ParseExcelInput> = payload.try_into()?;
                let sanitized = unvalidated.sanitize();
                let validated = sanitized.validate()?;
                let input = validated.into_inner();
                let result = self.do_parse_excel(&input)?;
                serde_json::to_value(result).map_err(|e| {
                    serde_json::to_string(&ErrorPayload {
                        category: "sys".into(),
                        code: "SYS_SERIALIZE".into(),
                        message: e.to_string(),
                        field: None,
                        context: None,
                    })
                    .unwrap_or_default()
                })
            }
            "validate_excel" => {
                registry_excel_guard().check_raw(&payload)?;
                let unvalidated: Unvalidated<ParseExcelInput> = payload.try_into()?;
                let sanitized = unvalidated.sanitize();
                let validated = sanitized.validate()?;
                let input = validated.into_inner();
                let result = self.do_validate_excel(&input)?;
                serde_json::to_value(result).map_err(|e| {
                    serde_json::to_string(&ErrorPayload {
                        category: "sys".into(),
                        code: "SYS_SERIALIZE".into(),
                        message: e.to_string(),
                        field: None,
                        context: None,
                    })
                    .unwrap_or_default()
                })
            }
            "import_devices" => {
                DeserializeGuard::default().check_raw(&payload)?;
                let unvalidated: Unvalidated<ImportDevicesInput> = payload.try_into()?;
                let sanitized = unvalidated.sanitize();
                let validated = sanitized.validate()?;
                let input = validated.into_inner();
                let result = self.do_import_devices(&input, ctx)?;
                serde_json::to_value(result).map_err(|e| {
                    serde_json::to_string(&ErrorPayload {
                        category: "sys".into(),
                        code: "SYS_SERIALIZE".into(),
                        message: e.to_string(),
                        field: None,
                        context: None,
                    })
                    .unwrap_or_default()
                })
            }
            _ => Err(serde_json::to_string(&ErrorPayload {
                category: "sys".into(),
                code: "SYS_UNKNOWN_COMMAND".into(),
                message: format!("unknown command: {}", command),
                field: None,
                context: None,
            })
            .unwrap_or_default()),
        }
    }

    fn schema(&self) -> ModuleSchema {
        ModuleSchema {
            name: "feature-excel-import".into(),
            description: "Excel import module — calamine parsing + device batch import".into(),
            commands: vec![
                CommandSchema {
                    name: "parse_excel".into(),
                    description: "Parse Excel file, returns headers and data rows".into(),
                    version: "1.0.0".into(),
                    input_schema: None,
                    output_schema: None,
                },
                CommandSchema {
                    name: "validate_excel".into(),
                    description: "Parse Excel and validate headers contain required fields".into(),
                    version: "1.0.0".into(),
                    input_schema: None,
                    output_schema: None,
                },
                CommandSchema {
                    name: "import_devices".into(),
                    description: "Batch import devices via device_module.create_device".into(),
                    version: "1.0.0".into(),
                    input_schema: None,
                    output_schema: None,
                },
            ],
        }
    }
}

#[cfg(test)]
mod security_tests {
    use super::*;

    fn minimal_zip_central_entry(compressed: u32, uncompressed: u32) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(ZIP_CENTRAL_SIGNATURE);
        bytes.extend_from_slice(&[0; 4]);
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes.extend_from_slice(&[0; 8]);
        bytes.extend_from_slice(&compressed.to_le_bytes());
        bytes.extend_from_slice(&uncompressed.to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());
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
    fn rejects_path_bearing_registry_excel_name() {
        assert!(validate_excel_buffer(b"a,b\n1,2\n", "../devices.csv").is_err());
        assert!(validate_excel_buffer(b"a,b\n1,2\n", "folder\\devices.csv").is_err());
    }

    #[test]
    fn rejects_registry_archive_expansion_beyond_budget() {
        let zip = zip_with_single_central_entry(1024, 65 * 1024 * 1024);
        assert!(validate_excel_buffer(&zip, "devices.xlsx").is_err());
    }

    #[test]
    fn device_import_rows_have_explicit_amplification_limit() {
        let input = ImportDevicesInput {
            rows: (0..=REGISTRY_DEVICE_IMPORT_ROWS_MAX)
                .map(|index| DeviceImportRow {
                    serial_no: format!("SN{index}"),
                    model_name: None,
                    warehouse_name: None,
                    status: None,
                    notes: None,
                })
                .collect(),
        };
        let result = input.validate();
        assert!(!result.is_valid());
        assert!(
            result
                .errors
                .iter()
                .any(|error| error.code == "VAL_TOO_MANY_ITEMS")
        );
    }
}

#[cfg(test)]
mod tenant_scope_tests {
    use super::*;

    #[test]
    fn import_reference_lookups_are_limited_to_data_scope() {
        let module = FeatureExcelImport::new();
        let pool = Pool::new(SqliteConnectionManager::memory()).expect("in-memory pool");
        pool.get()
            .expect("connection")
            .execute_batch(
                "CREATE TABLE device_models (id TEXT, name TEXT, tenant_id TEXT);
                 CREATE TABLE warehouses (id TEXT, name TEXT, tenant_id TEXT);
                 INSERT INTO device_models VALUES
                    ('foreign-model', 'Shared Model', 'other-tenant'),
                    ('local-model', 'Shared Model', 'test-tenant');
                 INSERT INTO warehouses VALUES
                    ('foreign-warehouse', 'Shared Warehouse', 'other-tenant'),
                    ('local-warehouse', 'Shared Warehouse', 'test-tenant');",
            )
            .expect("fixtures");
        *module.pool.lock().expect("pool lock") = Some(pool);
        let ctx = crate::test_context();

        assert_eq!(
            module
                .lookup_model_id_by_name(ctx.data_scope(), "Shared Model")
                .expect("model lookup"),
            Some("local-model".into())
        );
        assert_eq!(
            module
                .lookup_warehouse_id_by_name(ctx.data_scope(), "Shared Warehouse")
                .expect("warehouse lookup"),
            Some("local-warehouse".into())
        );
    }
}
