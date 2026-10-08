use std::sync::{Arc, LazyLock};

use axum::extract::Multipart;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

use crate::error::AppError;
use crate::services::excel_import_service::IMPORT_FILE_BYTES_MAX;

const IMPORT_MULTIPART_PARTS_MAX: usize = 4;
const APPROVED_FIELD_BYTES_MAX: usize = 16;
const IMPORT_CONCURRENCY_MAX: usize = 2;

static IMPORT_CONCURRENCY: LazyLock<Arc<Semaphore>> =
    LazyLock::new(|| Arc::new(Semaphore::new(IMPORT_CONCURRENCY_MAX)));

#[derive(Debug)]
pub struct ExcelImportMultipart {
    pub file_name: String,
    pub file_data: Vec<u8>,
    pub approved_execution: bool,
    // The permit intentionally travels with the parsed upload so it remains
    // held through archive/workbook parsing and row-level execution, not only
    // while Multipart fields are being read.
    _permit: OwnedSemaphorePermit,
}

/// Parse an Excel-import multipart request once, fail-closed.
///
/// P4 owns the aggregate HTTP body limit. This helper owns multipart shape and
/// P7-B import concurrency: parser errors are propagated, one file is accepted,
/// optional approval is singular and tiny, unknown/repeated fields are rejected,
/// and the decoded file is checked against the narrower single-file budget before
/// any spreadsheet parser is invoked. Admission fails fast instead of queueing a
/// mutation behind an outer HTTP timeout whose outcome could become ambiguous.
pub async fn parse_excel_import_multipart(
    multipart: &mut Multipart,
    allow_approved: bool,
) -> Result<ExcelImportMultipart, AppError> {
    let permit = Arc::clone(&IMPORT_CONCURRENCY)
        .try_acquire_owned()
        .map_err(|_| AppError::RateLimited {
            retry_after_secs: 1,
        })?;

    let mut part_count = 0usize;
    let mut file: Option<(String, Vec<u8>)> = None;
    let mut approved_execution: Option<bool> = None;

    loop {
        let field = multipart
            .next_field()
            .await
            .map_err(|error| AppError::BadRequest(format!("multipart 解析失败: {error}")))?;
        let Some(field) = field else {
            break;
        };

        part_count += 1;
        if part_count > IMPORT_MULTIPART_PARTS_MAX {
            return Err(AppError::BadRequest(format!(
                "multipart 字段数量超过上限 {IMPORT_MULTIPART_PARTS_MAX}"
            )));
        }

        let name = field.name().unwrap_or("").to_string();
        match name.as_str() {
            "file" => {
                if file.is_some() {
                    return Err(AppError::BadRequest(
                        "multipart 只能包含一个 file 字段".to_string(),
                    ));
                }
                let file_name = field
                    .file_name()
                    .filter(|name| !name.trim().is_empty())
                    .ok_or_else(|| AppError::BadRequest("file 字段缺少文件名".to_string()))?
                    .to_string();
                let bytes = field
                    .bytes()
                    .await
                    .map_err(|error| AppError::BadRequest(format!("读取上传文件失败: {error}")))?;
                if bytes.len() > IMPORT_FILE_BYTES_MAX {
                    return Err(AppError::BadRequest(format!(
                        "导入文件超过 {} MiB 单文件上限",
                        IMPORT_FILE_BYTES_MAX / 1024 / 1024
                    )));
                }
                file = Some((file_name, bytes.to_vec()));
            }
            "approved" if allow_approved => {
                if approved_execution.is_some() {
                    return Err(AppError::BadRequest(
                        "multipart 只能包含一个 approved 字段".to_string(),
                    ));
                }
                let bytes = field.bytes().await.map_err(|error| {
                    AppError::BadRequest(format!("读取 approved 字段失败: {error}"))
                })?;
                if bytes.len() > APPROVED_FIELD_BYTES_MAX {
                    return Err(AppError::BadRequest("approved 字段过长".to_string()));
                }
                let value = std::str::from_utf8(&bytes)
                    .map_err(|_| AppError::BadRequest("approved 必须是 UTF-8 文本".to_string()))?
                    .trim();
                approved_execution = Some(match value {
                    "true" | "TRUE" | "True" => true,
                    "false" | "FALSE" | "False" | "" => false,
                    _ => {
                        return Err(AppError::BadRequest(
                            "approved 只能是 true 或 false".to_string(),
                        ));
                    }
                });
            }
            "approved" => {
                return Err(AppError::BadRequest(
                    "此导入接口不接受 approved 字段".to_string(),
                ));
            }
            _ => {
                return Err(AppError::BadRequest(format!(
                    "不支持的 multipart 字段: {}",
                    if name.is_empty() { "<unnamed>" } else { &name }
                )));
            }
        }
    }

    let (file_name, file_data) =
        file.ok_or_else(|| AppError::BadRequest("缺少 file 上传字段".to_string()))?;

    Ok(ExcelImportMultipart {
        file_name,
        file_data,
        approved_execution: approved_execution.unwrap_or(false),
        _permit: permit,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn multipart_shape_and_concurrency_budgets_are_explicit_and_finite() {
        assert!((1..=8).contains(&IMPORT_MULTIPART_PARTS_MAX));
        assert!((1..=64).contains(&APPROVED_FIELD_BYTES_MAX));
        assert!((1..=4).contains(&IMPORT_CONCURRENCY_MAX));
        assert!(IMPORT_FILE_BYTES_MAX <= 25 * 1024 * 1024);
    }

    #[tokio::test]
    async fn import_concurrency_admission_fails_fast() {
        let first = Arc::clone(&IMPORT_CONCURRENCY).try_acquire_owned().unwrap();
        let second = Arc::clone(&IMPORT_CONCURRENCY).try_acquire_owned().unwrap();
        assert!(Arc::clone(&IMPORT_CONCURRENCY).try_acquire_owned().is_err());
        drop(first);
        assert!(Arc::clone(&IMPORT_CONCURRENCY).try_acquire_owned().is_ok());
        drop(second);
    }
}
