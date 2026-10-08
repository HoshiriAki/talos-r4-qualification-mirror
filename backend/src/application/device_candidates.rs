use std::sync::Arc;

use system_core::ExecutionContext;

use crate::repositories::{
    DEVICE_IMPORT_MATCH_CANDIDATES_MAX, RepositoryError, RepositoryProvider,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceMatchCandidate {
    pub serial_no: String,
    pub normalized_serial_no: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceResolveResult {
    pub ok: bool,
    pub reason: String,
    pub cleaned_input_serial_no: String,
    pub matched_serial_no: String,
    pub by: String,
}

#[derive(Debug)]
pub enum DeviceCandidateQueryError {
    TooManyCandidates { max: usize },
    Repository(RepositoryError),
}

impl From<RepositoryError> for DeviceCandidateQueryError {
    fn from(value: RepositoryError) -> Self {
        Self::Repository(value)
    }
}

#[derive(Clone)]
pub struct DeviceCandidateQueryService {
    repository_provider: Arc<dyn RepositoryProvider>,
}

impl DeviceCandidateQueryService {
    pub fn new(repository_provider: Arc<dyn RepositoryProvider>) -> Self {
        Self {
            repository_provider,
        }
    }

    pub fn list_for_import(
        &self,
        ctx: &ExecutionContext,
    ) -> Result<Vec<DeviceMatchCandidate>, DeviceCandidateQueryError> {
        let scoped = self.repository_provider.bind(ctx)?;
        let raw_serials = scoped.device_candidates().list_for_import()?;
        if raw_serials.len() > DEVICE_IMPORT_MATCH_CANDIDATES_MAX {
            return Err(DeviceCandidateQueryError::TooManyCandidates {
                max: DEVICE_IMPORT_MATCH_CANDIDATES_MAX,
            });
        }
        Ok(raw_serials
            .into_iter()
            .map(|serial_no| {
                let normalized_serial_no = normalize_serial_no_for_match(&serial_no);
                DeviceMatchCandidate {
                    serial_no,
                    normalized_serial_no,
                }
            })
            .filter(|candidate| !candidate.normalized_serial_no.is_empty())
            .collect())
    }

    pub fn resolve_for_import(
        &self,
        input_serial_no: &str,
        device_candidates: &[DeviceMatchCandidate],
    ) -> DeviceResolveResult {
        resolve_device_serial_no_for_import(input_serial_no, device_candidates)
    }
}

pub fn normalize_serial_no_for_match(value: &str) -> String {
    let without_controls: String = value
        .chars()
        .filter(|c| {
            !('\u{00}'..='\u{1F}').contains(c)
                && *c != '\u{7F}'
                && !('\u{80}'..='\u{9F}').contains(c)
        })
        .collect();
    let trimmed = without_controls.trim().to_uppercase();
    let leading: String = trimmed
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric())
        .collect();
    if !leading.is_empty() {
        leading
    } else {
        trimmed
            .chars()
            .filter(|c| c.is_ascii_alphanumeric())
            .collect()
    }
}

pub(crate) fn is_valid_serial_no(value: &str) -> bool {
    !value.is_empty() && value.chars().all(|c| c.is_ascii_alphanumeric())
}

fn resolve_device_serial_no_for_import(
    input_serial_no: &str,
    device_candidates: &[DeviceMatchCandidate],
) -> DeviceResolveResult {
    let cleaned = normalize_serial_no_for_match(input_serial_no);

    if !is_valid_serial_no(&cleaned) {
        return DeviceResolveResult {
            ok: false,
            reason: "设备序列号只能包含英文字母和数字".to_string(),
            cleaned_input_serial_no: cleaned,
            matched_serial_no: String::new(),
            by: String::new(),
        };
    }

    let exact_matches: Vec<&DeviceMatchCandidate> = device_candidates
        .iter()
        .filter(|candidate| candidate.normalized_serial_no == cleaned)
        .collect();

    if exact_matches.len() == 1 {
        return DeviceResolveResult {
            ok: true,
            reason: String::new(),
            cleaned_input_serial_no: cleaned.clone(),
            matched_serial_no: exact_matches[0].serial_no.clone(),
            by: "exact".to_string(),
        };
    }
    if exact_matches.len() > 1 {
        return DeviceResolveResult {
            ok: false,
            reason: "后五位匹配到多台设备，请提供更完整的序列号".to_string(),
            cleaned_input_serial_no: cleaned,
            matched_serial_no: String::new(),
            by: String::new(),
        };
    }

    let suffix_matches: Vec<&DeviceMatchCandidate> = device_candidates
        .iter()
        .filter(|candidate| candidate.normalized_serial_no.ends_with(&cleaned))
        .collect();

    if suffix_matches.is_empty() {
        return DeviceResolveResult {
            ok: false,
            reason: "未找到匹配的设备序列号".to_string(),
            cleaned_input_serial_no: cleaned,
            matched_serial_no: String::new(),
            by: String::new(),
        };
    }
    if suffix_matches.len() > 1 {
        return DeviceResolveResult {
            ok: false,
            reason: "后五位匹配到多台设备，请提供更完整的序列号".to_string(),
            cleaned_input_serial_no: cleaned,
            matched_serial_no: String::new(),
            by: String::new(),
        };
    }

    DeviceResolveResult {
        ok: true,
        reason: String::new(),
        cleaned_input_serial_no: cleaned,
        matched_serial_no: suffix_matches[0].serial_no.clone(),
        by: "suffix".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        DeviceMatchCandidate, normalize_serial_no_for_match, resolve_device_serial_no_for_import,
    };

    #[test]
    fn import_serial_normalization_preserves_legacy_contract() {
        assert_eq!(normalize_serial_no_for_match("  abC123 / note"), "ABC123");
        assert_eq!(normalize_serial_no_for_match("\u{0007} xy9 "), "XY9");
    }

    #[test]
    fn import_serial_resolution_preserves_exact_suffix_and_ambiguity_rules() {
        let candidates = vec![
            DeviceMatchCandidate {
                serial_no: "UNITABC123".into(),
                normalized_serial_no: "UNITABC123".into(),
            },
            DeviceMatchCandidate {
                serial_no: "OTHERXYZ999".into(),
                normalized_serial_no: "OTHERXYZ999".into(),
            },
        ];
        let exact = resolve_device_serial_no_for_import("unitabc123", &candidates);
        assert!(exact.ok);
        assert_eq!(exact.by, "exact");
        assert_eq!(exact.matched_serial_no, "UNITABC123");

        let suffix = resolve_device_serial_no_for_import("xyz999", &candidates);
        assert!(suffix.ok);
        assert_eq!(suffix.by, "suffix");
        assert_eq!(suffix.matched_serial_no, "OTHERXYZ999");

        let ambiguous = resolve_device_serial_no_for_import(
            "123",
            &[
                candidates[0].clone(),
                DeviceMatchCandidate {
                    serial_no: "SECOND123".into(),
                    normalized_serial_no: "SECOND123".into(),
                },
            ],
        );
        assert!(!ambiguous.ok);
        assert!(ambiguous.reason.contains("多台设备"));
    }
}
