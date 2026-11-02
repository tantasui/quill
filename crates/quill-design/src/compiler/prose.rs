//! Recognizes a few design rules written as prose. Anything that isn't
//! recognized is listed for the author, so nothing is silently dropped. This is
//! deliberately simple phrase matching: no AI, and the result is deterministic.

use super::diagnostic::{Code, Diagnostic, Span};
use super::extract::ProseLine;
use crate::{FontWeight, Length};

/// More "not applied" notes than this would bury everything else.
const MAX_NOT_APPLIED: usize = 40;

#[derive(Debug, Default)]
pub(crate) struct Rules {
    pub heading_weight: Option<FontWeight>,
    pub no_shadow: bool,
    pub square_corners: bool,
    pub page_width: Option<(Length, Span)>,
}

fn contains_any(text: &str, phrases: &[&str]) -> bool {
    phrases.iter().any(|p| text.contains(p))
}

/// Sections that describe the whole system, rather than one component's example.
fn section_states_rules(context: &str) -> bool {
    let excluded = ["example", "prompt", "component", "button", "card"];
    !contains_any(context, &excluded)
}

/// Sections whose every line is meant as a rule.
fn is_rule_section(context: &str) -> bool {
    contains_any(
        context,
        &[
            "do's",
            "dos",
            "don't",
            "dont",
            "rule",
            "guideline",
            "constraint",
            "principle",
            "guardrail",
            "avoid",
        ],
    )
}

/// A sentence that reads as an instruction wherever it appears. "No" only counts
/// at the start: "No shadows" is a rule, "gives no values" is a description.
fn reads_as_rule(lower: &str) -> bool {
    let padded = format!(" {lower} ");
    lower.starts_with("no ")
        || contains_any(
            &padded,
            &[
                " never ",
                " always ",
                " don't ",
                " do not ",
                " avoid ",
                " must ",
                " only use ",
            ],
        )
}

fn weight_rule(lower: &str) -> Option<FontWeight> {
    let never_bold = [
        "never bold",
        "no bold",
        "avoid bold",
        "never use bold",
        "don't use bold",
        "do not use bold",
        "without bold",
    ];
    if contains_any(lower, &never_bold) {
        return FontWeight::new(400).ok();
    }
    if lower.contains("weight") && contains_any(lower, &["only", "single", "always", "one weight"])
    {
        let number = lower
            .split(|c: char| !c.is_ascii_digit())
            .filter(|n| n.len() == 3)
            .find_map(|n| n.parse::<u16>().ok())
            .filter(|n| n % 100 == 0)?;
        return FontWeight::new(number).ok();
    }
    None
}

fn shadow_rule(lower: &str) -> bool {
    let phrases = [
        "no box-shadow",
        "no box shadow",
        "no shadow",
        "never use shadow",
        "never use box",
        "avoid shadow",
        "avoid box-shadow",
        "without shadow",
        "don't use shadow",
        "do not use shadow",
    ];
    contains_any(lower, &phrases)
}

fn corner_rule(lower: &str) -> bool {
    let phrases = [
        "no rounded",
        "no border-radius",
        "no border radius",
        "no radius",
        "sharp corners",
        "square corners",
        "never round",
    ];
    contains_any(lower, &phrases)
}

fn page_width_rule(line: &str, lower: &str) -> Option<Length> {
    if !contains_any(
        lower,
        &["max-width", "max width", "page width", "container width"],
    ) {
        return None;
    }
    line.split_whitespace()
        .map(|word| word.trim_matches(|c: char| !c.is_alphanumeric() && c != '%'))
        .find_map(|word| {
            let is_sized = word.ends_with("px") || word.ends_with("rem");
            is_sized.then(|| Length::parse(word).ok()).flatten()
        })
}

pub(crate) fn apply(lines: &[ProseLine], diagnostics: &mut Vec<Diagnostic>) -> Rules {
    let mut rules = Rules::default();
    let mut not_applied = 0;
    for line in lines {
        if !section_states_rules(&line.context) {
            continue;
        }
        let lower = line.text.to_lowercase();
        // A rule that repeats an earlier one still counts as applied.
        let mut matched = None;
        if let Some(weight) = weight_rule(&lower) {
            let weight = *rules.heading_weight.get_or_insert(weight);
            matched = Some(format!("heading weight is now {}", weight.to_css()));
        } else if shadow_rule(&lower) {
            rules.no_shadow = true;
            matched = Some("shadows are off".to_string());
        } else if corner_rule(&lower) {
            rules.square_corners = true;
            matched = Some("corners are square".to_string());
        } else if let Some(width) = page_width_rule(&line.text, &lower) {
            rules.page_width.get_or_insert((width, line.span));
            matched = Some(format!("page width is now {width}"));
        }
        if let Some(effect) = matched {
            diagnostics.push(
                Diagnostic::new(
                    Code::RuleApplied,
                    format!("applied the rule “{}”: {effect}", preview(&line.text)),
                )
                .at(line.span),
            );
        } else if is_rule_section(&line.context) || reads_as_rule(&lower) {
            not_applied += 1;
            if not_applied <= MAX_NOT_APPLIED {
                diagnostics.push(
                    Diagnostic::new(
                        Code::RuleNotApplied,
                        format!("this rule wasn't applied: “{}”", preview(&line.text)),
                    )
                    .at(line.span),
                );
            }
        }
    }
    if not_applied > MAX_NOT_APPLIED {
        diagnostics.push(Diagnostic::new(
            Code::RuleNotApplied,
            format!(
                "{} more rules weren't applied",
                not_applied - MAX_NOT_APPLIED
            ),
        ));
    }
    rules
}

fn preview(text: &str) -> String {
    const MAX: usize = 90;
    if text.chars().count() <= MAX {
        text.to_string()
    } else {
        let cut: String = text.chars().take(MAX).collect();
        format!("{}…", cut.trim_end())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(text: &str, context: &str) -> ProseLine {
        ProseLine {
            text: text.to_string(),
            span: Span {
                line: 7,
                column: 1,
                len: 1,
            },
            context: context.to_string(),
        }
    }

    fn run(lines: &[ProseLine]) -> (Rules, Vec<Diagnostic>) {
        let mut diagnostics = Vec::new();
        let rules = apply(lines, &mut diagnostics);
        (rules, diagnostics)
    }

    #[test]
    fn recognizes_weight_rules() {
        for (text, weight) in [
            ("Never bold.", "400"),
            ("Never use bold in headlines.", "400"),
            ("Use only weight 400 at every size.", "400"),
            (
                "A single weight (500) forces hierarchy through size.",
                "500",
            ),
            ("Headlines are always weight 300.", "300"),
        ] {
            let (rules, diagnostics) = run(&[line(text, "typography")]);
            assert_eq!(rules.heading_weight.unwrap().to_css(), weight, "{text}");
            assert_eq!(diagnostics[0].code, Code::RuleApplied, "{text}");
            assert_eq!(diagnostics[0].span.unwrap().line, 7);
        }
    }

    #[test]
    fn recognizes_shadow_corner_and_width_rules() {
        let (rules, _) = run(&[
            line("No box-shadow anywhere.", "do's and don'ts"),
            line("Use sharp corners everywhere.", "shapes"),
            line("Page max-width: 1120px.", "layout"),
        ]);
        assert!(rules.no_shadow);
        assert!(rules.square_corners);
        assert_eq!(rules.page_width.unwrap().0.to_css(), "1120px");
    }

    #[test]
    fn a_repeated_rule_still_counts_as_applied() {
        let (rules, diagnostics) = run(&[
            line("A single weight (400) forces hierarchy.", "typography"),
            line("Never bold.", "do's and don'ts"),
        ]);
        assert_eq!(rules.heading_weight.unwrap().to_css(), "400");
        let codes: Vec<_> = diagnostics.iter().map(|d| d.code).collect();
        assert_eq!(codes, [Code::RuleApplied, Code::RuleApplied]);
    }

    #[test]
    fn component_examples_are_not_rules() {
        let (rules, diagnostics) = run(&[
            line(
                "No shadow — border does the elevation work.",
                "example component prompts",
            ),
            line("Never bold the label.", "components / buttons"),
        ]);
        assert!(!rules.no_shadow);
        assert!(rules.heading_weight.is_none());
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn lists_rules_it_cannot_apply() {
        let (_, diagnostics) = run(&[
            line("Use warm photography of real people.", "do's and don'ts"),
            line("Never animate on scroll.", "overview"),
            line("The brand feels calm and quiet.", "overview"),
            line("Like a piano in an empty room.", "overview"),
            line("This page gives no values at all.", "overview"),
        ]);
        assert_eq!(diagnostics.len(), 2, "plain description isn't a rule");
        assert_eq!(diagnostics[0].code, Code::RuleNotApplied);
        assert!(diagnostics[0].message.contains("warm photography"));
    }

    #[test]
    fn caps_the_list_of_unapplied_rules() {
        let lines: Vec<_> = (0..100)
            .map(|i| line(&format!("Rule {i}."), "rules"))
            .collect();
        let (_, diagnostics) = run(&lines);
        assert_eq!(diagnostics.len(), MAX_NOT_APPLIED + 1);
        assert!(diagnostics.last().unwrap().message.contains("60 more"));
    }

    #[test]
    fn shortens_long_rules_in_messages() {
        let long = "x".repeat(300);
        let (_, diagnostics) = run(&[line(&long, "rules")]);
        assert!(diagnostics[0].message.chars().count() < 150);
    }
}
