//! Wall-clock timestamp with nanosecond precision.
//!
//! Wraps [`SystemTime`] — the canonical Rust wall-clock type — so that
//! no precision is lost in internal storage. Serializes to RFC 3339
//! (an ISO 8601 profile) in UTC. Displays in the local timezone when
//! the platform provides one.

use std::time::SystemTime;

use chrono::{DateTime, Local, SecondsFormat, Utc};
use serde::{Deserialize, Serialize};

/// Wall-clock timestamp with nanosecond precision.
///
/// Internally a [`SystemTime`], which is `Copy` and carries full
/// nanosecond resolution on all supported platforms.
///
/// - **Serde**: RFC 3339 with nanoseconds, always UTC (`"…Z"` suffix).
/// - **Display**: RFC 3339 in the local timezone (or UTC if unavailable).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Timestamp(SystemTime);

impl Timestamp {
    /// Capture the current wall-clock time.
    pub fn now() -> Self {
        Self(SystemTime::now())
    }
}

impl From<SystemTime> for Timestamp {
    fn from(t: SystemTime) -> Self {
        Self(t)
    }
}

impl From<Timestamp> for SystemTime {
    fn from(t: Timestamp) -> Self {
        t.0
    }
}

// ---------------------------------------------------------------------------
// Display — local time, ISO 8601
// ---------------------------------------------------------------------------

impl std::fmt::Display for Timestamp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let local: DateTime<Local> = self.0.into();
        write!(f, "{}", local.to_rfc3339_opts(SecondsFormat::Nanos, false))
    }
}

// ---------------------------------------------------------------------------
// Serde — always UTC so the on-disk format is timezone-independent
// ---------------------------------------------------------------------------

impl Serialize for Timestamp {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let utc: DateTime<Utc> = self.0.into();
        serializer.serialize_str(&utc.to_rfc3339_opts(SecondsFormat::Nanos, true))
    }
}

impl<'de> Deserialize<'de> for Timestamp {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        let dt = DateTime::parse_from_rfc3339(&s).map_err(serde::de::Error::custom)?;
        let system_time: SystemTime = dt.with_timezone(&Utc).into();
        Ok(Self(system_time))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_through_serde() {
        let ts = Timestamp::now();
        let json = serde_json::to_string(&ts).unwrap();
        let back: Timestamp = serde_json::from_str(&json).unwrap();
        assert_eq!(ts, back);
    }

    #[test]
    fn serialized_form_is_rfc3339_utc() {
        let ts = Timestamp::now();
        let json = serde_json::to_string(&ts).unwrap();
        // Unquote the JSON string.
        let raw = json.trim_matches('"');
        assert!(raw.ends_with('Z'), "expected UTC suffix, got: {raw}");
        // Must parse back.
        DateTime::parse_from_rfc3339(raw).expect("valid RFC 3339");
    }

    #[test]
    fn display_uses_local_offset() {
        let ts = Timestamp::now();
        let displayed = ts.to_string();
        // Local offset is platform-dependent, but the string must be
        // valid RFC 3339 regardless.
        DateTime::parse_from_rfc3339(&displayed).expect("display must produce valid RFC 3339");
    }
}
