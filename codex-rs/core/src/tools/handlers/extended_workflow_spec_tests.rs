use super::*;
use pretty_assertions::assert_eq;

#[test]
fn validator_exposes_all_four_structured_result_contracts() {
    let ToolSpec::Function(tool) = create_validate_workflow_result_tool() else {
        panic!("workflow validator must be a function tool");
    };
    let result_schema = tool
        .parameters
        .properties
        .as_ref()
        .and_then(|properties| properties.get("result"))
        .expect("result property");
    let alternatives = result_schema.one_of.as_ref().expect("structured oneOf");
    assert_eq!(alternatives.len(), 4);
    for alternative in alternatives {
        assert_eq!(alternative.additional_properties, Some(false.into()));
        assert!(
            alternative
                .required
                .as_ref()
                .is_some_and(|fields| !fields.is_empty())
        );
    }
}

#[test]
fn validator_derives_subject_and_severity_without_breaking_legacy_callers() {
    let ToolSpec::Function(tool) = create_validate_workflow_result_tool() else {
        panic!("workflow validator must be a function tool");
    };
    assert_eq!(
        tool.parameters.required,
        Some(vec![
            "kind".to_string(),
            "result_id".to_string(),
            "result".to_string(),
        ])
    );
    assert!(
        tool.parameters
            .properties
            .as_ref()
            .is_some_and(|properties| properties.contains_key("expected_subject"))
    );

    let reviewer = tool
        .parameters
        .properties
        .as_ref()
        .and_then(|properties| properties.get("result"))
        .and_then(|result| result.one_of.as_ref())
        .and_then(|alternatives| {
            alternatives.iter().find(|alternative| {
                alternative
                    .properties
                    .as_ref()
                    .is_some_and(|properties| properties.contains_key("verdict"))
            })
        })
        .expect("reviewer result schema");
    let finding = reviewer
        .properties
        .as_ref()
        .and_then(|properties| properties.get("findings"))
        .and_then(|findings| findings.items.as_deref())
        .expect("reviewer finding schema");
    assert!(
        finding
            .properties
            .as_ref()
            .is_some_and(|properties| properties.contains_key("severity"))
    );
    assert!(
        finding
            .required
            .as_ref()
            .is_some_and(|required| !required.iter().any(|field| field == "severity"))
    );
}
