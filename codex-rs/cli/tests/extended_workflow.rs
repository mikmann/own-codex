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
                "codex-extended-workflow v0.147.1-alpha-1",
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
        ));
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
