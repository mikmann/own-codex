use super::Session;
use super::turn_context::TurnContext;
use crate::extended_workflow::WorkflowCompletionGate;
use crate::extended_workflow::WorkflowTurnRuntimeState;

pub(super) async fn completion_gate(
    sess: &Session,
    turn_context: &TurnContext,
) -> WorkflowCompletionGate {
    let Some(runtime) = turn_context
        .extension_data
        .get::<WorkflowTurnRuntimeState>()
    else {
        return WorkflowCompletionGate::Satisfied;
    };
    if !runtime.requires_reviewer_completion() {
        return WorkflowCompletionGate::Satisfied;
    }
    let mut validations = match sess
        .services
        .agent_control
        .list_workflow_reviewer_validations(sess.thread_id, &turn_context.sub_id)
        .await
    {
        Ok(validations) => validations,
        Err(error) => {
            return WorkflowCompletionGate::Blocked(format!(
                "Extended workflow reviewer state could not be verified: {error}. The ticket is not validated."
            ));
        }
    };
    for validation in &mut validations {
        let completed = match sess
            .services
            .agent_control
            .completed_workflow_agent(validation.agent_thread_id)
            .await
        {
            Ok(completed) => completed,
            Err(error) => {
                return WorkflowCompletionGate::Blocked(format!(
                    "Extended workflow reviewer output for `{}` could not be verified: {error}. The ticket is not validated.",
                    validation.agent_thread_id
                ));
            }
        };
        validation.reviewer_pass &= completed.output_revision == validation.output_revision
            && completed.output_sha256 == validation.output_sha256;
    }
    runtime.reviewer_completion_gate(&validations)
}
