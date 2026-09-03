use super::*;
use crate::agent::role::apply_workflow_role_material_to_config;
use crate::agent::role::load_workflow_role_material;
use crate::config::AgentRoleConfig;
use crate::config::Config;
use crate::config::ConfigBuilder;
use crate::extended_workflow::RequestClass;
use codex_config::extended_workflow::WorkflowProfileSource;
use std::fs;
use tempfile::TempDir;

fn classification(domains: &[&str]) -> RequestClassification {
    RequestClassification {
        class: RequestClass::WorkOrder,
        risk: RequestRisk::Routine,
        domains: domains.iter().map(|domain| (*domain).to_string()).collect(),
        has_material_uncertainty: false,
        complexity_score: 3,
        reasons: Vec::new(),
    }
}

fn profile(name: &str, role: &str, selection_keywords: &[&str]) -> WorkflowAgentProfileConfig {
    WorkflowAgentProfileConfig {
        name: name.to_string(),
        role: role.to_string(),
        agent_type: None,
        selection_keywords: selection_keywords
            .iter()
            .map(|keyword| (*keyword).to_string())
            .collect(),
        source: WorkflowProfileSource::Profiles,
        model: None,
        reasoning_effort: None,
        token_budget: None,
        context_budget: None,
    }
}

#[test]
fn profiles_without_keywords_preserve_domain_then_declaration_order() {
    let profiles = vec![
        profile("first", "General", &[]),
        profile("security", "Security", &[]),
        profile("third", "General", &[]),
    ];

    let selected = select_profiles(
        &classification(&["security"]),
        "security change",
        &profiles,
        3,
    );

    assert_eq!(
        selected
            .iter()
            .map(|selection| selection.profile.name.as_str())
            .collect::<Vec<_>>(),
        vec!["security", "first", "third"]
    );
}

#[test]
fn explicit_german_and_english_phrases_outrank_builtin_domain_scores() {
    let profiles = vec![
        profile("security", "Security", &[]),
        profile(
            "database",
            "Database migration",
            &["schema migration", "datenbank"],
        ),
    ];

    for request in [
        "Die Schema Migration muss sicher geplant werden",
        "Die Datenbank benötigt einen Rollback",
    ] {
        let selected = select_profiles(&classification(&["security"]), request, &profiles, 1);
        assert_eq!(selected[0].profile.name, "database");
        assert_eq!(selected[0].matched_keywords.len(), 1);
    }
}

#[test]
fn keyword_matching_uses_lexical_boundaries_instead_of_substrings() {
    let profiles = vec![
        profile("first", "General", &[]),
        profile("sql", "SQL", &["sql"]),
    ];

    let selected = select_profiles(&classification(&[]), "PostgreSQL update", &profiles, 2);

    assert_eq!(selected[0].profile.name, "first");
    assert!(selected[1].matched_keywords.is_empty());
}

#[test]
fn keyword_ties_fall_back_to_domain_score_then_declaration_order() {
    let profiles = vec![
        profile("general", "General", &["migration"]),
        profile("security", "Security", &["migration"]),
        profile("later", "Security", &["migration"]),
    ];

    let selected = select_profiles(&classification(&["security"]), "migration", &profiles, 3);

    assert_eq!(
        selected
            .iter()
            .map(|selection| selection.profile.name.as_str())
            .collect::<Vec<_>>(),
        vec!["security", "later", "general"]
    );
}

async fn config_with_role(role_contents: &str) -> (TempDir, Config) {
    let home = TempDir::new().expect("temporary codex home");
    let agents_dir = home.path().join("agents");
    fs::create_dir_all(&agents_dir).expect("agents directory");
    fs::write(agents_dir.join("custom.toml"), role_contents).expect("role file");
    fs::write(
        home.path().join("config.toml"),
        r#"
[agents.custom]
description = "Custom workflow role"
config_file = "./agents/custom.toml"
"#,
    )
    .expect("config file");
    let config = ConfigBuilder::default()
        .codex_home(home.path().to_path_buf())
        .fallback_cwd(Some(home.path().to_path_buf()))
        .build()
        .await
        .expect("load config");
    (home, config)
}

fn custom_role_plan() -> super::super::WorkflowTurnPlan {
    let mut workflow_config = codex_config::extended_workflow::ExtendedWorkflowConfig {
        enabled: true,
        ..Default::default()
    };
    workflow_config.experts.min = 1;
    workflow_config.experts.max = 1;
    workflow_config.experts.profiles = vec![WorkflowAgentProfileConfig {
        agent_type: Some("custom".to_string()),
        ..profile("custom_profile", "Custom role", &["migration"])
    }];
    super::super::plan_turn(
        "Implement the database migration",
        "role-preflight-turn",
        &workflow_config,
    )
}

#[tokio::test]
async fn preflight_binds_a_trusted_role_and_detects_later_file_drift() {
    let (home, config) =
        config_with_role(r#"developer_instructions = "Inspect database migration safety.""#).await;
    let mut plan = custom_role_plan();

    preflight_process_plan(
        &mut plan.experts,
        &config,
        "Implement the database migration",
    )
    .await
    .expect("role preflight");

    let profile = &plan.experts.selected[0];
    assert_eq!(
        (
            profile.role_source.as_deref(),
            profile.role_preflight,
            profile.role_fingerprint.as_ref().map(String::len),
        ),
        (
            Some("configured-file"),
            super::super::WorkflowRolePreflightStatus::Validated,
            Some(12),
        )
    );
    let mut spawn_config = config.clone();
    apply_profile_role_binding(profile, &mut spawn_config)
        .await
        .expect("unchanged role binding");
    let mut resume_config = config.clone();
    apply_persisted_role_binding(
        &mut resume_config,
        "custom",
        profile
            .role_fingerprint_sha256
            .as_deref()
            .expect("persisted fingerprint"),
    )
    .await
    .expect("unchanged resumed role binding");

    fs::write(
        home.path().join("agents/custom.toml"),
        r#"developer_instructions = "Changed after planning.""#,
    )
    .expect("change role file");
    let mut spawn_config = config.clone();
    let error = apply_profile_role_binding(profile, &mut spawn_config)
        .await
        .expect_err("drift must fail closed");
    assert!(error.contains("changed after preflight"));
    let mut resume_config = config.clone();
    let error = apply_persisted_role_binding(
        &mut resume_config,
        "custom",
        profile
            .role_fingerprint_sha256
            .as_deref()
            .expect("persisted fingerprint"),
    )
    .await
    .expect_err("resume drift must fail closed");
    assert!(error.contains("instead of resuming this child"));
}

#[tokio::test]
async fn validated_role_material_is_applied_without_rereading_the_path() {
    let (home, config) =
        config_with_role(r#"developer_instructions = "Original validated instructions.""#).await;
    let material = load_workflow_role_material(&config, "custom")
        .await
        .expect("validated role material");
    fs::write(
        home.path().join("agents/custom.toml"),
        r#"developer_instructions = "Changed after the validated read.""#,
    )
    .expect("change role file");

    let mut child_config = config.clone();
    apply_workflow_role_material_to_config(&mut child_config, &material)
        .await
        .expect("apply already validated material");

    assert_eq!(
        child_config.developer_instructions.as_deref(),
        Some("Original validated instructions.")
    );
}

#[tokio::test]
async fn preflight_rejects_unknown_roles_and_permission_expansion() {
    let (_home, config) = config_with_role(
        r#"
developer_instructions = "Inspect only."
sandbox_mode = "workspace-write"
"#,
    )
    .await;
    let mut unsafe_plan = custom_role_plan();
    let error = preflight_process_plan(
        &mut unsafe_plan.experts,
        &config,
        "Implement the database migration",
    )
    .await
    .expect_err("permission expansion");
    assert!(error.contains("sandbox_mode"));
    assert!(!error.contains("Inspect only"));

    let mut missing_plan = custom_role_plan();
    missing_plan.experts.selected[0].agent_type = Some("missing".to_string());
    let error = preflight_process_plan(
        &mut missing_plan.experts,
        &config,
        "Implement the database migration",
    )
    .await
    .expect_err("unknown role");
    assert!(error.contains("unknown agent_type 'missing'"));
}

#[tokio::test]
async fn preflight_rejects_role_instructions_above_the_hard_token_limit() {
    let instructions = "x".repeat(32_004);
    let role_contents = format!("developer_instructions = {instructions:?}");
    let (_home, config) = config_with_role(&role_contents).await;
    let mut plan = custom_role_plan();

    let error = preflight_process_plan(
        &mut plan.experts,
        &config,
        "Implement the database migration",
    )
    .await
    .expect_err("oversized role");

    assert!(error.contains("hard limit is 8000"));
    assert!(!error.contains(&instructions));
}

#[tokio::test]
async fn preflight_marks_large_bounded_role_instructions_for_manual_review() {
    let instructions = "x".repeat(4_004);
    let role_contents = format!("developer_instructions = {instructions:?}");
    let (_home, config) = config_with_role(&role_contents).await;
    let mut plan = custom_role_plan();

    preflight_process_plan(
        &mut plan.experts,
        &config,
        "Implement the database migration",
    )
    .await
    .expect("bounded role should pass preflight");

    assert_eq!(
        (
            plan.experts.selected[0].role_instruction_tokens,
            plan.experts.selected[0].role_preflight,
        ),
        (
            1_001,
            super::super::WorkflowRolePreflightStatus::ManualReviewRecommended,
        )
    );
}

#[tokio::test]
async fn preflight_rejects_root_request_and_role_that_exceed_context_budget() {
    let instructions = "x".repeat(4_000);
    let role_contents = format!("developer_instructions = {instructions:?}");
    let (_home, config) = config_with_role(&role_contents).await;
    let mut plan = custom_role_plan();
    plan.experts.selected[0].context_budget = Some(1_000);

    let error = preflight_process_plan(
        &mut plan.experts,
        &config,
        "Implement the database migration",
    )
    .await
    .expect_err("request and role must fit the configured context budget");

    assert!(error.contains("root request and resolved role instructions"));
    assert!(!error.contains(&instructions));
}

#[tokio::test]
async fn preflight_rejects_a_role_file_outside_the_trusted_config_directory() {
    let (_home, mut config) = config_with_role(r#"developer_instructions = "Inspect only.""#).await;
    let outside = TempDir::new().expect("outside role directory");
    let outside_role = outside.path().join("outside.toml");
    fs::write(
        &outside_role,
        r#"developer_instructions = "Read an unrelated file.""#,
    )
    .expect("outside role file");
    config.agent_roles.insert(
        "custom".to_string(),
        AgentRoleConfig {
            description: Some("Outside workflow role".to_string()),
            config_file: Some(outside_role),
            nickname_candidates: None,
        },
    );
    let mut plan = custom_role_plan();

    let error = preflight_process_plan(
        &mut plan.experts,
        &config,
        "Implement the database migration",
    )
    .await
    .expect_err("outside role path must fail closed");

    assert!(error.contains("outside its trusted configuration directory"));
    assert!(!error.contains("Read an unrelated file"));
}
