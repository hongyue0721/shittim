//! Strict RFC 6901 JSON Pointer.
//!
//! Canonical form only: root is the empty string; non-root pointers start with `/`.
//! Segments encode `~` as `~0` and `/` as `~1`. Decode is strict (no bare `~`).
//! Literal `%` is allowed inside pointer segments; URI percent-decoding happens
//! once in `$ref` fragment handling before a pointer is parsed here.
//! JSON Schema `$anchor` / non-pointer fragments are not represented here.

use crate::error::SchemaToolError;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::fmt;

/// RFC 6901 JSON Pointer in canonical encoded form.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct JsonPointer(String);

impl JsonPointer {
    /// Root document pointer (`""`).
    pub fn root() -> Self {
        Self(String::new())
    }

    /// Parse a canonical or strict-decodable pointer string.
    ///
    /// Accepts `""` (root) or a string that starts with `/`. Rejects bare tokens
    /// and incomplete `~` escapes. Literal `%` is allowed (URI percent-decoding
    /// is not performed here).
    pub fn parse(raw: &str) -> Result<Self> {
        if raw.is_empty() {
            return Ok(Self::root());
        }
        if !raw.starts_with('/') {
            return Err(SchemaToolError::msg(format!(
                "JSON Pointer must be empty or start with '/': {raw:?}"
            ))
            .into());
        }
        // Validate each encoded segment can be strictly decoded.
        for segment in raw.split('/').skip(1) {
            decode_segment(segment).map_err(|detail| {
                SchemaToolError::msg(format!(
                    "invalid JSON Pointer segment {segment:?} in {raw:?}: {detail}"
                ))
            })?;
        }
        Ok(Self(raw.to_string()))
    }

    /// Build a pointer from already-decoded (raw) segments.
    pub fn from_decoded_segments<I, S>(segments: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let mut out = String::new();
        for segment in segments {
            out.push('/');
            out.push_str(&encode_segment(segment.as_ref()));
        }
        Self(out)
    }

    pub fn is_root(&self) -> bool {
        self.0.is_empty()
    }

    /// Canonical encoded form (`""` or `/a~1b`).
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Append one decoded segment, returning a new pointer.
    pub fn child(&self, segment: &str) -> Self {
        let encoded = encode_segment(segment);
        if self.0.is_empty() {
            Self(format!("/{encoded}"))
        } else {
            Self(format!("{}/{encoded}", self.0))
        }
    }

    /// Append an array index segment.
    pub fn index(&self, index: usize) -> Self {
        self.child(&index.to_string())
    }

    /// Strictly decode into raw (unescaped) segments. Root yields an empty vec.
    pub fn decoded_segments(&self) -> Result<Vec<String>> {
        if self.0.is_empty() {
            return Ok(Vec::new());
        }
        self.0
            .split('/')
            .skip(1)
            .map(|segment| {
                decode_segment(segment).map_err(|detail| {
                    SchemaToolError::msg(format!(
                        "invalid JSON Pointer segment {segment:?} in {:?}: {detail}",
                        self.0
                    ))
                    .into()
                })
            })
            .collect()
    }

    /// Join this pointer under another base (base must be a prefix path).
    pub fn join(&self, other: &JsonPointer) -> Self {
        if other.is_root() {
            return self.clone();
        }
        if self.is_root() {
            return other.clone();
        }
        Self(format!("{}{}", self.0, other.0))
    }
}

impl Default for JsonPointer {
    fn default() -> Self {
        Self::root()
    }
}

impl fmt::Display for JsonPointer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl AsRef<str> for JsonPointer {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

/// Encode one decoded segment per RFC 6901 (`~` -> `~0`, `/` -> `~1`).
pub fn encode_segment(segment: &str) -> String {
    segment.replace('~', "~0").replace('/', "~1")
}

/// Strictly decode one encoded segment. Rejects incomplete escapes.
pub fn decode_segment(encoded: &str) -> Result<String, String> {
    let mut out = String::with_capacity(encoded.len());
    let mut chars = encoded.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '~' {
            match chars.next() {
                Some('0') => out.push('~'),
                Some('1') => out.push('/'),
                Some(other) => {
                    return Err(format!(
                        "invalid escape ~{other}; only ~0 and ~1 are allowed"
                    ));
                }
                None => return Err("truncated escape at end of segment".into()),
            }
        } else {
            out.push(ch);
        }
    }
    Ok(out)
}

/// Parse a JSON Schema `$ref` fragment that has already been URI-percent-decoded.
///
/// - `""`: root
/// - `/...`: pointer (literal `%` allowed)
/// - bare name / `$anchor`: unsupported
pub fn pointer_from_decoded_fragment(fragment: &str) -> Result<JsonPointer> {
    if fragment.is_empty() {
        return Ok(JsonPointer::root());
    }
    if !fragment.starts_with('/') {
        return Err(SchemaToolError::msg(format!(
            "JSON Schema $anchor / non-pointer fragment is not supported: #{fragment}"
        ))
        .into());
    }
    JsonPointer::parse(fragment)
}

/// Parse an array-index token from a JSON Pointer segment.
///
/// Accepts only base-10 integers without leading zeros (`0` is allowed; `01`,
/// `-`, empty, and non-digits are rejected).
pub fn parse_array_index_token(token: &str) -> Result<usize, String> {
    if token.is_empty() {
        return Err("empty array index token".into());
    }
    if token == "-" {
        return Err("JSON Pointer '-' array token is not supported for evaluation".into());
    }
    if !token.bytes().all(|b| b.is_ascii_digit()) {
        return Err(format!(
            "array index token is not a decimal integer: {token:?}"
        ));
    }
    if token.len() > 1 && token.starts_with('0') {
        return Err(format!(
            "array index token has leading zero (not canonical): {token:?}"
        ));
    }
    token
        .parse::<usize>()
        .map_err(|error| format!("array index token out of range {token:?}: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn root_and_child_roundtrip() {
        let root = JsonPointer::root();
        assert!(root.is_root());
        assert_eq!(root.as_str(), "");
        let child = root.child("properties").child("a/b").child("c~d");
        assert_eq!(child.as_str(), "/properties/a~1b/c~0d");
        assert_eq!(
            child.decoded_segments().unwrap(),
            vec!["properties", "a/b", "c~d"]
        );
    }

    #[test]
    fn allows_literal_percent_and_rejects_bad_escapes() {
        assert!(JsonPointer::parse("/a~").is_err());
        assert!(JsonPointer::parse("/a~2").is_err());
        let with_percent = JsonPointer::parse("/a%2Fb").unwrap();
        assert_eq!(with_percent.as_str(), "/a%2Fb");
        assert_eq!(with_percent.decoded_segments().unwrap(), vec!["a%2Fb"]);
        assert!(pointer_from_decoded_fragment("defs").is_err());
        assert!(pointer_from_decoded_fragment("/$defs/x").is_ok());
    }

    #[test]
    fn index_segment_and_array_token_rules() {
        let p = JsonPointer::root().child("oneOf").index(1);
        assert_eq!(p.as_str(), "/oneOf/1");
        assert_eq!(parse_array_index_token("0").unwrap(), 0);
        assert_eq!(parse_array_index_token("10").unwrap(), 10);
        assert!(parse_array_index_token("01").is_err());
        assert!(parse_array_index_token("-").is_err());
        assert!(parse_array_index_token("1a").is_err());
    }
}
