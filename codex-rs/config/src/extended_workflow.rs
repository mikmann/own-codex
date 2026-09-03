use codex_protocol::config_types::WorkflowRequestClass;
use codex_protocol::config_types::WorkflowType;
use codex_protocol::openai_models::ReasoningEffort;
use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;
use std::collections::HashMap;
use std::collections::HashSet;

pub const EXTENDED_WORKFLOW_MAX_AGENTS: usize = 6;
pub const EXTENDED_WORKFLOW_MAX_PROFILES: usize = 16;
pub const EXTENDED_WORKFLOW_MAX_PROFILE_FIELD_BYTES: usize = 256;
pub const EXTENDED_WORKFLOW_MAX_SELECTION_KEYWORDS: usize = 12;
pub const EXTENDED_WORKFLOW_MAX_SELECTION_KEYWORD_BYTES: usize = 64;
pub const EXTENDED_WORKFLOW_MAX_PROCESS_PROFILE_BYTES: usize = 16 * 1_024;
pub const DEFAULT_WORKFLOW_MIN_AGENTS: usize = 2;
pub const DEFAULT_WORKFLOW_MAX_AGENTS: usize = 4;
pub const DEFAULT_MAX_RESULT_BYTES: usize = 8 * 1024;
pub const HARD_MAX_RESULT_BYTES: usize = 9_000;

/// Repository- and user-configurable settings for the extended workflow.
#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq, Eq, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct ExtendedWorkflowToml {
    /// Enables classification and workflow guidance.
    ///
    /// The `codex-extended-workflow` launcher supplies an enabled internal default. Shared
    /// libraries and the compatibility launcher default to disabled unless configuration says
    /// otherwise.
    pub enabled: Option<bool>,
    /// Selects automatic classification, bypass, concept, or work-order orchestration.
    pub workflow_type: Option<WorkflowType>,
    /// Marks normal model-visible workflow diagnostics as audit-enabled. No files are written.
    pub audit_mode: Option<bool>,
    /// Enforced cumulative provider-token stop for the active workflow and all subagents.
    ///
    /// Enforcement happens after each provider response, so responses already in flight can cross
    /// the configured limit before later sampling is stopped.
    pub workflow_budget_tokens: Option<u64>,
    /// Hard UTF-8 byte limit for one structured expert, reviewer, concept, or ticket result.
    pub max_result_bytes: Option<usize>,
    pub experts: Option<WorkflowProcessToml>,
    pub reviewers: Option<WorkflowProcessToml>,
}

/// Independent settings for either the expert or reviewer process.
#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq, Eq, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct WorkflowProcessToml {
    pub enabled: Option<bool>,
    pub min: Option<usize>,
    pub max: Option<usize>,
    /// Advisory token budget for one complete expert or reviewer round.
    pub round_budget_tokens: Option<u64>,
    /// Replaces the built-in catalog when present.
    pub profiles: Option<Vec<WorkflowAgentProfileToml>>,
    /// Appends profiles to the built-in or explicitly replaced catalog.
    pub additional_profiles: Option<Vec<WorkflowAgentProfileToml>>,
}

/// A selectable expert or reviewer profile.
#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq, Eq, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct WorkflowAgentProfileToml {
    pub name: String,
    pub role: String,
    /// Optional existing `[agents.<name>]` role to layer onto the child.
    pub agent_type: Option<String>,
    /// Local deterministic selection hints. These strings are never injected as instructions.
    #[serde(default)]
    pub selection_keywords: Vec<String>,
    /// Resolved by the existing spawn-agent model catalog. Not an allowlist.
    pub model: Option<String>,
    /// Validated against the selected model by the existing spawn-agent resolver.
    pub reasoning_effort: Option<ReasoningEffort>,
    /// Advisory per-subagent token budget; exact provider enforcement is not implied.
    pub token_budget: Option<u64>,
    /// Hard local context-window ceiling for this profile.
    ///
    /// The selected model's smaller provider/catalog limit remains authoritative.
    pub context_budget: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtendedWorkflowConfig {
    pub enabled: bool,
    pub workflow_type: WorkflowType,
    /// True when the effective value came from current-process CLI/session flags.
    pub workflow_type_from_session_flags: bool,
    /// Latest effective class for runtime status surfaces; never loaded from TOML.
    pub effective_request_class: Option<WorkflowRequestClass>,
    pub audit_mode: bool,
    pub workflow_budget_tokens: Option<u64>,
    pub max_result_bytes: usize,
    pub experts: WorkflowProcessConfig,
    pub reviewers: WorkflowProcessConfig,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowProcessConfig {
    pub enabled: bool,
    pub min: usize,
    pub max: usize,
    pub round_budget_tokens: Option<u64>,
    pub profiles: Vec<WorkflowAgentProfileConfig>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowAgentProfileConfig {
    pub name: String,
    pub role: String,
    pub agent_type: Option<String>,
    pub selection_keywords: Vec<String>,
    pub source: WorkflowProfileSource,
    pub model: Option<String>,
    pub reasoning_effort: Option<ReasoningEffort>,
    pub token_budget: Option<u64>,
    pub context_budget: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkflowProfileSource {
    BuiltIn,
    Profiles,
    AdditionalProfiles,
}

impl WorkflowProfileSource {
    pub fn label(self) -> &'static str {
        match self {
            Self::BuiltIn => "built-in",
            Self::Profiles => "profiles",
            Self::AdditionalProfiles => "additional_profiles",
        }
    }

    fn toml_field(self) -> &'static str {
        match self {
            Self::BuiltIn => "built_in_profiles",
            Self::Profiles => "profiles",
            Self::AdditionalProfiles => "additional_profiles",
        }
    }
}

impl Default for ExtendedWorkflowConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            workflow_type: WorkflowType::Auto,
            workflow_type_from_session_flags: false,
            effective_request_class: None,
            audit_mode: false,
            workflow_budget_tokens: None,
            max_result_bytes: DEFAULT_MAX_RESULT_BYTES,
            experts: WorkflowProcessConfig::experts_default(),
            reviewers: WorkflowProcessConfig::reviewers_default(),
        }
    }
}

impl ExtendedWorkflowConfig {
    pub fn resolve(config: Option<&ExtendedWorkflowToml>) -> Result<Self, String> {
        Self::resolve_with_default(config, /*enabled_by_default*/ false)
    }

    pub fn resolve_with_default(
        config: Option<&ExtendedWorkflowToml>,
        enabled_by_default: bool,
    ) -> Result<Self, String> {
        let Some(config) = config else {
            return Ok(Self {
                enabled: enabled_by_default,
                ..Self::default()
            });
        };
        if config
            .workflow_budget_tokens
            .is_some_and(|budget| i64::try_from(budget).is_err())
        {
            return Err(format!(
                "extended_workflow.workflow_budget_tokens must be at most {} tokens",
                i64::MAX
            ));
        }
        let max_result_bytes = config.max_result_bytes.unwrap_or(DEFAULT_MAX_RESULT_BYTES);
        if !(1..=HARD_MAX_RESULT_BYTES).contains(&max_result_bytes) {
            return Err(format!(
                "extended_workflow.max_result_bytes must be between 1 and {HARD_MAX_RESULT_BYTES}"
            ));
        }

        Ok(Self {
            enabled: config.enabled.unwrap_or(enabled_by_default),
            workflow_type: config.workflow_type.unwrap_or_default(),
            workflow_type_from_session_flags: false,
            effective_request_class: None,
            audit_mode: config.audit_mode.unwrap_or(false),
            workflow_budget_tokens: config.workflow_budget_tokens,
            max_result_bytes,
            experts: WorkflowProcessConfig::resolve(
                "experts",
                config.experts.as_ref(),
                default_expert_profiles,
            )?,
            reviewers: WorkflowProcessConfig::resolve(
                "reviewers",
                config.reviewers.as_ref(),
                default_reviewer_profiles,
            )?,
        })
    }
}

impl WorkflowProcessConfig {
    fn experts_default() -> Self {
        Self::built_in_defaults(default_expert_profiles())
    }

    fn reviewers_default() -> Self {
        Self::built_in_defaults(default_reviewer_profiles())
    }

    fn built_in_defaults(profiles: Vec<WorkflowAgentProfileConfig>) -> Self {
        Self {
            enabled: true,
            min: DEFAULT_WORKFLOW_MIN_AGENTS,
            max: DEFAULT_WORKFLOW_MAX_AGENTS,
            round_budget_tokens: None,
            profiles,
        }
    }

    fn resolve(
        process_name: &str,
        config: Option<&WorkflowProcessToml>,
        default_profiles: fn() -> Vec<WorkflowAgentProfileConfig>,
    ) -> Result<Self, String> {
        let enabled = config.and_then(|value| value.enabled).unwrap_or(true);
        let min = config
            .and_then(|value| value.min)
            .unwrap_or(DEFAULT_WORKFLOW_MIN_AGENTS);
        let max = config
            .and_then(|value| value.max)
            .unwrap_or(DEFAULT_WORKFLOW_MAX_AGENTS);
        if min > max || max > EXTENDED_WORKFLOW_MAX_AGENTS {
            return Err(format!(
                "extended_workflow.{process_name} must satisfy 0 <= min <= max <= {EXTENDED_WORKFLOW_MAX_AGENTS}; got min={min}, max={max}"
            ));
        }
        let round_budget_tokens = config.and_then(|value| value.round_budget_tokens);
        validate_positive_optional(
            &format!("extended_workflow.{process_name}.round_budget_tokens"),
            round_budget_tokens,
        )?;

        let mut profiles = match config.and_then(|value| value.profiles.as_ref()) {
            Some(profiles) => profiles
                .iter()
                .cloned()
                .map(|profile| {
                    WorkflowAgentProfileConfig::from_toml(profile, WorkflowProfileSource::Profiles)
                })
                .collect::<Result<Vec<_>, _>>()?,
            None => default_profiles(),
        };
        if let Some(additional_profiles) =
            config.and_then(|value| value.additional_profiles.as_ref())
        {
            profiles.extend(
                additional_profiles
                    .iter()
                    .cloned()
                    .map(|profile| {
                        WorkflowAgentProfileConfig::from_toml(
                            profile,
                            WorkflowProfileSource::AdditionalProfiles,
                        )
                    })
                    .collect::<Result<Vec<_>, _>>()?,
            );
        }
        validate_profiles(process_name, &profiles)?;
        if enabled && max > profiles.len() {
            return Err(format!(
                "extended_workflow.{process_name}.max is {max}, but only {} distinct profiles are configured",
                profiles.len()
            ));
        }

        Ok(Self {
            enabled,
            min,
            max,
            round_budget_tokens,
            profiles,
        })
    }
}

impl WorkflowAgentProfileConfig {
    fn from_toml(
        value: WorkflowAgentProfileToml,
        source: WorkflowProfileSource,
    ) -> Result<Self, String> {
        let selection_keywords = value
            .selection_keywords
            .into_iter()
            .map(|keyword| normalize_selection_keyword(&keyword))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self {
            name: value.name,
            role: value.role,
            agent_type: value.agent_type,
            selection_keywords,
            source,
            model: value.model,
            reasoning_effort: value.reasoning_effort,
            token_budget: value.token_budget,
            context_budget: value.context_budget,
        })
    }
}

fn validate_profiles(
    process_name: &str,
    profiles: &[WorkflowAgentProfileConfig],
) -> Result<(), String> {
    if profiles.len() > EXTENDED_WORKFLOW_MAX_PROFILES {
        return Err(format!(
            "extended_workflow.{process_name}.profiles must contain at most {EXTENDED_WORKFLOW_MAX_PROFILES} profiles; got {}",
            profiles.len()
        ));
    }
    let mut names = HashMap::new();
    let mut roles = HashMap::new();
    let mut aggregate_bytes = 0_usize;
    for (index, profile) in profiles.iter().enumerate() {
        let source_index = profiles[..index]
            .iter()
            .filter(|earlier| earlier.source == profile.source)
            .count();
        let path = format!(
            "extended_workflow.{process_name}.{}[{source_index}]",
            profile.source.toml_field()
        );
        let name = profile.name.trim();
        let role = profile.role.trim();
        if name.is_empty() {
            return Err(format!("{path}.name must not be empty"));
        }
        validate_profile_field_length(&format!("{path}.name"), &profile.name)?;
        if role.is_empty() {
            return Err(format!("{path}.role must not be empty"));
        }
        validate_profile_field_length(&format!("{path}.role"), &profile.role)?;
        if let Some(first_path) = names.insert(name.to_lowercase(), path.clone()) {
            return Err(format!(
                "extended_workflow.{process_name} profile names must be unique; duplicate `{name}` appears in {first_path} and {path}"
            ));
        }
        if let Some(first_path) = roles.insert(role.to_lowercase(), path.clone()) {
            return Err(format!(
                "extended_workflow.{process_name} profile roles must be unique; duplicate `{role}` appears in {first_path} and {path}"
            ));
        }
        validate_non_empty_optional(&format!("{path}.agent_type"), profile.agent_type.as_deref())?;
        validate_non_empty_optional(&format!("{path}.model"), profile.model.as_deref())?;
        if let Some(agent_type) = profile.agent_type.as_deref() {
            validate_profile_field_length(&format!("{path}.agent_type"), agent_type)?;
        }
        if let Some(model) = profile.model.as_deref() {
            validate_profile_field_length(&format!("{path}.model"), model)?;
        }
        if profile.selection_keywords.len() > EXTENDED_WORKFLOW_MAX_SELECTION_KEYWORDS {
            return Err(format!(
                "{path}.selection_keywords must contain at most {EXTENDED_WORKFLOW_MAX_SELECTION_KEYWORDS} entries; got {}",
                profile.selection_keywords.len()
            ));
        }
        let mut keywords = HashSet::new();
        for (keyword_index, keyword) in profile.selection_keywords.iter().enumerate() {
            if keyword.len() > EXTENDED_WORKFLOW_MAX_SELECTION_KEYWORD_BYTES {
                return Err(format!(
                    "{path}.selection_keywords[{keyword_index}] must be at most {EXTENDED_WORKFLOW_MAX_SELECTION_KEYWORD_BYTES} UTF-8 bytes; got {}",
                    keyword.len()
                ));
            }
            if !keywords.insert(keyword) {
                return Err(format!(
                    "{path}.selection_keywords must be unique after normalization; duplicate `{keyword}`"
                ));
            }
        }
        aggregate_bytes = aggregate_bytes
            .saturating_add(profile.name.len())
            .saturating_add(profile.role.len())
            .saturating_add(profile.agent_type.as_deref().map_or(0, str::len))
            .saturating_add(profile.model.as_deref().map_or(0, str::len))
            .saturating_add(
                profile
                    .selection_keywords
                    .iter()
                    .map(String::len)
                    .sum::<usize>(),
            );
        validate_positive_optional(&format!("{path}.token_budget"), profile.token_budget)?;
        validate_positive_optional(&format!("{path}.context_budget"), profile.context_budget)?;
        if profile
            .context_budget
            .is_some_and(|context_budget| i64::try_from(context_budget).is_err())
        {
            return Err(format!(
                "{path}.context_budget must be at most {} tokens",
                i64::MAX
            ));
        }
    }
    if aggregate_bytes > EXTENDED_WORKFLOW_MAX_PROCESS_PROFILE_BYTES {
        return Err(format!(
            "extended_workflow.{process_name}.profiles contain {aggregate_bytes} UTF-8 bytes of profile metadata; the hard aggregate limit is {EXTENDED_WORKFLOW_MAX_PROCESS_PROFILE_BYTES}"
        ));
    }
    Ok(())
}

fn normalize_selection_keyword(keyword: &str) -> Result<String, String> {
    if keyword.len() > EXTENDED_WORKFLOW_MAX_SELECTION_KEYWORD_BYTES {
        return Err(format!(
            "selection_keywords entries must be at most {EXTENDED_WORKFLOW_MAX_SELECTION_KEYWORD_BYTES} UTF-8 bytes; got {}",
            keyword.len()
        ));
    }
    let normalized = keyword
        .to_lowercase()
        .split(|character: char| !character.is_alphanumeric())
        .filter(|token| !token.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    if normalized.is_empty() {
        return Err("selection_keywords entries must not be empty".to_string());
    }
    if normalized.len() > EXTENDED_WORKFLOW_MAX_SELECTION_KEYWORD_BYTES {
        return Err(format!(
            "selection_keywords entries must be at most {EXTENDED_WORKFLOW_MAX_SELECTION_KEYWORD_BYTES} UTF-8 bytes; got {}",
            normalized.len()
        ));
    }
    Ok(normalized)
}

fn validate_profile_field_length(path: &str, value: &str) -> Result<(), String> {
    if value.len() > EXTENDED_WORKFLOW_MAX_PROFILE_FIELD_BYTES {
        Err(format!(
            "{path} must be at most {EXTENDED_WORKFLOW_MAX_PROFILE_FIELD_BYTES} UTF-8 bytes; got {}",
            value.len()
        ))
    } else {
        Ok(())
    }
}

fn validate_non_empty_optional(path: &str, value: Option<&str>) -> Result<(), String> {
    if value.is_some_and(|value| value.trim().is_empty()) {
        Err(format!("{path} must not be empty when configured"))
    } else {
        Ok(())
    }
}

fn validate_positive_optional(path: &str, value: Option<u64>) -> Result<(), String> {
    if value == Some(0) {
        Err(format!("{path} must be positive when configured"))
    } else {
        Ok(())
    }
}

fn profile(name: &str, role: &str) -> WorkflowAgentProfileConfig {
    WorkflowAgentProfileConfig {
        name: name.to_string(),
        role: role.to_string(),
        agent_type: None,
        selection_keywords: Vec::new(),
        source: WorkflowProfileSource::BuiltIn,
        model: None,
        reasoning_effort: None,
        token_budget: None,
        context_budget: None,
    }
}

fn configured_profile(
    name: &str,
    role: &str,
    model: &str,
    reasoning_effort: ReasoningEffort,
) -> WorkflowAgentProfileConfig {
    WorkflowAgentProfileConfig {
        model: Some(model.to_string()),
        reasoning_effort: Some(reasoning_effort),
        ..profile(name, role)
    }
}

fn default_expert_profiles() -> Vec<WorkflowAgentProfileConfig> {
    vec![
        configured_profile(
            "expert_1",
            "Architecture",
            "gpt-5.6-sol",
            ReasoningEffort::High,
        ),
        configured_profile(
            "expert_2",
            "Orchestration and context",
            "gpt-5.6-terra",
            ReasoningEffort::XHigh,
        ),
        configured_profile(
            "expert_3",
            "Security and permissions",
            "gpt-5.5",
            ReasoningEffort::High,
        ),
        configured_profile(
            "expert_4",
            "Testing and compatibility",
            "gpt-5.6-sol",
            ReasoningEffort::Max,
        ),
        profile("cli_ux", "CLI and user experience"),
        profile("performance", "Performance and concurrency"),
    ]
}

fn default_reviewer_profiles() -> Vec<WorkflowAgentProfileConfig> {
    vec![
        configured_profile(
            "reviewer_1",
            "Correctness and architecture",
            "gpt-5.6-sol",
            ReasoningEffort::High,
        ),
        configured_profile(
            "reviewer_2",
            "Security and permissions",
            "gpt-5.6-terra",
            ReasoningEffort::XHigh,
        ),
        configured_profile(
            "reviewer_3",
            "Testing and regression",
            "gpt-5.5",
            ReasoningEffort::High,
        ),
        configured_profile(
            "reviewer_4",
            "Product and workflow compatibility",
            "gpt-5.6-sol",
            ReasoningEffort::Max,
        ),
        profile("performance", "Performance"),
        profile("documentation", "Documentation and user experience"),
    ]
}

#[cfg(test)]
#[path = "extended_workflow_tests.rs"]
mod tests;
