use super::*;

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
