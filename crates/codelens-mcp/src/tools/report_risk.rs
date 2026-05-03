pub(crate) fn infer_risk_level(
    summary: &str,
    top_findings: &[String],
    next_actions: &[String],
) -> &'static str {
    let evidence = format!("{} {}", summary, top_findings.join(" ")).to_ascii_lowercase();
    let actions = next_actions.join(" ").to_ascii_lowercase();
    if [
        "blocker",
        "circular",
        "destructive",
        "breaking",
        "high risk",
        "error",
        "failing",
    ]
    .iter()
    .any(|needle| evidence.contains(needle))
        || ["blocker", "destructive", "breaking", "error", "failing"]
            .iter()
            .any(|needle| actions.contains(needle))
        || has_positive_cycle_evidence(top_findings)
    {
        "high"
    } else if top_findings.len() >= 3
        || ["coupling", "dead code", "stale"]
            .iter()
            .any(|needle| evidence.contains(needle))
        || (["impact", "risk"]
            .iter()
            .any(|needle| evidence.contains(needle))
            && has_positive_numeric_signal(top_findings))
    {
        "medium"
    } else {
        "low"
    }
}

fn has_positive_cycle_evidence(top_findings: &[String]) -> bool {
    top_findings.iter().any(|finding| {
        let finding = finding.to_ascii_lowercase();
        if finding.contains("circular") {
            return true;
        }
        if !finding.contains("cycle") {
            return false;
        }
        positive_count_before_word(&finding, "cycle").unwrap_or(true)
    })
}

fn positive_count_before_word(text: &str, word: &str) -> Option<bool> {
    let index = text.find(word)?;
    let before = text[..index].trim_end();
    let digits = before
        .chars()
        .rev()
        .take_while(|ch| ch.is_ascii_digit())
        .collect::<String>()
        .chars()
        .rev()
        .collect::<String>();
    if digits.is_empty() {
        return None;
    }
    digits.parse::<u64>().ok().map(|count| count > 0)
}

fn has_positive_numeric_signal(top_findings: &[String]) -> bool {
    top_findings
        .iter()
        .any(|finding| finding.chars().any(|ch| matches!(ch, '1'..='9')))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn infer_risk_level_does_not_upgrade_zero_cycle_action_to_high() {
        let findings = vec!["0 importer(s), 0 impacted file(s), 0 cycle hit(s)".to_owned()];
        let actions = vec![
            "Check cycle hits before moving ownership boundaries".to_owned(),
            "Semantic enrichment unavailable; report uses structural evidence only.".to_owned(),
        ];

        assert_eq!(
            infer_risk_level(
                "Module boundary report with inbound/outbound and structural risk.",
                &findings,
                &actions,
            ),
            "low"
        );
    }

    #[test]
    fn infer_risk_level_keeps_positive_cycle_findings_high() {
        let findings = vec!["1 importer(s), 3 impacted file(s), 2 cycle hit(s)".to_owned()];

        assert_eq!(
            infer_risk_level("Module boundary report.", &findings, &[]),
            "high"
        );
    }
}
