use codex_tools::JsonSchema;
use codex_tools::ResponsesApiTool;
use codex_tools::ToolSpec;
use serde_json::json;
use std::collections::BTreeMap;

pub(crate) fn create_plan_workflow_reviewers_tool() -> ToolSpec {
    let properties = BTreeMap::from([(
        "change_summary".to_string(),
        JsonSchema::string(Some(
            format!(
                "Compact factual summary of the actual files, symbols, risks, and tests changed. Hard limit: {} UTF-8 bytes.",
                crate::extended_workflow::MAX_WORKFLOW_CHANGE_SUMMARY_BYTES
            ),
        ))
        .with_max_length(crate::extended_workflow::MAX_WORKFLOW_CHANGE_SUMMARY_BYTES),
    )]);
    ToolSpec::Function(ResponsesApiTool {
        name: "plan_workflow_reviewers".to_string(),
        description: "Select distinct, change-relevant read-only reviewer profiles within the configured reviewer bounds and advisory budgets. Call after implementing the selected ticket."
            .to_string(),
        strict: false,
        defer_loading: None,
        parameters: JsonSchema::object(
            properties,
            Some(vec!["change_summary".to_string()]),
            Some(false.into()),
        ),
        output_schema: None,
    })
}

pub(crate) fn create_validate_workflow_result_tool() -> ToolSpec {
    let result_schema = JsonSchema::one_of(
        vec![
            expert_result_schema(),
            reviewer_result_schema(),
            ticket_schema(),
            concept_schema(),
        ],
        Some(
            "One complete ExpertResult, ReviewerResult, WorkflowTicket, or WorkflowConcept object. The selected kind must match the object."
                .to_string(),
        ),
    );
    let properties = BTreeMap::from([
        (
            "kind".to_string(),
            JsonSchema::string_enum(
                vec![
                    json!("expert"),
                    json!("reviewer"),
                    json!("ticket"),
                    json!("concept"),
                ],
                Some("Structured result contract to validate.".to_string()),
            ),
        ),
        (
            "result_id".to_string(),
            JsonSchema::string(Some(
                "Stable agent task/profile id (or ticket id) used to enforce at most one correction attempt."
                    .to_string(),
            ))
            .with_max_length(crate::extended_workflow::MAX_WORKFLOW_RESULT_ID_BYTES),
        ),
        (
            "agent_task".to_string(),
            JsonSchema::string(Some(
                "Canonical task name returned by spawn_agent. Required for expert and reviewer results; omit for tickets."
                    .to_string(),
            ))
            .with_max_length(crate::extended_workflow::MAX_WORKFLOW_AGENT_REFERENCE_BYTES),
        ),
        (
            "expected_subject".to_string(),
            JsonSchema::string(Some(
                "Deprecated compatibility field. The server derives the authoritative subject binding; callers may omit this value."
                    .to_string(),
            ))
            .with_max_length(crate::extended_workflow::MAX_WORKFLOW_EXPECTED_SUBJECT_BYTES),
        ),
        (
            "selected".to_string(),
            JsonSchema::boolean(Some(
                "For ticket results only: select this accepted ticket as the one the main agent will implement. Omit or use false for other results. A completed selection is immutable for the turn."
                    .to_string(),
            )),
        ),
        (
            "result".to_string(),
            result_schema,
        ),
    ]);
    ToolSpec::Function(ResponsesApiTool {
        name: "validate_workflow_result".to_string(),
        description: "Fail-closed validation for expert, reviewer, and ticket results. A failed result gets at most one targeted correction attempt; rejected results must not be used."
            .to_string(),
        strict: false,
        defer_loading: None,
        parameters: JsonSchema::object(
            properties,
            Some(vec![
                "kind".to_string(),
                "result_id".to_string(),
                "result".to_string(),
            ]),
            Some(false.into()),
        ),
        output_schema: Some(json!({
            "type": "object",
            "properties": {
                "accepted": { "type": "boolean" },
                "already_accepted": { "type": "boolean" },
                "retry_allowed": { "type": "boolean" },
                "review_cycle_allowed": { "type": "boolean" },
                "attempts": { "type": "integer" },
                "errors": { "type": "array", "items": { "type": "string" } },
                "has_unresolved_findings": { "type": "boolean" },
                "has_high_or_critical": { "type": "boolean" },
                "formal_status": { "type": "string", "enum": ["valid", "invalid"] },
                "reviewer_verdict": { "type": ["string", "null"], "enum": ["pass", "needs_changes", "blocked", null] },
                "gate_status": { "type": "string", "enum": ["eligible", "needs_resolution", "excluded"] },
                "error_code": { "type": ["string", "null"] },
                "finding_count": { "type": "integer" },
                "max_severity": { "type": ["string", "null"], "enum": ["Low", "Medium", "High", "Critical", null] },
                "change_digest_sha256": { "type": ["string", "null"] },
                "passing_reviewers": { "type": ["integer", "null"] },
                "required_reviewers": { "type": ["integer", "null"] },
                "result_bytes": { "type": "integer" },
                "max_result_bytes": { "type": "integer" },
                "validation_key": { "type": "string" },
                "selected": { "type": "boolean" }
            },
            "required": [
                "accepted",
                "retry_allowed",
                "review_cycle_allowed",
                "attempts",
                "errors",
                "has_unresolved_findings",
                "has_high_or_critical",
                "formal_status",
                "reviewer_verdict",
                "gate_status",
                "error_code",
                "finding_count",
                "max_severity",
                "change_digest_sha256",
                "passing_reviewers",
                "required_reviewers",
                "result_bytes",
                "max_result_bytes",
                "validation_key",
                "selected"
            ],
            "additionalProperties": false
        })),
    })
}

fn required_object(properties: BTreeMap<String, JsonSchema>) -> JsonSchema {
    let required = properties.keys().cloned().collect();
    JsonSchema::object(properties, Some(required), Some(false.into()))
}

fn string_array(description: &str) -> JsonSchema {
    JsonSchema::array(JsonSchema::string(None), Some(description.to_string()))
}

fn evidence_schema() -> JsonSchema {
    required_object(BTreeMap::from([
        ("file".to_string(), JsonSchema::string(None)),
        ("symbol".to_string(), JsonSchema::string(None)),
        ("explanation".to_string(), JsonSchema::string(None)),
    ]))
}

fn evidence_array() -> JsonSchema {
    JsonSchema::array(
        evidence_schema(),
        Some("Concrete file and symbol evidence.".to_string()),
    )
}

fn expert_result_schema() -> JsonSchema {
    required_object(BTreeMap::from([
        ("subject".to_string(), JsonSchema::string(None)),
        ("summary".to_string(), JsonSchema::string(None)),
        (
            "findings".to_string(),
            string_array("Substantive findings."),
        ),
        (
            "recommendations".to_string(),
            string_array("Recommendations or decisions."),
        ),
        ("evidence".to_string(), evidence_array()),
        (
            "uncertainties".to_string(),
            string_array("Uncertainties or open questions; use an empty array when none."),
        ),
    ]))
}

fn ticket_schema() -> JsonSchema {
    required_object(BTreeMap::from([
        ("title".to_string(), JsonSchema::string(None)),
        ("goal".to_string(), JsonSchema::string(None)),
        ("context".to_string(), JsonSchema::string(None)),
        (
            "requirements".to_string(),
            string_array("Concrete requirements."),
        ),
        (
            "acceptance_criteria".to_string(),
            string_array("Testable acceptance criteria."),
        ),
        (
            "dependencies".to_string(),
            string_array("Dependencies; state explicitly when none are known."),
        ),
        (
            "test_and_validation".to_string(),
            string_array("Required tests and validation."),
        ),
        (
            "risks_or_open_questions".to_string(),
            string_array("Known risks or open questions; state explicitly when none are known."),
        ),
    ]))
}

fn concept_schema() -> JsonSchema {
    required_object(BTreeMap::from([
        ("title".to_string(), JsonSchema::string(None)),
        ("objective".to_string(), JsonSchema::string(None)),
        ("context".to_string(), JsonSchema::string(None)),
        ("decisions".to_string(), string_array("Decisions.")),
        (
            "alternatives".to_string(),
            string_array("Considered alternatives."),
        ),
        (
            "risks_or_open_questions".to_string(),
            string_array("Known risks or open questions."),
        ),
        (
            "tickets".to_string(),
            JsonSchema::array(
                ticket_schema(),
                Some("Optional ticket series; use an empty array when omitted.".to_string()),
            ),
        ),
    ]))
}

fn reviewer_finding_schema() -> JsonSchema {
    let properties = BTreeMap::from([
        (
            "severity".to_string(),
            JsonSchema::string_enum(
                vec![
                    json!("Low"),
                    json!("Medium"),
                    json!("High"),
                    json!("Critical"),
                ],
                None,
            ),
        ),
        (
            "impact".to_string(),
            JsonSchema::string_enum(
                vec![json!("local"), json!("material"), json!("system_wide")],
                None,
            ),
        ),
        (
            "likelihood".to_string(),
            JsonSchema::string_enum(
                vec![json!("unlikely"), json!("plausible"), json!("expected")],
                None,
            ),
        ),
        (
            "recoverability".to_string(),
            JsonSchema::string_enum(
                vec![json!("easy"), json!("costly"), json!("irreversible")],
                None,
            ),
        ),
        ("rationale".to_string(), JsonSchema::string(None)),
        ("file".to_string(), JsonSchema::string(None)),
        ("symbol".to_string(), JsonSchema::string(None)),
        ("recommended_fix".to_string(), JsonSchema::string(None)),
        (
            "missing_tests".to_string(),
            string_array("Missing tests; use an empty array when none are missing."),
        ),
    ]);
    JsonSchema::object(
        properties,
        Some(vec![
            "impact".to_string(),
            "likelihood".to_string(),
            "recoverability".to_string(),
            "rationale".to_string(),
            "file".to_string(),
            "symbol".to_string(),
            "recommended_fix".to_string(),
            "missing_tests".to_string(),
        ]),
        Some(false.into()),
    )
}

fn reviewer_result_schema() -> JsonSchema {
    required_object(BTreeMap::from([
        ("subject".to_string(), JsonSchema::string(None)),
        (
            "verdict".to_string(),
            JsonSchema::string_enum(
                vec![json!("pass"), json!("needs_changes"), json!("blocked")],
                None,
            ),
        ),
        ("summary".to_string(), JsonSchema::string(None)),
        (
            "findings".to_string(),
            JsonSchema::array(
                reviewer_finding_schema(),
                Some("Concrete reviewer findings; empty only for a pass verdict.".to_string()),
            ),
        ),
        ("evidence".to_string(), evidence_array()),
        (
            "missing_tests".to_string(),
            string_array("Round-level missing tests; use an empty array when none are missing."),
        ),
    ]))
}

#[cfg(test)]
#[path = "extended_workflow_spec_tests.rs"]
mod tests;
