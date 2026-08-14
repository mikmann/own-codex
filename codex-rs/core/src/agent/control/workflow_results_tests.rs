use super::workflow_assignment_payload;
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
