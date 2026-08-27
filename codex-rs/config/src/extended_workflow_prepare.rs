use crate::config_toml::ConfigToml;
use crate::extended_workflow::ExtendedWorkflowConfig;
use codex_git_utils::get_git_repo_root;
use serde::Serialize;
use sha2::Digest;
use sha2::Sha256;
use std::fs;
use std::fs::OpenOptions;
use std::io;
use std::io::ErrorKind;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;
use thiserror::Error;

const CODEX_DIR: &str = ".codex";
const CONFIG_FILE: &str = "config.toml";
static PREPARE_TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PrepareAction {
    Create,
    Unchanged,
    Conflict,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PrepareWorkflowScope {
    Repository,
    Global,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PrepareWorkflowPlan {
    pub scope: PrepareWorkflowScope,
    pub repository_root: PathBuf,
    pub config_path: PathBuf,
    pub action: PrepareAction,
    pub detail: String,
    pub before_sha256: Option<String>,
    #[serde(skip)]
    desired_contents: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PrepareWorkflowReport {
    pub repository_root: PathBuf,
    pub created: Vec<PathBuf>,
    pub updated: Vec<PathBuf>,
    pub reused: Vec<PathBuf>,
}

#[derive(Debug, Error)]
pub enum PrepareWorkflowError {
    #[error("not inside a Git worktree; run this command from a repository or pass --cd <DIR>")]
    OutsideGitRepository,
    #[error("cannot inspect {path}: {source}")]
    Inspect { path: PathBuf, source: io::Error },
    #[error("conflict in {path}: {message}; no files were changed")]
    Conflict { path: PathBuf, message: String },
    #[error("repository files changed after the preview; run the command again")]
    PreviewStale,
    #[error("unsafe path {path}: {message}")]
    UnsafePath { path: PathBuf, message: String },
    #[error("cannot write {path}: {source}")]
    Write { path: PathBuf, source: io::Error },
}

pub fn plan_extended_workflow_preparation(
    start: &Path,
) -> Result<PrepareWorkflowPlan, PrepareWorkflowError> {
    let repository_root =
        get_git_repo_root(start).ok_or(PrepareWorkflowError::OutsideGitRepository)?;
    let repository_root =
        fs::canonicalize(&repository_root).map_err(|source| PrepareWorkflowError::Inspect {
            path: repository_root.clone(),
            source,
        })?;
    let codex_dir = repository_root.join(CODEX_DIR);
    let config_path = codex_dir.join(CONFIG_FILE);
    ensure_existing_path_within_root(&repository_root, &codex_dir)?;
    ensure_directory_when_present(&codex_dir)?;
    ensure_existing_path_within_root(&repository_root, &config_path)?;
    ensure_file_when_present(&config_path)?;

    let existing = match fs::read_to_string(&config_path) {
        Ok(existing) => Some(existing),
        Err(error) if error.kind() == ErrorKind::NotFound => None,
        Err(source) => {
            return Err(PrepareWorkflowError::Inspect {
                path: config_path,
                source,
            });
        }
    };
    let before_sha256 = existing.as_deref().map(content_sha256);

    let (action, detail, desired_contents) = match existing.as_deref() {
        None => {
            let contents = prepared_document()?;
            (
                PrepareAction::Create,
                "create the complete repository workflow configuration; AGENTS.md and .codex/agents are not required"
                    .to_string(),
                Some(contents),
            )
        }
        Some(contents) => plan_existing_config(&config_path, contents)?,
    };

    Ok(PrepareWorkflowPlan {
        scope: PrepareWorkflowScope::Repository,
        repository_root,
        config_path,
        action,
        detail,
        before_sha256,
        desired_contents,
    })
}

pub fn plan_global_extended_workflow_preparation(
    codex_home: &Path,
) -> Result<PrepareWorkflowPlan, PrepareWorkflowError> {
    let repository_root = canonicalize_planned_directory(codex_home)?;
    ensure_directory_when_present(&repository_root)?;
    let config_path = repository_root.join(CONFIG_FILE);
    ensure_existing_path_within_root(&repository_root, &config_path)?;
    ensure_file_when_present(&config_path)?;
    let existing = match fs::read_to_string(&config_path) {
        Ok(existing) => Some(existing),
        Err(error) if error.kind() == ErrorKind::NotFound => None,
        Err(source) => {
            return Err(PrepareWorkflowError::Inspect {
                path: config_path,
                source,
            });
        }
    };
    let before_sha256 = existing.as_deref().map(content_sha256);
    let (action, detail, desired_contents) = match existing.as_deref() {
        None => (
            PrepareAction::Create,
            "create the complete global extended-workflow configuration".to_string(),
            Some(prepared_document()?),
        ),
        Some(contents) => plan_existing_config(&config_path, contents)?,
    };
    Ok(PrepareWorkflowPlan {
        scope: PrepareWorkflowScope::Global,
        repository_root,
        config_path,
        action,
        detail,
        before_sha256,
        desired_contents,
    })
}

fn canonicalize_planned_directory(path: &Path) -> Result<PathBuf, PrepareWorkflowError> {
    match fs::canonicalize(path) {
        Ok(path) => Ok(path),
        Err(source) if source.kind() == ErrorKind::NotFound => {
            let parent = path.parent().ok_or_else(|| PrepareWorkflowError::Inspect {
                path: path.to_path_buf(),
                source: io::Error::new(
                    ErrorKind::InvalidInput,
                    "the global Codex home has no parent directory",
                ),
            })?;
            let file_name = path
                .file_name()
                .ok_or_else(|| PrepareWorkflowError::Inspect {
                    path: path.to_path_buf(),
                    source: io::Error::new(
                        ErrorKind::InvalidInput,
                        "the global Codex home has no directory name",
                    ),
                })?;
            let canonical_parent =
                fs::canonicalize(parent).map_err(|source| PrepareWorkflowError::Inspect {
                    path: parent.to_path_buf(),
                    source,
                })?;
            Ok(canonical_parent.join(file_name))
        }
        Err(source) => Err(PrepareWorkflowError::Inspect {
            path: path.to_path_buf(),
            source,
        }),
    }
}

pub fn apply_extended_workflow_preparation(
    plan: &PrepareWorkflowPlan,
) -> Result<PrepareWorkflowReport, PrepareWorkflowError> {
    if plan.action == PrepareAction::Conflict {
        return Err(PrepareWorkflowError::Conflict {
            path: plan.config_path.clone(),
            message: plan.detail.clone(),
        });
    }
    if plan.scope == PrepareWorkflowScope::Repository
        && get_git_repo_root(&plan.repository_root)
            .and_then(|root| fs::canonicalize(root).ok())
            .as_deref()
            != Some(plan.repository_root.as_path())
    {
        return Err(PrepareWorkflowError::PreviewStale);
    }
    let codex_dir = plan
        .config_path
        .parent()
        .ok_or_else(|| PrepareWorkflowError::UnsafePath {
            path: plan.config_path.clone(),
            message: "prepared config path has no parent directory".to_string(),
        })?
        .to_path_buf();
    ensure_existing_path_within_root(&plan.repository_root, &codex_dir)?;
    ensure_directory_when_present(&codex_dir)?;
    ensure_existing_path_within_root(&plan.repository_root, &plan.config_path)?;
    ensure_file_when_present(&plan.config_path)?;
    let current = match fs::read_to_string(&plan.config_path) {
        Ok(contents) => Some(contents),
        Err(error) if error.kind() == ErrorKind::NotFound => None,
        Err(source) => {
            return Err(PrepareWorkflowError::Inspect {
                path: plan.config_path.clone(),
                source,
            });
        }
    };
    if current.as_deref().map(content_sha256) != plan.before_sha256 {
        return Err(PrepareWorkflowError::PreviewStale);
    }

    let mut report = PrepareWorkflowReport {
        repository_root: plan.repository_root.clone(),
        created: Vec::new(),
        updated: Vec::new(),
        reused: Vec::new(),
    };
    match plan.action {
        PrepareAction::Unchanged => report.reused.push(plan.config_path.clone()),
        PrepareAction::Create => {
            let contents =
                plan.desired_contents
                    .as_deref()
                    .ok_or_else(|| PrepareWorkflowError::Conflict {
                        path: plan.config_path.clone(),
                        message: "the preview contains no safe desired content".to_string(),
                    })?;
            fs::create_dir_all(&codex_dir).map_err(|source| PrepareWorkflowError::Write {
                path: codex_dir.clone(),
                source,
            })?;
            ensure_existing_path_within_root(&plan.repository_root, &codex_dir)?;
            write_new_file_atomically(&plan.config_path, contents).map_err(|source| {
                if source.kind() == ErrorKind::AlreadyExists {
                    PrepareWorkflowError::PreviewStale
                } else {
                    PrepareWorkflowError::Write {
                        path: plan.config_path.clone(),
                        source,
                    }
                }
            })?;
            report.created.push(plan.config_path.clone());
        }
        PrepareAction::Conflict => unreachable!("conflicts return before applying"),
    }
    Ok(report)
}

/// Publishes a fully written file without ever replacing an existing destination.
///
/// A same-directory hard link is the commit point: it is atomic, cannot cross a filesystem,
/// and fails with `AlreadyExists` if another process creates the destination after preview.
fn write_new_file_atomically(path: &Path, contents: &str) -> io::Result<()> {
    let parent = path.parent().ok_or_else(|| {
        io::Error::new(
            ErrorKind::InvalidInput,
            format!("path {} has no parent directory", path.display()),
        )
    })?;
    let file_name = path.file_name().ok_or_else(|| {
        io::Error::new(
            ErrorKind::InvalidInput,
            format!("path {} has no file name", path.display()),
        )
    })?;
    let sequence = PREPARE_TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let temporary_path = parent.join(format!(
        ".{}.codex-extended-workflow-{}-{sequence}.tmp",
        file_name.to_string_lossy(),
        std::process::id()
    ));

    let mut temporary = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temporary_path)?;
    if let Err(error) = temporary
        .write_all(contents.as_bytes())
        .and_then(|()| temporary.sync_all())
    {
        drop(temporary);
        let _ = fs::remove_file(&temporary_path);
        return Err(error);
    }
    drop(temporary);

    let publish_result = fs::hard_link(&temporary_path, path);
    let cleanup_result = fs::remove_file(&temporary_path);
    match publish_result {
        Err(error) => Err(error),
        Ok(()) => cleanup_result,
    }
}

fn plan_existing_config(
    config_path: &Path,
    contents: &str,
) -> Result<(PrepareAction, String, Option<String>), PrepareWorkflowError> {
    let parsed =
        toml::from_str::<ConfigToml>(contents).map_err(|error| PrepareWorkflowError::Conflict {
            path: config_path.to_path_buf(),
            message: format!("existing Codex configuration is not valid TOML: {error}"),
        })?;
    if let Some(workflow) = parsed.extended_workflow.as_ref() {
        let _resolved = ExtendedWorkflowConfig::resolve_with_default(
            Some(workflow),
            /*enabled_by_default*/ true,
        )
        .map_err(|message| PrepareWorkflowError::Conflict {
            path: config_path.to_path_buf(),
            message,
        })?;
        if workflow.enabled == Some(false) {
            return Ok((
                PrepareAction::Conflict,
                "extended_workflow is explicitly disabled".to_string(),
                None,
            ));
        }
        return Ok((
            PrepareAction::Unchanged,
            "existing valid extended_workflow configuration will be reused unchanged".to_string(),
            None,
        ));
    }

    Ok((
        PrepareAction::Conflict,
        "existing config.toml has no [extended_workflow] table and is never replaced automatically; add `[extended_workflow]` followed by `enabled = true` manually, then run the command again"
            .to_string(),
        None,
    ))
}

fn prepared_document() -> Result<String, PrepareWorkflowError> {
    Ok(r#"# Prepared by codex-extended-workflow.
# Existing configuration files are never overwritten; merge this template manually if needed.

[extended_workflow]
enabled = true
audit_mode = true
# 0 means unlimited. A non-zero value covers the main agent, experts, reviewers, and compactions.
workflow_budget_tokens = 0
max_result_bytes = 8192

[extended_workflow.experts]
enabled = true
min = 2
max = 4

[[extended_workflow.experts.profiles]]
name = "expert_1"
role = "Architecture"
model = "gpt-5.6-sol"
reasoning_effort = "high"

[[extended_workflow.experts.profiles]]
name = "expert_2"
role = "Orchestration and context"
model = "gpt-5.6-terra"
reasoning_effort = "xhigh"

[[extended_workflow.experts.profiles]]
name = "expert_3"
role = "Security and permissions"
model = "gpt-5.5"
reasoning_effort = "high"

[[extended_workflow.experts.profiles]]
name = "expert_4"
role = "Testing and compatibility"
model = "gpt-5.6-sol"
reasoning_effort = "max"

[extended_workflow.reviewers]
enabled = true
min = 2
max = 4

[[extended_workflow.reviewers.profiles]]
name = "reviewer_1"
role = "Correctness and architecture"
model = "gpt-5.6-sol"
reasoning_effort = "high"

[[extended_workflow.reviewers.profiles]]
name = "reviewer_2"
role = "Security and permissions"
model = "gpt-5.6-terra"
reasoning_effort = "xhigh"

[[extended_workflow.reviewers.profiles]]
name = "reviewer_3"
role = "Testing and regression"
model = "gpt-5.5"
reasoning_effort = "high"

[[extended_workflow.reviewers.profiles]]
name = "reviewer_4"
role = "Product and workflow compatibility"
model = "gpt-5.6-sol"
reasoning_effort = "max"

[tui]
status_line = ["model-with-reasoning", "session-tokens", "workflow-tokens"]
status_line_use_colors = true
"#
    .to_string())
}

fn ensure_directory_when_present(path: &Path) -> Result<(), PrepareWorkflowError> {
    match fs::metadata(path) {
        Ok(metadata) if metadata.is_dir() => Ok(()),
        Ok(_) => Err(PrepareWorkflowError::UnsafePath {
            path: path.to_path_buf(),
            message: "expected a directory but found another file type".to_string(),
        }),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(source) => Err(PrepareWorkflowError::Inspect {
            path: path.to_path_buf(),
            source,
        }),
    }
}

fn ensure_file_when_present(path: &Path) -> Result<(), PrepareWorkflowError> {
    match fs::metadata(path) {
        Ok(metadata) if metadata.is_file() => Ok(()),
        Ok(_) => Err(PrepareWorkflowError::UnsafePath {
            path: path.to_path_buf(),
            message: "expected a regular file but found another file type".to_string(),
        }),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(source) => Err(PrepareWorkflowError::Inspect {
            path: path.to_path_buf(),
            source,
        }),
    }
}

fn ensure_existing_path_within_root(
    repository_root: &Path,
    path: &Path,
) -> Result<(), PrepareWorkflowError> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(()),
        Err(source) => {
            return Err(PrepareWorkflowError::Inspect {
                path: path.to_path_buf(),
                source,
            });
        }
    };
    if metadata.file_type().is_symlink() {
        return Err(PrepareWorkflowError::UnsafePath {
            path: path.to_path_buf(),
            message: "symbolic links are not modified by repository preparation".to_string(),
        });
    }
    let canonical = fs::canonicalize(path).map_err(|source| PrepareWorkflowError::Inspect {
        path: path.to_path_buf(),
        source,
    })?;
    if !canonical.starts_with(repository_root) {
        return Err(PrepareWorkflowError::UnsafePath {
            path: path.to_path_buf(),
            message: "resolved path escapes the selected Git checkout".to_string(),
        });
    }
    Ok(())
}

fn content_sha256(contents: &str) -> String {
    format!("{:x}", Sha256::digest(contents.as_bytes()))
}

#[cfg(test)]
#[path = "extended_workflow_prepare_tests.rs"]
mod tests;
