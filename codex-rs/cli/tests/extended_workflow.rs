use anyhow::Result;
use predicates::prelude::*;
use std::fs;
use tempfile::TempDir;

fn command(name: &str) -> Result<assert_cmd::Command> {
    Ok(assert_cmd::Command::new(codex_utils_cargo_bin::cargo_bin(
        name,
    )?))
}

fn repository() -> Result<TempDir> {
    let repo = tempfile::tempdir()?;
    fs::create_dir(repo.path().join(".git"))?;
    Ok(repo)
}

#[test]
fn primary_and_compatibility_launchers_report_the_distribution_version() -> Result<()> {
    for binary in ["codex-extended-workflow", "codex"] {
        command(binary)?
            .arg("--version")
            .assert()
            .success()
            .stdout(predicate::str::contains(
                "codex-extended-workflow v0.147.1-alpha-10",
            ));
    }
    Ok(())
}

#[test]
fn help_resume_and_completion_use_the_primary_name() -> Result<()> {
    command("codex-extended-workflow")?
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "codex-extended-workflow [OPTIONS]",
        ))
        .stdout(predicate::str::contains("--workflow-token-limit <TOKENS>"))
        .stdout(predicate::str::contains("--workflow-type <TYPE>"))
        .stdout(predicate::str::contains("--expert-min <COUNT>"))
        .stdout(predicate::str::contains("--expert-max <COUNT>"))
        .stdout(predicate::str::contains("--reviewer-min <COUNT>"))
        .stdout(predicate::str::contains("--reviewer-max <COUNT>"))
        .stdout(predicate::str::contains("--disable-experts"))
        .stdout(predicate::str::contains("--disable-reviewers"))
        .stdout(predicate::str::contains("--shared-state"))
        .stdout(predicate::str::contains("Extended workflow options"))
        .stdout(predicate::str::contains("Use 0"))
        .stdout(predicate::str::contains("for unlimited"));
    command("codex-extended-workflow")?
        .args(["exec", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Usage: codex-extended-workflow exec",
        ));
    command("codex-extended-workflow")?
        .args(["resume", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Usage: codex-extended-workflow resume",
        ));
    command("codex-extended-workflow")?
        .args(["completion", "powershell"])
        .assert()
        .success()
        .stdout(predicate::str::contains("codex-extended-workflow"));
    Ok(())
}

#[test]
fn workflow_token_limit_is_available_to_runtime_commands() -> Result<()> {
    command("codex-extended-workflow")?
        .args([
            "--workflow-token-limit",
            "12000",
            "completion",
            "powershell",
        ])
        .assert()
        .success();
    Ok(())
}

#[test]
fn workflow_type_accepts_documented_values_and_rejects_other_spellings() -> Result<()> {
    for value in ["auto", "inactive", "concept", "work-order"] {
        command("codex-extended-workflow")?
            .args(["--workflow-type", value, "completion", "powershell"])
            .assert()
            .success();
    }
    command("codex-extended-workflow")?
        .args(["--workflow-type", "work_order"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "expected one of: auto, inactive, concept, work-order",
        ));
    Ok(())
}

#[test]
fn shared_state_is_explicit_and_rejects_ambiguous_or_compatibility_use() -> Result<()> {
    let explicit_home = tempfile::tempdir()?;
    command("codex-extended-workflow")?
        .env("CODEX_HOME", explicit_home.path())
        .args(["--shared-state", "completion", "powershell"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "--shared-state cannot be combined with CODEX_HOME",
        ));
    command("codex")?
        .args(["--shared-state", "completion", "powershell"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "--shared-state is only supported by codex-extended-workflow",
        ));
    Ok(())
}

#[test]
fn workflow_count_arguments_reject_values_above_six() -> Result<()> {
    for argument in [
        "--expert-min",
        "--expert-max",
        "--reviewer-min",
        "--reviewer-max",
    ] {
        command("codex-extended-workflow")?
            .args([argument, "7", "completion", "powershell"])
            .assert()
            .failure()
            .stderr(predicate::str::contains(
                "agent count must be between 0 and 6",
            ));
    }
    Ok(())
}

#[test]
fn workflow_count_arguments_reject_reversed_explicit_ranges() -> Result<()> {
    for (min_argument, max_argument) in [
        ("--expert-min", "--expert-max"),
        ("--reviewer-min", "--reviewer-max"),
    ] {
        command("codex-extended-workflow")?
            .args([
                min_argument,
                "4",
                max_argument,
                "2",
                "completion",
                "powershell",
            ])
            .assert()
            .failure()
            .stderr(predicate::str::contains(format!(
                "{min_argument} (4) must not exceed {max_argument} (2)"
            )));
    }
    Ok(())
}

#[test]
fn expert_and_reviewer_processes_can_both_be_disabled_from_the_cli() -> Result<()> {
    command("codex-extended-workflow")?
        .args([
            "--disable-experts",
            "--disable-reviewers",
            "completion",
            "powershell",
        ])
        .assert()
        .success();
    Ok(())
}

#[test]
fn prepare_defaults_to_a_non_interactive_read_only_preview() -> Result<()> {
    let repo = repository()?;
    command("codex-extended-workflow")?
        .current_dir(repo.path())
        .arg("prepare-extend-workflow")
        .assert()
        .success()
        .stdout(predicate::str::contains("[Create]"))
        .stdout(predicate::str::contains(
            "Preview only; no files were changed",
        ));
    assert!(!repo.path().join(".codex").exists());
    Ok(())
}

#[test]
fn prepare_apply_is_idempotent_and_creates_only_the_required_config() -> Result<()> {
    let repo = repository()?;
    command("codex-extended-workflow")?
        .current_dir(repo.path())
        .args(["prepare-extend-workflow", "--apply"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Created:"));
    assert!(repo.path().join(".codex/config.toml").is_file());
    assert!(!repo.path().join("AGENTS.md").exists());
    assert!(!repo.path().join(".codex/agents").exists());

    command("codex-extended-workflow")?
        .current_dir(repo.path())
        .args(["prepare-extend-workflow", "--apply"])
        .assert()
        .success()
        .stdout(predicate::str::contains("[Unchanged]"))
        .stdout(predicate::str::contains("Reused unchanged:"));
    Ok(())
}

#[test]
fn prepare_rejects_conflicts_and_non_repositories_without_writing() -> Result<()> {
    let repo = repository()?;
    fs::create_dir(repo.path().join(".codex"))?;
    let config = repo.path().join(".codex/config.toml");
    let original = "[extended_workflow]\nenabled = false\n";
    fs::write(&config, original)?;
    command("codex-extended-workflow")?
        .current_dir(repo.path())
        .args(["prepare-extend-workflow", "--apply"])
        .assert()
        .failure()
        .stdout(predicate::str::contains("[Conflict]"));
    assert_eq!(fs::read_to_string(config)?, original);

    let outside = tempfile::tempdir()?;
    let filesystem_root = outside.path().ancestors().last().expect("filesystem root");
    let non_repository = filesystem_root.join(format!(
        "codex-extended-workflow-outside-{}",
        std::process::id()
    ));
    command("codex-extended-workflow")?
        .args(["prepare-extend-workflow", "--cd"])
        .arg(non_repository)
        .assert()
        .failure()
        .stderr(predicate::str::contains("not inside a Git worktree"));
    Ok(())
}

#[test]
fn prepare_never_replaces_an_existing_config_without_workflow_table() -> Result<()> {
    let repo = repository()?;
    fs::create_dir(repo.path().join(".codex"))?;
    let config = repo.path().join(".codex/config.toml");
    let original = "model = \"gpt-test\"\n# preserve exactly\n";
    fs::write(&config, original)?;

    command("codex-extended-workflow")?
        .current_dir(repo.path())
        .args(["prepare-extend-workflow", "--apply"])
        .assert()
        .failure()
        .stdout(predicate::str::contains("[Conflict]"))
        .stdout(predicate::str::contains("never replaced automatically"));
    assert_eq!(fs::read_to_string(config)?, original);
    Ok(())
}

#[test]
fn upstream_update_command_is_disabled_for_the_fork() -> Result<()> {
    command("codex-extended-workflow")?
        .arg("update")
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "update` is disabled for this fork",
        ));
    Ok(())
}
