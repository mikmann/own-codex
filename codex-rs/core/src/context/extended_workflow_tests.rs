use super::*;
use crate::extended_workflow::plan_turn;

#[test]
fn policy_states_that_audit_mode_writes_no_files() {
    let policy = ExtendedWorkflowPolicy::try_new(&ExtendedWorkflowConfig::default())
        .expect("default policy should fit");
    assert!(policy.body().contains("never creates files"));
    assert!(policy.render().starts_with(POLICY_PREFIX));
}

#[test]
fn policy_keeps_experts_advisory_and_the_main_agent_accountable() {
    let policy = ExtendedWorkflowPolicy::try_new(&ExtendedWorkflowConfig::default())
        .expect("default policy should fit");
    assert!(
        policy
            .body()
            .contains("help the main agent create concepts, make decisions, or create tickets")
    );
    assert!(
        policy
            .body()
            .contains("Experts never make the final decision")
    );
    assert!(policy.body().contains(
        "the main agent evaluates and integrates their contributions and remains accountable"
    ));
}

#[test]
fn policy_defines_reviewers_as_an_independent_quality_gate() {
    let policy = ExtendedWorkflowPolicy::try_new(&ExtendedWorkflowConfig::default())
        .expect("default policy should fit");
    assert!(
        policy
            .body()
            .contains("independent post-implementation quality gate")
    );
    assert!(policy.body().contains(
        "purpose, necessity, alignment with the root request and accepted ticket, and functional and technical correctness"
    ));
    assert!(
        policy
            .body()
            .contains("Reviewers inspect and report read-only; they never modify the workspace")
    );
}

#[test]
fn policy_closes_only_agents_with_terminal_validation_outcomes() {
    let policy = ExtendedWorkflowPolicy::try_new(&ExtendedWorkflowConfig::default())
        .expect("default policy should fit");
    assert!(policy.body().contains(
        "After an expert result is accepted or terminally rejected, call close_agent for that agent"
    ));
    assert!(
        policy
            .body()
            .contains("Never close an agent while a correction is still allowed")
    );
    assert!(policy.body().contains(
        "Close a reviewer only after it passes or its validation/review cycles are terminally exhausted"
    ));
}

#[test]
fn policy_requires_clm_safe_windows_commands_and_native_patching() {
    let policy = ExtendedWorkflowPolicy::try_new(&ExtendedWorkflowConfig::default())
        .expect("default policy should fit");
    assert!(policy.body().contains("Constrained Language Mode"));
    assert!(policy.body().contains("avoid [pscustomobject]"));
    assert!(
        policy
            .body()
            .contains("never invoke apply_patch through PowerShell")
    );
}

#[test]
fn turn_context_is_bounded_json() {
    let plan = plan_turn(
        "Was ist Rust?",
        "turn-context-test",
        &ExtendedWorkflowConfig::default(),
    );
    let context = ExtendedWorkflowTurnContext::try_new(&plan).expect("turn plan should fit");
    assert!(context.body().contains("simple_answer"));
    assert!(context.body().len() < 8_192);
}

#[test]
fn oversized_profile_configuration_is_rejected_at_the_context_boundary() {
    use codex_config::extended_workflow::EXTENDED_WORKFLOW_MAX_PROFILE_FIELD_BYTES;
    use codex_config::extended_workflow::WorkflowAgentProfileConfig;

    let profile = |index: usize| WorkflowAgentProfileConfig {
        name: format!(
            "{index}-{}",
            "n".repeat(EXTENDED_WORKFLOW_MAX_PROFILE_FIELD_BYTES - 2)
        ),
        role: format!(
            "{index}-{}",
            "r".repeat(EXTENDED_WORKFLOW_MAX_PROFILE_FIELD_BYTES - 2)
        ),
        agent_type: Some("a".repeat(EXTENDED_WORKFLOW_MAX_PROFILE_FIELD_BYTES)),
        model: Some("m".repeat(EXTENDED_WORKFLOW_MAX_PROFILE_FIELD_BYTES)),
        reasoning_effort: None,
        token_budget: Some(u64::MAX),
        context_budget: Some(u64::MAX),
    };
    let mut config = ExtendedWorkflowConfig::default();
    config.experts.profiles = (0..6).map(profile).collect();
    config.reviewers.profiles = (6..12).map(profile).collect();

    assert!(ExtendedWorkflowPolicy::try_new(&config).is_err());
}
