//! Strict RFC 6901 JSON Pointer selection and mutation.
//!
//! Canonical form only: root is the empty string; non-root pointers start with `/`.
//! Segments encode `~` as `~0` and `/` as `~1`. Decode is strict (no bare `~`).
//! Literal `%` is allowed inside pointer segments; URI percent-decoding happens
//! once in `$ref` fragment handling before a pointer is parsed here.
//! JSON Schema `$anchor` / non-pointer fragments are not represented here.
//!
//! Selection operates on arbitrary [`serde_json::Value`] documents and is
//! intentionally independent of Schema graph identity or cycle handling.

use crate::error::SchemaToolError;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
use std::fmt;

/// RFC 6901 JSON Pointer in canonical encoded form.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct JsonPointer(String);

impl JsonPointer {
    /// Root document pointer (`""`).
    pub fn root() -> Self {
        Self(String::new())
    }

    /// Parse a strict JSON Pointer.
    ///
    /// Accepts `""` (root) or a string that starts with `/`. Rejects bare tokens
    /// and malformed `~` escapes. Literal `%` is allowed because URI decoding is
    /// not part of JSON Pointer parsing.
    pub fn parse(raw: &str) -> Result<Self, SchemaToolError> {
        if raw.is_empty() {
            return Ok(Self::root());
        }
        if !raw.starts_with('/') {
            return Err(pointer_syntax_error(
                raw,
                "pointer must be empty or start with '/'",
            ));
        }
        for (segment_index, segment) in raw.split('/').skip(1).enumerate() {
            decode_segment(segment).map_err(|detail| {
                pointer_syntax_error(
                    raw,
                    format!(
                        "invalid segment {} {segment:?}: {detail}",
                        segment_index + 1
                    ),
                )
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
    pub fn decoded_segments(&self) -> Result<Vec<String>, SchemaToolError> {
        if self.0.is_empty() {
            return Ok(Vec::new());
        }
        self.0
            .split('/')
            .skip(1)
            .enumerate()
            .map(|(segment_index, segment)| {
                decode_segment(segment).map_err(|detail| {
                    pointer_syntax_error(
                        &self.0,
                        format!(
                            "invalid segment {} {segment:?}: {detail}",
                            segment_index + 1
                        ),
                    )
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

impl<'de> Deserialize<'de> for JsonPointer {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = String::deserialize(deserializer)?;
        Self::parse(&raw).map_err(serde::de::Error::custom)
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

/// Select a value from an arbitrary JSON document using a parsed pointer.
///
/// Syntax errors are produced by [`JsonPointer::parse`]. This evaluator reports
/// only evaluation errors: missing members, invalid array index tokens,
/// out-of-bounds indexes, and traversal through scalars.
pub fn select_json_value<'a>(
    document: &'a Value,
    pointer: &JsonPointer,
) -> Result<&'a Value, SchemaToolError> {
    let segments = pointer.decoded_segments()?;
    select_decoded_segments(document, pointer, &segments)
}

/// Parse and evaluate a strict pointer in one call.
pub fn select_json_value_at_pointer<'a>(
    document: &'a Value,
    raw_pointer: &str,
) -> Result<&'a Value, SchemaToolError> {
    let pointer = JsonPointer::parse(raw_pointer)?;
    select_json_value(document, &pointer)
}

/// A generic JSON mutation operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JsonMutationOperation {
    Add,
    Replace,
}

impl fmt::Display for JsonMutationOperation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Add => f.write_str("add"),
            Self::Replace => f.write_str("replace"),
        }
    }
}

/// Apply a strict JSON Pointer mutation.
///
/// `add` requires an absent object member or inserts into an array at
/// `0..=len`; root add is rejected. `replace` requires an existing member or
/// array index `< len`; root replace replaces the whole document. The `-` token
/// is never accepted.
pub fn apply_json_mutation(
    document: &mut Value,
    operation: JsonMutationOperation,
    pointer: &JsonPointer,
    value: Value,
) -> Result<(), SchemaToolError> {
    if pointer.is_root() {
        return match operation {
            JsonMutationOperation::Add => Err(mutation_error(
                operation,
                pointer,
                0,
                "add at the document root is not supported",
            )),
            JsonMutationOperation::Replace => {
                *document = value;
                Ok(())
            }
        };
    }

    let segments = pointer.decoded_segments()?;
    let (last, parent_segments) = segments
        .split_last()
        .expect("non-root pointer always has a final segment");
    let parent = traverse_decoded(document, parent_segments, |failure| {
        mutation_traversal_error(operation, pointer, failure)
    })?;
    let final_token_index = segments.len();

    match parent {
        Value::Object(map) => match operation {
            JsonMutationOperation::Add if map.contains_key(last) => Err(mutation_error(
                operation,
                pointer,
                final_token_index,
                format!("object member {last:?} already exists"),
            )),
            JsonMutationOperation::Add => {
                map.insert(last.clone(), value);
                Ok(())
            }
            JsonMutationOperation::Replace if !map.contains_key(last) => Err(mutation_error(
                operation,
                pointer,
                final_token_index,
                format!("object member {last:?} does not exist"),
            )),
            JsonMutationOperation::Replace => {
                map.insert(last.clone(), value);
                Ok(())
            }
        },
        Value::Array(items) => {
            let index = parse_array_index_token(last).map_err(|detail| {
                mutation_error(
                    operation,
                    pointer,
                    final_token_index,
                    format!("invalid array index: {detail}"),
                )
            })?;
            match operation {
                JsonMutationOperation::Add if index > items.len() => Err(mutation_error(
                    operation,
                    pointer,
                    final_token_index,
                    format!(
                        "array insertion index {index} is out of bounds for length {}",
                        items.len()
                    ),
                )),
                JsonMutationOperation::Add => {
                    items.insert(index, value);
                    Ok(())
                }
                JsonMutationOperation::Replace if index >= items.len() => Err(mutation_error(
                    operation,
                    pointer,
                    final_token_index,
                    format!(
                        "array replacement index {index} is out of bounds for length {}",
                        items.len()
                    ),
                )),
                JsonMutationOperation::Replace => {
                    items[index] = value;
                    Ok(())
                }
            }
        }
        scalar => Err(mutation_error(
            operation,
            pointer,
            final_token_index,
            format!(
                "cannot mutate child {last:?} of {}",
                json_value_kind(scalar)
            ),
        )),
    }
}

/// Encode one decoded segment per RFC 6901 (`~` -> `~0`, `/` -> `~1`).
pub fn encode_segment(segment: &str) -> String {
    segment.replace('~', "~0").replace('/', "~1")
}

/// Strictly decode one encoded segment. Rejects incomplete or unknown escapes.
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
pub fn pointer_from_decoded_fragment(fragment: &str) -> Result<JsonPointer, SchemaToolError> {
    if fragment.is_empty() {
        return Ok(JsonPointer::root());
    }
    if !fragment.starts_with('/') {
        return Err(SchemaToolError::msg(format!(
            "JSON Schema $anchor / non-pointer fragment is not supported: #{fragment}"
        )));
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
    if !token.bytes().all(|byte| byte.is_ascii_digit()) {
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

fn select_decoded_segments<'a>(
    document: &'a Value,
    pointer: &JsonPointer,
    segments: &[String],
) -> Result<&'a Value, SchemaToolError> {
    traverse_decoded(document, segments, |failure| {
        pointer_evaluation_error(pointer, failure)
    })
}

#[derive(Debug)]
struct TraversalFailure {
    token_index: usize,
    detail: String,
}

#[derive(Debug, Clone)]
enum TraversalStep {
    ObjectMember(String),
    ArrayIndex(usize),
}

trait DecodedTraversalCursor: Sized {
    fn value(&self) -> &Value;
    fn descend(self, step: TraversalStep) -> Self;
}

impl DecodedTraversalCursor for &Value {
    fn value(&self) -> &Value {
        self
    }

    fn descend(self, step: TraversalStep) -> Self {
        match step {
            TraversalStep::ObjectMember(member) => &self
                .as_object()
                .expect("traversal step was classified against an object")[&member],
            TraversalStep::ArrayIndex(index) => &self
                .as_array()
                .expect("traversal step was classified against an array")[index],
        }
    }
}

impl DecodedTraversalCursor for &mut Value {
    fn value(&self) -> &Value {
        self
    }

    fn descend(self, step: TraversalStep) -> Self {
        match step {
            TraversalStep::ObjectMember(member) => self
                .as_object_mut()
                .expect("traversal step was classified against an object")
                .get_mut(&member)
                .expect("object member existence was classified before descent"),
            TraversalStep::ArrayIndex(index) => &mut self
                .as_array_mut()
                .expect("traversal step was classified against an array")[index],
        }
    }
}

fn traverse_decoded<C, E>(
    mut current: C,
    segments: &[String],
    map_error: impl Fn(TraversalFailure) -> E,
) -> Result<C, E>
where
    C: DecodedTraversalCursor,
{
    for (depth, token) in segments.iter().enumerate() {
        let step =
            classify_traversal_step(current.value(), token, depth + 1).map_err(&map_error)?;
        current = current.descend(step);
    }
    Ok(current)
}

fn classify_traversal_step(
    current: &Value,
    token: &str,
    token_index: usize,
) -> Result<TraversalStep, TraversalFailure> {
    match current {
        Value::Object(map) => {
            if map.contains_key(token) {
                Ok(TraversalStep::ObjectMember(token.to_owned()))
            } else {
                Err(TraversalFailure {
                    token_index,
                    detail: format!("object member {token:?} does not exist"),
                })
            }
        }
        Value::Array(items) => {
            let index = parse_array_index_token(token).map_err(|detail| TraversalFailure {
                token_index,
                detail,
            })?;
            if index < items.len() {
                Ok(TraversalStep::ArrayIndex(index))
            } else {
                Err(TraversalFailure {
                    token_index,
                    detail: format!(
                        "array index {index} is out of bounds for length {}",
                        items.len()
                    ),
                })
            }
        }
        scalar => Err(TraversalFailure {
            token_index,
            detail: format!(
                "cannot traverse token {token:?} through {}",
                json_value_kind(scalar)
            ),
        }),
    }
}

fn pointer_syntax_error(pointer: &str, detail: impl Into<String>) -> SchemaToolError {
    SchemaToolError::PointerSyntax {
        pointer: pointer.to_string(),
        detail: detail.into(),
    }
}

fn pointer_evaluation_error(pointer: &JsonPointer, failure: TraversalFailure) -> SchemaToolError {
    SchemaToolError::PointerEvaluation {
        pointer: pointer.as_str().to_string(),
        token_index: failure.token_index,
        detail: failure.detail,
    }
}

fn mutation_traversal_error(
    operation: JsonMutationOperation,
    pointer: &JsonPointer,
    failure: TraversalFailure,
) -> SchemaToolError {
    mutation_error(
        operation,
        pointer,
        failure.token_index,
        format!("parent traversal failed: {}", failure.detail),
    )
}

fn mutation_error(
    operation: JsonMutationOperation,
    pointer: &JsonPointer,
    token_index: usize,
    detail: impl Into<String>,
) -> SchemaToolError {
    SchemaToolError::Mutation {
        operation: operation.to_string(),
        pointer: pointer.as_str().to_string(),
        token_index,
        detail: detail.into(),
    }
}

fn json_value_kind(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

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
    fn syntax_allows_literal_percent_and_rejects_bad_escapes() {
        assert!(matches!(
            JsonPointer::parse("a"),
            Err(SchemaToolError::PointerSyntax { .. })
        ));
        assert!(matches!(
            JsonPointer::parse("/a~"),
            Err(SchemaToolError::PointerSyntax { .. })
        ));
        assert!(matches!(
            JsonPointer::parse("/a~2"),
            Err(SchemaToolError::PointerSyntax { .. })
        ));
        let with_percent = JsonPointer::parse("/a%2Fb").unwrap();
        assert_eq!(with_percent.as_str(), "/a%2Fb");
        assert_eq!(with_percent.decoded_segments().unwrap(), vec!["a%2Fb"]);
        assert!(pointer_from_decoded_fragment("defs").is_err());
        assert!(pointer_from_decoded_fragment("/$defs/x").is_ok());
    }

    #[test]
    fn selection_handles_root_escapes_and_literal_percent() {
        let document = json!({
            "a/b": {"c~d": 1},
            "a%2Fb": 2
        });
        assert_eq!(
            select_json_value(&document, &JsonPointer::root()).unwrap(),
            &document
        );
        assert_eq!(
            select_json_value_at_pointer(&document, "/a~1b/c~0d").unwrap(),
            &json!(1)
        );
        assert_eq!(
            select_json_value_at_pointer(&document, "/a%2Fb").unwrap(),
            &json!(2)
        );
    }

    #[test]
    fn selection_distinguishes_evaluation_failures() {
        let document = json!({"items": ["zero"], "scalar": true});
        for pointer in [
            "/missing",
            "/items/01",
            "/items/-",
            "/items/1",
            "/scalar/child",
        ] {
            assert!(matches!(
                select_json_value_at_pointer(&document, pointer),
                Err(SchemaToolError::PointerEvaluation { .. })
            ));
        }
    }

    #[test]
    fn serde_deserialization_is_strict_and_serialization_is_canonical() {
        for invalid in [r#""a""#, r#""/a~""#, r#""/a~2""#] {
            assert!(
                serde_json::from_str::<JsonPointer>(invalid).is_err(),
                "serde must not bypass strict parsing for {invalid}"
            );
        }

        let pointer = JsonPointer::from_decoded_segments(["properties", "a/b", "c~d"]);
        let encoded = serde_json::to_string(&pointer).unwrap();
        assert_eq!(encoded, r#""/properties/a~1b/c~0d""#);
        assert_eq!(
            serde_json::from_str::<JsonPointer>(&encoded).unwrap(),
            pointer
        );
    }

    #[test]
    fn selection_distinguishes_object_numeric_names_from_array_indexes() {
        let document = json!({
            "object": {"01": "member"},
            "array": ["zero", "one"]
        });
        let object_pointer = JsonPointer::from_decoded_segments(["object", "01"]);
        assert_eq!(
            select_json_value(&document, &object_pointer).unwrap(),
            "member"
        );

        let array_pointer = JsonPointer::from_decoded_segments(["array", "01"]);
        assert!(matches!(
            select_json_value(&document, &array_pointer),
            Err(SchemaToolError::PointerEvaluation { token_index: 2, .. })
        ));
    }

    #[test]
    fn index_segment_and_array_token_rules() {
        let pointer = JsonPointer::root().child("oneOf").index(1);
        assert_eq!(pointer.as_str(), "/oneOf/1");
        assert_eq!(parse_array_index_token("0").unwrap(), 0);
        assert_eq!(parse_array_index_token("10").unwrap(), 10);
        assert!(parse_array_index_token("01").is_err());
        assert!(parse_array_index_token("-").is_err());
        assert!(parse_array_index_token("1a").is_err());
    }

    #[test]
    fn add_requires_absent_object_member_and_inserts_array_values() {
        let mut document = json!({"object": {"existing": 1}, "array": [1, 3]});
        apply_json_mutation(
            &mut document,
            JsonMutationOperation::Add,
            &JsonPointer::parse("/object/new").unwrap(),
            json!(2),
        )
        .unwrap();
        apply_json_mutation(
            &mut document,
            JsonMutationOperation::Add,
            &JsonPointer::parse("/array/1").unwrap(),
            json!(2),
        )
        .unwrap();
        apply_json_mutation(
            &mut document,
            JsonMutationOperation::Add,
            &JsonPointer::parse("/array/3").unwrap(),
            json!(4),
        )
        .unwrap();
        assert_eq!(
            document,
            json!({"object": {"existing": 1, "new": 2}, "array": [1, 2, 3, 4]})
        );
        assert!(matches!(
            apply_json_mutation(
                &mut document,
                JsonMutationOperation::Add,
                &JsonPointer::parse("/object/existing").unwrap(),
                json!(9),
            ),
            Err(SchemaToolError::Mutation { .. })
        ));
    }

    #[test]
    fn replace_requires_existing_target_and_supports_root() {
        let mut document = json!({"object": {"value": 1}, "array": [1]});
        apply_json_mutation(
            &mut document,
            JsonMutationOperation::Replace,
            &JsonPointer::parse("/object/value").unwrap(),
            json!(2),
        )
        .unwrap();
        apply_json_mutation(
            &mut document,
            JsonMutationOperation::Replace,
            &JsonPointer::parse("/array/0").unwrap(),
            json!(3),
        )
        .unwrap();
        assert_eq!(document, json!({"object": {"value": 2}, "array": [3]}));
        apply_json_mutation(
            &mut document,
            JsonMutationOperation::Replace,
            &JsonPointer::root(),
            json!({"root": true}),
        )
        .unwrap();
        assert_eq!(document, json!({"root": true}));
    }

    #[test]
    fn mutation_errors_report_exact_token_coordinates_and_remain_atomic() {
        let cases = [
            ("/missing/child", 1),
            ("/object/missing/child", 2),
            ("/array/01/value", 2),
            ("/array/1", 2),
        ];
        for (raw_pointer, expected_token_index) in cases {
            let mut document = json!({"object": {}, "array": [0]});
            let before = document.clone();
            let error = apply_json_mutation(
                &mut document,
                JsonMutationOperation::Replace,
                &JsonPointer::parse(raw_pointer).unwrap(),
                json!(9),
            )
            .unwrap_err();
            assert!(matches!(
                error,
                SchemaToolError::Mutation { token_index, .. }
                    if token_index == expected_token_index
            ));
            assert_eq!(document, before);
        }
    }

    #[test]
    fn mutation_rejects_root_add_dash_missing_and_out_of_bounds() {
        let cases = [
            (JsonMutationOperation::Add, "", json!(null)),
            (JsonMutationOperation::Add, "/array/-", json!(2)),
            (JsonMutationOperation::Add, "/array/2", json!(2)),
            (JsonMutationOperation::Replace, "/array/1", json!(2)),
            (JsonMutationOperation::Replace, "/missing", json!(2)),
        ];
        for (operation, raw_pointer, value) in cases {
            let mut document = json!({"array": [1]});
            let before = document.clone();
            let pointer = JsonPointer::parse(raw_pointer).unwrap();
            assert!(matches!(
                apply_json_mutation(&mut document, operation, &pointer, value),
                Err(SchemaToolError::Mutation { .. })
            ));
            assert_eq!(document, before, "failed mutation must not partially apply");
        }
    }
}
