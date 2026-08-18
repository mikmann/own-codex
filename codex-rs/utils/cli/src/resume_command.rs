//! Shared formatting for user-facing `codex-extended-workflow resume` command hints.

use codex_protocol::ThreadId;
use codex_shell_command::parse_command::shlex_join;
use std::ffi::OsStr;

const STATE_SCOPE_ENV: &str = "CODEX_EXTENDED_WORKFLOW_STATE_SCOPE";
const SHARED_STATE_SCOPE: &str = "shared";
const COMPATIBILITY_STATE_SCOPE: &str = "compatibility";

fn resume_command_prefix() -> &'static str {
    resume_command_prefix_for_scope(std::env::var_os(STATE_SCOPE_ENV).as_deref())
}

fn resume_command_prefix_for_scope(state_scope: Option<&OsStr>) -> &'static str {
    match state_scope {
        Some(scope) if scope == OsStr::new(SHARED_STATE_SCOPE) => {
            "codex-extended-workflow --shared-state resume"
        }
        Some(scope) if scope == OsStr::new(COMPATIBILITY_STATE_SCOPE) => "codex resume",
        _ => "codex-extended-workflow resume",
    }
}

pub fn resume_command(thread_name: Option<&str>, thread_id: Option<ThreadId>) -> Option<String> {
    let resume_target = thread_name
        .filter(|name| !name.is_empty())
        .map(str::to_string)
        .or_else(|| thread_id.map(|thread_id| thread_id.to_string()));
    resume_target.map(|target| {
        let prefix = resume_command_prefix();
        let needs_double_dash = target.starts_with('-');
        let escaped = shlex_join(&[target]);
        if needs_double_dash {
            format!("{prefix} -- {escaped}")
        } else {
            format!("{prefix} {escaped}")
        }
    })
}

pub fn resume_hint(thread_name: Option<&str>, thread_id: Option<ThreadId>) -> Option<String> {
    let thread_id = thread_id?;
    match thread_name.filter(|name| !name.is_empty()) {
        Some(thread_name) => Some(format!(
            "{}, then select {thread_name} ({thread_id})",
            resume_command_prefix()
        )),
        None => resume_command(/*thread_name*/ None, Some(thread_id)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn prefers_name_over_id() {
        let thread_id = ThreadId::from_string("123e4567-e89b-12d3-a456-426614174000").unwrap();
        let command = resume_command(Some("my-thread"), Some(thread_id));
        assert_eq!(
            command,
            Some("codex-extended-workflow resume my-thread".to_string())
        );
    }

    #[test]
    fn formats_thread_id_when_name_is_missing() {
        let thread_id = ThreadId::from_string("123e4567-e89b-12d3-a456-426614174000").unwrap();
        let command = resume_command(/*thread_name*/ None, Some(thread_id));
        assert_eq!(
            command,
            Some("codex-extended-workflow resume 123e4567-e89b-12d3-a456-426614174000".to_string())
        );
    }

    #[test]
    fn returns_none_without_a_resume_target() {
        let command = resume_command(/*thread_name*/ None, /*thread_id*/ None);
        assert_eq!(command, None);
    }

    #[test]
    fn shared_state_scope_is_preserved_in_resume_commands() {
        assert_eq!(
            resume_command_prefix_for_scope(Some(OsStr::new("shared"))),
            "codex-extended-workflow --shared-state resume"
        );
        assert_eq!(
            resume_command_prefix_for_scope(Some(OsStr::new("isolated"))),
            "codex-extended-workflow resume"
        );
        assert_eq!(
            resume_command_prefix_for_scope(Some(OsStr::new("compatibility"))),
            "codex resume"
        );
    }

    #[test]
    fn quotes_thread_names_when_needed() {
        let command = resume_command(Some("-starts-with-dash"), /*thread_id*/ None);
        assert_eq!(
            command,
            Some("codex-extended-workflow resume -- -starts-with-dash".to_string())
        );

        let command = resume_command(Some("two words"), /*thread_id*/ None);
        assert_eq!(
            command,
            Some("codex-extended-workflow resume 'two words'".to_string())
        );

        let command = resume_command(Some("quote'case"), /*thread_id*/ None);
        assert_eq!(
            command,
            Some("codex-extended-workflow resume \"quote'case\"".to_string())
        );
    }

    #[test]
    fn resume_hint_names_picker_item_with_id() {
        let thread_id = ThreadId::from_string("123e4567-e89b-12d3-a456-426614174000").unwrap();
        let hint = resume_hint(Some("my-thread"), Some(thread_id));
        assert_eq!(
            hint,
            Some(
                "codex-extended-workflow resume, then select my-thread (123e4567-e89b-12d3-a456-426614174000)"
                    .to_string()
            )
        );
    }

    #[test]
    fn resume_hint_uses_direct_id_command_without_name() {
        let thread_id = ThreadId::from_string("123e4567-e89b-12d3-a456-426614174000").unwrap();
        let hint = resume_hint(/*thread_name*/ None, Some(thread_id));
        assert_eq!(
            hint,
            Some("codex-extended-workflow resume 123e4567-e89b-12d3-a456-426614174000".to_string())
        );
    }

    #[test]
    fn resume_hint_requires_thread_id() {
        let hint = resume_hint(Some("my-thread"), /*thread_id*/ None);
        assert_eq!(hint, None);
    }
}
