use std::fmt;

use serde::{Deserialize, Serialize};

use crate::{Color, Length, Offset, TokenError};

/// A box shadow, or none. Only a single, outer shadow is supported.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub enum Shadow {
    None,
    Drop {
        x: Offset,
        y: Offset,
        blur: Length,
        spread: Offset,
        color: Color,
    },
}

const HELP: &str = "use `none`, or offsets, optional blur and spread, and a color, such as 0 1px 2px #00000020";

impl Shadow {
    pub fn parse(input: &str) -> Result<Self, TokenError> {
        let error = |reason: &str| TokenError::new("shadow", input, reason);
        let text = input.trim();
        if text.eq_ignore_ascii_case("none") {
            return Ok(Shadow::None);
        }
        let parts = split_outside_parens(text);
        if parts.iter().any(|p| p.eq_ignore_ascii_case("inset")) {
            return Err(error("inset shadows aren't supported"));
        }
        if text.contains(',') && parts.iter().any(|p| p.ends_with(',')) {
            return Err(error("only a single shadow is supported"));
        }
        // The color comes first or last in CSS. Numbers never parse as a color.
        let (color, lengths) = match parts.split_last() {
            Some((last, rest)) if Color::parse(last).is_ok() => (Color::parse(last), rest),
            _ => match parts.split_first() {
                Some((first, rest)) if Color::parse(first).is_ok() => (Color::parse(first), rest),
                _ => return Err(error(HELP)),
            },
        };
        let color = color.map_err(|_| error(HELP))?;
        let (x, y, blur, spread) = match lengths {
            [x, y] => (*x, *y, "0", "0"),
            [x, y, blur] => (*x, *y, *blur, "0"),
            [x, y, blur, spread] => (*x, *y, *blur, *spread),
            _ => return Err(error(HELP)),
        };
        Ok(Shadow::Drop {
            x: Offset::parse(x)?,
            y: Offset::parse(y)?,
            blur: Length::parse(blur)?,
            spread: Offset::parse(spread)?,
            color,
        })
    }

    pub fn to_css(self) -> String {
        match self {
            Shadow::None => "none".to_string(),
            Shadow::Drop {
                x,
                y,
                blur,
                spread,
                color,
            } => format!(
                "{} {} {} {} {}",
                x.to_css(),
                y.to_css(),
                blur.to_css(),
                spread.to_css(),
                color.to_css()
            ),
        }
    }
}

/// Splits on whitespace, keeping `rgb(0 0 0 / 50%)` as one part.
fn split_outside_parens(text: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let (mut depth, mut start) = (0usize, None);
    for (i, c) in text.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            _ => {}
        }
        if c.is_whitespace() && depth == 0 {
            if let Some(s) = start.take() {
                parts.push(&text[s..i]);
            }
        } else if start.is_none() {
            start = Some(i);
        }
    }
    if let Some(s) = start {
        parts.push(&text[s..]);
    }
    parts
}

impl TryFrom<String> for Shadow {
    type Error = TokenError;
    fn try_from(s: String) -> Result<Self, TokenError> {
        Shadow::parse(&s)
    }
}

impl From<Shadow> for String {
    fn from(s: Shadow) -> String {
        s.to_css()
    }
}

impl fmt::Display for Shadow {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_css())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_normalizes() {
        for (input, expected) in [
            ("none", "none"),
            ("NONE", "none"),
            ("0 1px #000", "0px 1px 0px 0px #000000"),
            ("0 1px 2px #000", "0px 1px 2px 0px #000000"),
            ("0 1px 2px -1px #000", "0px 1px 2px -1px #000000"),
            ("#000 0 1px", "0px 1px 0px 0px #000000"),
            (
                "0 1px 2px rgba(16, 24, 40, 0.05)",
                "0px 1px 2px 0px #1018280d",
            ),
            ("0 1px 2px rgb(0 0 0 / 50%)", "0px 1px 2px 0px #00000080"),
        ] {
            assert_eq!(Shadow::parse(input).unwrap().to_css(), expected, "{input}");
        }
    }

    #[test]
    fn rejects_invalid_shadows() {
        for input in [
            "",
            "soft",
            "0 1px",
            "#000",
            "1px #000",
            "0 1px -2px #000",
            "0 1px 2px 3px 4px #000",
            "inset 0 1px #000",
            "0 1px #000, 0 2px #111",
            "0 1px #000; color: red",
        ] {
            assert!(Shadow::parse(input).is_err(), "{input} should be rejected");
        }
    }
}
