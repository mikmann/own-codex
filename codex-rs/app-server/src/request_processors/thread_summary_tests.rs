use super::*;

use anyhow::Result;
use codex_protocol::protocol::SessionSource;
use codex_protocol::protocol::SubAgentSource;
use codex_protocol::protocol::USER_MESSAGE_BEGIN;
use pretty_assertions::assert_eq;
use serde_json::json;
use std::path::PathBuf;

#[test]
fn display_metadata_cannot_replace_persisted_read_only_workflow_marker() {
    let parent_thread_id = ThreadId::new();
    let encoded_role = codex_protocol::protocol::encode_extended_thread_spawn_role(
        None,
        /*read_only*/ true,
        Some("Reviewer"),
        Some("Security"),
    );
    let source = SessionSource::SubAgent(SubAgentSource::ThreadSpawn {
        parent_thread_id,
        depth: 1,
        agent_path: None,
        agent_nickname: None,
        agent_role: encoded_role.clone(),
    });

    let enriched = with_thread_spawn_agent_metadata(
        source,
        Some("display-name".to_string()),
        Some("display role without authorization marker".to_string()),
    );

    let SessionSource::SubAgent(SubAgentSource::ThreadSpawn {
        agent_nickname,
        agent_role,
        ..
    }) = enriched
    else {
        panic!("expected thread-spawn source");
    };
    assert_eq!(agent_nickname.as_deref(), Some("display-name"));
    assert_eq!(agent_role, encoded_role);
}

#[test]
fn extract_conversation_summary_prefers_plain_user_messages() -> Result<()> {
    let conversation_id = ThreadId::from_string("3f941c35-29b3-493b-b0a4-e25800d9aeb0")?;
    let timestamp = Some("2025-09-05T16:53:11.850Z".to_string());
    let path = PathBuf::from("rollout.jsonl");

    let head = vec![
        json!({
            "session_id": conversation_id.to_string(),
            "id": conversation_id.to_string(),
            "timestamp": timestamp,
            "cwd": "/",
            "originator": "codex",
            "cli_version": "0.0.0",
            "model_provider": "test-provider"
        }),
        json!({
            "type": "message",
            "role": "user",
            "content": [{
                "type": "input_text",
                "text": "# AGENTS.md instructions for project\n\n<INSTRUCTIONS>\n<AGENTS.md contents>\n</INSTRUCTIONS>".to_string(),
            }],
        }),
        json!({
            "type": "message",
            "role": "user",
            "content": [{
                "type": "input_text",
                "text": format!("<prior context> {USER_MESSAGE_BEGIN}Count to 5"),
            }],
        }),
    ];

    let session_meta = serde_json::from_value::<SessionMeta>(head[0].clone())?;

    let summary = extract_conversation_summary(
        path.clone(),
        &head,
        &session_meta,
        /*git*/ None,
        "test-provider",
        timestamp.clone(),
    )
    .expect("summary");

    let expected = ConversationSummary {
        conversation_id,
        timestamp: timestamp.clone(),
        updated_at: timestamp,
        path,
        preview: "Count to 5".to_string(),
        model_provider: "test-provider".to_string(),
        cwd: PathBuf::from("/"),
        cli_version: "0.0.0".to_string(),
        source: codex_protocol::protocol::SessionSource::VSCode,
        git_info: None,
    };

    assert_eq!(summary, expected);
    Ok(())
}
