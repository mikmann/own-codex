use anyhow::Result;
use codex_protocol::protocol::CodexErrorInfo;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::Op;
use codex_protocol::user_input::UserInput;
use core_test_support::responses::ev_assistant_message;
use core_test_support::responses::ev_completed;
use core_test_support::responses::ev_completed_with_tokens;
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
use core_test_support::wait_for_event;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;
use std::time::Duration;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn workflow_limit_counts_literal_provider_tokens_and_stops_without_retry() -> Result<()> {
    skip_if_no_network!(Ok(()));

    let server = start_mock_server().await;
    let response = mount_sse_once(
        &server,
        sse(vec![
            ev_response_created("workflow-limit"),
            ev_assistant_message("answer", "Four."),
            ev_completed_with_tokens("workflow-limit", /*total_tokens*/ 30),
        ]),
    )
    .await;
    let mut builder = test_codex().with_config(|config| {
        config.extended_workflow.enabled = true;
        config.extended_workflow.workflow_budget_tokens = Some(30);
        config.extended_workflow.experts.enabled = false;
        config.extended_workflow.reviewers.enabled = false;
    });
    let test = builder.build_with_auto_env(&server).await?;

    test.codex
        .submit(Op::UserInput {
            items: vec![UserInput::Text {
                text: "What is two plus two?".to_string(),
                text_elements: Vec::new(),
            }],
            final_output_json_schema: None,
            responsesapi_client_metadata: None,
            additional_context: Default::default(),
            thread_settings: Default::default(),
        })
        .await?;

    let EventMsg::TokenCount(token_count) = wait_for_event(
        &test.codex,
        |event| matches!(event, EventMsg::TokenCount(event) if event.workflow_budget.is_some()),
    )
    .await
    else {
        unreachable!();
    };
    assert_eq!(
        token_count.workflow_budget,
        Some(codex_protocol::protocol::WorkflowTokenBudgetStatus {
            session_tokens_used: 30,
            workflow_tokens_used: 30,
            base_limit_tokens: 30,
            effective_limit_tokens: 30,
            active: true,
        })
    );
    let EventMsg::Error(error) =
        wait_for_event(&test.codex, |event| matches!(event, EventMsg::Error(_))).await
    else {
        unreachable!();
    };
    assert_eq!(
        error.codex_error_info,
        Some(CodexErrorInfo::SessionBudgetExceeded)
    );
    assert!(error.message.contains("workflow token limit reached"));
    wait_for_event(&test.codex, |event| {
        matches!(event, EventMsg::TurnComplete(_))
    })
    .await;
    assert_eq!(
        response.requests().len(),
        1,
        "workflow limit must not retry"
    );

    Ok(())
}

fn request_body_contains(request: &wiremock::Request, text: &str) -> bool {
    request_body_json(request).is_some_and(|body| body.to_string().contains(text))
}

fn request_header<'a>(request: &'a wiremock::Request, name: &str) -> Option<&'a str> {
    request
        .headers
        .get(name)
        .and_then(|value| value.to_str().ok())
}

fn request_body_json(request: &wiremock::Request) -> Option<Value> {
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
    body.and_then(|body| serde_json::from_slice(&body).ok())
}

fn selected_workflow_task_name(value: &Value, process: &str) -> Option<String> {
    match value {
        Value::String(text) => {
            let (_, remainder) = text.split_once("<extended_workflow_turn>")?;
            let (plan, _) = remainder.split_once("</extended_workflow_turn>")?;
            serde_json::from_str::<Value>(plan.trim())
                .ok()?
                .pointer(&format!("/{process}/selected/0/task_name"))?
                .as_str()
                .map(str::to_string)
        }
        Value::Array(values) => values
            .iter()
            .find_map(|value| selected_workflow_task_name(value, process)),
        Value::Object(values) => values
            .values()
            .find_map(|value| selected_workflow_task_name(value, process)),
        Value::Null | Value::Bool(_) | Value::Number(_) => None,
    }
}

fn selected_reviewer_task_name(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => serde_json::from_str::<Value>(text)
            .ok()
            .and_then(|parsed| selected_reviewer_task_name(&parsed)),
        Value::Array(values) => values.iter().find_map(selected_reviewer_task_name),
        Value::Object(values) => {
            if let Some(task_name) = values.get("task_name").and_then(Value::as_str)
                && task_name.contains("_reviewer_")
            {
                return Some(task_name.to_string());
            }
            values.values().find_map(selected_reviewer_task_name)
        }
        Value::Null | Value::Bool(_) | Value::Number(_) => None,
    }
}

#[derive(Debug)]
struct WorkflowExpertSpawnResponder {
    child_task: &'static str,
    call_id: &'static str,
}

impl wiremock::Respond for WorkflowExpertSpawnResponder {
    fn respond(&self, request: &wiremock::Request) -> wiremock::ResponseTemplate {
        let body = request_body_json(request).expect("responses request should contain JSON");
        let task_name = selected_workflow_task_name(&body, "experts")
            .expect("workflow turn context should select an expert task");
        let arguments = json!({
            "message": self.child_task,
            "task_name": task_name,
            "fork_turns": "none"
        })
        .to_string();
        core_test_support::responses::sse_response(sse(vec![
            ev_response_created("resp-root-spawn"),
            ev_function_call_with_namespace(
                self.call_id,
                "collaboration",
                "spawn_agent",
                &arguments,
            ),
            ev_completed("resp-root-spawn"),
        ]))
    }
}

#[derive(Debug)]
struct WorkflowReviewerSpawnResponder {
    call_id: &'static str,
}

impl wiremock::Respond for WorkflowReviewerSpawnResponder {
    fn respond(&self, request: &wiremock::Request) -> wiremock::ResponseTemplate {
        let body = request_body_json(request).expect("responses request should contain JSON");
        let task_name = selected_reviewer_task_name(&body)
            .expect("reviewer planning output should contain a selected reviewer task");
        let arguments = json!({
            "message": "route the selected reviewer",
            "task_name": task_name,
            "fork_turns": "none"
        })
        .to_string();
        core_test_support::responses::sse_response(sse(vec![
            ev_response_created("resp-root-reviewer-spawn"),
            ev_function_call_with_namespace(
                self.call_id,
                "collaboration",
                "spawn_agent",
                &arguments,
            ),
            ev_completed("resp-root-reviewer-spawn"),
        ]))
    }
}

#[derive(Debug)]
enum WorkflowReviewerFollowupResponder {
    Validate {
        call_id: &'static str,
        result: &'static str,
    },
    Close {
        call_id: &'static str,
    },
}

impl wiremock::Respond for WorkflowReviewerFollowupResponder {
    fn respond(&self, request: &wiremock::Request) -> wiremock::ResponseTemplate {
        let body = request_body_json(request).expect("responses request should contain JSON");
        let task_name = selected_reviewer_task_name(&body)
            .expect("reviewer follow-up should retain the selected task");
        let (response_id, call_id, tool_name, arguments) = match self {
            Self::Validate { call_id, result } => (
                "resp-root-reviewer-validate",
                *call_id,
                "validate_workflow_result",
                json!({
                    "kind": "reviewer",
                    "result_id": "ephemeral-reviewer-result",
                    "expected_subject": "ignored server-side",
                    "agent_task": task_name,
                    "result": serde_json::from_str::<Value>(result)
                        .expect("reviewer result fixture should be valid JSON")
                })
                .to_string(),
            ),
            Self::Close { call_id } => (
                "resp-root-reviewer-close",
                *call_id,
                "close_agent",
                json!({"target": task_name}).to_string(),
            ),
        };
        core_test_support::responses::sse_response(sse(vec![
            ev_response_created(response_id),
            ev_function_call(call_id, tool_name, &arguments),
            ev_completed(response_id),
        ]))
    }
}

#[derive(Debug)]
struct BodyContains(&'static str);

impl wiremock::Match for BodyContains {
    fn matches(&self, request: &wiremock::Request) -> bool {
        request_body_contains(request, self.0)
    }
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
    assert_eq!(
        spawn_properties
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        vec![
            "fork_turns",
            "message",
            "model",
            "reasoning_effort",
            "task_name",
        ]
    );
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
async fn accepted_ticket_selection_is_idempotent_across_tool_retry() -> Result<()> {
    skip_if_no_network!(Ok(()));
    let server = start_mock_server().await;
    let args = json!({
        "kind": "ticket",
        "result_id": "selected-ticket",
        "expected_subject": "change the parser ticket",
        "selected": true,
        "result": {
            "title": "Change the parser",
            "goal": "Change the parser safely",
            "context": "The root request requires a parser change",
            "requirements": ["Keep the accepted grammar compatible"],
            "acceptance_criteria": ["The parser change passes its regression tests"],
            "dependencies": ["Existing parser fixtures"],
            "test_and_validation": ["Run parser integration tests"],
            "risks_or_open_questions": ["No known open questions"]
        }
    })
    .to_string();
    let response_mock = mount_sse_sequence(
        &server,
        vec![
            sse(vec![
                ev_response_created("resp-select"),
                ev_function_call("call-select", "validate_workflow_result", &args),
                ev_completed("resp-select"),
            ]),
            sse(vec![
                ev_response_created("resp-select-retry"),
                ev_function_call("call-select-retry", "validate_workflow_result", &args),
                ev_completed("resp-select-retry"),
            ]),
            sse(vec![
                ev_response_created("resp-selected"),
                ev_assistant_message("msg-selected", "ticket selected"),
                ev_completed("resp-selected"),
            ]),
        ],
    )
    .await;
    let mut builder = test_codex().with_config(|config| {
        config.extended_workflow.enabled = true;
    });
    let test = builder.build(&server).await?;
    test.submit_turn("Change the parser.").await?;

    let requests = response_mock.requests();
    assert_eq!(requests.len(), 3);
    let first: Value = serde_json::from_str(
        &requests[1]
            .function_call_output_text("call-select")
            .expect("first selection output"),
    )?;
    let repeated: Value = serde_json::from_str(
        &requests[2]
            .function_call_output_text("call-select-retry")
            .expect("repeated selection output"),
    )?;
    assert_eq!(
        (first["accepted"].as_bool(), first["selected"].as_bool()),
        (Some(true), Some(true))
    );
    assert_eq!(
        (
            repeated["accepted"].as_bool(),
            repeated["selected"].as_bool(),
            repeated["already_accepted"].as_bool(),
        ),
        (Some(true), Some(true), Some(true))
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn workflow_expert_shell_reads_but_write_fails_closed_without_creating_a_file() -> Result<()>
{
    skip_if_no_network!(Ok(()));
    const ROOT_PROMPT: &str =
        "Explain the security and architecture implications of an unknown permission.";
    const CHILD_TASK: &str = "Inspect read-only enforcement for the assigned workflow.";
    const SPAWN_CALL_ID: &str = "call-spawn-read-only";
    const READ_CALL_ID: &str = "call-read-read-only";
    const WRITE_CALL_ID: &str = "call-write-read-only";
    const READ_SENTINEL: &str = "workflow read access confirmed";

    let server = start_mock_server().await;
    wiremock::Mock::given(wiremock::matchers::method("POST"))
        .and(BodyContains(ROOT_PROMPT))
        .respond_with(WorkflowExpertSpawnResponder {
            child_task: CHILD_TASK,
            call_id: SPAWN_CALL_ID,
        })
        .up_to_n_times(1)
        .mount(&server)
        .await;
    let read_command = if cfg!(target_os = "windows") {
        "Get-Content -LiteralPath workflow-read-only-source.txt"
    } else {
        "cat workflow-read-only-source.txt"
    };
    let read_args = json!({
        "command": read_command,
        "timeout_ms": 5_000
    })
    .to_string();
    mount_sse_once_match(
        &server,
        |request: &wiremock::Request| {
            request_header(request, "x-openai-subagent") == Some("collab_spawn")
                && request_body_json(request)
                    .and_then(|body| body.get("input").and_then(Value::as_array).cloned())
                    .is_some_and(|items| {
                        items.iter().any(|item| {
                            item.get("type").and_then(Value::as_str) == Some("agent_message")
                        })
                    })
        },
        sse(vec![
            ev_response_created("resp-child-read"),
            ev_function_call(READ_CALL_ID, "shell_command", &read_args),
            ev_completed("resp-child-read"),
        ]),
    )
    .await;
    let write_args = json!({
        "command": "echo forbidden > workflow-read-only-marker.txt",
        "timeout_ms": 5_000
    })
    .to_string();
    let child_write = mount_sse_once_match(
        &server,
        |request: &wiremock::Request| {
            request_header(request, "x-openai-subagent") == Some("collab_spawn")
                && request_body_contains(request, READ_CALL_ID)
        },
        sse(vec![
            ev_response_created("resp-child-write-attempt"),
            ev_function_call(WRITE_CALL_ID, "shell_command", &write_args),
            ev_completed("resp-child-write-attempt"),
        ]),
    )
    .await;
    let child_finished = mount_sse_once_match(
        &server,
        |request: &wiremock::Request| {
            request_header(request, "x-openai-subagent") == Some("collab_spawn")
                && request_body_contains(request, WRITE_CALL_ID)
        },
        sse(vec![
            ev_response_created("resp-child-finished"),
            ev_assistant_message(
                "msg-child-finished",
                r#"{"subject":"read-only enforcement","summary":"write was rejected","findings":["the child remained read-only"],"recommendations":["retain the permission ceiling"],"evidence":[{"file":"codex-rs/core/src/tools/orchestrator.rs","symbol":"ToolOrchestrator::run","explanation":"the runtime rejects unsafe execution"}],"uncertainties":["none after the observed rejection"]}"#,
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
        config.set_windows_sandbox_enabled(/*value*/ true);
    });
    let test = builder.build(&server).await?;
    std::fs::write(
        test.cwd_path().join("workflow-read-only-source.txt"),
        READ_SENTINEL,
    )?;
    let marker = test.cwd_path().join("workflow-read-only-marker.txt");
    test.submit_turn(ROOT_PROMPT).await?;

    let read_output = tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            if let Some(output) = child_write.function_call_output_text(READ_CALL_ID) {
                break output;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await?;
    assert!(read_output.contains(READ_SENTINEL), "{read_output}");
    let write_output = tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            if let Some(output) = child_finished.function_call_output_text(WRITE_CALL_ID) {
                break output;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await?;
    assert!(!write_output.trim().is_empty());
    assert!(!marker.exists(), "read-only expert created {marker:?}");
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn maximum_bounded_reviewer_contract_persists_and_validates_child() -> Result<()> {
    maximum_bounded_reviewer_contract_validates_child(/*ephemeral*/ false).await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn maximum_bounded_reviewer_contract_validates_ephemeral_child_after_close() -> Result<()> {
    maximum_bounded_reviewer_contract_validates_child(/*ephemeral*/ true).await
}

async fn maximum_bounded_reviewer_contract_validates_child(ephemeral: bool) -> Result<()> {
    skip_if_no_network!(Ok(()));
    const PLAN_CALL_ID: &str = "call-plan-max-reviewer";
    const SPAWN_CALL_ID: &str = "call-spawn-max-reviewer";
    const WAIT_CALL_ID: &str = "call-wait-max-reviewer";
    const VALIDATE_CALL_ID: &str = "call-validate-max-reviewer";
    const CLOSE_CALL_ID: &str = "call-close-max-reviewer";
    const REVIEW_RESULT: &str = r#"{"subject":"HTTP reviewer task","verdict":"pass","summary":"the complete task reached the ephemeral reviewer validation gate","findings":[],"evidence":[{"file":"codex-rs/core/src/extended_workflow.rs","symbol":"spawn_directive_for_task","explanation":"the task includes its bounded result contract"}],"missing_tests":[]}"#;
    let root_prefix = "What is HTTP? ";
    let root_prompt = format!("{root_prefix}{}", "r".repeat(3_000 - root_prefix.len()));
    let summary_prefix = "CLI security and tests. ";
    let change_summary = format!(
        "{summary_prefix}{}",
        "s".repeat(1_500 - summary_prefix.len())
    );

    let server = start_mock_server().await;
    let plan_args = json!({"change_summary": &change_summary}).to_string();
    let root_prompt_match = root_prompt.clone();
    mount_sse_once_match(
        &server,
        move |request: &wiremock::Request| request_body_contains(request, &root_prompt_match),
        sse(vec![
            ev_response_created("resp-plan-max-reviewer"),
            ev_function_call(PLAN_CALL_ID, "plan_workflow_reviewers", &plan_args),
            ev_completed("resp-plan-max-reviewer"),
        ]),
    )
    .await;
    wiremock::Mock::given(wiremock::matchers::method("POST"))
        .and(BodyContains(PLAN_CALL_ID))
        .respond_with(WorkflowReviewerSpawnResponder {
            call_id: SPAWN_CALL_ID,
        })
        .up_to_n_times(1)
        .mount(&server)
        .await;
    let child_request = mount_sse_once_match(
        &server,
        |request: &wiremock::Request| {
            request_header(request, "x-openai-subagent") == Some("collab_spawn")
                && request_body_contains(request, "<extended_workflow_result_contract>")
        },
        sse(vec![
            ev_response_created("resp-max-reviewer-child"),
            ev_assistant_message("msg-max-reviewer-child", REVIEW_RESULT),
            ev_completed("resp-max-reviewer-child"),
        ]),
    )
    .await;
    mount_sse_once_match(
        &server,
        |request: &wiremock::Request| request_body_contains(request, SPAWN_CALL_ID),
        sse(vec![
            ev_response_created("resp-max-reviewer-wait"),
            ev_function_call_with_namespace(
                WAIT_CALL_ID,
                "collaboration",
                "wait_agent",
                &json!({"timeout_ms": 2_000}).to_string(),
            ),
            ev_completed("resp-max-reviewer-wait"),
        ]),
    )
    .await;
    wiremock::Mock::given(wiremock::matchers::method("POST"))
        .and(BodyContains(WAIT_CALL_ID))
        .respond_with(WorkflowReviewerFollowupResponder::Validate {
            call_id: VALIDATE_CALL_ID,
            result: REVIEW_RESULT,
        })
        .up_to_n_times(1)
        .mount(&server)
        .await;
    wiremock::Mock::given(wiremock::matchers::method("POST"))
        .and(BodyContains(VALIDATE_CALL_ID))
        .respond_with(WorkflowReviewerFollowupResponder::Close {
            call_id: CLOSE_CALL_ID,
        })
        .up_to_n_times(1)
        .mount(&server)
        .await;
    let final_request = mount_sse_once_match(
        &server,
        |request: &wiremock::Request| request_body_contains(request, CLOSE_CALL_ID),
        sse(vec![
            ev_response_created("resp-max-reviewer-finished"),
            ev_assistant_message("msg-max-reviewer-finished", "reviewer validated and closed"),
            ev_completed("resp-max-reviewer-finished"),
        ]),
    )
    .await;

    let mut builder = test_codex().with_config(move |config| {
        config.ephemeral = ephemeral;
        config.extended_workflow.enabled = true;
        config.extended_workflow.experts.enabled = false;
        config.extended_workflow.reviewers.min = 1;
        config.extended_workflow.reviewers.max = 1;
        config.multi_agent_v2.min_wait_timeout_ms = 10;
        config.multi_agent_v2.default_wait_timeout_ms = 2_000;
    });
    let test = builder.build(&server).await?;
    let completion = test.submit_turn_and_return_complete(&root_prompt).await?;
    assert_eq!(
        completion.last_agent_message.as_deref(),
        Some("reviewer validated and closed")
    );

    tokio::time::timeout(Duration::from_secs(10), async {
        while child_request.requests().is_empty() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await?;
    let request = child_request
        .requests()
        .into_iter()
        .find(|request| {
            request.header("x-openai-subagent").as_deref() == Some("collab_spawn")
                && request.inputs_of_type("agent_message").len() == 1
        })
        .expect("reviewer child should issue a request");
    let agent_messages = request.inputs_of_type("agent_message");
    assert_eq!(agent_messages.len(), 1);
    let delivered_content = agent_messages[0]["content"]
        .as_array()
        .expect("workflow task should use agent_message content");
    assert_eq!(delivered_content.len(), 1);
    assert_eq!(delivered_content[0]["type"], "input_text");
    let delivered_task = delivered_content[0]["text"]
        .as_str()
        .expect("workflow task should be structured plaintext");
    assert!(delivered_task.contains("Message Type: NEW_TASK"));
    assert!(delivered_task.contains(&root_prompt));
    assert!(delivered_task.contains(&change_summary));
    assert!(delivered_task.contains("<extended_workflow_result_contract>"));
    let wait_output = server
        .received_requests()
        .await
        .expect("responses requests should be recorded")
        .into_iter()
        .filter_map(|request| request_body_json(&request))
        .filter_map(|body| body.get("input").and_then(Value::as_array).cloned())
        .flatten()
        .find(|item| {
            item.get("type").and_then(Value::as_str) == Some("function_call_output")
                && item.get("call_id").and_then(Value::as_str) == Some(WAIT_CALL_ID)
        })
        .and_then(|item| {
            item.get("output")
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
        .expect("wait follow-up should contain the wait_agent output");
    let wait_payload = serde_json::from_str::<Value>(&wait_output)
        .unwrap_or_else(|error| panic!("invalid wait_agent output {wait_output:?}: {error}"));
    assert_eq!(
        wait_payload,
        json!({"message": "Wait completed.", "timed_out": false})
    );
    let final_request = final_request.single_request();
    let validation_output = final_request
        .function_call_output_text(VALIDATE_CALL_ID)
        .expect("persisted reviewer assignment should pass validation");
    let validation_payload: Value =
        serde_json::from_str(&validation_output).unwrap_or_else(|error| {
            panic!("invalid validate_workflow_result output {validation_output:?}: {error}")
        });
    assert_eq!(
        (
            validation_payload["accepted"].as_bool(),
            validation_payload["retry_allowed"].as_bool(),
        ),
        (Some(true), Some(false))
    );
    let close_output = final_request
        .function_call_output_text(CLOSE_CALL_ID)
        .expect("accepted ephemeral reviewer should be closable");
    let close_payload: Value = serde_json::from_str(&close_output)
        .unwrap_or_else(|error| panic!("invalid close_agent output {close_output:?}: {error}"));
    assert!(close_payload.get("previous_status").is_some());
    Ok(())
}
