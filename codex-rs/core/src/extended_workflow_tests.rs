use super::*;

const TEST_TURN_ID: &str = "turn-test-1";

#[test]
fn simple_answer_uses_no_experts_by_default() {
    let plan = plan_turn(
        "Was bedeutet HTTP?",
        TEST_TURN_ID,
        &ExtendedWorkflowConfig::default(),
    );
    assert_eq!(plan.classification.class, RequestClass::SimpleAnswer);
    assert!(plan.experts.selected.is_empty());
}

#[test]
fn assignment_subject_excludes_only_the_appended_result_contract() {
    let marker = WORKFLOW_RESULT_CONTRACT_MARKER;
    let task = format!("Inspect marker handling{marker}inside the user task");
    let persisted_message = format!("{task}{marker}Result contract: {{...}}");

    assert_eq!(workflow_assignment_subject(&persisted_message), task);
    assert_eq!(workflow_assignment_subject("plain task"), "plain task");
}

#[test]
fn assignment_subject_extracts_the_bound_root_request() {
    let assignment = format!(
        "Root request SHA-256: digest\nRoot request:\nImplement the lifecycle gate.\n\nAssigned expert role: Architecture\nAnalyze this exact request.{WORKFLOW_RESULT_CONTRACT_MARKER}Result contract: {{...}}",
    );

    assert_eq!(
        workflow_assignment_subject(&assignment),
        "Implement the lifecycle gate.",
    );
}

#[test]
fn complex_security_answer_can_use_experts() {
    let plan = plan_turn(
        "Erkläre die Security- und Architekturfolgen einer unbekannten Permission.",
        TEST_TURN_ID,
        &ExtendedWorkflowConfig::default(),
    );
    assert_eq!(plan.classification.class, RequestClass::SimpleAnswer);
    assert!(plan.experts.selected.len() >= 2);
}

#[test]
fn work_order_uses_configured_range() {
    let plan = plan_turn(
        "Implementiere CLI, Tests und Sicherheitsvalidierung.",
        TEST_TURN_ID,
        &ExtendedWorkflowConfig::default(),
    );
    assert_eq!(plan.classification.class, RequestClass::WorkOrder);
    assert!((2..=4).contains(&plan.experts.selected.len()));
}

#[test]
fn concept_is_detected_without_change_signal() {
    let classification = classify_request("Erstelle ein Architekturkonzept und eine Roadmap.");
    assert_eq!(classification.class, RequestClass::Concept);
}

#[test]
fn concept_with_explicit_implementation_is_a_work_order() {
    let classification =
        classify_request("Erstelle ein Konzept und implementiere danach die ausgewählte Lösung.");
    assert_eq!(classification.class, RequestClass::WorkOrder);
}

#[test]
fn english_and_german_change_imperatives_are_work_orders_without_substring_false_positives() {
    for input in [
        "Add a command.",
        "Create a file.",
        "Change the parser.",
        "Remove the flag.",
        "Delete the cache.",
        "Fix the resume path.",
        "Implement the handler.",
        "Modify the validation gate.",
        "Edit the configuration.",
        "Please continue.",
        "Go ahead.",
        "Implementiere den Handler.",
        "Ändere die Konfiguration.",
        "Lösche den veralteten Befehl.",
        "Bitte mach weiter.",
        "Setze die Umsetzung fort.",
    ] {
        assert_eq!(
            classify_request(input).class,
            RequestClass::WorkOrder,
            "{input}"
        );
    }
    for input in [
        "What is an address?",
        "Explain this fixture.",
        "What does builder mean?",
    ] {
        assert_eq!(
            classify_request(input).class,
            RequestClass::SimpleAnswer,
            "{input}"
        );
    }
    assert_eq!(
        classify_request("Create a concept.").class,
        RequestClass::Concept
    );
}

#[test]
fn zero_zero_starts_no_agents() {
    let mut config = ExtendedWorkflowConfig::default();
    config.experts.min = 0;
    config.experts.max = 0;
    config.experts.profiles.clear();
    let plan = plan_turn("Implementiere eine Änderung.", TEST_TURN_ID, &config);
    assert!(plan.experts.enabled);
    assert!(plan.experts.selected.is_empty());
}

#[test]
fn expert_and_reviewer_processes_can_be_disabled_independently_or_together() {
    for (experts_enabled, reviewers_enabled) in [(false, true), (true, false), (false, false)] {
        let mut config = ExtendedWorkflowConfig::default();
        config.experts.enabled = experts_enabled;
        config.reviewers.enabled = reviewers_enabled;

        let plan = plan_turn("Implementiere eine Änderung.", TEST_TURN_ID, &config);

        assert_eq!(
            (
                plan.experts.enabled,
                plan.experts.selected.is_empty(),
                plan.reviewer_policy_enabled,
            ),
            (experts_enabled, !experts_enabled, reviewers_enabled)
        );
    }
}

#[test]
fn insufficient_budget_blocks_before_spawn() {
    let mut config = ExtendedWorkflowConfig::default();
    config.experts.round_budget_tokens = Some(1);
    let plan = plan_turn("Implementiere eine Änderung.", TEST_TURN_ID, &config);
    assert!(plan.experts.budget_blocked);
    assert!(plan.experts.selected.is_empty());
}

#[test]
fn workflow_budget_does_not_reduce_the_planned_agent_count() {
    let config = ExtendedWorkflowConfig {
        workflow_budget_tokens: Some(1),
        ..Default::default()
    };
    let plan = plan_turn("Implementiere eine Änderung.", TEST_TURN_ID, &config);
    assert_eq!(plan.experts.selected.len(), config.experts.min);
    assert!(!plan.experts.budget_blocked);
    assert_eq!(plan.advisory_workflow_budget, Some(1));
}

#[test]
fn zero_workflow_budget_is_unlimited_for_agent_planning() {
    let mut config = ExtendedWorkflowConfig {
        workflow_budget_tokens: Some(0),
        ..Default::default()
    };
    config.experts.min = 3;
    config.experts.max = 3;
    for profile in &mut config.experts.profiles {
        profile.token_budget = Some(1_500);
    }

    let plan = plan_turn("Change the parser.", TEST_TURN_ID, &config);
    assert_eq!(plan.experts.selected.len(), 3);
    assert!(!plan.experts.budget_blocked);
}

#[test]
fn concept_contract_accepts_an_explicit_complete_design() {
    let validation = validate_concept(
        &WorkflowConcept {
            title: "Design".to_string(),
            objective: "Choose an approach".to_string(),
            context: "Repository context".to_string(),
            decisions: vec!["Use existing plans".to_string()],
            alternatives: vec!["No change".to_string()],
            risks_or_open_questions: vec!["None known".to_string()],
            tickets: Vec::new(),
        },
        "design approach",
    );
    assert!(validation.valid, "{:?}", validation.errors);
}

#[test]
fn reviewer_roles_are_distinct() {
    let plan = plan_reviewers(
        "CLI security concurrency tests changed",
        TEST_TURN_ID,
        &ExtendedWorkflowConfig::default(),
    );
    assert!(distinct_roles(&plan));
}

#[test]
fn workflow_task_names_are_turn_bound_kind_specific_and_agent_path_safe() {
    let config = ExtendedWorkflowConfig::default();
    let first = plan_turn("Implementiere CLI und Security.", "turn-one", &config);
    let second = plan_turn("Implementiere CLI und Security.", "turn-two", &config);
    let reviewers = plan_reviewers("CLI and security changes", "turn-one", &config);
    let first_names = first
        .experts
        .selected
        .iter()
        .map(|profile| profile.task_name.as_str())
        .collect::<HashSet<_>>();
    let second_names = second
        .experts
        .selected
        .iter()
        .map(|profile| profile.task_name.as_str())
        .collect::<HashSet<_>>();
    let reviewer_names = reviewers
        .selected
        .iter()
        .map(|profile| profile.task_name.as_str())
        .collect::<HashSet<_>>();

    assert!(first_names.is_disjoint(&second_names));
    assert!(first_names.is_disjoint(&reviewer_names));
    for task_name in first_names
        .into_iter()
        .chain(second_names)
        .chain(reviewer_names)
    {
        codex_protocol::AgentPath::root()
            .join(task_name)
            .expect("planner-generated task name should be a valid agent path segment");
    }
}

#[test]
fn severity_is_reproducible() {
    assert_eq!(
        severity_for_dimensions(
            FindingImpact::SystemWide,
            FindingLikelihood::Expected,
            FindingRecoverability::Irreversible,
        ),
        FindingSeverity::Critical
    );
    assert_eq!(
        severity_for_dimensions(
            FindingImpact::Local,
            FindingLikelihood::Unlikely,
            FindingRecoverability::Easy,
        ),
        FindingSeverity::Low
    );
}

#[test]
fn incomplete_expert_result_is_rejected() {
    let validation = validate_expert_result(
        &ExpertResult {
            subject: "target".to_string(),
            summary: String::new(),
            findings: Vec::new(),
            recommendations: Vec::new(),
            evidence: Vec::new(),
            uncertainties: Vec::new(),
        },
        "target",
    );
    assert!(!validation.valid);
    assert!(validation.errors.len() >= 4);
}

#[test]
fn expert_result_rejects_blank_items_and_missing_uncertainty_statement() {
    let validation = validate_expert_result(
        &ExpertResult {
            subject: "lifecycle gate".to_string(),
            summary: "Reviewed the lifecycle gate.".to_string(),
            findings: vec![String::new()],
            recommendations: vec!["Keep the gate.".to_string()],
            evidence: vec![WorkflowEvidence {
                file: "src/lib.rs".to_string(),
                symbol: "gate".to_string(),
                explanation: "The gate owns completion.".to_string(),
            }],
            uncertainties: Vec::new(),
        },
        "Implement the lifecycle gate.",
    );

    assert!(!validation.valid);
    assert!(
        validation
            .errors
            .iter()
            .any(|error| error.contains("findings[0]")),
    );
    assert!(
        validation
            .errors
            .iter()
            .any(|error| error.contains("uncertainties")),
    );
}

#[test]
fn passing_reviewer_requires_concrete_evidence() {
    let validation = validate_reviewer_result(
        &ReviewerResult {
            subject: "lifecycle gate".to_string(),
            verdict: ReviewVerdict::Pass,
            summary: "The lifecycle gate matches the request.".to_string(),
            findings: Vec::new(),
            evidence: Vec::new(),
            missing_tests: Vec::new(),
        },
        "Implement the lifecycle gate.",
    );

    assert!(!validation.valid);
    assert!(
        validation
            .errors
            .iter()
            .any(|error| error.contains("evidence")),
    );
}

#[test]
fn contracts_reject_unknown_fields() {
    let result = serde_json::from_value::<ExpertResult>(serde_json::json!({
        "subject": "target",
        "summary": "summary",
        "findings": ["finding"],
        "recommendations": ["recommendation"],
        "evidence": [{"file": "src/lib.rs", "symbol": "run", "explanation": "proof"}],
        "uncertainties": [],
        "unexpected": true
    }));
    assert!(result.is_err());
}

#[test]
fn mismatched_reviewer_severity_is_rejected() {
    let result = ReviewerResult {
        subject: "change".to_string(),
        verdict: ReviewVerdict::NeedsChanges,
        summary: "one issue".to_string(),
        findings: vec![ReviewerFinding {
            severity: FindingSeverity::Low,
            impact: FindingImpact::SystemWide,
            likelihood: FindingLikelihood::Expected,
            recoverability: FindingRecoverability::Irreversible,
            rationale: "unsafe".to_string(),
            file: "src/lib.rs".to_string(),
            symbol: "run".to_string(),
            recommended_fix: "fix it".to_string(),
            missing_tests: vec!["regression".to_string()],
        }],
        evidence: vec![WorkflowEvidence {
            file: "src/lib.rs".to_string(),
            symbol: "run".to_string(),
            explanation: "unsafe path".to_string(),
        }],
        missing_tests: vec!["regression".to_string()],
    };
    let validation = validate_reviewer_result(&result, "change");
    assert!(!validation.valid);
    assert!(
        validation
            .errors
            .iter()
            .any(|error| error.contains("severity"))
    );
}

#[test]
fn pass_verdict_rejects_high_or_critical_findings() {
    let result = ReviewerResult {
        subject: "security change".to_string(),
        verdict: ReviewVerdict::Pass,
        summary: "reviewed security change".to_string(),
        findings: vec![ReviewerFinding {
            severity: FindingSeverity::High,
            impact: FindingImpact::SystemWide,
            likelihood: FindingLikelihood::Plausible,
            recoverability: FindingRecoverability::Costly,
            rationale: "material security risk".to_string(),
            file: "src/lib.rs".to_string(),
            symbol: "run".to_string(),
            recommended_fix: "fail closed".to_string(),
            missing_tests: vec!["security regression".to_string()],
        }],
        evidence: vec![WorkflowEvidence {
            file: "src/lib.rs".to_string(),
            symbol: "run".to_string(),
            explanation: "reachable path".to_string(),
        }],
        missing_tests: vec!["security regression".to_string()],
    };

    let validation = validate_reviewer_result(&result, "security change");
    assert!(!validation.valid);
    assert!(
        validation
            .errors
            .iter()
            .any(|error| error.contains("pass verdict"))
    );
}

#[test]
fn trivial_subject_is_not_accepted_as_relevant() {
    let validation = validate_expert_result(
        &ExpertResult {
            subject: "a".to_string(),
            summary: "architecture result".to_string(),
            findings: vec!["finding".to_string()],
            recommendations: vec!["recommendation".to_string()],
            evidence: vec![WorkflowEvidence {
                file: "src/lib.rs".to_string(),
                symbol: "run".to_string(),
                explanation: "evidence".to_string(),
            }],
            uncertainties: Vec::new(),
        },
        "architecture review",
    );
    assert!(!validation.valid);
    assert!(
        validation
            .errors
            .iter()
            .any(|error| error.contains("three characters"))
    );
}

#[test]
fn runtime_ledger_rejects_unselected_duplicate_and_premature_reviewer_spawns() {
    let config = ExtendedWorkflowConfig::default();
    let turn_plan = plan_turn(
        "Implementiere CLI, Security und Tests.",
        TEST_TURN_ID,
        &config,
    );
    let root_request = "Implementiere CLI, Security und Tests.";
    let runtime = WorkflowTurnRuntimeState::new(&turn_plan, root_request);
    let selected = turn_plan.experts.selected.first().expect("selected expert");

    assert!(runtime.reserve_spawn_for_task(&selected.task_name).is_ok());
    assert!(
        runtime
            .reserve_spawn_for_task(&selected.task_name)
            .expect_err("duplicate")
            .contains("already consumed")
    );
    assert!(
        runtime
            .reserve_spawn_for_task("not_selected")
            .expect_err("unselected")
            .contains("no selected workflow profile")
    );
    let reviewer_plan = plan_reviewers("security and CLI changes", TEST_TURN_ID, &config);
    let reviewer = reviewer_plan.selected.first().expect("selected reviewer");
    assert!(
        runtime
            .reserve_spawn_for_task(&reviewer.task_name)
            .expect_err("reviewer plan missing")
            .contains("no selected workflow profile")
    );
    runtime
        .install_reviewers(&reviewer_plan, "security and CLI changes")
        .expect("install reviewers");
    let remaining_expert = turn_plan
        .experts
        .selected
        .get(1)
        .expect("second selected expert");
    assert_eq!(
        runtime
            .selected_profile_for_task(&remaining_expert.task_name)
            .expect("expert should remain selected after reviewer planning"),
        (WorkflowAgentKind::Expert, remaining_expert.clone())
    );
    assert!(runtime.reserve_spawn_for_task(&reviewer.task_name).is_ok());
}

#[test]
fn workflow_spawn_task_is_root_bound_and_context_budgeted() {
    let root_request = "Change the CLI parser and validate resume behavior.";
    let mut config = ExtendedWorkflowConfig::default();
    let plan = plan_turn(root_request, TEST_TURN_ID, &config);
    let selected = plan.experts.selected.first().expect("selected expert");
    let runtime = WorkflowTurnRuntimeState::new(&plan, root_request);
    let directive = runtime
        .spawn_directive_for_task(&selected.task_name)
        .expect("server-bound task");
    assert!(directive.canonical_task.contains(root_request));
    assert!(directive.canonical_task.contains(&format!(
        "Root request SHA-256: {}",
        plan.root_request_sha256
    )));
    assert!(directive.canonical_task.contains(
        "Support the main agent in creating concepts, making decisions, or creating tickets"
    ));
    assert!(
        directive
            .canonical_task
            .contains(WORKFLOW_RESULT_CONTRACT_MARKER)
    );
    assert!(
        directive
            .canonical_task
            .contains("do not present your contribution as the final decision, concept, or ticket")
    );

    let change_summary = "Updated the CLI parser and added resume coverage.";
    let reviewer_plan = plan_reviewers(change_summary, TEST_TURN_ID, &config);
    runtime
        .install_reviewers(&reviewer_plan, change_summary)
        .expect("reviewer round should install");
    let reviewer = reviewer_plan.selected.first().expect("selected reviewer");
    let reviewer_directive = runtime
        .spawn_directive_for_task(&reviewer.task_name)
        .expect("server-bound reviewer task");
    assert!(reviewer_directive.canonical_task.contains(
        "Verify their purpose, necessity, alignment with the root request and accepted ticket, and functional and technical correctness"
    ));
    assert!(reviewer_directive.canonical_task.contains("work read-only"));
    assert!(
        reviewer_directive
            .canonical_task
            .contains(WORKFLOW_RESULT_CONTRACT_MARKER)
    );
    assert!(
        reviewer_directive
            .canonical_task
            .contains("do not modify the workspace")
    );

    for profile in &mut config.experts.profiles {
        profile.context_budget = Some(1);
    }
    let constrained_plan = plan_turn(root_request, TEST_TURN_ID, &config);
    let constrained_runtime = WorkflowTurnRuntimeState::new(&constrained_plan, root_request);
    let constrained = constrained_plan
        .experts
        .selected
        .first()
        .expect("selected constrained expert");
    assert!(
        constrained_runtime
            .spawn_directive_for_task(&constrained.task_name)
            .expect_err("task must exceed one token")
            .contains("no agent was started")
    );
}

#[test]
fn maximum_bounded_reviewer_task_is_spawnable_and_context_budget_counts_contract() {
    let root_prefix = "Implement the workflow. ";
    let root_request = format!(
        "{root_prefix}{}",
        "r".repeat(MAX_WORKFLOW_ROOT_REQUEST_BYTES - root_prefix.len())
    );
    let summary_prefix = "CLI security and tests. ";
    let change_summary = format!(
        "{summary_prefix}{}",
        "s".repeat(MAX_WORKFLOW_CHANGE_SUMMARY_BYTES - summary_prefix.len())
    );
    let mut config = ExtendedWorkflowConfig::default();
    for (index, profile) in config.reviewers.profiles.iter_mut().enumerate() {
        let prefix = format!("role-{index}-");
        profile.role = format!(
            "{prefix}{}",
            "r".repeat(
                codex_config::extended_workflow::EXTENDED_WORKFLOW_MAX_PROFILE_FIELD_BYTES
                    - prefix.len()
            )
        );
    }

    let turn_plan = plan_turn(&root_request, TEST_TURN_ID, &config);
    assert!(!turn_plan.task_binding_blocked);
    let runtime = WorkflowTurnRuntimeState::new(&turn_plan, &root_request);
    let planning_input = runtime
        .reviewer_planning_input(&change_summary)
        .expect("maximum bounded reviewer planning input");
    assert!(planning_input.len() <= MAX_WORKFLOW_TASK_BYTES);
    let reviewer_plan = plan_reviewers(&change_summary, TEST_TURN_ID, &config);
    runtime
        .install_reviewers(&reviewer_plan, &change_summary)
        .expect("maximum bounded reviewer round should install");
    let reviewer = reviewer_plan.selected.first().expect("selected reviewer");
    let directive = runtime
        .spawn_directive_for_task(&reviewer.task_name)
        .expect("maximum bounded reviewer task should be spawnable");
    assert!(directive.canonical_task.len() <= MAX_COMPLETE_WORKFLOW_TASK_BYTES);
    let (assignment, _contract) = directive
        .canonical_task
        .rsplit_once(WORKFLOW_RESULT_CONTRACT_MARKER)
        .expect("complete task should contain the result contract");
    let assignment_tokens = u64::try_from(assignment.len().div_ceil(4)).expect("token estimate");
    let complete_tokens =
        u64::try_from(directive.canonical_task.len().div_ceil(4)).expect("token estimate");
    assert!(complete_tokens > assignment_tokens);

    for profile in &mut config.reviewers.profiles {
        profile.context_budget = Some(assignment_tokens);
    }
    let constrained_turn_plan = plan_turn(&root_request, TEST_TURN_ID, &config);
    let constrained_runtime = WorkflowTurnRuntimeState::new(&constrained_turn_plan, &root_request);
    let constrained_reviewer_plan = plan_reviewers(&change_summary, TEST_TURN_ID, &config);
    constrained_runtime
        .install_reviewers(&constrained_reviewer_plan, &change_summary)
        .expect("constrained reviewer round should install");
    let constrained_reviewer = constrained_reviewer_plan
        .selected
        .first()
        .expect("selected constrained reviewer");
    assert!(
        constrained_runtime
            .spawn_directive_for_task(&constrained_reviewer.task_name)
            .expect_err("complete task contract must exceed the assignment-only ceiling")
            .contains("complete server-bound task contract")
    );
}

#[test]
fn oversized_reviewer_summary_is_rejected_before_the_round_is_fixed() {
    let config = ExtendedWorkflowConfig::default();
    let root_request = "Implement the CLI workflow and tests.";
    let turn_plan = plan_turn(root_request, TEST_TURN_ID, &config);
    let runtime = WorkflowTurnRuntimeState::new(&turn_plan, root_request);
    let oversized_summary = "s".repeat(MAX_WORKFLOW_CHANGE_SUMMARY_BYTES + 1);
    let oversized_plan = plan_reviewers(&oversized_summary, TEST_TURN_ID, &config);

    assert!(
        runtime
            .reviewer_planning_input(&oversized_summary)
            .expect_err("oversized planning summary")
            .contains("hard limit is 1500")
    );
    assert!(
        runtime
            .install_reviewers(&oversized_plan, &oversized_summary)
            .expect_err("oversized reviewer round")
            .contains("hard limit is 1500")
    );

    let compact_summary = "CLI workflow and test changes.";
    let compact_plan = plan_reviewers(compact_summary, TEST_TURN_ID, &config);
    runtime
        .install_reviewers(&compact_plan, compact_summary)
        .expect("a failed oversized attempt must not fix the reviewer round");
}

#[test]
fn simple_answer_runtime_ledger_rejects_expert_spawn() {
    let config = ExtendedWorkflowConfig::default();
    let turn_plan = plan_turn("Was bedeutet HTTP?", TEST_TURN_ID, &config);
    let runtime = WorkflowTurnRuntimeState::new(&turn_plan, "Was bedeutet HTTP?");
    assert!(turn_plan.experts.selected.is_empty());
    assert!(
        runtime
            .reserve_spawn_for_task("workflow_unknown_expert_1")
            .is_err()
    );
}

#[test]
fn token_estimate_saturates_instead_of_overflowing() {
    let mut config = ExtendedWorkflowConfig::default();
    config.experts.min = 2;
    config.experts.max = 2;
    for profile in &mut config.experts.profiles {
        profile.token_budget = Some(u64::MAX);
    }
    let plan = plan_turn("Implementiere CLI und Tests.", TEST_TURN_ID, &config);
    assert_eq!(plan.experts.estimated_tokens, u64::MAX);
}

#[test]
fn work_order_reviewer_gate_requires_distinct_passing_agents_and_roles() {
    let config = ExtendedWorkflowConfig::default();
    let turn_plan = plan_turn(
        "Implementiere CLI, Security und Tests.",
        TEST_TURN_ID,
        &config,
    );
    let root_request = "Implementiere CLI, Security und Tests.";
    let runtime = WorkflowTurnRuntimeState::new(&turn_plan, root_request);
    assert!(runtime.requires_reviewer_completion());
    let reviewer_plan = plan_reviewers("security and CLI changes", TEST_TURN_ID, &config);
    runtime
        .install_reviewers(&reviewer_plan, "security and CLI changes")
        .expect("reviewer plan should install");
    let required = reviewer_plan.selected.len();
    let unselected = (0..required)
        .map(|index| codex_agent_graph_store::WorkflowValidationRecord {
            validation_key: format!("reviewer:unselected-{index}"),
            result_kind: "reviewer".to_string(),
            agent_thread_id: Some(codex_protocol::ThreadId::new()),
            agent_role: Some(format!("Unselected role {index}")),
            submissions: 1,
            accepted: true,
            review_cycles: 1,
            output_revision: 1,
            output_sha256: format!("unselected-{index}"),
            reviewer_pass: true,
            has_high_or_critical: false,
        })
        .collect::<Vec<_>>();
    assert!(matches!(
        runtime.reviewer_completion_gate(&unselected),
        WorkflowCompletionGate::Continue(_)
    ));
    let validations = reviewer_plan
        .selected
        .iter()
        .enumerate()
        .map(
            |(index, reviewer)| codex_agent_graph_store::WorkflowValidationRecord {
                validation_key: format!("reviewer:{index}"),
                result_kind: "reviewer".to_string(),
                agent_thread_id: Some(codex_protocol::ThreadId::new()),
                agent_role: Some(reviewer.role.clone()),
                submissions: 1,
                accepted: true,
                review_cycles: 1,
                output_revision: 1,
                output_sha256: format!("reviewer-{index}"),
                reviewer_pass: index + 1 < required,
                has_high_or_critical: index + 1 == required,
            },
        )
        .collect::<Vec<_>>();
    assert!(matches!(
        runtime.reviewer_completion_gate(&validations),
        WorkflowCompletionGate::Continue(_)
    ));

    let passing = validations
        .into_iter()
        .map(
            |validation| codex_agent_graph_store::WorkflowValidationRecord {
                reviewer_pass: true,
                has_high_or_critical: false,
                ..validation
            },
        )
        .collect::<Vec<_>>();
    assert_eq!(
        runtime.reviewer_completion_gate(&passing),
        WorkflowCompletionGate::Satisfied
    );
}

#[test]
fn simple_answer_does_not_require_reviewer_completion() {
    let config = ExtendedWorkflowConfig::default();
    let runtime = WorkflowTurnRuntimeState::new(
        &plan_turn("Was bedeutet HTTP?", TEST_TURN_ID, &config),
        "Was bedeutet HTTP?",
    );
    assert!(!runtime.requires_reviewer_completion());
    assert_eq!(
        runtime.reviewer_completion_gate(&[]),
        WorkflowCompletionGate::Satisfied
    );
}
