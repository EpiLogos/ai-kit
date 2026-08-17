//! Human durations (`10m`, `2s`, `90s`, `1h30m`) for manifests.
//!
//! `std::time::Duration` has no TOML-friendly representation, and a bare integer
//! would leave the unit ambiguous in hand-written manifests.

use std::fmt;
use std::time::Duration;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::error::{err, AikitError, Result};

/// A duration written the way a person writes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct HumanDuration(Duration);

impl HumanDuration {
    pub fn new(d: Duration) -> Self {
        Self(d)
    }

    pub fn from_secs(secs: u64) -> Self {
        Self(Duration::from_secs(secs))
    }

    pub fn as_duration(self) -> Duration {
        self.0
    }

    pub fn as_secs(self) -> u64 {
        self.0.as_secs()
    }

    pub fn as_millis(self) -> u128 {
        self.0.as_millis()
    }

    pub fn parse(raw: &str) -> Result<Self> {
        let raw = raw.trim();
        if raw.is_empty() {
            return err("duration.malformed", "an empty string is not a duration");
        }
        let mut total = Duration::ZERO;
        let mut digits = String::new();
        let mut saw_unit = false;
        for ch in raw.chars() {
            if ch.is_ascii_digit() {
                digits.push(ch);
                continue;
            }
            if digits.is_empty() {
                return err(
                    "duration.malformed",
                    format!("`{raw}` has a unit without a preceding number"),
                );
            }
            let value: u64 = digits.parse().map_err(|_| {
                AikitError::new("duration.malformed", format!("`{raw}` has an unparseable number"))
            })?;
            digits.clear();
            saw_unit = true;
            let unit = match ch {
                'h' => Duration::from_secs(3600),
                'm' => Duration::from_secs(60),
                's' => Duration::from_secs(1),
                _ => {
                    return err(
                        "duration.malformed",
                        format!("`{raw}` uses unknown unit `{ch}` (expected h, m, s or ms)"),
                    )
                }
            };
            total += unit * value as u32;
        }
        if !digits.is_empty() {
            if saw_unit {
                return err(
                    "duration.malformed",
                    format!("`{raw}` ends with a number that has no unit"),
                );
            }
            // A bare number means seconds; this keeps `timeout = 30` working.
            let value: u64 = digits.parse().map_err(|_| {
                AikitError::new("duration.malformed", format!("`{raw}` is not a number"))
            })?;
            total = Duration::from_secs(value);
        }
        Ok(Self(total))
    }
}

impl fmt::Display for HumanDuration {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let secs = self.0.as_secs();
        if secs == 0 {
            return write!(f, "0s");
        }
        let (h, m, s) = (secs / 3600, (secs % 3600) / 60, secs % 60);
        let mut out = String::new();
        if h > 0 {
            out.push_str(&format!("{h}h"));
        }
        if m > 0 {
            out.push_str(&format!("{m}m"));
        }
        if s > 0 {
            out.push_str(&format!("{s}s"));
        }
        f.write_str(&out)
    }
}

impl Serialize for HumanDuration {
    fn serialize<S: Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for HumanDuration {
    fn deserialize<D: Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Raw {
            Text(String),
            Secs(u64),
        }
        match Raw::deserialize(d)? {
            Raw::Text(t) => HumanDuration::parse(&t).map_err(serde::de::Error::custom),
            Raw::Secs(n) => Ok(HumanDuration::from_secs(n)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_units_used_by_the_specification() {
        assert_eq!(HumanDuration::parse("10m").unwrap().as_secs(), 600);
        assert_eq!(HumanDuration::parse("2s").unwrap().as_secs(), 2);
        assert_eq!(HumanDuration::parse("90s").unwrap().as_secs(), 90);
        assert_eq!(HumanDuration::parse("1h30m").unwrap().as_secs(), 5400);
    }

    #[test]
    fn a_bare_number_means_seconds() {
        assert_eq!(HumanDuration::parse("30").unwrap().as_secs(), 30);
    }

    #[test]
    fn a_trailing_unitless_number_after_a_unit_is_an_error() {
        assert_eq!(
            HumanDuration::parse("1m30").unwrap_err().code(),
            "duration.malformed"
        );
    }

    #[test]
    fn rejects_unknown_units_and_empty_input() {
        assert!(HumanDuration::parse("5x").is_err());
        assert!(HumanDuration::parse("").is_err());
        assert!(HumanDuration::parse("m").is_err());
    }

    #[test]
    fn renders_back_to_a_parseable_string() {
        for raw in ["10m", "2s", "1h30m", "90s"] {
            let d = HumanDuration::parse(raw).unwrap();
            let round = HumanDuration::parse(&d.to_string()).unwrap();
            assert_eq!(d, round, "round trip failed for {raw}");
        }
    }
}
