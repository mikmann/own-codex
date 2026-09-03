//! Applies agent-role configuration layers on top of an existing session config.
//!
//! Roles are selected at spawn time and are loaded with the same config machinery as
//! `config.toml`. This module resolves built-in and user-defined role files, inserts the role as a
//! high-precedence layer, and preserves the caller's current model, reasoning effort, provider,
//! and service tier unless the role layer sets them. It does not decide when to spawn a sub-agent
//! or which role to use; the multi-agent tool handler owns that orchestration.

use crate::config::AgentRoleConfig;
use crate::config::Config;
use crate::config::ConfigOverrides;
use crate::config::agent_roles::parse_agent_role_file_contents;
use crate::config::deserialize_config_toml_with_base;
use anyhow::anyhow;
use codex_config::ConfigLayerEntry;
use codex_config::ConfigLayerSource;
use codex_config::ConfigLayerStack;
use codex_config::config_toml::ConfigToml;
use codex_config::loader::resolve_relative_paths_in_config_toml;
use codex_exec_server::LOCAL_FS;
use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::path::Path;
use std::path::PathBuf;
use std::sync::LazyLock;
use toml::Value as TomlValue;

/// The role name used when a caller omits `agent_type`.
pub const DEFAULT_ROLE_NAME: &str = "default";
const AGENT_TYPE_UNAVAILABLE_ERROR: &str = "agent type is currently not available";

/// Bounded, validated material used to bind an extended-workflow plan to one role revision.
pub(crate) struct WorkflowRoleMaterial {
    pub(crate) role_name: String,
    pub(crate) source: &'static str,
    pub(crate) canonical_path: Option<PathBuf>,
    pub(crate) config_bytes: Vec<u8>,
    pub(crate) instruction_tokens: u64,
    role_layer_toml: TomlValue,
}

/// Resolves one workflow role without applying it to a child configuration.
///
/// Configured role files must remain inside the configuration layer that declared them (or its
/// discovered `agents` directory), and workflow roles may contain only developer instructions or
/// settings that monotonically reduce privileges. The returned bytes are suitable for a
/// plan-to-spawn fingerprint; callers must resolve them again immediately before spawning.
pub(crate) async fn load_workflow_role_material(
    config: &Config,
    role_name: &str,
) -> Result<WorkflowRoleMaterial, String> {
    let role = resolve_role_config(config, role_name)
        .ok_or_else(|| format!("unknown agent_type '{role_name}'"))?;
    let is_built_in = !config.agent_roles.contains_key(role_name);
    let Some(config_file) = role.config_file.as_ref() else {
        return Ok(WorkflowRoleMaterial {
            role_name: role_name.to_string(),
            source: if is_built_in {
                "built-in"
            } else {
                "configured-inline"
            },
            canonical_path: None,
            config_bytes: role
                .description
                .as_deref()
                .unwrap_or_default()
                .as_bytes()
                .to_vec(),
            instruction_tokens: 0,
            role_layer_toml: TomlValue::Table(Default::default()),
        });
    };

    let (source, canonical_path, contents, role_file_label, role_config_base) = if is_built_in {
        let contents = built_in::config_file_contents(config_file)
            .ok_or_else(|| format!("built-in agent_type '{role_name}' is unavailable"))?
            .to_string();
        (
            "built-in",
            None,
            contents,
            config_file.clone(),
            config.codex_home.as_path().to_path_buf(),
        )
    } else {
        let canonical_path = tokio::fs::canonicalize(config_file)
            .await
            .map_err(|error| {
                format!(
                    "agent_type '{role_name}' could not resolve role file {}: {error}",
                    config_file.display()
                )
            })?;
        validate_workflow_role_path(config, role_name, &canonical_path).await?;
        let contents = tokio::fs::read_to_string(&canonical_path)
            .await
            .map_err(|error| {
                format!(
                    "agent_type '{role_name}' could not read role file {}: {error}",
                    canonical_path.display()
                )
            })?;
        let role_config_base = canonical_path
            .parent()
            .ok_or_else(|| format!("agent_type '{role_name}' has no trusted role directory"))?
            .to_path_buf();
        (
            "configured-file",
            Some(canonical_path.clone()),
            contents,
            canonical_path,
            role_config_base,
        )
    };

    let parsed = parse_agent_role_file_contents(
        &contents,
        &role_file_label,
        &role_config_base,
        Some(role_name),
    )
    .map_err(|_error| {
        tracing::warn!(
            role_name,
            path = %role_file_label.display(),
            "failed to parse extended-workflow role file"
        );
        format!(
            "agent_type '{role_name}' role file {} is invalid or contains unsupported values",
            role_file_label.display()
        )
    })?;
    if !is_built_in {
        validate_workflow_role_restrictions(role_name, &parsed.config)?;
    }
    let instruction_tokens = parsed
        .config
        .get("developer_instructions")
        .and_then(TomlValue::as_str)
        .map(|instructions| u64::try_from(instructions.len().div_ceil(4)).unwrap_or(u64::MAX))
        .unwrap_or(0);
    deserialize_config_toml_with_base(parsed.config.clone(), &role_config_base).map_err(
        |_error| {
            tracing::warn!(
                role_name,
                path = %role_file_label.display(),
                "failed to validate extended-workflow role file"
            );
            format!(
                "agent_type '{role_name}' role file {} is not a valid configuration layer",
                role_file_label.display()
            )
        },
    )?;
    let role_layer_toml = resolve_relative_paths_in_config_toml(parsed.config, &role_config_base)
        .map_err(|_error| {
        tracing::warn!(
            role_name,
            path = %role_file_label.display(),
            "failed to resolve extended-workflow role file paths"
        );
        format!(
            "agent_type '{role_name}' role file {} contains invalid relative paths",
            role_file_label.display()
        )
    })?;

    Ok(WorkflowRoleMaterial {
        role_name: role_name.to_string(),
        source,
        canonical_path,
        config_bytes: contents.into_bytes(),
        instruction_tokens,
        role_layer_toml,
    })
}

/// Applies exactly the already validated bytes represented by `material`.
///
/// This avoids a second path-based read between fingerprint validation and role application. The
/// caller remains responsible for comparing the material fingerprint with its persisted or
/// server-owned expected value before calling this function.
pub(crate) async fn apply_workflow_role_material_to_config(
    config: &mut Config,
    material: &WorkflowRoleMaterial,
) -> Result<(), String> {
    if material
        .role_layer_toml
        .as_table()
        .is_some_and(toml::map::Map::is_empty)
    {
        return Ok(());
    }
    let preserve_current_provider = material.role_layer_toml.get("model_provider").is_none();
    let preserve_current_service_tier = material.role_layer_toml.get("service_tier").is_none();
    *config = reload::build_next_config(
        config,
        material.role_layer_toml.clone(),
        RoleDeveloperInstructions::PreserveCallerInstructions,
        preserve_current_provider,
        preserve_current_service_tier,
    )
    .await
    .map_err(|_error| {
        tracing::warn!(
            role_name = %material.role_name,
            "failed to apply validated extended-workflow role layer"
        );
        format!(
            "agent_type '{}' could not apply its validated role layer",
            material.role_name
        )
    })?;
    Ok(())
}

async fn validate_workflow_role_path(
    config: &Config,
    role_name: &str,
    canonical_path: &Path,
) -> Result<(), String> {
    let mut declaring_root = None;
    let mut discovery_roots = Vec::new();
    for layer in config.config_layer_stack.all_layers_low_to_high() {
        let Some(config_folder) = layer.config_folder() else {
            continue;
        };
        discovery_roots.push(config_folder.as_path().join("agents"));
        if layer
            .config
            .get("agents")
            .and_then(TomlValue::as_table)
            .and_then(|agents| agents.get(role_name))
            .and_then(TomlValue::as_table)
            .is_some_and(|role| role.contains_key("config_file"))
        {
            declaring_root = Some(config_folder.as_path().to_path_buf());
        }
    }

    let candidate_roots = declaring_root.map_or(discovery_roots, |root| vec![root]);
    for root in candidate_roots {
        let Ok(canonical_root) = tokio::fs::canonicalize(&root).await else {
            continue;
        };
        if canonical_path.starts_with(&canonical_root) {
            return Ok(());
        }
    }
    Err(format!(
        "agent_type '{role_name}' role file {} is outside its trusted configuration directory",
        canonical_path.display()
    ))
}

fn validate_workflow_role_restrictions(
    role_name: &str,
    role_config: &TomlValue,
) -> Result<(), String> {
    let Some(table) = role_config.as_table() else {
        return Err(format!(
            "agent_type '{role_name}' role configuration is not a table"
        ));
    };
    for (key, value) in table {
        let strictly_reduces_permissions = match key.as_str() {
            "developer_instructions" => true,
            "approval_policy" => value.as_str() == Some("never"),
            "sandbox_mode" => value.as_str() == Some("read-only"),
            "allow_login_shell"
            | "include_apps_instructions"
            | "include_collaboration_mode_instructions"
            | "include_environment_context" => value.as_bool() == Some(false),
            "web_search" => value.as_str() == Some("disabled"),
            _ => false,
        };
        if !strictly_reduces_permissions {
            return Err(format!(
                "agent_type '{role_name}' role setting '{key}' is not permitted in an extended workflow because it could replace profile-owned behavior or widen child capabilities"
            ));
        }
    }
    Ok(())
}

/// Applies a named role layer to `config` while preserving caller-owned provider settings.
///
/// The role layer is inserted at session-flag precedence so it can override persisted config, but
/// the caller's current `model_provider` and `service_tier` remain sticky runtime choices unless
/// the role explicitly sets the corresponding top-level config key. Rebuilding the config without
/// those overrides would make a spawned agent silently fall back to default settings.
pub(crate) async fn apply_role_to_config(
    config: &mut Config,
    role_name: Option<&str>,
) -> Result<(), String> {
    apply_role_to_config_with_developer_instructions(
        config,
        role_name,
        RoleDeveloperInstructions::UseConfigLayers,
    )
    .await
}

/// Applies a v2 role without losing developer instructions selected by its caller.
///
/// A role's own top-level developer instructions still take precedence. When its role file omits
/// that setting, rebuilding the config must not restore inherited instructions from older layers.
pub(crate) async fn apply_role_to_config_for_multi_agent_v2(
    config: &mut Config,
    role_name: Option<&str>,
) -> Result<(), String> {
    apply_role_to_config_with_developer_instructions(
        config,
        role_name,
        RoleDeveloperInstructions::PreserveCallerInstructions,
    )
    .await
}

#[derive(Clone, Copy)]
enum RoleDeveloperInstructions {
    UseConfigLayers,
    PreserveCallerInstructions,
}

async fn apply_role_to_config_with_developer_instructions(
    config: &mut Config,
    role_name: Option<&str>,
    developer_instructions: RoleDeveloperInstructions,
) -> Result<(), String> {
    let role_name = role_name.unwrap_or(DEFAULT_ROLE_NAME);

    let role = resolve_role_config(config, role_name)
        .cloned()
        .ok_or_else(|| format!("unknown agent_type '{role_name}'"))?;

    apply_role_to_config_inner(config, role_name, &role, developer_instructions)
        .await
        .map_err(|err| {
            tracing::warn!("failed to apply role to config: {err}");
            AGENT_TYPE_UNAVAILABLE_ERROR.to_string()
        })
}

async fn apply_role_to_config_inner(
    config: &mut Config,
    role_name: &str,
    role: &AgentRoleConfig,
    developer_instructions: RoleDeveloperInstructions,
) -> anyhow::Result<()> {
    let is_built_in = !config.agent_roles.contains_key(role_name);
    let Some(config_file) = role.config_file.as_ref() else {
        return Ok(());
    };
    let role_layer_toml = load_role_layer_toml(config, config_file, is_built_in, role_name).await?;
    if role_layer_toml
        .as_table()
        .is_some_and(toml::map::Map::is_empty)
    {
        return Ok(());
    }
    let preserve_current_provider = role_layer_toml.get("model_provider").is_none();
    let preserve_current_service_tier = role_layer_toml.get("service_tier").is_none();

    *config = reload::build_next_config(
        config,
        role_layer_toml,
        developer_instructions,
        preserve_current_provider,
        preserve_current_service_tier,
    )
    .await?;
    Ok(())
}

async fn load_role_layer_toml(
    config: &Config,
    config_file: &Path,
    is_built_in: bool,
    role_name: &str,
) -> anyhow::Result<TomlValue> {
    let (role_config_toml, role_config_base) = if is_built_in {
        let role_config_contents = built_in::config_file_contents(config_file)
            .map(str::to_owned)
            .ok_or(anyhow!("No corresponding config content"))?;
        let role_config_toml: TomlValue = toml::from_str(&role_config_contents)?;
        (role_config_toml, config.codex_home.as_path())
    } else {
        let role_config_contents = tokio::fs::read_to_string(config_file).await?;
        let role_config_base = config_file
            .parent()
            .ok_or(anyhow!("No corresponding config content"))?;
        let role_config_toml = parse_agent_role_file_contents(
            &role_config_contents,
            config_file,
            role_config_base,
            Some(role_name),
        )?
        .config;
        (role_config_toml, role_config_base)
    };

    deserialize_config_toml_with_base(role_config_toml.clone(), role_config_base)?;
    Ok(resolve_relative_paths_in_config_toml(
        role_config_toml,
        role_config_base,
    )?)
}

pub(crate) fn resolve_role_config<'a>(
    config: &'a Config,
    role_name: &str,
) -> Option<&'a AgentRoleConfig> {
    config
        .agent_roles
        .get(role_name)
        .or_else(|| built_in::configs().get(role_name))
}

mod reload {
    use super::*;

    pub(super) async fn build_next_config(
        config: &Config,
        role_layer_toml: TomlValue,
        developer_instructions: RoleDeveloperInstructions,
        preserve_current_provider: bool,
        preserve_current_service_tier: bool,
    ) -> anyhow::Result<Config> {
        let preserve_current_model = role_layer_toml.get("model").is_none();
        let preserve_current_reasoning_effort =
            role_layer_toml.get("model_reasoning_effort").is_none();
        let mut overrides = reload_overrides(
            config,
            preserve_current_model,
            preserve_current_provider,
            preserve_current_service_tier,
        );
        if let (RoleDeveloperInstructions::PreserveCallerInstructions, Some(_), None) = (
            developer_instructions,
            &config.multi_agent_v2.subagent_developer_instructions,
            role_layer_toml.get("developer_instructions"),
        ) {
            overrides
                .developer_instructions
                .clone_from(&config.developer_instructions);
        }
        let config_layer_stack = build_config_layer_stack(config, &role_layer_toml)?;
        let merged_config = deserialize_effective_config(config, &config_layer_stack)?;

        let mut next_config = Config::load_config_with_layer_stack(
            LOCAL_FS.as_ref(),
            merged_config,
            overrides,
            config.codex_home.clone(),
            config_layer_stack,
            config.extended_workflow.enabled,
        )
        .await?;
        if preserve_current_reasoning_effort {
            next_config
                .model_reasoning_effort
                .clone_from(&config.model_reasoning_effort);
        }
        Ok(next_config)
    }

    fn build_config_layer_stack(
        config: &Config,
        role_layer_toml: &TomlValue,
    ) -> anyhow::Result<ConfigLayerStack> {
        let mut layers = existing_layers(config);
        insert_layer(&mut layers, role_layer(role_layer_toml.clone()));
        Ok(ConfigLayerStack::new(
            layers,
            config.config_layer_stack.requirements().clone(),
            config.config_layer_stack.requirements_toml().clone(),
        )?)
    }

    fn deserialize_effective_config(
        config: &Config,
        config_layer_stack: &ConfigLayerStack,
    ) -> anyhow::Result<ConfigToml> {
        Ok(deserialize_config_toml_with_base(
            config_layer_stack.effective_config(),
            &config.codex_home,
        )?)
    }

    fn existing_layers(config: &Config) -> Vec<ConfigLayerEntry> {
        config
            .config_layer_stack
            .all_layers_low_to_high()
            .cloned()
            .collect()
    }

    fn insert_layer(layers: &mut Vec<ConfigLayerEntry>, layer: ConfigLayerEntry) {
        let insertion_index =
            layers.partition_point(|existing_layer| existing_layer.name <= layer.name);
        layers.insert(insertion_index, layer);
    }

    fn role_layer(role_layer_toml: TomlValue) -> ConfigLayerEntry {
        ConfigLayerEntry::new(ConfigLayerSource::SessionFlags, role_layer_toml)
    }

    fn reload_overrides(
        config: &Config,
        preserve_current_model: bool,
        preserve_current_provider: bool,
        preserve_current_service_tier: bool,
    ) -> ConfigOverrides {
        ConfigOverrides {
            cwd: Some(config.cwd.to_path_buf()),
            model: preserve_current_model
                .then(|| config.model.clone())
                .flatten(),
            model_provider: preserve_current_provider.then(|| config.model_provider_id.clone()),
            service_tier: preserve_current_service_tier.then(|| config.service_tier.clone()),
            psp: Some(config.psp),
            codex_linux_sandbox_exe: config.codex_linux_sandbox_exe.clone(),
            main_execve_wrapper_exe: config.main_execve_wrapper_exe.clone(),
            ..Default::default()
        }
    }
}

pub(crate) mod spawn_tool_spec {
    use super::*;

    /// Builds the spawn-agent tool description text from built-in and configured roles.
    pub(crate) fn build(user_defined_agent_roles: &BTreeMap<String, AgentRoleConfig>) -> String {
        let built_in_roles = built_in::configs();
        build_from_configs(built_in_roles, user_defined_agent_roles)
    }

    // This function is not inlined for testing purpose.
    fn build_from_configs(
        built_in_roles: &BTreeMap<String, AgentRoleConfig>,
        user_defined_roles: &BTreeMap<String, AgentRoleConfig>,
    ) -> String {
        let mut seen = BTreeSet::new();
        let mut formatted_roles = Vec::new();
        for (name, declaration) in user_defined_roles {
            if seen.insert(name.as_str()) {
                formatted_roles.push(format_role(name, declaration));
            }
        }
        for (name, declaration) in built_in_roles {
            if seen.insert(name.as_str()) {
                formatted_roles.push(format_role(name, declaration));
            }
        }

        format!("Available roles:\n{}", formatted_roles.join("\n"))
    }

    fn format_role(name: &str, declaration: &AgentRoleConfig) -> String {
        if let Some(description) = &declaration.description {
            let locked_settings_note = declaration
                .config_file
                .as_ref()
                .and_then(|config_file| {
                    built_in::config_file_contents(config_file)
                        .map(str::to_owned)
                        .or_else(|| std::fs::read_to_string(config_file).ok())
                })
                .and_then(|contents| toml::from_str::<TomlValue>(&contents).ok())
                .map(|role_toml| {
                    let model = role_toml
                        .get("model")
                        .and_then(TomlValue::as_str);
                    let reasoning_effort = role_toml
                        .get("model_reasoning_effort")
                        .and_then(TomlValue::as_str);
                    let service_tier = role_toml
                        .get("service_tier")
                        .and_then(TomlValue::as_str);

                    let model_and_reasoning_note = match (model, reasoning_effort) {
                        (Some(model), Some(reasoning_effort)) => format!(
                            "\n- This role's model is set to `{model}` and its reasoning effort is set to `{reasoning_effort}`. These settings cannot be changed."
                        ),
                        (Some(model), None) => {
                            format!(
                                "\n- This role's model is set to `{model}` and cannot be changed."
                            )
                        }
                        (None, Some(reasoning_effort)) => {
                            format!(
                                "\n- This role's reasoning effort is set to `{reasoning_effort}` and cannot be changed."
                            )
                        }
                        (None, None) => String::new(),
                    };
                    let service_tier_note = service_tier
                        .map(|service_tier| {
                            format!(
                                "\n- This role's service tier is set to `{service_tier}`. If it is supported by the resolved model, it takes precedence over a valid spawn request service tier."
                            )
                        })
                        .unwrap_or_default();
                    format!("{model_and_reasoning_note}{service_tier_note}")
                })
                .unwrap_or_default();
            format!("{name}: {{\n{description}{locked_settings_note}\n}}")
        } else {
            format!("{name}: no description")
        }
    }
}

mod built_in {
    use super::*;

    /// Returns the cached built-in role declarations defined in this module.
    pub(super) fn configs() -> &'static BTreeMap<String, AgentRoleConfig> {
        static CONFIG: LazyLock<BTreeMap<String, AgentRoleConfig>> = LazyLock::new(|| {
            BTreeMap::from([
                (
                    DEFAULT_ROLE_NAME.to_string(),
                    AgentRoleConfig {
                        description: Some("Default agent.".to_string()),
                        config_file: None,
                        nickname_candidates: None,
                    }
                ),
                (
                    "explorer".to_string(),
                    AgentRoleConfig {
                        description: Some(r#"Use `explorer` for specific codebase questions.
Explorers are fast and authoritative.
They must be used to ask specific, well-scoped questions on the codebase.
Rules:
- In order to avoid redundant work, you should avoid exploring the same problem that explorers have already covered. Typically, you should trust the explorer results without additional verification. You are still allowed to inspect the code yourself to gain the needed context!
- You are encouraged to spawn up multiple explorers in parallel when you have multiple distinct questions to ask about the codebase that can be answered independently. This allows you to get more information faster without waiting for one question to finish before asking the next. While waiting for the explorer results, you can continue working on other local tasks that do not depend on those results. This parallelism is a key advantage of delegation, so use it whenever you have multiple questions to ask.
- Reuse existing explorers for related questions."#.to_string()),
                        config_file: Some("explorer.toml".to_string().parse().unwrap_or_default()),
                        nickname_candidates: None,
                    }
                ),
                (
                    "worker".to_string(),
                    AgentRoleConfig {
                        description: Some(r#"Use for execution and production work.
Typical tasks:
- Implement part of a feature
- Fix tests or bugs
- Split large refactors into independent chunks
Rules:
- Explicitly assign **ownership** of the task (files / responsibility). When the subtask involves code changes, you should clearly specify which files or modules the worker is responsible for. This helps avoid merge conflicts and ensures accountability. For example, you can say "Worker 1 is responsible for updating the authentication module, while Worker 2 will handle the database layer." By defining clear ownership, you can delegate more effectively and reduce coordination overhead.
- Always tell workers they are **not alone in the codebase**, and they should not revert the edits made by others, and they should adjust their implementation to accommodate the changes made by others. This is important because there may be multiple workers making changes in parallel, and they need to be aware of each other's work to avoid conflicts and ensure a cohesive final product."#.to_string()),
                        config_file: None,
                        nickname_candidates: None,
                    }
                ),
                // Awaiter is temp removed
//                 (
//                     "awaiter".to_string(),
//                     AgentRoleConfig {
//                         description: Some(r#"Use an `awaiter` agent EVERY TIME you must run a command that will take some very long time.
// This includes, but not only:
// * testing
// * monitoring of a long running process
// * explicit ask to wait for something
//
// Rules:
// - When an awaiter is running, you can work on something else. If you need to wait for its completion, use the largest possible timeout.
// - Be patient with the `awaiter`.
// - Do not use an awaiter for every compilation/test if it won't take time. Only use if for long running commands.
// - Close the awaiter when you're done with it."#.to_string()),
//                         config_file: Some("awaiter.toml".to_string().parse().unwrap_or_default()),
//                     }
//                 )
            ])
        });
        &CONFIG
    }

    /// Resolves a built-in role `config_file` path to embedded content.
    pub(super) fn config_file_contents(path: &Path) -> Option<&'static str> {
        const EXPLORER: &str = include_str!("builtins/explorer.toml");
        const AWAITER: &str = include_str!("builtins/awaiter.toml");
        match path.to_str()? {
            "explorer.toml" => Some(EXPLORER),
            "awaiter.toml" => Some(AWAITER),
            _ => None,
        }
    }
}

#[cfg(test)]
#[path = "role_tests.rs"]
mod tests;
