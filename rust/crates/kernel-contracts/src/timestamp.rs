//! Canonical RFC 3339 timestamp parsing for contract projection boundaries.

use chrono::{DateTime, SecondsFormat, Utc};
use thiserror::Error;

/// Failure while parsing a timestamp that must be emitted at UTC second precision.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum CanonicalTimestampError {
    /// The input is not an RFC 3339 date-time. Input is parsed exactly and is never trimmed.
    #[error("invalid RFC 3339 timestamp")]
    InvalidRfc3339,
    /// The parsed instant contains a non-zero subsecond component.
    #[error("timestamp contains non-zero fractional seconds")]
    NonZeroFractionalSeconds,
}

/// Parses RFC 3339 text exactly and emits the represented instant as UTC seconds with `Z`.
///
/// Offsets and all-zero fractional seconds are accepted. Non-zero fractional seconds are
/// rejected rather than truncated or rounded.
pub fn canonicalize_rfc3339_seconds(input: &str) -> Result<String, CanonicalTimestampError> {
    let timestamp =
        DateTime::parse_from_rfc3339(input).map_err(|_| CanonicalTimestampError::InvalidRfc3339)?;
    if timestamp.timestamp_subsec_nanos() != 0 {
        return Err(CanonicalTimestampError::NonZeroFractionalSeconds);
    }
    Ok(timestamp
        .with_timezone(&Utc)
        .to_rfc3339_opts(SecondsFormat::Secs, true))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonicalizes_z_offset_and_zero_fraction() {
        assert_eq!(
            canonicalize_rfc3339_seconds("2030-01-01T00:00:00Z").unwrap(),
            "2030-01-01T00:00:00Z"
        );
        assert_eq!(
            canonicalize_rfc3339_seconds("2030-01-01T08:00:00+08:00").unwrap(),
            "2030-01-01T00:00:00Z"
        );
        assert_eq!(
            canonicalize_rfc3339_seconds("2030-01-01T00:00:00.000Z").unwrap(),
            "2030-01-01T00:00:00Z"
        );
    }

    #[test]
    fn rejects_nonzero_fraction_invalid_text_and_whitespace() {
        assert_eq!(
            canonicalize_rfc3339_seconds("2030-01-01T00:00:00.001Z"),
            Err(CanonicalTimestampError::NonZeroFractionalSeconds)
        );
        assert_eq!(
            canonicalize_rfc3339_seconds("not-a-time"),
            Err(CanonicalTimestampError::InvalidRfc3339)
        );
        assert_eq!(
            canonicalize_rfc3339_seconds(" 2030-01-01T00:00:00Z"),
            Err(CanonicalTimestampError::InvalidRfc3339)
        );
    }
}
