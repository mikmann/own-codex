use super::*;
use tempfile::TempDir;

fn repository() -> TempDir {
    let temp = tempfile::tempdir().expect("temporary repository");
    fs::create_dir(temp.path().join(".git")).expect("git marker");
    temp
}

#[test]
fn plans_and_applies_complete_empty_repository() {
    let repo = repository();
    let plan = plan_extended_workflow_preparation(repo.path()).expect("plan");
    assert_eq!(plan.action, PrepareAction::Create);
    assert!(
        !repo.path().join(CODEX_DIR).exists(),
        "preview must not write"
    );
    let report = apply_extended_workflow_preparation(&plan).expect("apply");
    assert_eq!(report.created, vec![plan.config_path.clone()]);
    let contents = fs::read_to_string(&plan.config_path).expect("prepared config");
    assert!(contents.contains("[extended_workflow]"));
    assert!(contents.contains("workflow_type = \"auto\""));
    assert!(contents.contains("workflow_budget_tokens = 0"));
    assert!(contents.contains("name = \"expert_1\""));
    assert!(contents.contains("selection_keywords = [\"architecture\""));
    assert!(contents.contains("extended_workflow.experts.additional_profiles"));
    assert!(contents.contains("model = \"gpt-5.6-terra\""));
    assert!(contents.contains("name = \"reviewer_4\""));
    assert!(contents.contains("reasoning_effort = \"max\""));
    assert!(contents.contains("\"session-tokens\", \"workflow-tokens\""));
    assert!(!repo.path().join("AGENTS.md").exists());
    assert!(!repo.path().join(CODEX_DIR).join("agents").exists());
}

#[test]
fn global_preparation_uses_the_selected_home_and_never_overwrites() {
    let home = tempfile::tempdir().expect("global home");
    let plan = plan_global_extended_workflow_preparation(home.path()).expect("global plan");
    assert_eq!(plan.scope, PrepareWorkflowScope::Global);
    assert_eq!(plan.action, PrepareAction::Create);
    assert_eq!(
        plan.config_path,
        fs::canonicalize(home.path())
            .expect("canonical global home")
            .join(CONFIG_FILE)
    );
    apply_extended_workflow_preparation(&plan).expect("global apply");

    let original = fs::read_to_string(&plan.config_path).expect("global config");
    let second = plan_global_extended_workflow_preparation(home.path()).expect("second plan");
    assert_eq!(second.action, PrepareAction::Unchanged);
    apply_extended_workflow_preparation(&second).expect("unchanged apply");
    assert_eq!(
        fs::read_to_string(&plan.config_path).expect("preserved global config"),
        original
    );
}

#[test]
fn global_preparation_creates_a_missing_extended_workflow_home() {
    let parent = tempfile::tempdir().expect("global parent");
    let home = parent.path().join(".codex-extended-workflow");
    let plan = plan_global_extended_workflow_preparation(&home).expect("global plan");
    assert_eq!(plan.scope, PrepareWorkflowScope::Global);
    assert_eq!(plan.action, PrepareAction::Create);
    assert!(!home.exists(), "preview must not create the global home");

    let report = apply_extended_workflow_preparation(&plan).expect("global apply");
    assert_eq!(report.created, vec![plan.config_path.clone()]);
    assert!(plan.config_path.is_file());
}

#[test]
fn existing_codex_configuration_requires_manual_merge_and_stays_byte_identical() {
    let repo = repository();
    let codex_dir = repo.path().join(CODEX_DIR);
    fs::create_dir(&codex_dir).expect("codex dir");
    let original = "model = \"gpt-test\"\n# keep me\n";
    fs::write(codex_dir.join(CONFIG_FILE), original).expect("config");
    let plan = plan_extended_workflow_preparation(repo.path()).expect("plan");
    assert_eq!(plan.action, PrepareAction::Conflict);
    assert!(plan.detail.contains("never replaced automatically"));
    assert!(apply_extended_workflow_preparation(&plan).is_err());
    assert_eq!(
        fs::read_to_string(plan.config_path).expect("config"),
        original
    );
}

#[test]
fn partial_workflow_table_without_enabled_is_reused_under_primary_launcher_default() {
    let repo = repository();
    let codex_dir = repo.path().join(".codex");
    fs::create_dir_all(&codex_dir).expect("codex dir");
    fs::write(
        codex_dir.join("config.toml"),
        "[extended_workflow.experts]\nmin = 1\nmax = 1\n",
    )
    .expect("partial config");

    let plan = plan_extended_workflow_preparation(repo.path()).expect("plan");
    assert_eq!(plan.action, PrepareAction::Unchanged);
    assert!(plan.detail.contains("reused unchanged"));
}

#[test]
fn atomic_create_never_replaces_an_existing_destination() {
    let temp = tempfile::tempdir().expect("temporary directory");
    let destination = temp.path().join("config.toml");
    fs::write(&destination, "user data\n").expect("existing destination");

    let error = write_new_file_atomically(&destination, "workflow data\n")
        .expect_err("existing destination must not be replaced");

    assert_eq!(error.kind(), ErrorKind::AlreadyExists);
    assert_eq!(
        fs::read_to_string(destination).expect("preserved destination"),
        "user data\n"
    );
}

#[test]
fn repeated_invocation_is_unchanged() {
    let repo = repository();
    let first = plan_extended_workflow_preparation(repo.path()).expect("first plan");
    apply_extended_workflow_preparation(&first).expect("first apply");
    let second = plan_extended_workflow_preparation(repo.path()).expect("second plan");
    assert_eq!(second.action, PrepareAction::Unchanged);
    let report = apply_extended_workflow_preparation(&second).expect("second apply");
    assert_eq!(report.reused, vec![second.config_path]);
}

#[test]
fn conflicting_or_invalid_configuration_is_not_changed() {
    let repo = repository();
    let codex_dir = repo.path().join(CODEX_DIR);
    fs::create_dir(&codex_dir).expect("codex dir");
    let config = codex_dir.join(CONFIG_FILE);
    let original = "[extended_workflow]\nenabled = false\n";
    fs::write(&config, original).expect("config");
    let plan = plan_extended_workflow_preparation(repo.path()).expect("plan");
    assert_eq!(plan.action, PrepareAction::Conflict);
    assert!(apply_extended_workflow_preparation(&plan).is_err());
    assert_eq!(fs::read_to_string(config).expect("unchanged"), original);
}

#[test]
fn stale_preview_is_rejected() {
    let repo = repository();
    let plan = plan_extended_workflow_preparation(repo.path()).expect("plan");
    fs::create_dir(repo.path().join(CODEX_DIR)).expect("codex dir");
    fs::write(&plan.config_path, "model = \"changed\"\n").expect("race write");
    assert!(matches!(
        apply_extended_workflow_preparation(&plan),
        Err(PrepareWorkflowError::PreviewStale)
    ));
}

#[test]
fn outside_git_repository_is_rejected() {
    let temp = tempfile::tempdir().expect("temporary directory");
    let filesystem_root = temp.path().ancestors().last().expect("filesystem root");
    let outside = filesystem_root.join(format!(
        "codex-extended-workflow-outside-{}",
        std::process::id()
    ));
    assert!(matches!(
        plan_extended_workflow_preparation(&outside),
        Err(PrepareWorkflowError::OutsideGitRepository)
    ));
}

#[test]
fn nested_working_directory_resolves_the_repository_root() {
    let repo = repository();
    let nested = repo.path().join("src/nested");
    fs::create_dir_all(&nested).expect("nested directory");
    let plan = plan_extended_workflow_preparation(&nested).expect("plan");
    assert_eq!(
        plan.repository_root,
        fs::canonicalize(repo.path()).expect("canonical repository")
    );
}

#[test]
fn unusable_codex_path_is_reported_without_overwriting_it() {
    let repo = repository();
    let codex_path = repo.path().join(CODEX_DIR);
    fs::write(&codex_path, "user data").expect("blocking file");
    assert!(plan_extended_workflow_preparation(repo.path()).is_err());
    assert_eq!(
        fs::read_to_string(codex_path).expect("preserved"),
        "user data"
    );
}

#[cfg(unix)]
#[test]
fn missing_write_permissions_are_reported_without_changing_config() {
    use std::os::unix::fs::PermissionsExt;

    let repo = repository();
    let codex_dir = repo.path().join(CODEX_DIR);
    let plan = plan_extended_workflow_preparation(repo.path()).expect("plan");
    fs::create_dir(&codex_dir).expect("codex dir");
    let mut permissions = fs::metadata(&codex_dir).expect("metadata").permissions();
    permissions.set_mode(0o500);
    fs::set_permissions(&codex_dir, permissions).expect("read-only directory");
    let result = apply_extended_workflow_preparation(&plan);
    let mut permissions = fs::metadata(&codex_dir).expect("metadata").permissions();
    permissions.set_mode(0o700);
    fs::set_permissions(&codex_dir, permissions).expect("restore permissions");
    assert!(matches!(result, Err(PrepareWorkflowError::Write { .. })));
    assert!(!plan.config_path.exists());
}

#[cfg(unix)]
#[test]
fn symlinked_codex_directory_is_rejected() {
    use std::os::unix::fs::symlink;
    let repo = repository();
    let outside = tempfile::tempdir().expect("outside");
    symlink(outside.path(), repo.path().join(CODEX_DIR)).expect("symlink");
    assert!(matches!(
        plan_extended_workflow_preparation(repo.path()),
        Err(PrepareWorkflowError::UnsafePath { .. })
    ));
}
