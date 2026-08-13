use super::*;

#[test]
fn defaults_are_independent_and_bounded() {
    let config = ExtendedWorkflowConfig::default();
    assert!(!config.enabled);
    assert_eq!((config.experts.min, config.experts.max), (2, 4));
    assert_eq!((config.reviewers.min, config.reviewers.max), (2, 4));
    assert!(config.experts.enabled);
    assert!(config.reviewers.enabled);
}

#[test]
fn launcher_default_only_applies_when_enabled_is_not_configured() {
    assert!(
        ExtendedWorkflowConfig::resolve_with_default(None, true)
            .expect("launcher default")
            .enabled
    );
    let omitted = ExtendedWorkflowToml::default();
    assert!(
        ExtendedWorkflowConfig::resolve_with_default(Some(&omitted), true)
            .expect("omitted enabled value")
            .enabled
    );
    let explicitly_disabled = ExtendedWorkflowToml {
        enabled: Some(false),
        ..Default::default()
    };
    assert!(
        !ExtendedWorkflowConfig::resolve_with_default(Some(&explicitly_disabled), true)
            .expect("explicitly disabled")
            .enabled
    );
}

#[test]
fn enabled_zero_zero_means_active_with_no_automatic_agents() {
    let config = ExtendedWorkflowToml {
        experts: Some(WorkflowProcessToml {
            enabled: Some(true),
            min: Some(0),
            max: Some(0),
            profiles: Some(Vec::new()),
            ..Default::default()
        }),
        ..Default::default()
    };
    let resolved = ExtendedWorkflowConfig::resolve(Some(&config)).expect("valid config");
    assert!(resolved.experts.enabled);
    assert_eq!((resolved.experts.min, resolved.experts.max), (0, 0));
    assert!(resolved.experts.profiles.is_empty());
}

#[test]
fn validates_all_min_max_boundaries() {
    for (min, max, valid) in [
        (0, 0, true),
        (0, 6, true),
        (6, 6, true),
        (4, 2, false),
        (0, 7, false),
    ] {
        let config = ExtendedWorkflowToml {
            experts: Some(WorkflowProcessToml {
                min: Some(min),
                max: Some(max),
                ..Default::default()
            }),
            ..Default::default()
        };
        assert_eq!(
            ExtendedWorkflowConfig::resolve(Some(&config)).is_ok(),
            valid,
            "min={min}, max={max}"
        );
    }
}

#[test]
fn rejects_duplicate_reviewer_roles() {
    let duplicate = WorkflowAgentProfileToml {
        name: "second".to_string(),
        role: "Security".to_string(),
        ..Default::default()
    };
    let config = ExtendedWorkflowToml {
        reviewers: Some(WorkflowProcessToml {
            min: Some(1),
            max: Some(2),
            profiles: Some(vec![
                WorkflowAgentProfileToml {
                    name: "first".to_string(),
                    role: "security".to_string(),
                    ..Default::default()
                },
                duplicate,
            ]),
            ..Default::default()
        }),
        ..Default::default()
    };
    let error = ExtendedWorkflowConfig::resolve(Some(&config)).expect_err("duplicate role");
    assert!(error.contains("profile roles must be unique"));
}

#[test]
fn expert_and_reviewer_processes_can_both_be_disabled() {
    let config = ExtendedWorkflowToml {
        experts: Some(WorkflowProcessToml {
            enabled: Some(false),
            profiles: Some(Vec::new()),
            ..Default::default()
        }),
        reviewers: Some(WorkflowProcessToml {
            enabled: Some(false),
            profiles: Some(Vec::new()),
            ..Default::default()
        }),
        ..Default::default()
    };
    let resolved = ExtendedWorkflowConfig::resolve(Some(&config)).expect("disabled config");
    assert!(!resolved.experts.enabled);
    assert!(!resolved.reviewers.enabled);
}

#[test]
fn profile_models_are_not_restricted_by_a_hard_coded_allowlist() {
    let parsed: ExtendedWorkflowToml = toml::from_str(
        r#"
[experts]
min = 1
max = 1

[[experts.profiles]]
name = "specialist"
role = "Specialist"
model = "future-model"
reasoning_effort = "high"
"#,
    )
    .expect("known reasoning enum and arbitrary model name");
    let resolved = ExtendedWorkflowConfig::resolve(Some(&parsed)).expect("resolved profile");
    assert_eq!(
        resolved.experts.profiles[0].model.as_deref(),
        Some("future-model")
    );
    assert_eq!(
        resolved.experts.profiles[0].reasoning_effort,
        Some(ReasoningEffort::High)
    );
}

#[test]
fn zero_advisory_budgets_are_rejected_with_the_config_path() {
    let config = ExtendedWorkflowToml {
        reviewers: Some(WorkflowProcessToml {
            round_budget_tokens: Some(0),
            ..Default::default()
        }),
        ..Default::default()
    };
    let error = ExtendedWorkflowConfig::resolve(Some(&config)).expect_err("zero budget");
    assert!(error.contains("extended_workflow.reviewers.round_budget_tokens"));
}

#[test]
fn rejects_more_than_six_profiles_even_when_the_process_is_disabled() {
    let profiles = (0..7)
        .map(|index| WorkflowAgentProfileToml {
            name: format!("expert-{index}"),
            role: format!("Role {index}"),
            ..Default::default()
        })
        .collect();
    let config = ExtendedWorkflowToml {
        experts: Some(WorkflowProcessToml {
            enabled: Some(false),
            profiles: Some(profiles),
            ..Default::default()
        }),
        ..Default::default()
    };

    let error = ExtendedWorkflowConfig::resolve(Some(&config)).expect_err("too many profiles");
    assert!(error.contains("must contain at most 6 profiles"));
}

#[test]
fn rejects_oversized_profile_fields_with_the_config_path() {
    let config = ExtendedWorkflowToml {
        experts: Some(WorkflowProcessToml {
            min: Some(1),
            max: Some(1),
            profiles: Some(vec![WorkflowAgentProfileToml {
                name: "x".repeat(EXTENDED_WORKFLOW_MAX_PROFILE_FIELD_BYTES + 1),
                role: "Architecture".to_string(),
                ..Default::default()
            }]),
            ..Default::default()
        }),
        ..Default::default()
    };

    let error = ExtendedWorkflowConfig::resolve(Some(&config)).expect_err("oversized name");
    assert!(error.contains("extended_workflow.experts.profiles[0].name"));
    assert!(error.contains("at most 256 UTF-8 bytes"));
}
