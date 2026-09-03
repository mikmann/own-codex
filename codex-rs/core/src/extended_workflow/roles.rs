use super::RequestClassification;
use super::RequestRisk;
use super::WorkflowAgentPlan;
use super::WorkflowProcessPlan;
use crate::agent::role::DEFAULT_ROLE_NAME;
use crate::agent::role::WorkflowRoleMaterial;
use crate::agent::role::apply_workflow_role_material_to_config;
use crate::agent::role::load_workflow_role_material;
use crate::config::Config;
use codex_config::extended_workflow::WorkflowAgentProfileConfig;
use sha2::Digest;
use sha2::Sha256;

const MAX_ROLE_INSTRUCTION_TOKENS: u64 = 8_000;
const ROLE_INSTRUCTION_REVIEW_TOKENS: u64 = 1_000;
const WORKFLOW_ROLE_SECURITY_BINDING: &str =
    "extended-workflow-role-v1;read-only;approval-never;no-child-agents;no-apps;same-digest";

pub(super) struct ProfileSelection<'a> {
    pub(super) profile: &'a WorkflowAgentProfileConfig,
    pub(super) matched_keywords: Vec<String>,
    pub(super) selection_reason: String,
}

pub(super) fn select_profiles<'a>(
    classification: &RequestClassification,
    selection_text: &str,
    profiles: &'a [WorkflowAgentProfileConfig],
    count: usize,
) -> Vec<ProfileSelection<'a>> {
    let has_explicit_keywords = profiles
        .iter()
        .any(|profile| !profile.selection_keywords.is_empty());
    let selection_tokens = has_explicit_keywords.then(|| {
        lexical_tokens(&format!(
            "{} {}",
            selection_text,
            classification.domains.join(" ")
        ))
    });
    let mut ranked = profiles
        .iter()
        .enumerate()
        .map(|(index, profile)| {
            let profile_text = format!("{} {}", profile.name, profile.role).to_lowercase();
            let domain_score = classification
                .domains
                .iter()
                .filter(|domain| super::profile_matches_domain(&profile_text, domain))
                .count();
            let risk_score = usize::from(
                classification.risk != RequestRisk::Routine
                    && (profile_text.contains("security")
                        || profile_text.contains("sicherheit")
                        || profile_text.contains("reliability")
                        || profile_text.contains("zuverläss")),
            );
            let matched_keywords = selection_tokens.as_ref().map_or_else(Vec::new, |tokens| {
                profile
                    .selection_keywords
                    .iter()
                    .filter(|keyword| phrase_matches(tokens, &lexical_tokens(keyword)))
                    .cloned()
                    .collect()
            });
            (
                matched_keywords.len(),
                domain_score + risk_score,
                index,
                profile,
                matched_keywords,
            )
        })
        .collect::<Vec<_>>();

    if has_explicit_keywords {
        ranked.sort_by_key(|(keywords, score, index, _, _)| {
            (
                std::cmp::Reverse(*keywords),
                std::cmp::Reverse(*score),
                *index,
            )
        });
    } else {
        // Preserve the Alpha-9 profile ordering when no explicit keyword exists.
        ranked.sort_by_key(|(_, score, index, _, _)| (std::cmp::Reverse(*score), *index));
    }

    ranked
        .into_iter()
        .take(count)
        .map(
            |(keyword_score, domain_score, index, profile, matched_keywords)| {
                let selection_reason = if keyword_score > 0 {
                    format!(
                        "{keyword_score} explicit keyword match(es), domain/risk score {domain_score}, declaration order {}",
                        index + 1
                    )
                } else {
                    format!(
                        "domain/risk score {domain_score}, declaration order {}",
                        index + 1
                    )
                };
                ProfileSelection {
                    profile,
                    matched_keywords,
                    selection_reason,
                }
            },
        )
        .collect()
}

pub(crate) async fn preflight_process_plan(
    plan: &mut WorkflowProcessPlan,
    config: &Config,
    selection_text: &str,
) -> Result<(), String> {
    for profile in &mut plan.selected {
        let role_name = profile.agent_type.as_deref().unwrap_or(DEFAULT_ROLE_NAME);
        let material = load_workflow_role_material(config, role_name).await?;
        if material.instruction_tokens > MAX_ROLE_INSTRUCTION_TOKENS {
            return Err(format!(
                "workflow profile `{}` resolves agent_type `{role_name}` to an estimated {} instruction tokens; the hard limit is {MAX_ROLE_INSTRUCTION_TOKENS}",
                profile.name, material.instruction_tokens
            ));
        }
        let request_tokens = u64::try_from(selection_text.len().div_ceil(4)).unwrap_or(u64::MAX);
        if let Some(context_budget) = profile.context_budget
            && request_tokens.saturating_add(material.instruction_tokens) > context_budget
        {
            return Err(format!(
                "workflow profile `{}` cannot fit the root request and resolved role instructions into its {context_budget}-token context_budget",
                profile.name
            ));
        }
        let fingerprint = role_fingerprint(&material);
        profile.role_source = Some(material.source.to_string());
        profile.role_fingerprint = Some(fingerprint[..12].to_string());
        profile.role_fingerprint_sha256 = Some(fingerprint);
        profile.role_instruction_tokens = material.instruction_tokens;
        profile.role_preflight = if material.instruction_tokens > ROLE_INSTRUCTION_REVIEW_TOKENS {
            super::WorkflowRolePreflightStatus::ManualReviewRecommended
        } else {
            super::WorkflowRolePreflightStatus::Validated
        };
    }
    Ok(())
}

pub(crate) async fn apply_profile_role_binding(
    profile: &WorkflowAgentPlan,
    config: &mut Config,
) -> Result<(), String> {
    let expected = profile.role_fingerprint_sha256.as_deref().ok_or_else(|| {
        format!(
            "workflow profile `{}` has no validated role fingerprint; create a new workflow plan",
            profile.name
        )
    })?;
    let role_name = profile.agent_type.as_deref().unwrap_or(DEFAULT_ROLE_NAME);
    let material = load_workflow_role_material(config, role_name).await?;
    if role_fingerprint(&material) != expected {
        return Err(format!(
            "workflow profile `{}` agent_type `{role_name}` changed after preflight; create a new workflow plan before spawning agents",
            profile.name
        ));
    }
    apply_workflow_role_material_to_config(config, &material).await
}

pub(crate) async fn apply_persisted_role_binding(
    config: &mut Config,
    role_name: &str,
    expected_fingerprint: &str,
) -> Result<(), String> {
    if expected_fingerprint.len() != 64
        || !expected_fingerprint
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
    {
        return Err("persisted workflow role fingerprint is invalid".to_string());
    }
    let material = load_workflow_role_material(config, role_name).await?;
    if role_fingerprint(&material) != expected_fingerprint {
        return Err(format!(
            "agent_type `{role_name}` changed after the workflow child was created; start a new workflow plan instead of resuming this child"
        ));
    }
    apply_workflow_role_material_to_config(config, &material).await
}

fn role_fingerprint(material: &WorkflowRoleMaterial) -> String {
    let mut hasher = Sha256::new();
    hash_field(&mut hasher, material.role_name.as_bytes());
    hash_field(&mut hasher, material.source.as_bytes());
    if let Some(path) = material.canonical_path.as_deref() {
        hash_field(&mut hasher, path.as_os_str().as_encoded_bytes());
    } else {
        hash_field(&mut hasher, &[]);
    }
    hash_field(&mut hasher, &material.config_bytes);
    hash_field(&mut hasher, WORKFLOW_ROLE_SECURITY_BINDING.as_bytes());
    format!("{:x}", hasher.finalize())
}

fn hash_field(hasher: &mut Sha256, value: &[u8]) {
    hasher.update((value.len() as u64).to_be_bytes());
    hasher.update(value);
}

fn lexical_tokens(value: &str) -> Vec<String> {
    value
        .to_lowercase()
        .split(|character: char| !character.is_alphanumeric())
        .filter(|token| !token.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

fn phrase_matches(haystack: &[String], needle: &[String]) -> bool {
    !needle.is_empty()
        && needle.len() <= haystack.len()
        && haystack
            .windows(needle.len())
            .any(|window| window == needle)
}

#[cfg(test)]
#[path = "roles_tests.rs"]
mod tests;
