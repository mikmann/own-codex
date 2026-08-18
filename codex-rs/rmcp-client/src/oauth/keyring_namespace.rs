use anyhow::Context;
use anyhow::Result;
use anyhow::bail;
use codex_utils_home_dir::find_codex_auth_home;
use sha2::Digest;
use sha2::Sha256;
use std::ffi::OsStr;
use std::path::Path;

const STATE_SCOPE_ENV: &str = "CODEX_EXTENDED_WORKFLOW_STATE_SCOPE";
const COMPATIBILITY_STATE_SCOPE: &str = "compatibility";

pub(super) fn direct_keyring_account(server_name: &str, server_url: &str) -> Result<String> {
    let state_scope = std::env::var_os(STATE_SCOPE_ENV);
    let auth_home = find_codex_auth_home()?;
    direct_keyring_account_for_scope(&auth_home, state_scope.as_deref(), server_name, server_url)
}

fn direct_keyring_account_for_scope(
    auth_home: &Path,
    state_scope: Option<&OsStr>,
    server_name: &str,
    server_url: &str,
) -> Result<String> {
    let store_key = super::compute_store_key(server_name, server_url)?;
    match state_scope {
        None => Ok(store_key),
        Some(scope) if scope == OsStr::new(COMPATIBILITY_STATE_SCOPE) => Ok(store_key),
        Some(scope)
            if ["isolated", "shared", "explicit"]
                .into_iter()
                .any(|value| scope == OsStr::new(value)) =>
        {
            let mut hasher = Sha256::new();
            hasher.update(auth_home.as_os_str().as_encoded_bytes());
            let namespace = format!("{:x}", hasher.finalize());
            let namespace = namespace
                .get(..16)
                .context("MCP OAuth auth-home namespace is unexpectedly short")?;
            Ok(format!("extended|{namespace}|{store_key}"))
        }
        Some(scope) => bail!("unsupported extended-workflow state scope {scope:?}"),
    }
}

#[cfg(test)]
#[path = "keyring_namespace_tests.rs"]
mod tests;
