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
fn policy_closes_only_agents_with_terminal_validation_outcomes() {
    let policy = ExtendedWorkflowPolicy::try_new(&ExtendedWorkflowConfig::default())
        .expect("default policy should fit");
    assert!(policy.body().contains(
        "After a result is accepted or terminally rejected, call close_agent for that agent"
    ));
    assert!(
        policy
            .body()
            .contains("Never close an agent while a correction is still allowed")
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
