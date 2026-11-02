use std::fmt;

use serde::{Deserialize, Serialize};

use crate::TokenError;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Unit {
    Px,
    Rem,
    Em,
    Percent,
}

impl Unit {
    fn as_str(self) -> &'static str {
        match self {
            Unit::Px => "px",
            Unit::Rem => "rem",
            Unit::Em => "em",
            Unit::Percent => "%",
        }
    }
}

const UNIT_HELP: &str = "use a number with px, rem, em or %, such as 16px or 1.5rem";

/// Splits `1.5rem` into its number and unit. A bare `0` is the only unitless
/// value allowed, as in CSS.
fn parse_number_and_unit(input: &str) -> Option<(f32, Unit)> {
    let text = input.trim().to_ascii_lowercase();
    let split = text
        .find(|c: char| c.is_ascii_alphabetic() || c == '%')
        .unwrap_or(text.len());
    let (number, unit) = text.split_at(split);
    let value: f32 = number.parse().ok().filter(|n: &f32| n.is_finite())?;
    let unit = match unit {
        "px" => Unit::Px,
        "rem" => Unit::Rem,
        "em" => Unit::Em,
        "%" => Unit::Percent,
        "" if value == 0.0 => Unit::Px,
        _ => return None,
    };
    Some((value, unit))
}

/// A non-negative CSS length, for sizes such as widths and radii.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Length {
    value: f32,
    unit: Unit,
}

impl Length {
    pub fn parse(input: &str) -> Result<Self, TokenError> {
        let (value, unit) = parse_number_and_unit(input)
            .ok_or_else(|| TokenError::new("length", input, UNIT_HELP))?;
        if value < 0.0 {
            return Err(TokenError::new("length", input, "it can't be negative"));
        }
        Ok(Length { value, unit })
    }

    pub fn to_css(self) -> String {
        format!("{}{}", self.value, self.unit.as_str())
    }

    /// The size in CSS pixels, taking 1rem as 16px. Sizes relative to their
    /// context, `em` and `%`, have no fixed answer.
    pub fn to_px(self) -> Option<f32> {
        match self.unit {
            Unit::Px => Some(self.value),
            Unit::Rem => Some(self.value * 16.0),
            Unit::Em | Unit::Percent => None,
        }
    }
}

impl TryFrom<String> for Length {
    type Error = TokenError;
    fn try_from(s: String) -> Result<Self, TokenError> {
        Length::parse(&s)
    }
}

impl From<Length> for String {
    fn from(l: Length) -> String {
        l.to_css()
    }
}

impl fmt::Display for Length {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_css())
    }
}

/// A CSS length that may be negative, for shadow offsets. It is a separate type
/// so a negative value can't reach a field that requires a size.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Offset {
    value: f32,
    unit: Unit,
}

impl Offset {
    pub fn parse(input: &str) -> Result<Self, TokenError> {
        let (value, unit) = parse_number_and_unit(input)
            .ok_or_else(|| TokenError::new("offset", input, UNIT_HELP))?;
        Ok(Offset { value, unit })
    }

    pub fn to_css(self) -> String {
        format!("{}{}", self.value, self.unit.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_normalizes() {
        for (input, expected) in [
            ("16px", "16px"),
            ("1.5rem", "1.5rem"),
            (" 38REM ", "38rem"),
            ("2em", "2em"),
            ("50%", "50%"),
            ("0", "0px"),
            ("0px", "0px"),
            (".5rem", "0.5rem"),
        ] {
            assert_eq!(Length::parse(input).unwrap().to_css(), expected, "{input}");
        }
    }

    #[test]
    fn converts_fixed_sizes_to_pixels() {
        assert_eq!(Length::parse("12px").unwrap().to_px(), Some(12.0));
        assert_eq!(Length::parse("2rem").unwrap().to_px(), Some(32.0));
        assert_eq!(Length::parse("2em").unwrap().to_px(), None);
        assert_eq!(Length::parse("50%").unwrap().to_px(), None);
    }

    #[test]
    fn rejects_invalid_lengths() {
        for input in [
            "",
            "12",
            "px",
            "64-128px",
            "1.5.2rem",
            "10vw",
            "-4px",
            "inherit",
            "calc(1px + 2px)",
            "12px; color: red",
            "NaNpx",
            "infpx",
            "1e999px",
        ] {
            assert!(Length::parse(input).is_err(), "{input} should be rejected");
        }
    }

    #[test]
    fn offsets_allow_negative_values() {
        assert_eq!(Offset::parse("-1px").unwrap().to_css(), "-1px");
        assert_eq!(Offset::parse("0").unwrap().to_css(), "0px");
        assert!(Offset::parse("-1").is_err());
        assert!(Offset::parse("1px 2px").is_err());
    }
}
