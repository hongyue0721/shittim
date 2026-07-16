use thiserror::Error;

/// Stable stage at which a contract failure occurred.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContractFailureStage {
    /// Caller input failed the selected JSON Schema.
    CallerSchemaValidation,
    /// A generated envelope wire type failed to decode after Schema validation.
    WireDecodeAfterSchema,
    /// A generated method payload failed to decode after Schema validation.
    PayloadDecodeAfterSchema,
    /// A generated discriminator could not be mapped to its generated payload variant.
    GeneratedDiscriminatorMapping,
    /// The embedded Schema catalog could not be loaded, compiled, or queried.
    SchemaCatalog,
}

/// Safe preflight-oriented classification of a contract failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContractFailureClassification {
    /// The selected Schema rejected caller-controlled input.
    CallerInvalid,
    /// Generated code, the embedded catalog, or another internal contract mechanism failed.
    InternalContractFailure,
}

/// Structured contract failure information for preflight decisions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClassifiedContractFailure {
    /// Stable failure stage.
    pub stage: ContractFailureStage,
    /// Relevant Schema identifier when one is available.
    pub schema_id: Option<String>,
    /// Whether the failure is caller input or an internal contract failure.
    pub classification: ContractFailureClassification,
}

/// Error produced by generated contracts, validation, or canonicalization.
#[derive(Debug, Error)]
pub enum ContractError {
    /// Caller-controlled JSON failed the explicitly selected Schema.
    #[error("JSON schema validation failed for {schema_id}: {detail}")]
    SchemaValidation { schema_id: String, detail: String },

    /// A generated raw envelope failed to decode after its Schema had passed.
    #[error("wire decode after schema validation failed for {schema_id}: {detail}")]
    WireDecodeAfterSchema { schema_id: String, detail: String },

    /// A generated payload failed to decode after its envelope Schema had passed.
    #[error(
        "payload decode after schema validation failed for {schema_id} ({discriminator}): {detail}"
    )]
    PayloadDecodeAfterSchema {
        schema_id: String,
        discriminator: String,
        detail: String,
    },

    /// Generated discriminator and payload mappings were internally inconsistent.
    #[error("generated discriminator mapping failed for {schema_id}: {discriminator}")]
    GeneratedDiscriminatorMapping {
        schema_id: String,
        discriminator: String,
    },

    /// The requested Schema ID is absent from the embedded catalog.
    #[error("unknown schema id: {schema_id}")]
    UnknownSchema { schema_id: String },

    /// RFC 8785 canonicalization failed.
    #[error("canonicalization error: {0}")]
    Canonicalize(String),

    /// JSON needed by an internal contract mechanism was invalid.
    #[error("invalid JSON: {0}")]
    InvalidJson(String),

    /// The embedded Schema catalog could not be parsed, compiled, or loaded.
    #[error("schema catalog load error: {0}")]
    Catalog(String),
}

impl ContractError {
    /// Returns the stable stage associated with this error.
    pub fn stage(&self) -> ContractFailureStage {
        match self {
            Self::SchemaValidation { .. } => ContractFailureStage::CallerSchemaValidation,
            Self::WireDecodeAfterSchema { .. } => ContractFailureStage::WireDecodeAfterSchema,
            Self::PayloadDecodeAfterSchema { .. } => ContractFailureStage::PayloadDecodeAfterSchema,
            Self::GeneratedDiscriminatorMapping { .. } => {
                ContractFailureStage::GeneratedDiscriminatorMapping
            }
            Self::UnknownSchema { .. }
            | Self::Catalog(_)
            | Self::Canonicalize(_)
            | Self::InvalidJson(_) => ContractFailureStage::SchemaCatalog,
        }
    }

    /// Returns a stable classification suitable for Value preflight.
    ///
    /// Canonicalization and invalid-JSON errors cannot normally occur for an already parsed
    /// preflight `Value`; if they do occur through an internal path they fail closed as internal
    /// contract failures.
    pub fn classification_for_preflight(&self) -> ClassifiedContractFailure {
        let classification = if matches!(self, Self::SchemaValidation { .. }) {
            ContractFailureClassification::CallerInvalid
        } else {
            ContractFailureClassification::InternalContractFailure
        };
        ClassifiedContractFailure {
            stage: self.stage(),
            schema_id: self.schema_id().map(str::to_owned),
            classification,
        }
    }

    fn schema_id(&self) -> Option<&str> {
        match self {
            Self::SchemaValidation { schema_id, .. }
            | Self::WireDecodeAfterSchema { schema_id, .. }
            | Self::PayloadDecodeAfterSchema { schema_id, .. }
            | Self::GeneratedDiscriminatorMapping { schema_id, .. }
            | Self::UnknownSchema { schema_id } => Some(schema_id),
            Self::Catalog(_) | Self::Canonicalize(_) | Self::InvalidJson(_) => None,
        }
    }
}
