use thiserror::Error;

#[derive(Debug, Error)]
pub enum ContractError {
    #[error("JSON schema validation failed for {schema_id}: {detail}")]
    SchemaValidation { schema_id: String, detail: String },

    #[error("unknown schema id: {0}")]
    UnknownSchema(String),

    #[error("canonicalization error: {0}")]
    Canonicalize(String),

    #[error("invalid JSON: {0}")]
    InvalidJson(String),

    #[error("schema catalog load error: {0}")]
    Catalog(String),
}
