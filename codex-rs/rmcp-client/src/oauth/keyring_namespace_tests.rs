use super::*;
use pretty_assertions::assert_eq;
use tempfile::TempDir;

#[test]
fn compatibility_scope_preserves_the_legacy_keyring_account() -> Result<()> {
    let auth_home = TempDir::new()?;
    let expected = super::super::compute_store_key("server", "https://example.test/mcp")?;

    assert_eq!(
        direct_keyring_account_for_scope(
            auth_home.path(),
            Some(OsStr::new("compatibility")),
            "server",
            "https://example.test/mcp",
        )?,
        expected
    );
    Ok(())
}

#[test]
fn extended_scopes_isolate_direct_keyring_accounts_by_auth_home() -> Result<()> {
    let first_home = TempDir::new()?;
    let second_home = TempDir::new()?;
    let accounts = ["isolated", "shared", "explicit"]
        .into_iter()
        .map(|scope| {
            direct_keyring_account_for_scope(
                first_home.path(),
                Some(OsStr::new(scope)),
                "server",
                "https://example.test/mcp",
            )
        })
        .collect::<Result<Vec<_>>>()?;

    assert_eq!(accounts[0], accounts[1]);
    assert_eq!(accounts[1], accounts[2]);
    assert_ne!(
        accounts[0],
        direct_keyring_account_for_scope(
            second_home.path(),
            Some(OsStr::new("isolated")),
            "server",
            "https://example.test/mcp",
        )?
    );
    Ok(())
}

#[test]
fn extended_account_is_stable_when_the_auth_home_is_created() -> Result<()> {
    let parent = TempDir::new()?;
    let auth_home = parent.path().join("missing-auth-home");
    let before = direct_keyring_account_for_scope(
        &auth_home,
        Some(OsStr::new("shared")),
        "server",
        "https://example.test/mcp",
    )?;

    std::fs::create_dir(&auth_home)?;

    assert_eq!(
        direct_keyring_account_for_scope(
            &auth_home,
            Some(OsStr::new("shared")),
            "server",
            "https://example.test/mcp",
        )?,
        before
    );
    Ok(())
}

#[test]
fn unknown_scope_is_rejected() {
    let auth_home = TempDir::new().expect("auth home");
    assert!(
        direct_keyring_account_for_scope(
            auth_home.path(),
            Some(OsStr::new("unknown")),
            "server",
            "https://example.test/mcp",
        )
        .is_err()
    );
}
