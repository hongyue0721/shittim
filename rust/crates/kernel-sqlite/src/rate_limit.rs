//! Transaction-bound production implementation of `domain_policy::RateLimitPort`.

use crate::WriteTransaction;
use chrono::Utc;
use domain_policy::{
    PolicyError, PolicyErrorCode, RateLimitConsume, RateLimitPort, RateLimitPreview,
    RateLimitRequest,
};
use rusqlite::params;

/// Rate-limit authority borrowed from an active `BEGIN IMMEDIATE` transaction.
#[derive(Debug, Clone, Copy)]
pub struct TransactionRateLimitPort<'transaction, 'connection> {
    transaction: &'transaction WriteTransaction<'connection>,
}

impl<'transaction, 'connection> TransactionRateLimitPort<'transaction, 'connection> {
    pub(crate) const fn new(transaction: &'transaction WriteTransaction<'connection>) -> Self {
        Self { transaction }
    }
}

impl RateLimitPort for TransactionRateLimitPort<'_, '_> {
    fn preview(&self, request: &RateLimitRequest<'_>) -> Result<RateLimitPreview, PolicyError> {
        let facts = validated_facts(request)?;
        let used = count_consumptions(self.transaction.connection(), request, facts.window_start)?;
        Ok(if used < request.count {
            RateLimitPreview::Available
        } else {
            RateLimitPreview::Exceeded
        })
    }

    fn check_and_consume(
        &self,
        request: &RateLimitRequest<'_>,
    ) -> Result<RateLimitConsume, PolicyError> {
        let facts = validated_facts(request)?;
        let used = count_consumptions(self.transaction.connection(), request, facts.window_start)?;
        if used >= request.count {
            return Ok(RateLimitConsume::Exceeded);
        }
        self.transaction
            .connection()
            .execute(
                "INSERT INTO policy_rate_limit_consumptions(\
                    schema_version, rule_id, rule_revision, rate_key, consumed_at_micros\
                 ) VALUES (1, ?1, ?2, ?3, ?4)",
                params![
                    request.rule_id,
                    request.rule_revision,
                    request.key.0,
                    facts.instant_micros,
                ],
            )
            .map_err(|_| rate_limit_error("failed to persist rate-limit consumption"))?;
        Ok(RateLimitConsume::Consumed)
    }
}

#[derive(Debug, Clone, Copy)]
struct ValidatedFacts {
    instant_micros: i64,
    window_start: i64,
}

fn validated_facts(request: &RateLimitRequest<'_>) -> Result<ValidatedFacts, PolicyError> {
    if request.rule_id.is_empty()
        || request.rule_revision <= 0
        || request.key.0.is_empty()
        || request.count <= 0
        || request.window_seconds <= 0
    {
        return Err(rate_limit_error("invalid rate-limit request facts"));
    }
    let instant_micros = request.instant.timestamp_micros();
    let window_micros = request
        .window_seconds
        .checked_mul(1_000_000)
        .ok_or_else(|| rate_limit_error("rate-limit window overflows microseconds"))?;
    let window_start = instant_micros
        .checked_sub(window_micros)
        .ok_or_else(|| rate_limit_error("rate-limit window precedes supported timestamp range"))?;
    if request.instant.with_timezone(&Utc).timestamp_micros() != instant_micros {
        return Err(rate_limit_error("invalid rate-limit instant"));
    }
    Ok(ValidatedFacts {
        instant_micros,
        window_start,
    })
}

fn count_consumptions(
    connection: &rusqlite::Connection,
    request: &RateLimitRequest<'_>,
    window_start: i64,
) -> Result<i64, PolicyError> {
    connection
        .query_row(
            "SELECT COUNT(*) FROM policy_rate_limit_consumptions \
             WHERE rule_id = ?1 AND rule_revision = ?2 AND rate_key = ?3 \
               AND consumed_at_micros > ?4",
            params![
                request.rule_id,
                request.rule_revision,
                request.key.0,
                window_start,
            ],
            |row| row.get(0),
        )
        .map_err(|_| rate_limit_error("failed to query rate-limit consumptions"))
}

fn rate_limit_error(message: &'static str) -> PolicyError {
    PolicyError {
        code: PolicyErrorCode::RateLimitFailed,
        message: message.to_string(),
    }
}
