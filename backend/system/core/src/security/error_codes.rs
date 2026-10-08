use serde_json::Value;

/// 结构化错误载体
/// Re-export from crate root, kept here for module cohesion.
pub use crate::ErrorPayload;

impl ErrorPayload {
    pub fn new(category: &str, code: &str, message: &str) -> Self {
        Self {
            category: category.to_string(),
            code: code.to_string(),
            message: message.to_string(),
            field: None,
            context: None,
        }
    }

    pub fn with_field(mut self, field: &str) -> Self {
        self.field = Some(field.to_string());
        self
    }

    pub fn with_context(mut self, context: Value) -> Self {
        self.context = Some(context);
        self
    }

    pub fn to_json_string(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }
}
