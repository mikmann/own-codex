//! Deterministic planning and validation for the extended subagent workflow.
//!
//! This module deliberately does not implement a second agent runtime. It plans work for the
//! existing MultiAgentV2 tools and validates the structured artifacts returned by those agents.

use codex_config::extended_workflow::ExtendedWorkflowConfig;
use codex_config::extended_workflow::WorkflowAgentProfileConfig;
use codex_config::extended_workflow::WorkflowProcessConfig;
use codex_protocol::ThreadId;
use codex_protocol::openai_models::ReasoningEffort;
use codex_protocol::protocol::SessionSource;
use serde::Deserialize;
use serde::Serialize;
use sha2::Digest;
use sha2::Sha256;
use std::collections::BTreeSet;
use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::Mutex;
use std::sync::PoisonError;

const DEFAULT_ESTIMATED_AGENT_TOKENS: u64 = 1_200;
const MAX_CLASSIFIER_INPUT_CHARS: usize = 32_000;
pub(crate) const MAX_WORKFLOW_TASK_BYTES: usize = 6_000;
pub(crate) const MAX_WORKFLOW_CHANGE_SUMMARY_BYTES: usize = 6_000;
pub(crate) const MAX_WORKFLOW_EXPECTED_SUBJECT_BYTES: usize = 4_096;
pub(crate) const MAX_WORKFLOW_RESULT_ID_BYTES: usize = 256;
pub(crate) const MAX_WORKFLOW_AGENT_REFERENCE_BYTES: usize = 512;
pub(crate) const WORKFLOW_RESULT_CONTRACT_MARKER: &str =
    "\n\n<extended_workflow_result_contract>\n";

pub(crate) fn require_bounded_workflow_text(
    label: &str,
    value: &str,
    max_bytes: usize,
) -> Result<(), String> {
    let bytes = value.len();
    if bytes > max_bytes {
        Err(format!(
            "{label} is {bytes} UTF-8 bytes; the hard limit is {max_bytes}"
        ))
    } else {
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RequestClass {
    SimpleAnswer,
    WorkOrder,
    Concept,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RequestRisk {
    Routine,
    Elevated,
    High,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RequestClassification {
    pub class: RequestClass,
    pub risk: RequestRisk,
    pub domains: Vec<String>,
    pub has_material_uncertainty: bool,
    pub complexity_score: u8,
    pub reasons: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowAgentPlan {
    pub name: String,
    pub role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<ReasoningEffort>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token_budget: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_budget: Option<u64>,
    pub read_only: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowProcessPlan {
    pub enabled: bool,
    pub requested_count: usize,
    pub selected: Vec<WorkflowAgentPlan>,
    pub estimated_tokens: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub advisory_round_budget: Option<u64>,
    pub budget_blocked: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub budget_message: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowTurnPlan {
    pub classification: RequestClassification,
    pub experts: WorkflowProcessPlan,
    pub reviewer_policy_enabled: bool,
    pub reviewer_min: usize,
    pub reviewer_max: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub advisory_workflow_budget: Option<u64>,
    pub budget_scope: String,
    pub audit_mode_enabled: bool,
    pub audit_files_written: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowAgentKind {
    Expert,
    Reviewer,
}

impl WorkflowAgentKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Expert => "Expert",
            Self::Reviewer => "Reviewer",
        }
    }
}

pub(crate) fn sha256_bytes(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub(crate) fn workflow_assignment_sha256(
    turn_id: &str,
    kind: WorkflowAgentKind,
    name: &str,
    role: &str,
    task: &str,
) -> String {
    let mut hasher = Sha256::new();
    hasher.update([match kind {
        WorkflowAgentKind::Expert => 0,
        WorkflowAgentKind::Reviewer => 1,
    }]);
    for field in [turn_id, name, role, task] {
        hasher.update((field.len() as u64).to_be_bytes());
        hasher.update(field.as_bytes());
    }
    format!("{:x}", hasher.finalize())
}

pub(crate) fn workflow_assignment_subject(message: &str) -> &str {
    message
        .rsplit_once(WORKFLOW_RESULT_CONTRACT_MARKER)
        .map_or(message, |(task, _contract)| task)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowSpawnAssignment {
    pub kind: WorkflowAgentKind,
    pub name: String,
    pub role: String,
    pub task: String,
}

/// Server-side ledger for the concrete workflow selection of one root turn.
///
/// The model-visible plan is guidance; this ledger is the enforcement boundary. A selected
/// profile can be reserved only once, reviewer selections must first be installed by the native
/// reviewer planner, and failed spawns release their reservation.
#[derive(Debug)]
pub struct WorkflowTurnRuntimeState {
    inner: Mutex<WorkflowTurnRuntimeInner>,
}

#[derive(Debug)]
struct WorkflowTurnRuntimeInner {
    classification: RequestClassification,
    experts: WorkflowProcessPlan,
    reviewers: Option<WorkflowProcessPlan>,
    reviewer_policy_enabled: bool,
    reviewer_min: usize,
    reviewer_max: usize,
    completion_gate_prompts: u8,
    reserved: HashSet<(WorkflowAgentKind, String)>,
    assignments: HashMap<ThreadId, WorkflowSpawnAssignment>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkflowCompletionGate {
    Satisfied,
    Continue(String),
    Blocked(String),
}

impl WorkflowTurnRuntimeState {
    pub fn new(plan: &WorkflowTurnPlan) -> Self {
        Self {
            inner: Mutex::new(WorkflowTurnRuntimeInner {
                classification: plan.classification.clone(),
                experts: plan.experts.clone(),
                reviewers: None,
                reviewer_policy_enabled: plan.reviewer_policy_enabled,
                reviewer_min: plan.reviewer_min,
                reviewer_max: plan.reviewer_max,
                completion_gate_prompts: 0,
                reserved: HashSet::new(),
                assignments: HashMap::new(),
            }),
        }
    }

    pub fn install_reviewers(&self, plan: &WorkflowProcessPlan) -> Result<(), String> {
        let mut inner = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
        match inner.reviewers.as_ref() {
            Some(existing) if existing == plan => Ok(()),
            Some(_) => Err(
                "reviewer selection was already fixed for this turn and cannot be replaced"
                    .to_string(),
            ),
            None => {
                inner.reviewers = Some(plan.clone());
                Ok(())
            }
        }
    }

    pub fn reserve_spawn(
        &self,
        kind: WorkflowAgentKind,
        name: &str,
        role: &str,
    ) -> Result<WorkflowAgentPlan, String> {
        let mut inner = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
        let process = match kind {
            WorkflowAgentKind::Expert => &inner.experts,
            WorkflowAgentKind::Reviewer => inner.reviewers.as_ref().ok_or_else(|| {
                "reviewer agents cannot start before plan_workflow_reviewers fixes the turn selection"
                    .to_string()
            })?,
        };
        if !process.enabled || process.budget_blocked {
            return Err(format!(
                "the selected {} process is disabled or blocked by its configured budget",
                kind.label().to_lowercase()
            ));
        }
        let selected = process
            .selected
            .iter()
            .find(|profile| profile.name == name && profile.role == role)
            .cloned()
            .ok_or_else(|| {
                format!(
                    "{} profile `{name}` with role `{role}` is not selected for this turn",
                    kind.label()
                )
            })?;
        let reservation = (kind, name.to_string());
        if !inner.reserved.insert(reservation) {
            return Err(format!(
                "{} profile `{name}` was already consumed for this turn",
                kind.label()
            ));
        }
        Ok(selected)
    }

    pub fn release_spawn(&self, kind: WorkflowAgentKind, name: &str) {
        self.inner
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .reserved
            .remove(&(kind, name.to_string()));
    }

    pub fn commit_spawn(
        &self,
        thread_id: ThreadId,
        kind: WorkflowAgentKind,
        name: &str,
        role: &str,
        task: &str,
    ) {
        self.inner
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .assignments
            .insert(
                thread_id,
                WorkflowSpawnAssignment {
                    kind,
                    name: name.to_string(),
                    role: role.to_string(),
                    task: task.to_string(),
                },
            );
    }

    pub fn assignment_for_thread(&self, thread_id: ThreadId) -> Option<WorkflowSpawnAssignment> {
        self.inner
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .assignments
            .get(&thread_id)
            .cloned()
    }

    pub fn reviewer_completion_gate(
        &self,
        validations: &[codex_agent_graph_store::WorkflowReviewerValidation],
    ) -> WorkflowCompletionGate {
        let mut inner = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
        if inner.classification.class != RequestClass::WorkOrder
            || !inner.reviewer_policy_enabled
            || inner.reviewer_max == 0
        {
            return WorkflowCompletionGate::Satisfied;
        }
        let Some(reviewers) = inner.reviewers.as_ref() else {
            let details = format!(
                "plan_workflow_reviewers must select the change-relevant reviewer roles within the configured min={} and max={} bounds before this work-order turn can complete",
                inner.reviewer_min, inner.reviewer_max
            );
            if inner.completion_gate_prompts < 2 {
                inner.completion_gate_prompts += 1;
                return WorkflowCompletionGate::Continue(format!(
                    "Extended workflow completion gate: {details}. Do not present the ticket as completed until the gate passes."
                ));
            }
            return WorkflowCompletionGate::Blocked(format!(
                "Extended workflow blocked after two targeted continuation attempts: {details}. The ticket is not validated."
            ));
        };
        let required = reviewers.selected.len();
        if required == 0 {
            return WorkflowCompletionGate::Satisfied;
        }
        let required_roles = reviewers
            .selected
            .iter()
            .map(|reviewer| reviewer.role.trim().to_lowercase())
            .collect::<HashSet<_>>();
        let passing_agents = validations
            .iter()
            .filter(|validation| {
                validation.reviewer_pass
                    && !validation.has_high_or_critical
                    && required_roles.contains(&validation.role.trim().to_lowercase())
            })
            .map(|validation| validation.agent_thread_id)
            .collect::<HashSet<_>>();
        let passing_roles = validations
            .iter()
            .filter(|validation| {
                validation.reviewer_pass
                    && !validation.has_high_or_critical
                    && required_roles.contains(&validation.role.trim().to_lowercase())
            })
            .map(|validation| validation.role.trim().to_lowercase())
            .filter(|role| !role.is_empty())
            .collect::<HashSet<_>>();
        if passing_agents.len() >= required && required_roles.is_subset(&passing_roles) {
            return WorkflowCompletionGate::Satisfied;
        }

        let details = if validations
            .iter()
            .any(|validation| !validation.reviewer_pass || validation.has_high_or_critical)
        {
            format!(
                "reviewer validation has not passed: {}/{required} distinct passing agents and {}/{required} distinct passing roles; address the findings, request a new output revision, and validate it",
                passing_agents.len(),
                passing_roles.len()
            )
        } else {
            format!(
                "reviewer validation is incomplete: {}/{required} distinct passing agents and {}/{required} distinct passing roles",
                passing_agents.len(),
                passing_roles.len()
            )
        };
        if inner.completion_gate_prompts < 2 {
            inner.completion_gate_prompts += 1;
            WorkflowCompletionGate::Continue(format!(
                "Extended workflow completion gate: {details}. Do not present the ticket as completed until the gate passes."
            ))
        } else {
            WorkflowCompletionGate::Blocked(format!(
                "Extended workflow blocked after two targeted continuation attempts: {details}. The ticket is not validated."
            ))
        }
    }

    pub fn requires_reviewer_completion(&self) -> bool {
        let inner = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
        inner.classification.class == RequestClass::WorkOrder
            && inner.reviewer_policy_enabled
            && inner.reviewer_max > 0
    }
}

pub fn is_root_workflow_source(source: &SessionSource) -> bool {
    matches!(
        source,
        SessionSource::Cli
            | SessionSource::VSCode
            | SessionSource::Exec
            | SessionSource::Mcp
            | SessionSource::Custom(_)
            | SessionSource::Unknown
    )
}

pub fn classify_request(input: &str) -> RequestClassification {
    let normalized = input
        .chars()
        .take(MAX_CLASSIFIER_INPUT_CHARS)
        .collect::<String>()
        .to_lowercase();
    let word_count = normalized.split_whitespace().count();

    let concept_signals = count_signals(
        &normalized,
        &[
            "konzept",
            "entwurf",
            "architekturvorschlag",
            "design proposal",
            "concept",
            "strategie",
            "strategy",
            "roadmap",
            "planung",
        ],
    );
    let work_signals = count_signals(
        &normalized,
        &[
            "implementier",
            "ändere",
            "aendere",
            "erstelle",
            "behebe",
            "reparier",
            "füge ",
            "fuege ",
            "refactor",
            "migrate",
            "update ",
            "build ",
            "commit",
            "patch",
            "fix ",
        ],
    );
    let high_risk_signals = count_signals(
        &normalized,
        &[
            "sicherheit",
            "security",
            "berechtigung",
            "permission",
            "credential",
            "secret",
            "produktion",
            "production",
            "löschen",
            "loeschen",
            "delete",
            "zahlung",
            "payment",
        ],
    );
    let uncertainty_signals = count_signals(
        &normalized,
        &[
            "unklar",
            "unsicher",
            "uncertain",
            "unknown",
            "prüfe zuerst",
            "pruefe zuerst",
            "investigate",
            "diagnose",
        ],
    );
    let domains = detect_domains(&normalized);
    let scope_signals = count_signals(
        &normalized,
        &[
            "test",
            "dokumentation",
            "documentation",
            "konfiguration",
            "configuration",
            "cli",
            "api",
            "datenbank",
            "database",
            "concurrency",
            "nebenläufig",
            "performance",
        ],
    );

    let strong_work_signals = count_signals(
        &normalized,
        &[
            "implementier",
            "ändere",
            "aendere",
            "behebe",
            "reparier",
            "refactor",
            "migrate",
            "commit",
            "patch",
            "fix ",
            "arbeitsauftrag",
        ],
    );
    let class = if concept_signals > 0 && strong_work_signals == 0 {
        RequestClass::Concept
    } else if work_signals > 0 {
        RequestClass::WorkOrder
    } else {
        RequestClass::SimpleAnswer
    };
    let risk = if high_risk_signals >= 2 {
        RequestRisk::High
    } else if high_risk_signals == 1 || uncertainty_signals > 0 {
        RequestRisk::Elevated
    } else {
        RequestRisk::Routine
    };
    let mut complexity_score = 0u8;
    complexity_score += u8::try_from(domains.len().saturating_sub(1).min(3)).unwrap_or(3);
    complexity_score += u8::from(word_count > 120);
    complexity_score += u8::from(word_count > 350);
    complexity_score += u8::from(scope_signals >= 3);
    complexity_score += match risk {
        RequestRisk::Routine => 0,
        RequestRisk::Elevated => 1,
        RequestRisk::High => 2,
    };
    complexity_score = complexity_score.min(6);

    let mut reasons = vec![match class {
        RequestClass::SimpleAnswer => "no implementation or concept signal detected".to_string(),
        RequestClass::WorkOrder => "implementation/change signal detected".to_string(),
        RequestClass::Concept => {
            "concept/design signal detected without an implementation signal".to_string()
        }
    }];
    if domains.len() > 1 {
        reasons.push(format!("{} distinct domains detected", domains.len()));
    }
    if risk != RequestRisk::Routine {
        reasons.push(format!("risk classified as {risk:?}").to_lowercase());
    }
    if uncertainty_signals > 0 {
        reasons.push("material uncertainty signal detected".to_string());
    }

    RequestClassification {
        class,
        risk,
        domains,
        has_material_uncertainty: uncertainty_signals > 0,
        complexity_score,
        reasons,
    }
}

pub fn plan_turn(input: &str, config: &ExtendedWorkflowConfig) -> WorkflowTurnPlan {
    let classification = classify_request(input);
    let desired_experts = desired_expert_count(&classification, &config.experts);
    let expert_workflow_budget = subagent_round_share(
        config.workflow_budget_tokens,
        config.reviewers.enabled && config.reviewers.max > 0,
    );
    let experts = plan_process(
        &classification,
        &config.experts,
        desired_experts,
        expert_workflow_budget,
    );
    WorkflowTurnPlan {
        classification,
        experts,
        reviewer_policy_enabled: config.reviewers.enabled && config.reviewers.max > 0,
        reviewer_min: config.reviewers.min,
        reviewer_max: config.reviewers.max,
        advisory_workflow_budget: config.workflow_budget_tokens,
        budget_scope: "Budgets are advisory pre-spawn estimates for subagents. The workflow budget is split conservatively between enabled expert and reviewer rounds; provider context/input/output and main-agent tokens are reported by existing telemetry and are not presented as hard limits."
            .to_string(),
        audit_mode_enabled: config.audit_mode,
        audit_files_written: false,
    }
}

pub fn plan_reviewers(
    change_summary: &str,
    config: &ExtendedWorkflowConfig,
) -> WorkflowProcessPlan {
    let classification = classify_request(change_summary);
    let desired = desired_reviewer_count(&classification, &config.reviewers);
    let reviewer_workflow_budget = subagent_round_share(
        config.workflow_budget_tokens,
        config.experts.enabled && config.experts.max > 0,
    );
    plan_process(
        &classification,
        &config.reviewers,
        desired,
        reviewer_workflow_budget,
    )
}

fn subagent_round_share(workflow_budget: Option<u64>, another_round_enabled: bool) -> Option<u64> {
    workflow_budget.map(|budget| {
        if another_round_enabled {
            budget / 2
        } else {
            budget
        }
    })
}

fn desired_expert_count(
    classification: &RequestClassification,
    process: &WorkflowProcessConfig,
) -> usize {
    if !process.enabled || process.max == 0 {
        return 0;
    }
    if classification.class == RequestClass::SimpleAnswer
        && classification.domains.len() < 2
        && classification.risk == RequestRisk::Routine
        && !classification.has_material_uncertainty
    {
        return 0;
    }
    dynamic_count(classification, process)
}

fn desired_reviewer_count(
    classification: &RequestClassification,
    process: &WorkflowProcessConfig,
) -> usize {
    if !process.enabled || process.max == 0 {
        0
    } else {
        dynamic_count(classification, process)
    }
}

fn dynamic_count(classification: &RequestClassification, process: &WorkflowProcessConfig) -> usize {
    let domain_pressure = classification.domains.len().saturating_sub(1);
    let risk_pressure = match classification.risk {
        RequestRisk::Routine => 0,
        RequestRisk::Elevated => 1,
        RequestRisk::High => 2,
    };
    let complexity_pressure = usize::from(classification.complexity_score >= 3)
        + usize::from(classification.complexity_score >= 5);
    process
        .min
        .saturating_add(domain_pressure)
        .saturating_add(risk_pressure)
        .saturating_add(complexity_pressure)
        .min(process.max)
}

fn plan_process(
    classification: &RequestClassification,
    process: &WorkflowProcessConfig,
    mut desired: usize,
    workflow_budget: Option<u64>,
) -> WorkflowProcessPlan {
    if !process.enabled || process.max == 0 || desired == 0 {
        return WorkflowProcessPlan {
            enabled: process.enabled,
            requested_count: 0,
            selected: Vec::new(),
            estimated_tokens: 0,
            advisory_round_budget: process.round_budget_tokens,
            budget_blocked: false,
            budget_message: None,
        };
    }

    let effective_budget = min_optional(process.round_budget_tokens, workflow_budget);
    let mut budget_message = None;
    if let Some(budget) = effective_budget {
        while desired > process.min
            && estimated_tokens_for_selection(classification, &process.profiles, desired) > budget
        {
            desired -= 1;
        }
        let minimum_estimate =
            estimated_tokens_for_selection(classification, &process.profiles, process.min);
        if minimum_estimate > budget {
            return WorkflowProcessPlan {
                enabled: true,
                requested_count: desired,
                selected: Vec::new(),
                estimated_tokens: minimum_estimate,
                advisory_round_budget: process.round_budget_tokens,
                budget_blocked: true,
                budget_message: Some(format!(
                    "advisory budget {budget} tokens is below the estimated {minimum_estimate} tokens required for the configured minimum of {} agents; no agent should be started",
                    process.min
                )),
            };
        }
        if desired < dynamic_count(classification, process) {
            budget_message = Some(format!(
                "agent count reduced to {desired} before spawning to stay within the advisory budget of {budget} tokens"
            ));
        }
    }

    let selected_profiles = select_profiles(classification, &process.profiles, desired);
    let estimated_tokens = selected_profiles
        .iter()
        .map(|profile| estimated_profile_tokens(profile))
        .fold(0, u64::saturating_add);
    WorkflowProcessPlan {
        enabled: true,
        requested_count: desired,
        selected: selected_profiles
            .into_iter()
            .map(|profile| WorkflowAgentPlan {
                name: profile.name.clone(),
                role: profile.role.clone(),
                agent_type: profile.agent_type.clone(),
                model: profile.model.clone(),
                reasoning_effort: profile.reasoning_effort.clone(),
                token_budget: profile.token_budget,
                context_budget: profile.context_budget,
                read_only: true,
            })
            .collect(),
        estimated_tokens,
        advisory_round_budget: process.round_budget_tokens,
        budget_blocked: false,
        budget_message,
    }
}

fn select_profiles<'a>(
    classification: &RequestClassification,
    profiles: &'a [WorkflowAgentProfileConfig],
    count: usize,
) -> Vec<&'a WorkflowAgentProfileConfig> {
    let mut ranked = profiles
        .iter()
        .enumerate()
        .map(|(index, profile)| {
            let text = format!("{} {}", profile.name, profile.role).to_lowercase();
            let relevance = classification
                .domains
                .iter()
                .filter(|domain| profile_matches_domain(&text, domain))
                .count();
            let risk_relevance = usize::from(
                classification.risk != RequestRisk::Routine
                    && (text.contains("security")
                        || text.contains("sicherheit")
                        || text.contains("reliability")
                        || text.contains("zuverläss")),
            );
            (relevance + risk_relevance, index, profile)
        })
        .collect::<Vec<_>>();
    ranked.sort_by_key(|(score, index, _)| (std::cmp::Reverse(*score), *index));
    ranked
        .into_iter()
        .take(count)
        .map(|(_, _, profile)| profile)
        .collect()
}

fn profile_matches_domain(profile: &str, domain: &str) -> bool {
    match domain {
        "architecture" => profile.contains("architect"),
        "security" => profile.contains("security") || profile.contains("sicherheit"),
        "testing" => profile.contains("test") || profile.contains("compatib"),
        "cli_ux" => {
            profile.contains("cli")
                || profile.contains("user experience")
                || profile.contains("documentation")
        }
        "data" => profile.contains("data") || profile.contains("database"),
        "concurrency" => {
            profile.contains("concurrency")
                || profile.contains("reliability")
                || profile.contains("performance")
        }
        _ => false,
    }
}

fn estimated_tokens_for_selection(
    classification: &RequestClassification,
    profiles: &[WorkflowAgentProfileConfig],
    count: usize,
) -> u64 {
    select_profiles(classification, profiles, count)
        .into_iter()
        .map(estimated_profile_tokens)
        .fold(0, u64::saturating_add)
}

fn estimated_profile_tokens(profile: &WorkflowAgentProfileConfig) -> u64 {
    profile
        .token_budget
        .unwrap_or(DEFAULT_ESTIMATED_AGENT_TOKENS)
}

fn min_optional(left: Option<u64>, right: Option<u64>) -> Option<u64> {
    match (left, right) {
        (Some(left), Some(right)) => Some(left.min(right)),
        (Some(value), None) | (None, Some(value)) => Some(value),
        (None, None) => None,
    }
}

fn count_signals(text: &str, signals: &[&str]) -> usize {
    signals
        .iter()
        .filter(|signal| text.contains(**signal))
        .count()
}

fn detect_domains(text: &str) -> Vec<String> {
    let candidates = [
        ("architecture", ["architektur", "architecture", "design"]),
        ("security", ["sicherheit", "security", "permission"]),
        ("testing", ["test", "quality", "kompatibilität"]),
        ("cli_ux", ["cli", "terminal", "ux"]),
        ("data", ["datenbank", "database", "migration"]),
        ("concurrency", ["nebenläufig", "concurrency", "parallel"]),
    ];
    candidates
        .into_iter()
        .filter(|(_, signals)| signals.iter().any(|signal| text.contains(signal)))
        .map(|(domain, _)| domain.to_string())
        .collect()
}

mod contracts;
pub use contracts::*;

pub fn distinct_roles(plan: &WorkflowProcessPlan) -> bool {
    let roles = plan
        .selected
        .iter()
        .map(|agent| agent.role.to_lowercase())
        .collect::<BTreeSet<_>>();
    roles.len() == plan.selected.len()
}

#[cfg(test)]
#[path = "extended_workflow_tests.rs"]
mod tests;
