use anyhow::Result;
use core_test_support::responses::ev_assistant_message;
use core_test_support::responses::ev_completed;
use core_test_support::responses::ev_function_call;
use core_test_support::responses::ev_function_call_with_namespace;
use core_test_support::responses::ev_response_created;
use core_test_support::responses::mount_sse_once;
use core_test_support::responses::mount_sse_once_match;
use core_test_support::responses::mount_sse_sequence;
use core_test_support::responses::sse;
use core_test_support::responses::start_mock_server;
use core_test_support::skip_if_no_network;
use core_test_support::test_codex::test_codex;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;
use std::time::Duration;

fn request_body_contains(request: &wiremock::Request, text: &str) -> bool {
    let compressed = request
        .headers
        .get("content-encoding")
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| {
            value
                .split(',')
                .any(|entry| entry.trim().eq_ignore_ascii_case("zstd"))
        });
    let body = if compressed {
        zstd::stream::decode_all(std::io::Cursor::new(&request.body)).ok()
    } else {
        Some(request.body.clone())
    };
    body.and_then(|body| String::from_utf8(body).ok())
        .is_some_and(|body| body.contains(text))
}

fn tool_names(body: &Value) -> Vec<&str> {
    body.get("tools")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|tool| tool.get("name").and_then(Value::as_str))
        .collect()
}

fn find_tool<'a>(body: &'a Value, name: &str) -> Option<&'a Value> {
    body.get("tools")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .find_map(|tool| {
            if tool.get("name").and_then(Value::as_str) == Some(name) {
                return Some(tool);
            }
            tool.get("tools")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .find(|child| child.get("name").and_then(Value::as_str) == Some(name))
        })
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn workflow_plan_and_validation_tools_are_model_visible() -> Result<()> {
    skip_if_no_network!(Ok(()));
    let server = start_mock_server().await;
    let request = mount_sse_once(
        &server,
        sse(vec![
            ev_response_created("resp-1"),
            ev_assistant_message("msg-1", "done"),
            ev_completed("resp-1"),
        ]),
    )
    .await;
    let mut builder = test_codex().with_config(|config| {
        config.extended_workflow.enabled = true;
        config.multi_agent_v2.expose_spawn_agent_model_overrides = false;
    });
    let test = builder.build(&server).await?;
    test.submit_turn("Was bedeutet HTTP?").await?;

    let request = request.single_request();
    let developer_text = request.message_input_texts("developer").join("\n");
    assert!(developer_text.contains("<extended_workflow_policy>"));
    assert!(developer_text.contains("<extended_workflow_turn>"));
    assert!(developer_text.contains("\"class\":\"simple_answer\""));
    assert!(developer_text.contains("\"selected\":[]"));
    assert!(developer_text.contains("\"audit_files_written\":false"));

    let body = request.body_json();
    let names = tool_names(&body);
    assert!(names.contains(&"plan_workflow_reviewers"));
    assert!(names.contains(&"validate_workflow_result"));
    let spawn_properties = find_tool(&body, "spawn_agent")
        .and_then(|tool| tool.pointer("/parameters/properties"))
        .and_then(Value::as_object)
        .expect("spawn_agent should be directly or namespace-visible with properties");
    for field in [
        "model",
        "reasoning_effort",
        "read_only",
        "workflow_name",
        "workflow_type",
        "workflow_role",
    ] {
        assert!(spawn_properties.contains_key(field), "missing {field}");
    }
    assert!(!test.cwd_path().join(".codex").exists());
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn disabled_workflow_adds_no_prompt_fragments_or_workflow_tools() -> Result<()> {
    skip_if_no_network!(Ok(()));
    let server = start_mock_server().await;
    let request = mount_sse_once(
        &server,
        sse(vec![
            ev_response_created("resp-1"),
            ev_assistant_message("msg-1", "done"),
            ev_completed("resp-1"),
        ]),
    )
    .await;
    let mut builder = test_codex().with_config(|config| {
        config.extended_workflow.enabled = false;
    });
    let test = builder.build(&server).await?;
    test.submit_turn("Implementiere eine Änderung.").await?;

    let request = request.single_request();
    let developer_text = request.message_input_texts("developer").join("\n");
    assert!(!developer_text.contains("<extended_workflow_policy>"));
    assert!(!developer_text.contains("<extended_workflow_turn>"));
    let body = request.body_json();
    let names = tool_names(&body);
    assert!(!names.contains(&"plan_workflow_reviewers"));
    assert!(!names.contains(&"validate_workflow_result"));
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ticket_validation_allows_exactly_one_changed_correction() -> Result<()> {
    skip_if_no_network!(Ok(()));
    let server = start_mock_server().await;
    let invalid_result = json!({
        "title": "",
        "goal": "",
        "context": "",
        "requirements": [],
        "acceptance_criteria": [],
        "dependencies": [],
        "test_and_validation": [],
        "risks_or_open_questions": []
    });
    let valid_result = json!({
        "title": "Validate workflow tickets",
        "goal": "Prove the structured validation retry contract",
        "context": "The root workflow validates tickets before implementation",
        "requirements": ["Reject incomplete tickets", "Allow one changed correction"],
        "acceptance_criteria": ["The corrected ticket is accepted on attempt two"],
        "dependencies": ["No external dependencies"],
        "test_and_validation": ["Inspect both function-call outputs"],
        "risks_or_open_questions": ["No known open questions"]
    });
    let invalid_args = json!({
        "kind": "ticket",
        "result_id": "ticket-validation",
        "expected_subject": "workflow ticket validation",
        "result": invalid_result
    })
    .to_string();
    let valid_args = json!({
        "kind": "ticket",
        "result_id": "ticket-validation",
        "expected_subject": "workflow ticket validation",
        "result": valid_result
    })
    .to_string();
    let response_mock = mount_sse_sequence(
        &server,
        vec![
            sse(vec![
                ev_response_created("resp-invalid"),
                ev_function_call("call-invalid", "validate_workflow_result", &invalid_args),
                ev_completed("resp-invalid"),
            ]),
            sse(vec![
                ev_response_created("resp-valid"),
                ev_function_call("call-valid", "validate_workflow_result", &valid_args),
                ev_completed("resp-valid"),
            ]),
            sse(vec![
                ev_response_created("resp-finished"),
                ev_assistant_message("msg-finished", "ticket validated"),
                ev_completed("resp-finished"),
            ]),
        ],
    )
    .await;
    let mut builder = test_codex().with_config(|config| {
        config.extended_workflow.enabled = true;
    });
    let test = builder.build(&server).await?;
    test.submit_turn("Validate this ticket contract.").await?;

    let requests = response_mock.requests();
    assert_eq!(requests.len(), 3);
    let invalid_output = requests[1]
        .function_call_output_text("call-invalid")
        .expect("first correction request should contain the rejected result");
    let valid_output = requests[2]
        .function_call_output_text("call-valid")
        .expect("final request should contain the accepted correction");
    let invalid_payload: Value = serde_json::from_str(&invalid_output)?;
    let valid_payload: Value = serde_json::from_str(&valid_output)?;
    assert_eq!(
        (
            invalid_payload["accepted"].as_bool(),
            invalid_payload["retry_allowed"].as_bool(),
            invalid_payload["attempts"].as_u64(),
        ),
        (Some(false), Some(true), Some(1))
    );
    assert_eq!(
        (
            valid_payload["accepted"].as_bool(),
            valid_payload["retry_allowed"].as_bool(),
            valid_payload["attempts"].as_u64(),
        ),
        (Some(true), Some(false), Some(2))
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn workflow_expert_shell_write_fails_closed_without_creating_a_file() -> Result<()> {
    skip_if_no_network!(Ok(()));
    const ROOT_PROMPT: &str =
        "Explain the security and architecture implications of an unknown permission.";
    const CHILD_TASK: &str = "Inspect read-only enforcement for the assigned workflow.";
    const SPAWN_CALL_ID: &str = "call-spawn-read-only";
    const WRITE_CALL_ID: &str = "call-write-read-only";

    let server = start_mock_server().await;
    let spawn_args = json!({
        "message": CHILD_TASK,
        "task_name": "architecture-inspection",
        "fork_turns": "none",
        "read_only": true,
        "workflow_name": "architecture",
        "workflow_type": "expert",
        "workflow_role": "Architecture"
    })
    .to_string();
    mount_sse_once_match(
        &server,
        |request: &wiremock::Request| request_body_contains(request, ROOT_PROMPT),
        sse(vec![
            ev_response_created("resp-root-spawn"),
            ev_function_call_with_namespace(
                SPAWN_CALL_ID,
                "collaboration",
                "spawn_agent",
                &spawn_args,
            ),
            ev_completed("resp-root-spawn"),
        ]),
    )
    .await;
    let write_args = json!({
        "command": "echo forbidden > workflow-read-only-marker.txt",
        "timeout_ms": 1_000
    })
    .to_string();
    mount_sse_once_match(
        &server,
        |request: &wiremock::Request| request_body_contains(request, CHILD_TASK),
        sse(vec![
            ev_response_created("resp-child-write"),
            ev_function_call(WRITE_CALL_ID, "shell_command", &write_args),
            ev_completed("resp-child-write"),
        ]),
    )
    .await;
    let child_followup = mount_sse_once_match(
        &server,
        |request: &wiremock::Request| request_body_contains(request, WRITE_CALL_ID),
        sse(vec![
            ev_response_created("resp-child-finished"),
            ev_assistant_message(
                "msg-child-finished",
                r#"{"subject":"read-only enforcement","summary":"write was rejected","findings":["the child remained read-only"],"recommendations":["retain the permission ceiling"],"evidence":[{"file":"codex-rs/core/src/tools/orchestrator.rs","symbol":"ToolOrchestrator::run","explanation":"the runtime rejects unsafe execution"}],"uncertainties":[]}"#,
            ),
            ev_completed("resp-child-finished"),
        ]),
    )
    .await;
    mount_sse_once_match(
        &server,
        |request: &wiremock::Request| request_body_contains(request, SPAWN_CALL_ID),
        sse(vec![
            ev_response_created("resp-root-finished"),
            ev_assistant_message("msg-root-finished", "expert started"),
            ev_completed("resp-root-finished"),
        ]),
    )
    .await;

    let mut builder = test_codex().with_config(|config| {
        config.extended_workflow.enabled = true;
    });
    let test = builder.build(&server).await?;
    let marker = test.cwd_path().join("workflow-read-only-marker.txt");
    test.submit_turn(ROOT_PROMPT).await?;

    let tool_output = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if let Some(output) = child_followup.function_call_output_text(WRITE_CALL_ID) {
                break output;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await?;
    assert!(!tool_output.trim().is_empty());
    assert!(!marker.exists(), "read-only expert created {marker:?}");
    Ok(())
}
