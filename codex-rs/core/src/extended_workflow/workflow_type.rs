use super::RequestClass;
use super::RequestClassification;
use codex_protocol::config_types::WorkflowType;

pub(super) fn apply_to_classification(
    mut classification: RequestClassification,
    workflow_type: WorkflowType,
) -> RequestClassification {
    let forced_class = match workflow_type {
        WorkflowType::Auto | WorkflowType::Inactive => return classification,
        WorkflowType::Concept => RequestClass::Concept,
        WorkflowType::WorkOrder => RequestClass::WorkOrder,
    };
    classification.class = forced_class;
    classification.reasons.push(format!(
        "request class forced by workflow type `{workflow_type}`"
    ));
    classification
}
