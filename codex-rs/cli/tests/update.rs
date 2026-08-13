use anyhow::Result;
use predicates::str::contains;
use std::path::Path;
use tempfile::TempDir;

fn codex_command(codex_home: &Path) -> Result<assert_cmd::Command> {
    let mut cmd = assert_cmd::Command::new(codex_utils_cargo_bin::cargo_bin("codex")?);
    cmd.env("CODEX_HOME", codex_home);
    Ok(cmd)
}

fn extended_workflow_command(codex_home: &Path) -> Result<assert_cmd::Command> {
    let mut cmd =
        assert_cmd::Command::new(codex_utils_cargo_bin::cargo_bin("codex-extended-workflow")?);
    cmd.env("CODEX_HOME", codex_home);
    Ok(cmd)
}

#[cfg(debug_assertions)]
#[tokio::test]
async fn compatibility_update_retains_upstream_debug_behavior() -> Result<()> {
    let codex_home = TempDir::new()?;

    codex_command(codex_home.path())?
        .arg("update")
        .assert()
        .failure()
        .stderr(contains("`codex update` is not available in debug builds"));

    Ok(())
}

#[tokio::test]
async fn extended_workflow_update_is_disabled_without_starting_interactive_prompt() -> Result<()> {
    let codex_home = TempDir::new()?;

    extended_workflow_command(codex_home.path())?
        .arg("update")
        .assert()
        .failure()
        .stderr(contains("`codex-extended-workflow update` is disabled"));

    Ok(())
}
