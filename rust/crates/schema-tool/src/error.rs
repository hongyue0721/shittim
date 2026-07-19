use thiserror::Error;

#[derive(Debug, Error)]
pub enum SchemaToolError {
    #[error("{0}")]
    Message(String),

    #[error("invalid JSON Pointer syntax {pointer:?}: {detail}")]
    PointerSyntax { pointer: String, detail: String },

    #[error("JSON Pointer evaluation failed {pointer:?} at token {token_index}: {detail}")]
    PointerEvaluation {
        pointer: String,
        token_index: usize,
        detail: String,
    },

    #[error("JSON mutation {operation} failed at {pointer:?} token {token_index}: {detail}")]
    Mutation {
        operation: String,
        pointer: String,
        token_index: usize,
        detail: String,
    },

    #[error(
        "internal schema registry invariant failed for {schema_id}#{pointer}: authoritative pointer could not be selected: {source}"
    )]
    InternalInvariant {
        schema_id: String,
        pointer: String,
        #[source]
        source: Box<SchemaToolError>,
    },

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
