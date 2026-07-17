//! Identifier helpers shared by target renderers.
//!
//! These helpers are pure string transforms. Language keyword escaping and
//! target-specific symbol policy live in each renderer, not here.

/// Convert an arbitrary label into PascalCase identifier segments.
pub fn to_pascal_case(input: &str) -> String {
    let mut out = String::new();
    let mut capitalize = true;
    for ch in input.chars() {
        if ch.is_ascii_alphanumeric() {
            if capitalize {
                out.extend(ch.to_uppercase());
                capitalize = false;
            } else {
                out.push(ch);
            }
        } else {
            capitalize = true;
        }
    }
    if out.is_empty() {
        "Anonymous".into()
    } else if out.starts_with(|ch: char| ch.is_ascii_digit()) {
        format!("N{out}")
    } else {
        out
    }
}

/// Convert an arbitrary label into snake_case without language keyword escaping.
pub fn to_snake_case(input: &str) -> String {
    let mut out = String::new();
    let mut previous_underscore = true;
    for ch in input.chars() {
        if ch.is_ascii_uppercase() {
            if !previous_underscore && !out.is_empty() {
                out.push('_');
            }
            out.push(ch.to_ascii_lowercase());
            previous_underscore = false;
        } else if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            previous_underscore = false;
        } else if !previous_underscore {
            out.push('_');
            previous_underscore = true;
        }
    }
    let value = out.trim_matches('_');
    if value.is_empty() {
        "value".into()
    } else {
        value.into()
    }
}

/// UPPER_SNAKE_CASE form of [`to_snake_case`].
pub fn to_upper_snake_case(input: &str) -> String {
    to_snake_case(input).to_ascii_uppercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pascal_and_snake_basics() {
        assert_eq!(to_pascal_case("AuditRecord"), "AuditRecord");
        assert_eq!(to_pascal_case("task_create_request"), "TaskCreateRequest");
        assert_eq!(to_snake_case("schemaVersion"), "schema_version");
        assert_eq!(to_snake_case("type"), "type");
        assert_eq!(to_upper_snake_case("EventEnvelope"), "EVENT_ENVELOPE");
    }
}
