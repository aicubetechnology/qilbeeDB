//! Bi-temporal data handling for QilbeeDB
//!
//! Implements event time and transaction time tracking for agent memory systems.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Event time - when the event actually occurred in the real world
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct EventTime(DateTime<Utc>);

impl EventTime {
    /// Create a new event time from the current moment
    pub fn now() -> Self {
        Self(Utc::now())
    }

    /// Create from a DateTime
    pub fn from_datetime(dt: DateTime<Utc>) -> Self {
        Self(dt)
    }

    /// Create from milliseconds since Unix epoch
    pub fn from_millis(millis: i64) -> Self {
        Self(DateTime::from_timestamp_millis(millis).unwrap_or_else(Utc::now))
    }

    /// Get as DateTime
    pub fn as_datetime(&self) -> DateTime<Utc> {
        self.0
    }

    /// Get as milliseconds since Unix epoch
    pub fn as_millis(&self) -> i64 {
        self.0.timestamp_millis()
    }

    /// Represents the beginning of time (for queries)
    pub fn min() -> Self {
        Self(DateTime::from_timestamp_millis(0).unwrap())
    }

    /// Represents the end of time (for queries)
    pub fn max() -> Self {
        Self(DateTime::from_timestamp_millis(i64::MAX / 1000).unwrap())
    }
}

impl Default for EventTime {
    fn default() -> Self {
        Self::now()
    }
}

/// Transaction time - when the data was stored in the database
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct TransactionTime(DateTime<Utc>);

impl TransactionTime {
    /// Create a new transaction time from the current moment
    pub fn now() -> Self {
        Self(Utc::now())
    }

    /// Create from a DateTime
    pub fn from_datetime(dt: DateTime<Utc>) -> Self {
        Self(dt)
    }

    /// Create from milliseconds since Unix epoch
    pub fn from_millis(millis: i64) -> Self {
        Self(DateTime::from_timestamp_millis(millis).unwrap_or_else(Utc::now))
    }

    /// Get as DateTime
    pub fn as_datetime(&self) -> DateTime<Utc> {
        self.0
    }

    /// Get as milliseconds since Unix epoch
    pub fn as_millis(&self) -> i64 {
        self.0.timestamp_millis()
    }

    /// Represents the beginning of time (for queries)
    pub fn min() -> Self {
        Self(DateTime::from_timestamp_millis(0).unwrap())
    }

    /// Represents the end of time / still valid (for queries)
    pub fn max() -> Self {
        Self(DateTime::from_timestamp_millis(i64::MAX / 1000).unwrap())
    }
}

impl Default for TransactionTime {
    fn default() -> Self {
        Self::now()
    }
}

/// Bi-temporal data wrapper
///
/// Tracks both when an event occurred (event time) and when it was recorded (transaction time).
/// This enables powerful temporal queries for agent memory systems.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BiTemporal<T> {
    /// The actual data
    pub data: T,

    /// When the event actually occurred
    pub event_time: EventTime,

    /// When this record was created/stored
    pub transaction_time: TransactionTime,

    /// When this record was invalidated (None if still valid)
    pub invalidated_at: Option<TransactionTime>,
}

impl<T> BiTemporal<T> {
    /// Create a new bi-temporal record with current timestamps
    pub fn new(data: T) -> Self {
        Self {
            data,
            event_time: EventTime::now(),
            transaction_time: TransactionTime::now(),
            invalidated_at: None,
        }
    }

    /// Create with a specific event time
    pub fn with_event_time(data: T, event_time: EventTime) -> Self {
        Self {
            data,
            event_time,
            transaction_time: TransactionTime::now(),
            invalidated_at: None,
        }
    }

    /// Create with both specific times
    pub fn with_times(
        data: T,
        event_time: EventTime,
        transaction_time: TransactionTime,
    ) -> Self {
        Self {
            data,
            event_time,
            transaction_time,
            invalidated_at: None,
        }
    }

    /// Check if this record is currently valid
    pub fn is_valid(&self) -> bool {
        self.invalidated_at.is_none()
    }

    /// Invalidate this record
    pub fn invalidate(&mut self) {
        self.invalidated_at = Some(TransactionTime::now());
    }

    /// Check if this record was valid at a specific transaction time
    pub fn was_valid_at(&self, at: TransactionTime) -> bool {
        self.transaction_time <= at
            && self
                .invalidated_at
                .map_or(true, |invalidated| invalidated > at)
    }

    /// Check if the event occurred within a time range
    pub fn event_in_range(&self, start: EventTime, end: EventTime) -> bool {
        self.event_time >= start && self.event_time <= end
    }

    /// Get a reference to the data
    pub fn get(&self) -> &T {
        &self.data
    }

    /// Get a mutable reference to the data
    pub fn get_mut(&mut self) -> &mut T {
        &mut self.data
    }

    /// Consume and return the data
    pub fn into_inner(self) -> T {
        self.data
    }
}

/// Temporal range for queries
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TemporalRange {
    pub start: EventTime,
    pub end: EventTime,
}

impl TemporalRange {
    /// Create a new temporal range
    pub fn new(start: EventTime, end: EventTime) -> Self {
        Self { start, end }
    }

    /// Create a range from now going back a duration
    pub fn last_days(days: i64) -> Self {
        let end = EventTime::now();
        let start = EventTime::from_millis(end.as_millis() - days * 24 * 60 * 60 * 1000);
        Self { start, end }
    }

    /// Create a range from now going back hours
    pub fn last_hours(hours: i64) -> Self {
        let end = EventTime::now();
        let start = EventTime::from_millis(end.as_millis() - hours * 60 * 60 * 1000);
        Self { start, end }
    }

    /// Create an unbounded range (all time)
    pub fn all() -> Self {
        Self {
            start: EventTime::min(),
            end: EventTime::max(),
        }
    }

    /// Check if a time is within this range
    pub fn contains(&self, time: EventTime) -> bool {
        time >= self.start && time <= self.end
    }
}

/// Point-in-time query specification
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AsOf {
    /// The event time to query as of
    pub event_time: Option<EventTime>,

    /// The transaction time to query as of (for time-travel queries)
    pub transaction_time: Option<TransactionTime>,
}

impl AsOf {
    /// Query as of the current time
    pub fn now() -> Self {
        Self {
            event_time: None,
            transaction_time: None,
        }
    }

    /// Query as of a specific event time
    pub fn event_time(time: EventTime) -> Self {
        Self {
            event_time: Some(time),
            transaction_time: None,
        }
    }

    /// Query as of a specific transaction time (time travel)
    pub fn transaction_time(time: TransactionTime) -> Self {
        Self {
            event_time: None,
            transaction_time: Some(time),
        }
    }

    /// Query as of both event and transaction time
    pub fn both(event_time: EventTime, transaction_time: TransactionTime) -> Self {
        Self {
            event_time: Some(event_time),
            transaction_time: Some(transaction_time),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread::sleep;
    use std::time::Duration;

    #[test]
    fn test_event_time() {
        let t1 = EventTime::now();
        sleep(Duration::from_millis(10));
        let t2 = EventTime::now();

        assert!(t2 > t1);
        assert!(t1.as_millis() < t2.as_millis());
    }

    #[test]
    fn test_event_time_from_millis() {
        let millis = 1700000000000i64;
        let time = EventTime::from_millis(millis);
        assert_eq!(time.as_millis(), millis);
    }

    #[test]
    fn test_bi_temporal_creation() {
        let data = "test data".to_string();
        let record = BiTemporal::new(data.clone());

        assert!(record.is_valid());
        assert_eq!(record.get(), &data);
    }

    #[test]
    fn test_bi_temporal_invalidation() {
        let mut record = BiTemporal::new("data".to_string());
        assert!(record.is_valid());

        record.invalidate();
        assert!(!record.is_valid());
    }

    #[test]
    fn test_bi_temporal_validity_at() {
        let before = TransactionTime::now();
        sleep(Duration::from_millis(50));

        let mut record = BiTemporal::new("data".to_string());
        let created = record.transaction_time;

        sleep(Duration::from_millis(50));

        // Record should be valid at its creation time
        assert!(record.was_valid_at(created));

        record.invalidate();

        sleep(Duration::from_millis(50));
        let after_invalidate = TransactionTime::now();

        // Before creation - not valid
        assert!(!record.was_valid_at(before));

        // At creation time - still valid
        assert!(record.was_valid_at(created));

        // After invalidation - not valid
        assert!(!record.was_valid_at(after_invalidate));
    }

    #[test]
    fn test_temporal_range() {
        let now = EventTime::now();
        let week_ago = EventTime::from_millis(now.as_millis() - 7 * 24 * 60 * 60 * 1000);
        let two_weeks_ago = EventTime::from_millis(now.as_millis() - 14 * 24 * 60 * 60 * 1000);
        let yesterday = EventTime::from_millis(now.as_millis() - 24 * 60 * 60 * 1000);

        // Create range after getting timestamps
        let range = TemporalRange::last_days(7);

        // Yesterday should definitely be in range
        assert!(range.contains(yesterday));
        assert!(!range.contains(two_weeks_ago));
    }

    #[test]
    fn test_as_of_queries() {
        let now = AsOf::now();
        assert!(now.event_time.is_none());
        assert!(now.transaction_time.is_none());

        let event_query = AsOf::event_time(EventTime::now());
        assert!(event_query.event_time.is_some());

        let tx_query = AsOf::transaction_time(TransactionTime::now());
        assert!(tx_query.transaction_time.is_some());
    }
}
