use super::PreviousSectionState;
use super::WorldStateHash;
use super::WorldStateSection;
use crate::context::ContextualUserFragment;
use crate::context::ExtendedWorkflowPolicy;
use codex_config::extended_workflow::ExtendedWorkflowConfig;

/// Effective extended-workflow policy currently visible to the root model.
#[derive(Clone, Debug)]
pub(crate) struct ExtendedWorkflowPolicyState {
    policy: ExtendedWorkflowPolicy,
}

impl ExtendedWorkflowPolicyState {
    pub(crate) fn try_new(config: &ExtendedWorkflowConfig) -> Result<Self, String> {
        Ok(Self {
            policy: ExtendedWorkflowPolicy::try_new(config)?,
        })
    }
}

impl WorldStateSection for ExtendedWorkflowPolicyState {
    const ID: &'static str = "extended_workflow_policy";
    type Snapshot = WorldStateHash;

    fn snapshot(&self) -> Self::Snapshot {
        WorldStateHash::from_fragment(&self.policy)
    }

    fn matches_current_legacy_fragment(&self, role: &str, text: &str) -> bool {
        role == self.policy.role() && text == self.policy.render()
    }

    fn render_diff(
        &self,
        previous: PreviousSectionState<'_, Self::Snapshot>,
    ) -> Option<Box<dyn ContextualUserFragment>> {
        match previous {
            PreviousSectionState::Known(previous) if previous == &self.snapshot() => None,
            PreviousSectionState::Unknown => None,
            PreviousSectionState::Known(_) | PreviousSectionState::Absent => {
                Some(Box::new(self.policy.clone()))
            }
        }
    }
}
