//! Explicit relevance accounting, independent of scheduling or forgetting policy.
use qilbee_core::{Error, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum AccountingOrigin {
    Native,
    LegacyAdopted,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct AccountingCheckpoint {
    pub at_millis: i64,
    pub active_rate_per_hour: Option<f64>,
    pub origin: AccountingOrigin,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct AccountedRelevance {
    pub score: f64,
    pub access_count: u32,
    pub last_accessed: i64,
    /// None means that prior accounting history is unknown, not never applied.
    #[serde(default)]
    pub checkpoint: Option<AccountingCheckpoint>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AccountingOutcome {
    pub adopted_legacy: bool,
    pub decayed_millis: u64,
    pub access_count_saturated: bool,
}

impl Default for AccountedRelevance {
    fn default() -> Self {
        Self::new()
    }
}

impl AccountedRelevance {
    pub fn new() -> Self {
        Self::new_at(chrono::Utc::now().timestamp_millis())
    }

    /// Compatibility access helper retaining the historical 0.1 boost.
    /// Use access_at to supply an explicit application policy and controlled time.
    pub fn access(&mut self) -> Result<AccountingOutcome> {
        self.access_at(chrono::Utc::now().timestamp_millis(), 0.1)
    }

    pub fn decay(&mut self, rate: f64) -> Result<AccountingOutcome> {
        self.decay_at(chrono::Utc::now().timestamp_millis(), rate)
    }

    pub fn should_forget(&self, min_relevance: f64) -> bool {
        self.score < min_relevance
    }

    pub fn new_at(now: i64) -> Self {
        Self {
            score: 1.0,
            access_count: 0,
            last_accessed: now,
            checkpoint: Some(AccountingCheckpoint {
                at_millis: now,
                active_rate_per_hour: None,
                origin: AccountingOrigin::Native,
            }),
        }
    }

    pub fn from_legacy(score: f64, access_count: u32, last_accessed: i64) -> Self {
        Self {
            score,
            access_count,
            last_accessed,
            checkpoint: None,
        }
    }

    fn validate(&self, now: i64) -> Result<()> {
        if !self.score.is_finite() || !(0.0..=1.0).contains(&self.score) {
            return Err(Error::ValidationError(
                "Invalid persisted relevance score".into(),
            ));
        }
        if self
            .checkpoint
            .is_some_and(|c| c.at_millis < self.last_accessed)
        {
            return Err(Error::ValidationError(
                "Inconsistent relevance accounting chronology".into(),
            ));
        }
        if now < self.last_accessed || self.checkpoint.is_some_and(|c| now < c.at_millis) {
            return Err(Error::ValidationError(
                "Relevance event precedes a recorded event".into(),
            ));
        }
        if let Some(rate) = self.checkpoint.and_then(|c| c.active_rate_per_hour) {
            Self::validate_rate(rate)?;
        }
        Ok(())
    }

    fn validate_rate(rate: f64) -> Result<()> {
        if !rate.is_finite() || rate < 0.0 {
            return Err(Error::ValidationError(
                "Decay rate must be finite and nonnegative".into(),
            ));
        }
        Ok(())
    }

    fn settle(&mut self, now: i64, initial_rate: Option<f64>) -> AccountingOutcome {
        let adopted_legacy = self.checkpoint.is_none();
        let mut checkpoint = self.checkpoint.unwrap_or(AccountingCheckpoint {
            at_millis: now,
            active_rate_per_hour: None,
            origin: AccountingOrigin::LegacyAdopted,
        });
        let rate = checkpoint.active_rate_per_hour.or_else(|| {
            (checkpoint.origin == AccountingOrigin::Native)
                .then_some(initial_rate)
                .flatten()
        });
        // i128 subtraction avoids overflow for the complete i64 timestamp range.
        let elapsed = (i128::from(now) - i128::from(checkpoint.at_millis)) as u64;
        let decayed_millis = if let Some(rate) = rate {
            self.score *= (-rate * (elapsed as f64 / 3_600_000.0)).exp();
            elapsed
        } else {
            0
        };
        checkpoint.at_millis = now;
        self.checkpoint = Some(checkpoint);
        AccountingOutcome {
            adopted_legacy,
            decayed_millis,
            access_count_saturated: false,
        }
    }

    /// Settle the previously activated rate, then activate the requested rate.
    /// The first native call may account its known interval at the supplied rate;
    /// legacy adoption never infers a rate for historical time.
    pub fn decay_at(&mut self, now: i64, rate_per_hour: f64) -> Result<AccountingOutcome> {
        self.validate(now)?;
        Self::validate_rate(rate_per_hour)?;
        let mut next = *self;
        let outcome = next.settle(now, Some(rate_per_hour));
        next.checkpoint.as_mut().unwrap().active_rate_per_hour = Some(rate_per_hour);
        *self = next;
        Ok(outcome)
    }

    /// Apply an explicitly supplied boost after settling any activated rate.
    /// Access without an activated rate never invents a rate or enables forgetting.
    pub fn access_at(&mut self, now: i64, boost: f64) -> Result<AccountingOutcome> {
        self.validate(now)?;
        if !boost.is_finite() || !(0.0..=1.0).contains(&boost) {
            return Err(Error::ValidationError(
                "Access boost must be finite and between zero and one".into(),
            ));
        }
        let mut next = *self;
        let mut outcome = next.settle(now, None);
        next.score = (next.score + boost).min(1.0);
        outcome.access_count_saturated = next.access_count == u32::MAX;
        next.access_count = next.access_count.saturating_add(1);
        next.last_accessed = now;
        *self = next;
        Ok(outcome)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const HOUR: i64 = 3_600_000;
    fn near(a: f64, b: f64) {
        assert!((a - b).abs() < 1e-12, "{a} != {b}");
    }
    #[test]
    fn partitioned_time_and_equal_timestamps_do_not_double_charge() {
        let mut once = AccountedRelevance::new_at(0);
        let mut split = once;
        once.decay_at(24 * HOUR, 0.1).unwrap();
        for h in 1..=24 {
            split.decay_at(h * HOUR, 0.1).unwrap();
        }
        near(once.score, split.score);
        let unchanged = split;
        let outcome = split.decay_at(24 * HOUR, 0.1).unwrap();
        assert_eq!(outcome.decayed_millis, 0);
        assert_eq!(split, unchanged);
    }
    #[test]
    fn access_and_prospective_rate_changes_preserve_timeline() {
        let mut sparse = AccountedRelevance::new_at(0);
        let mut dense = sparse;
        sparse.decay_at(0, 0.1).unwrap();
        dense.decay_at(0, 0.1).unwrap();
        for h in 1..4 {
            dense.decay_at(h * HOUR, 0.1).unwrap();
        }
        sparse.access_at(4 * HOUR, 0.05).unwrap();
        dense.access_at(4 * HOUR, 0.05).unwrap();
        dense.decay_at(5 * HOUR, 0.1).unwrap();
        sparse.decay_at(6 * HOUR, 0.2).unwrap();
        dense.decay_at(6 * HOUR, 0.2).unwrap();
        dense.decay_at(7 * HOUR, 0.2).unwrap();
        sparse.decay_at(8 * HOUR, 0.2).unwrap();
        dense.decay_at(8 * HOUR, 0.2).unwrap();
        near(sparse.score, dense.score);
        assert_eq!(sparse.access_count, 1);
    }
    #[test]
    fn legacy_adoption_preserves_score_and_is_distinguishable() {
        let mut legacy = AccountedRelevance::from_legacy(0.42, 7, 0);
        let adoption = legacy.decay_at(24 * HOUR, 0.1).unwrap();
        assert!(adoption.adopted_legacy);
        assert_eq!(adoption.decayed_millis, 0);
        assert_eq!(legacy.score, 0.42);
        let next = legacy.decay_at(25 * HOUR, 0.1).unwrap();
        assert!(!next.adopted_legacy);
        near(legacy.score, 0.42 * (-0.1_f64).exp());
    }
    #[test]
    fn access_before_rate_activation_never_guesses_legacy_history() {
        let mut legacy = AccountedRelevance::from_legacy(0.42, 7, 0);
        assert!(legacy.access_at(12 * HOUR, 0.03).unwrap().adopted_legacy);
        near(legacy.score, 0.45);
        assert_eq!(legacy.checkpoint.unwrap().active_rate_per_hour, None);
        assert_eq!(legacy.decay_at(24 * HOUR, 0.1).unwrap().decayed_millis, 0);
        near(legacy.score, 0.45);
        let mut native = AccountedRelevance::new_at(0);
        native.score = 0.5;
        native.access_at(12 * HOUR, 0.0).unwrap();
        native.decay_at(24 * HOUR, 0.1).unwrap();
        near(native.score, 0.5 * (-1.2_f64).exp());
    }
    #[test]
    fn invalid_events_leave_every_field_unchanged_and_counts_saturate() {
        let mut value = AccountedRelevance::new_at(100);
        value.decay_at(200, 0.1).unwrap();
        let original = value;
        for rate in [f64::NAN, f64::INFINITY, -0.1] {
            assert!(value.decay_at(300, rate).is_err());
            assert_eq!(value, original);
        }
        assert!(value.decay_at(199, 0.1).is_err());
        assert_eq!(value, original);
        assert!(value.access_at(199, 0.1).is_err());
        assert_eq!(value, original);
        for boost in [f64::NAN, f64::INFINITY, -0.1, 1.1] {
            assert!(value.access_at(300, boost).is_err());
            assert_eq!(value, original);
        }
        value.access_count = u32::MAX;
        assert!(value.access_at(300, 0.1).unwrap().access_count_saturated);
        assert_eq!(value.access_count, u32::MAX);
    }
}

/// An explicit application-selected mutation; no scheduling or deletion is implied.
#[derive(Debug, Clone, Copy)]
pub enum RelevanceChange {
    Decay {
        at_millis: i64,
        rate_per_hour: f64,
    },
    Access {
        at_millis: i64,
        boost: f64,
    },
    /// Observe the wall clock only after the backend acquires its mutation lock.
    AccessNow {
        boost: f64,
    },
    /// Observe the wall clock only after the backend acquires its mutation lock.
    DecayNow {
        rate_per_hour: f64,
    },
}
impl RelevanceChange {
    pub(crate) fn apply(self, relevance: &mut AccountedRelevance) -> Result<AccountingOutcome> {
        match self {
            Self::Decay {
                at_millis,
                rate_per_hour,
            } => relevance.decay_at(at_millis, rate_per_hour),
            Self::Access { at_millis, boost } => relevance.access_at(at_millis, boost),
            Self::AccessNow { boost } => {
                relevance.access_at(chrono::Utc::now().timestamp_millis(), boost)
            }
            Self::DecayNow { rate_per_hour } => {
                relevance.decay_at(chrono::Utc::now().timestamp_millis(), rate_per_hour)
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct RelevanceUpdate {
    pub episode: crate::episode::Episode,
    pub outcome: AccountingOutcome,
}
