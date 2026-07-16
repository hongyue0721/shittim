use thiserror::Error;

#[derive(Debug, Error)]
pub enum SchemaToolError {
    #[error("{0}")]
    Message(String),

    #[error("unsupported schema keyword `{keyword}` in {location}: {detail}")]
    UnsupportedKeyword {
        keyword: String,
        location: String,
        detail: String,
    },

    #[error("schema validation failed for {schema_id}: {detail}")]
    ValidationFailed { schema_id: String, detail: String },
}

impl SchemaToolError {
    pub fn msg(message: impl Into<String>) -> Self {
        Self::Message(message.into())
    }
}
