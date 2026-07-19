use crate::json_pointer::{select_json_value, JsonPointer};
use anyhow::{Context, Result};
use kernel_contracts::canonical::{canonical_json_bytes, sha256_hex};
use serde_json::Value;
use std::path::{Path, PathBuf};

/// Output representation for RFC 8785 canonical JSON.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CanonicalOutputMode {
    /// Raw RFC 8785 UTF-8 bytes.
    Bytes,
    /// Lowercase hexadecimal encoding of the RFC 8785 UTF-8 bytes.
    Hex,
    /// Lowercase SHA-256 hexadecimal digest of the RFC 8785 UTF-8 bytes.
    Hash,
}

/// Library request for canonicalizing a selected JSON value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CanonicalizeRequest {
    pub json_file: PathBuf,
    pub pointer: JsonPointer,
    pub output_mode: CanonicalOutputMode,
}

impl CanonicalizeRequest {
    pub fn new(
        json_file: impl Into<PathBuf>,
        pointer: JsonPointer,
        output_mode: CanonicalOutputMode,
    ) -> Self {
        Self {
            json_file: json_file.into(),
            pointer,
            output_mode,
        }
    }
}

/// Canonicalization result. Canonical bytes are computed exactly once and are
/// available to library callers independently of the chosen rendered output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CanonicalizeResult {
    canonical_bytes: Vec<u8>,
    rendered_output: Vec<u8>,
}

impl CanonicalizeResult {
    pub fn canonical_bytes(&self) -> &[u8] {
        &self.canonical_bytes
    }

    pub fn rendered_output(&self) -> &[u8] {
        &self.rendered_output
    }
}

/// Read a JSON document, select one value, and canonicalize it with the shared
/// `kernel-contracts` RFC 8785 implementation.
pub fn canonicalize_selected_json(request: &CanonicalizeRequest) -> Result<CanonicalizeResult> {
    let document = read_json_document(&request.json_file)?;
    let selected = select_json_value(&document, &request.pointer)?;
    canonicalize_value(selected, request.output_mode)
}

/// Canonicalize an already-selected JSON value.
pub fn canonicalize_value(
    value: &Value,
    output_mode: CanonicalOutputMode,
) -> Result<CanonicalizeResult> {
    let canonical_bytes = canonical_json_bytes(value).map_err(|error| anyhow::anyhow!(error))?;
    let rendered_output = match output_mode {
        CanonicalOutputMode::Bytes => canonical_bytes.clone(),
        CanonicalOutputMode::Hex => hex::encode(&canonical_bytes).into_bytes(),
        CanonicalOutputMode::Hash => sha256_hex(&canonical_bytes).into_bytes(),
    };
    Ok(CanonicalizeResult {
        canonical_bytes,
        rendered_output,
    })
}

/// Compatibility wrapper for programmatic callers that still provide only a
/// file and hash flag. New code should use [`canonicalize_selected_json`].
pub fn run(json_file: &Path, hash: bool) -> Result<()> {
    let mode = if hash {
        CanonicalOutputMode::Hash
    } else {
        CanonicalOutputMode::Bytes
    };
    let request = CanonicalizeRequest::new(json_file, JsonPointer::root(), mode);
    write_stdout(&canonicalize_selected_json(&request)?)
}

pub fn write_stdout(result: &CanonicalizeResult) -> Result<()> {
    use std::io::Write;
    let mut stdout = std::io::stdout().lock();
    stdout.write_all(result.rendered_output())?;
    stdout.flush()?;
    Ok(())
}

fn read_json_document(path: &Path) -> Result<Value> {
    let text = std::fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    serde_json::from_str(&text).with_context(|| format!("parse {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn empty_object_modes_are_exact() {
        let bytes = canonicalize_value(&json!({}), CanonicalOutputMode::Bytes).unwrap();
        assert_eq!(bytes.canonical_bytes(), b"{}");
        assert_eq!(bytes.rendered_output(), b"{}");

        let hex = canonicalize_value(&json!({}), CanonicalOutputMode::Hex).unwrap();
        assert_eq!(hex.canonical_bytes(), b"{}");
        assert_eq!(hex.rendered_output(), b"7b7d");

        let hash = canonicalize_value(&json!({}), CanonicalOutputMode::Hash).unwrap();
        assert_eq!(
            hash.rendered_output(),
            b"44136fa355b3678a1146ad16f7e8649e94fb4fc21fe77e8310c060f61caaff8a"
        );
    }

    #[test]
    fn official_rfc8785_example_uses_kernel_contracts_algorithm() {
        let value: Value = serde_json::from_str(include_str!(
            "../../../../schemas/examples/jcs/rfc8785-example.input.json"
        ))
        .unwrap();
        let expected =
            include_bytes!("../../../../schemas/examples/jcs/rfc8785-example.canonical.txt");
        let result = canonicalize_value(&value, CanonicalOutputMode::Bytes).unwrap();
        assert_eq!(
            result.rendered_output(),
            expected.strip_suffix(b"\n").unwrap_or(expected)
        );
    }
}
