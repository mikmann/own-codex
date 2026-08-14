use super::WorkflowResultCache;
use super::workflow_assignment_payload;
use codex_protocol::AgentPath;
use codex_protocol::ThreadId;
use codex_protocol::protocol::ExtendedThreadSpawnRoleMetadata;
use codex_protocol::protocol::SessionSource;
use codex_protocol::protocol::SubAgentSource;
use codex_protocol::protocol::encode_extended_thread_spawn_role_metadata;
use pretty_assertions::assert_eq;

#[test]
fn workflow_assignment_payload_only_strips_the_matching_new_task_envelope() {
    let author = "/root";
    let recipient = "/root/reviewer";
    let task = "Review the frozen candidate.";
    let matching = format!(
        "Message Type: NEW_TASK\nTask name: {recipient}\nSender: {author}\nPayload:\n{task}"
    );
    let different_author = matching.replace("Sender: /root", "Sender: /root/other");
    let different_recipient =
        matching.replace("Task name: /root/reviewer", "Task name: /root/other");
    let different_message_type = matching.replace("NEW_TASK", "MESSAGE");

    let actual = [
        workflow_assignment_payload(author, recipient, &matching),
        workflow_assignment_payload(author, recipient, &different_author),
        workflow_assignment_payload(author, recipient, &different_recipient),
        workflow_assignment_payload(author, recipient, &different_message_type),
    ];

    assert_eq!(
        actual,
        [
            task.to_string(),
            different_author,
            different_recipient,
            different_message_type,
        ]
    );
}

#[test]
fn workflow_result_cache_removes_only_the_completed_turn() {
    let cache = WorkflowResultCache::default();
    let completed_agent_id = ThreadId::new();
    let active_agent_id = ThreadId::new();
    cache.register(
        completed_agent_id,
        workflow_source("completed", "turn-completed"),
        "completed task".to_string(),
        /*model*/ None,
        /*reasoning_effort*/ None,
    );
    cache.register(
        active_agent_id,
        workflow_source("active", "turn-active"),
        "active task".to_string(),
        /*model*/ None,
        /*reasoning_effort*/ None,
    );

    cache.remove_for_turn("turn-completed");

    assert_eq!(
        (
            cache.get(completed_agent_id).is_some(),
            cache.get(active_agent_id).is_some(),
        ),
        (false, true)
    );
}

fn workflow_source(name: &str, workflow_turn_id: &str) -> SessionSource {
    SessionSource::SubAgent(SubAgentSource::ThreadSpawn {
        parent_thread_id: ThreadId::new(),
        depth: 1,
        agent_path: Some(
            AgentPath::try_from(format!("/root/{name}")).expect("valid workflow agent path"),
        ),
        agent_nickname: Some(name.to_string()),
        agent_role: encode_extended_thread_spawn_role_metadata(ExtendedThreadSpawnRoleMetadata {
            config_role: None,
            read_only: true,
            workflow_type: Some("Reviewer"),
            workflow_role: Some("Correctness"),
            workflow_name: Some(name),
            workflow_turn_id: Some(workflow_turn_id),
            assignment_sha256: Some("assignment"),
            workflow_root_sha256: Some("root"),
            context_budget: None,
        }),
    })
}
