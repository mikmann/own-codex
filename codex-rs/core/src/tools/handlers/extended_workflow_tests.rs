use super::*;

#[test]
fn result_kind_names_are_stable() {
    assert_eq!(ResultKind::Expert.as_str(), "expert");
    assert_eq!(ResultKind::Reviewer.as_str(), "reviewer");
    assert_eq!(ResultKind::Ticket.as_str(), "ticket");
    assert_eq!(ResultKind::Concept.as_str(), "concept");
}

#[test]
fn reviewer_findings_use_warning_events_for_yellow_rendering() {
    assert!(matches!(
        reviewer_finding_event("[Critical] finding".to_string()),
        EventMsg::Warning(_)
    ));
}
