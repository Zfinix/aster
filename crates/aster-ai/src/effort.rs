//! How much reasoning to ask a thinking model for.

use std::fmt;
use std::str::FromStr;

use serde::Deserialize;

/// Reasoning budget, lowest to highest. Providers that do not support thinking
/// ignore it; `Off` asks for none at all.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Effort {
    #[serde(alias = "none")]
    Off,
    #[default]
    Low,
    Medium,
    High,
}

impl Effort {
    pub const ALL: [Effort; 4] = [Effort::Off, Effort::Low, Effort::Medium, Effort::High];

    pub fn as_str(self) -> &'static str {
        match self {
            Effort::Off => "off",
            Effort::Low => "low",
            Effort::Medium => "medium",
            Effort::High => "high",
        }
    }

    /// The next level up, wrapping at the top. Drives the TUI's `/effort`.
    pub fn next(self) -> Effort {
        match self {
            Effort::Off => Effort::Low,
            Effort::Low => Effort::Medium,
            Effort::Medium => Effort::High,
            Effort::High => Effort::Off,
        }
    }
}

impl FromStr for Effort {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "off" | "none" | "" => Ok(Effort::Off),
            "low" => Ok(Effort::Low),
            "medium" | "med" => Ok(Effort::Medium),
            "high" => Ok(Effort::High),
            other => Err(format!(
                "unknown effort {other:?} (expected off, low, medium, or high)"
            )),
        }
    }
}

impl fmt::Display for Effort {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_spellings_users_type() {
        assert_eq!("HIGH".parse(), Ok(Effort::High));
        assert_eq!("med".parse(), Ok(Effort::Medium));
        assert_eq!("none".parse(), Ok(Effort::Off));
        assert!("turbo".parse::<Effort>().is_err());
    }

    #[test]
    fn next_wraps_at_the_top() {
        assert_eq!(Effort::High.next(), Effort::Off);
        assert_eq!(Effort::Low.next(), Effort::Medium);
    }
}
