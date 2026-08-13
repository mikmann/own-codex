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
    let mut validations = match sess
        .services
        .agent_control
        .list_workflow_validations(sess.thread_id, &turn_context.sub_id)
        .await
    {
        Ok(validations) => validations,
        Err(error) => {
            return WorkflowCompletionGate::Blocked(format!(
                "Extended workflow state could not be verified: {error}. The request is not validated."
            ));
        }
    };
    for validation in &mut validations {
        let Some(agent_thread_id) = validation.agent_thread_id else {
            continue;
        };
        if !validation.accepted {
            continue;
        }
        let completed = match sess
            .services
            .agent_control
            .completed_workflow_agent(agent_thread_id)
            .await
        {
            Ok(completed) => completed,
            Err(error) => {
                return WorkflowCompletionGate::Blocked(format!(
                    "Extended workflow agent output for `{agent_thread_id}` could not be verified: {error}. The request is not validated."
                ));
            }
        };
        validation.accepted &= completed.output_revision == validation.output_revision
            && completed.output_sha256 == validation.output_sha256;
        validation.reviewer_pass &= validation.accepted;
    }
    runtime.completion_gate(&validations)
}
