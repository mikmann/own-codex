use super::*;
use pretty_assertions::assert_eq;

fn args(values: &[&str]) -> Vec<OsString> {
    values.iter().map(OsString::from).collect()
}

#[test]
fn shared_state_scan_accepts_global_position_and_stops_at_double_dash() {
    assert_eq!(
        shared_state_request(args(&[
            "codex-extended-workflow",
            "resume",
            "--shared-state"
        ])),
        SharedStateRequest::Shared
    );
    assert_eq!(
        shared_state_request(args(&[
            "codex-extended-workflow",
            "exec",
            "--",
            "--shared-state",
        ])),
        SharedStateRequest::Default
    );
}

#[test]
fn extended_defaults_to_isolated_without_inheriting_a_parent_scope() {
    assert_eq!(
        resolve_state_root_mode(
            ProductFlavor::Extended,
            SharedStateRequest::Default,
            /*codex_home*/ None,
        )
        .expect("default scope"),
        StateRootMode::Isolated
    );
    assert_eq!(
        resolve_state_root_mode(
            ProductFlavor::Extended,
            SharedStateRequest::Default,
            /*codex_home*/ None,
        )
        .expect("extended child chooses its own default scope"),
        StateRootMode::Isolated
    );
}

#[test]
fn explicit_codex_home_is_unambiguous_without_shared_flag() {
    assert_eq!(
        resolve_state_root_mode(
            ProductFlavor::Extended,
            SharedStateRequest::Default,
            Some(OsStr::new("custom-home")),
        )
        .expect("explicit scope"),
        StateRootMode::Explicit
    );
    assert!(
        resolve_state_root_mode(
            ProductFlavor::Extended,
            SharedStateRequest::Shared,
            Some(OsStr::new("custom-home")),
        )
        .is_err()
    );
}

#[test]
fn compatibility_launcher_rejects_extended_shared_state_flag() {
    assert!(
        resolve_state_root_mode(
            ProductFlavor::Compatibility,
            SharedStateRequest::Shared,
            /*codex_home*/ None,
        )
        .is_err()
    );
}

#[test]
fn compatibility_launcher_ignores_parent_scope_and_preserves_legacy_auth() {
    assert_eq!(
        resolve_state_root_mode(
            ProductFlavor::Compatibility,
            SharedStateRequest::Default,
            /*codex_home*/ None,
        )
        .expect("direct compatibility scope"),
        StateRootMode::Compatibility
    );
    assert_eq!(
        resolve_state_root_mode(
            ProductFlavor::Compatibility,
            SharedStateRequest::Default,
            Some(OsStr::new("custom-home")),
        )
        .expect("compatibility CODEX_HOME retains legacy auth namespace"),
        StateRootMode::Compatibility
    );
}
