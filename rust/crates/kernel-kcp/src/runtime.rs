//! Production implementations of handler clock and identity ports.

use crate::{
    ClockError, IdGenerationError, KernelClock, KernelIdGenerator, OpaqueIdPurpose, UuidPurpose,
};
use chrono::{DateTime, Utc};
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Builder;

/// Kernel clock backed by the host operating system clock.
///
/// The returned value is an absolute UTC instant. KCP handlers remain responsible for their
/// documented first-read and completion-read ordering.
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemKernelClock;

impl KernelClock for SystemKernelClock {
    fn now_utc(&self) -> Result<DateTime<Utc>, ClockError> {
        system_time_to_utc(SystemTime::now())
    }
}

fn system_time_to_utc(value: SystemTime) -> Result<DateTime<Utc>, ClockError> {
    let (seconds, nanoseconds) = match value.duration_since(UNIX_EPOCH) {
        Ok(duration) => (
            i64::try_from(duration.as_secs()).map_err(|_| ClockError)?,
            duration.subsec_nanos(),
        ),
        Err(error) => {
            let duration = error.duration();
            let seconds = i64::try_from(duration.as_secs()).map_err(|_| ClockError)?;
            if duration.subsec_nanos() == 0 {
                (seconds.checked_neg().ok_or(ClockError)?, 0)
            } else {
                (
                    seconds
                        .checked_neg()
                        .and_then(|value| value.checked_sub(1))
                        .ok_or(ClockError)?,
                    1_000_000_000 - duration.subsec_nanos(),
                )
            }
        }
    };
    DateTime::<Utc>::from_timestamp(seconds, nanoseconds).ok_or(ClockError)
}

/// Kernel identity generator backed by random UUID version 4 allocations.
///
/// UUID version is an implementation detail rather than a wire-contract promise. Opaque values
/// deliberately expose no purpose prefix or caller-derived material.
#[derive(Debug, Default, Clone, Copy)]
pub struct RandomKernelIdGenerator;

impl KernelIdGenerator for RandomKernelIdGenerator {
    fn next_uuid(&self, _purpose: UuidPurpose) -> Result<String, IdGenerationError> {
        random_uuid_with(fill_from_os)
    }

    fn next_opaque_id(&self, _purpose: OpaqueIdPurpose) -> Result<String, IdGenerationError> {
        random_opaque_id_with(fill_from_os)
    }
}

fn fill_from_os(bytes: &mut [u8]) -> Result<(), IdGenerationError> {
    getrandom::fill(bytes).map_err(|_| IdGenerationError)
}

fn random_uuid_with(
    fill: impl FnOnce(&mut [u8]) -> Result<(), IdGenerationError>,
) -> Result<String, IdGenerationError> {
    let mut bytes = [0_u8; 16];
    fill(&mut bytes)?;
    Ok(Builder::from_random_bytes(bytes).into_uuid().to_string())
}

fn random_opaque_id_with(
    fill: impl FnOnce(&mut [u8]) -> Result<(), IdGenerationError>,
) -> Result<String, IdGenerationError> {
    let mut bytes = [0_u8; 16];
    fill(&mut bytes)?;
    Ok(hex::encode(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use uuid::Uuid;

    #[test]
    fn system_clock_returns_a_utc_instant_near_system_time() {
        let before = DateTime::<Utc>::from(SystemTime::now());
        let actual = SystemKernelClock.now_utc().expect("system clock");
        let after = DateTime::<Utc>::from(SystemTime::now());

        assert!(actual >= before);
        assert!(actual <= after);
    }

    #[test]
    fn system_time_conversion_handles_instants_before_the_epoch() {
        let value = UNIX_EPOCH
            .checked_sub(Duration::new(1, 250_000_000))
            .expect("pre-epoch SystemTime");
        let actual = system_time_to_utc(value).expect("convert pre-epoch instant");

        assert_eq!(actual.timestamp(), -2);
        assert_eq!(actual.timestamp_subsec_nanos(), 750_000_000);
    }

    #[test]
    fn system_time_conversion_rejects_instants_outside_chrono_range() {
        let value = UNIX_EPOCH
            .checked_add(Duration::from_secs(i64::MAX as u64))
            .expect("large SystemTime");

        assert_eq!(system_time_to_utc(value), Err(ClockError));
    }

    #[test]
    fn uuid_allocations_are_valid_for_every_purpose() {
        let generator = RandomKernelIdGenerator;
        let purposes = [
            UuidPurpose::Task,
            UuidPurpose::TaskScope,
            UuidPurpose::ContentOrigin,
            UuidPurpose::KernelReceipt,
            UuidPurpose::CreationProvenance,
            UuidPurpose::AuditRecord,
            UuidPurpose::Event,
        ];
        let values: Vec<_> = purposes
            .into_iter()
            .map(|purpose| generator.next_uuid(purpose).expect("uuid"))
            .collect();

        for value in values {
            let parsed = Uuid::parse_str(&value).expect("generated UUID text");
            assert_eq!(parsed.get_version_num(), 4);
        }
    }

    #[test]
    fn random_source_failure_uses_the_declared_error_channel() {
        let uuid_error = random_uuid_with(|_| Err(IdGenerationError)).expect_err("uuid failure");
        let opaque_error =
            random_opaque_id_with(|_| Err(IdGenerationError)).expect_err("opaque failure");

        assert_eq!(uuid_error, IdGenerationError);
        assert_eq!(opaque_error, IdGenerationError);
    }

    #[test]
    fn opaque_allocations_are_non_empty_and_independent() {
        let generator = RandomKernelIdGenerator;
        let correlation = generator
            .next_opaque_id(OpaqueIdPurpose::Correlation)
            .expect("correlation");
        let dedup = generator
            .next_opaque_id(OpaqueIdPurpose::EventDedup)
            .expect("dedup");

        assert_eq!(correlation.len(), 32);
        assert_eq!(dedup.len(), 32);
        assert!(correlation.bytes().all(|byte| byte.is_ascii_hexdigit()));
        assert!(dedup.bytes().all(|byte| byte.is_ascii_hexdigit()));
    }
}
