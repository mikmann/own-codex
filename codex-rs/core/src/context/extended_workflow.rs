use super::ContextualUserFragment;
use crate::extended_workflow::WorkflowTurnPlan;
use codex_config::extended_workflow::ExtendedWorkflowConfig;
use std::fmt::Write;

const POLICY_PREFIX: &str = "<extended_workflow_policy>";
const POLICY_SUFFIX: &str = "</extended_workflow_policy>";
const TURN_PREFIX: &str = "<extended_workflow_turn>";
const TURN_SUFFIX: &str = "</extended_workflow_turn>";
pub(crate) const MAX_EXTENDED_WORKFLOW_FRAGMENT_BYTES: usize = 9_000;

/// Stable workflow policy emitted once and refreshed only when its configuration changes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ExtendedWorkflowPolicy {
    body: String,
}

impl ExtendedWorkflowPolicy {
    pub(crate) fn try_new(config: &ExtendedWorkflowConfig) -> Result<Self, String> {
        let mut body = String::from(
            "This distribution uses the extended workflow. For each root-user request, obey the deterministic classification and process plan supplied in <extended_workflow_turn>.\n\
             Classification meanings: simple_answer gives a direct answer and starts no experts unless the plan explicitly selects them; work_order changes the workspace; concept produces a design and, when requested, one or more tickets.\n\
             For every selected expert, call spawn_agent exactly once with workflow_name, workflow_type=expert, workflow_role, the listed model/reasoning settings, read_only=true, and fork_turns=none. Give a compact, bounded task and require the ExpertResult contract. Do not exceed the selected count; schedule waves when runtime capacity is lower.\n\
             Validate each expert result with validate_workflow_result. One targeted correction via followup_task is allowed only when retry_allowed=true. An invalid second result is failed and must not be used. After a result is accepted or terminally rejected, call close_agent for that agent before starting a replacement or the next capacity-limited wave. Never close an agent while a correction is still allowed.\n\
             A work order may yield one ticket, a ticket series, or a concept followed by tickets. Every ticket must contain title, goal, context, requirements, acceptance criteria, dependencies, test/validation requirements, and risks/open questions. Use the existing plan mechanism; do not create ticket files unless the user asks. Implement only the currently selected ticket, and keep all writes in the main agent.\n\
             After implementation, if reviewer_policy_enabled=true, call plan_workflow_reviewers with the actual change summary. If it is false, start no reviewers. Spawn only the returned distinct reviewer roles with workflow_name, workflow_type=reviewer, workflow_role, and read_only=true. Require ReviewerResult, validate it, assess findings, fix confirmed issues in the main agent, and rerun affected tests. High or Critical findings block completion until resolved.\n\
             Low, Medium, and High are warnings; Critical is an error because it represents expected, system-wide, or irreversible harm. Severity text is mandatory and color is supplementary.\n\
             Token/context budgets are advisory preflight controls unless described as byte or retry limits. If budget_blocked=true, do not silently reduce quality: report the limitation and do not spawn.\n\
             Do not generate separate prompt or receipt files. audit_mode only adds diagnostics to the normal rollout and never creates files. Session rollouts remain the resume mechanism.\n",
        );
        let _ = writeln!(
            body,
            "Configured experts: enabled={}, min={}, max={}; reviewers: enabled={}, min={}, max={}; max structured result bytes={}; audit_mode={}.",
            config.experts.enabled,
            config.experts.min,
            config.experts.max,
            config.reviewers.enabled,
            config.reviewers.min,
            config.reviewers.max,
            config.max_result_bytes,
            config.audit_mode,
        );
        append_profiles(&mut body, "Expert profiles", &config.experts.profiles);
        append_profiles(&mut body, "Reviewer profiles", &config.reviewers.profiles);
        let policy = Self { body };
        ensure_fragment_bound("extended workflow policy", &policy)?;
        Ok(policy)
    }
}

fn append_profiles(
    body: &mut String,
    label: &str,
    profiles: &[codex_config::extended_workflow::WorkflowAgentProfileConfig],
) {
    let _ = write!(body, "{label}:");
    for profile in profiles {
        let _ = write!(body, " {}|{}", profile.name, profile.role);
        if let Some(agent_type) = &profile.agent_type {
            let _ = write!(body, "|agent_type={agent_type}");
        }
        if let Some(model) = &profile.model {
            let _ = write!(body, "|model={model}");
        }
        if let Some(reasoning) = &profile.reasoning_effort {
            let _ = write!(body, "|reasoning={reasoning:?}");
        }
        if let Some(tokens) = profile.token_budget {
            let _ = write!(body, "|tokens~{tokens}");
        }
        if let Some(context) = profile.context_budget {
            let _ = write!(body, "|context~{context}");
        }
        body.push(';');
    }
    body.push('\n');
}

impl ContextualUserFragment for ExtendedWorkflowPolicy {
    fn role(&self) -> &'static str {
        "developer"
    }

    fn requires_separate_message(&self) -> bool {
        true
    }

    fn markers(&self) -> (&'static str, &'static str) {
        Self::type_markers()
    }

    fn type_markers() -> (&'static str, &'static str) {
        (POLICY_PREFIX, POLICY_SUFFIX)
    }

    fn body(&self) -> String {
        self.body.clone()
    }
}

/// Per-turn deterministic classifier and agent-selection result.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ExtendedWorkflowTurnContext {
    body: String,
}

impl ExtendedWorkflowTurnContext {
    pub(crate) fn try_new(plan: &WorkflowTurnPlan) -> Result<Self, String> {
        let body = serde_json::to_string(plan)
            .map_err(|error| format!("failed to serialize extended workflow turn plan: {error}"))?;
        let context = Self { body };
        ensure_fragment_bound("extended workflow turn plan", &context)?;
        Ok(context)
    }
}

fn ensure_fragment_bound(
    label: &str,
    fragment: &impl ContextualUserFragment,
) -> Result<(), String> {
    let bytes = fragment.render().len();
    if bytes > MAX_EXTENDED_WORKFLOW_FRAGMENT_BYTES {
        Err(format!(
            "{label} is {bytes} UTF-8 bytes; the hard model-context limit is {MAX_EXTENDED_WORKFLOW_FRAGMENT_BYTES}"
        ))
    } else {
        Ok(())
    }
}

impl ContextualUserFragment for ExtendedWorkflowTurnContext {
    fn role(&self) -> &'static str {
        "developer"
    }

    fn requires_separate_message(&self) -> bool {
        true
    }

    fn markers(&self) -> (&'static str, &'static str) {
        Self::type_markers()
    }

    fn type_markers() -> (&'static str, &'static str) {
        (TURN_PREFIX, TURN_SUFFIX)
    }

    fn body(&self) -> String {
        self.body.clone()
    }
}

#[cfg(test)]
#[path = "extended_workflow_tests.rs"]
mod tests;
