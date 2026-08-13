use super::*;

#[test]
fn result_kind_names_are_stable() {
    assert_eq!(ResultKind::Expert.as_str(), "expert");
    assert_eq!(ResultKind::Reviewer.as_str(), "reviewer");
    assert_eq!(ResultKind::Ticket.as_str(), "ticket");
    assert_eq!(ResultKind::Concept.as_str(), "concept");
}

#[test]
fn critical_findings_use_error_events_and_other_severities_use_warnings() {
    assert!(matches!(
        reviewer_finding_event(FindingSeverity::Critical, "critical".to_string()),
        EventMsg::Error(_)
    ));
    for severity in [
        FindingSeverity::Low,
        FindingSeverity::Medium,
        FindingSeverity::High,
    ] {
        assert!(matches!(
            reviewer_finding_event(severity, "warning".to_string()),
            EventMsg::Warning(_)
        ));
    }
}
