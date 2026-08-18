use codex_utils_absolute_path::AbsolutePathBuf;
use dirs::home_dir;
use std::ffi::OsStr;
use std::path::PathBuf;

const EXTENDED_WORKFLOW_STATE_SCOPE_ENV: &str = "CODEX_EXTENDED_WORKFLOW_STATE_SCOPE";
const ISOLATED_STATE_SCOPE: &str = "isolated";
const SHARED_STATE_SCOPE: &str = "shared";

/// Returns the path to the Codex configuration directory, which can be
/// specified by the `CODEX_HOME` environment variable. If not set, defaults to
/// `~/.codex`, or to `~/.codex-extended-workflow` for an isolated extended
/// workflow process.
///
/// - If `CODEX_HOME` is set, the value must exist and be a directory. The
///   value will be canonicalized and this function will Err otherwise.
/// - If `CODEX_HOME` is not set, this function does not verify that the
///   directory exists.
pub fn find_codex_home() -> std::io::Result<AbsolutePathBuf> {
    let codex_home_env = std::env::var_os("CODEX_HOME").filter(|val| !val.is_empty());
    let state_scope = std::env::var_os(EXTENDED_WORKFLOW_STATE_SCOPE_ENV);
    find_codex_home_from_env(
        codex_home_env.as_deref(),
        state_scope.as_deref(),
        home_dir(),
    )
}

/// Returns the credential home for a resolved Codex state home.
///
/// Extended workflow invocations using shared state keep credentials in the
/// extended workflow home. All other modes keep credentials alongside state.
pub fn auth_home_for_state_home(codex_home: &std::path::Path) -> PathBuf {
    let state_scope = std::env::var_os(EXTENDED_WORKFLOW_STATE_SCOPE_ENV);
    auth_home_for_state_home_and_scope(codex_home, state_scope.as_deref())
}

/// Resolves the credential home directly from the current process environment.
pub fn find_codex_auth_home() -> std::io::Result<AbsolutePathBuf> {
    let state_home = find_codex_home()?;
    AbsolutePathBuf::from_absolute_path(auth_home_for_state_home(&state_home))
}

fn auth_home_for_state_home_and_scope(
    codex_home: &std::path::Path,
    state_scope: Option<&OsStr>,
) -> PathBuf {
    if state_scope == Some(OsStr::new(SHARED_STATE_SCOPE)) {
        codex_home.with_file_name(".codex-extended-workflow")
    } else {
        codex_home.to_path_buf()
    }
}

fn find_codex_home_from_env(
    codex_home_env: Option<&OsStr>,
    state_scope: Option<&OsStr>,
    user_home: Option<PathBuf>,
) -> std::io::Result<AbsolutePathBuf> {
    // Honor the `CODEX_HOME` environment variable when it is set to allow users
    // (and tests) to override the default location.
    match codex_home_env {
        Some(val) => {
            let path = PathBuf::from(val);
            let metadata = std::fs::metadata(&path).map_err(|err| match err.kind() {
                std::io::ErrorKind::NotFound => std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    format!("CODEX_HOME points to {path:?}, but that path does not exist"),
                ),
                _ => std::io::Error::new(
                    err.kind(),
                    format!("failed to read CODEX_HOME {path:?}: {err}"),
                ),
            })?;

            if !metadata.is_dir() {
                Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    format!("CODEX_HOME points to {path:?}, but that path is not a directory"),
                ))
            } else {
                let canonical = path.canonicalize().map_err(|err| {
                    std::io::Error::new(
                        err.kind(),
                        format!("failed to canonicalize CODEX_HOME {path:?}: {err}"),
                    )
                })?;
                AbsolutePathBuf::from_absolute_path(canonical)
            }
        }
        None => {
            let mut p = user_home.ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "Could not find home directory",
                )
            })?;
            let directory_name = if state_scope == Some(OsStr::new(ISOLATED_STATE_SCOPE)) {
                ".codex-extended-workflow"
            } else {
                ".codex"
            };
            p.push(directory_name);
            AbsolutePathBuf::from_absolute_path(p)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::auth_home_for_state_home_and_scope;
    use super::find_codex_home_from_env;
    use codex_utils_absolute_path::AbsolutePathBuf;
    use pretty_assertions::assert_eq;
    use std::ffi::OsStr;
    use std::fs;
    use std::io::ErrorKind;
    use tempfile::TempDir;

    #[test]
    fn find_codex_home_env_missing_path_is_fatal() {
        let temp_home = TempDir::new().expect("temp home");
        let missing = temp_home.path().join("missing-codex-home");
        let missing_str = missing
            .to_str()
            .expect("missing codex home path should be valid utf-8");

        let err = find_codex_home_from_env(
            Some(OsStr::new(missing_str)),
            /*state_scope*/ None,
            /*user_home*/ None,
        )
        .expect_err("missing CODEX_HOME");
        assert_eq!(err.kind(), ErrorKind::NotFound);
        assert!(
            err.to_string().contains("CODEX_HOME"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn find_codex_home_env_file_path_is_fatal() {
        let temp_home = TempDir::new().expect("temp home");
        let file_path = temp_home.path().join("codex-home.txt");
        fs::write(&file_path, "not a directory").expect("write temp file");
        let file_str = file_path
            .to_str()
            .expect("file codex home path should be valid utf-8");

        let err = find_codex_home_from_env(
            Some(OsStr::new(file_str)),
            /*state_scope*/ None,
            /*user_home*/ None,
        )
        .expect_err("file CODEX_HOME");
        assert_eq!(err.kind(), ErrorKind::InvalidInput);
        assert!(
            err.to_string().contains("not a directory"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn find_codex_home_env_valid_directory_canonicalizes() {
        let temp_home = TempDir::new().expect("temp home");
        let temp_str = temp_home
            .path()
            .to_str()
            .expect("temp codex home path should be valid utf-8");

        let resolved = find_codex_home_from_env(
            Some(OsStr::new(temp_str)),
            Some(OsStr::new("isolated")),
            /*user_home*/ None,
        )
        .expect("valid CODEX_HOME");
        let expected = temp_home
            .path()
            .canonicalize()
            .expect("canonicalize temp home");
        let expected = AbsolutePathBuf::from_absolute_path(expected).expect("absolute home");
        assert_eq!(resolved, expected);
    }

    #[test]
    fn find_codex_home_without_env_uses_default_home_dir() {
        let home = tempfile::tempdir().expect("home dir");
        let resolved = find_codex_home_from_env(
            /*codex_home_env*/ None,
            /*state_scope*/ None,
            Some(home.path().to_path_buf()),
        )
        .expect("default CODEX_HOME");
        let mut expected = home.path().to_path_buf();
        expected.push(".codex");
        let expected = AbsolutePathBuf::from_absolute_path(expected).expect("absolute home");
        assert_eq!(resolved, expected);
    }

    #[test]
    fn isolated_extended_workflow_uses_its_own_default_home_dir() {
        let home = tempfile::tempdir().expect("home dir");
        let resolved = find_codex_home_from_env(
            /*codex_home_env*/ None,
            Some(OsStr::new("isolated")),
            Some(home.path().to_path_buf()),
        )
        .expect("isolated CODEX_HOME");
        let expected =
            AbsolutePathBuf::from_absolute_path(home.path().join(".codex-extended-workflow"))
                .expect("absolute home");
        assert_eq!(resolved, expected);
    }

    #[test]
    fn shared_state_keeps_credentials_in_extended_workflow_home() {
        let home = tempfile::tempdir().expect("home dir");
        let state_home = home.path().join(".codex");

        assert_eq!(
            auth_home_for_state_home_and_scope(&state_home, Some(OsStr::new("shared")),),
            home.path().join(".codex-extended-workflow")
        );
        assert_eq!(
            auth_home_for_state_home_and_scope(&state_home, Some(OsStr::new("compatibility")),),
            state_home
        );
    }
}
