//! The `PreToolUse` hook, which is the only place a decision can reach the CLI from outside.
//!
//! Claude Code runs headless here, so there is nobody at the terminal to answer a prompt and
//! anything not on the allow list is refused before a person sees it. A hook is a program the
//! CLI runs before a tool call, and whose printed JSON it obeys. So this reads the call on
//! stdin, asks the panel, and prints what JJ said.
//!
//! **Every failure prints deny.** No backend, no answer in time, an unparseable payload, a
//! socket that will not open. The alternative would be a permission system that granted
//! everything the moment nothing was watching, which is worse than none because it looks like
//! one.
//!
//! **The exit status is always zero.** A hook that exits non zero is a hook that failed, and a
//! failed hook is ignored rather than obeyed. The refusal has to be a successful print.
//!
//! This does not replace `guard.sh`. Both hooks run on the same call and either can refuse.

use std::io::Read;
use std::path::Path;

use super::client::PanelClient;
use super::permission::{Request, Verdict, decision, read_call};

/// Reads the call, asks, and prints the decision. Never fails, by construction.
pub fn run(home: &Path, surface: &str, input: &mut dyn Read) -> String {
    let mut raw = String::new();
    if input.read_to_string(&mut raw).is_err() {
        return decision(Verdict::Deny, "the tool call could not be read");
    }

    let Ok(payload) = serde_json::from_str::<serde_json::Value>(&raw) else {
        return decision(Verdict::Deny, "the tool call was not readable JSON");
    };

    let (tool, detail) = read_call(&payload);
    let request = Request {
        // From the CLI's own session id plus the tool, so two questions in one turn are two
        // questions. Falls back to the clock, which is enough to tell them apart in order.
        id: mint(&payload, &tool),
        tool: tool.clone(),
        detail,
        surface: surface.to_string(),
        at: crate::army::event::now(),
    };

    let asked = request.id.clone();
    let verdict = ask(home, request).unwrap_or(Verdict::Deny);
    match verdict {
        Verdict::Allow => decision(Verdict::Allow, &format!("JJ allowed {tool} ({asked})")),
        Verdict::Deny => decision(
            Verdict::Deny,
            &format!("{tool} was not allowed. Ask JJ in the panel, or do it another way."),
        ),
    }
}

/// The part that can fail, kept separate so every way of failing lands on the same default.
fn ask(home: &Path, request: Request) -> Option<Verdict> {
    let at = super::socket_path(home);
    let client = PanelClient::connect(&at).ok()?;
    client.may_i(request).ok()
}

/// An id nothing else will reuse.
fn mint(payload: &serde_json::Value, tool: &str) -> String {
    let session = payload
        .get("session_id")
        .and_then(|v| v.as_str())
        .unwrap_or("no-session");
    format!("{session}:{tool}:{}", crate::army::event::now())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn verdict_in(printed: &str) -> String {
        let v: serde_json::Value = serde_json::from_str(printed).expect("valid JSON");
        v["hookSpecificOutput"]["permissionDecision"]
            .as_str()
            .unwrap()
            .to_string()
    }

    /// The whole point. Nothing listening must not mean everything is allowed.
    #[test]
    fn with_no_backend_running_the_answer_is_deny() {
        let dir = tempfile::tempdir().unwrap();
        let call = br#"{"session_id":"s","tool_name":"Bash","tool_input":{"command":"rm -rf /"}}"#;
        let printed = run(dir.path(), "jj", &mut &call[..]);
        assert_eq!(verdict_in(&printed), "deny");
    }

    #[test]
    fn a_payload_that_is_not_json_is_denied_rather_than_ignored() {
        let dir = tempfile::tempdir().unwrap();
        let printed = run(dir.path(), "jj", &mut &b"not json at all"[..]);
        assert_eq!(verdict_in(&printed), "deny");
    }

    /// A hook that exits non zero is ignored by the CLI, so a refusal has to be a successful
    /// print. This checks the shape the CLI actually reads.
    #[test]
    fn the_refusal_is_shaped_the_way_the_cli_reads_it() {
        let dir = tempfile::tempdir().unwrap();
        let printed = run(dir.path(), "jj", &mut &b"{}"[..]);
        let v: serde_json::Value = serde_json::from_str(&printed).unwrap();
        assert_eq!(v["hookSpecificOutput"]["hookEventName"], "PreToolUse");
        assert!(
            v["hookSpecificOutput"]["permissionDecisionReason"]
                .as_str()
                .is_some_and(|r| !r.is_empty()),
            "and it says why: {printed}"
        );
    }

    #[test]
    fn two_calls_in_one_session_are_two_questions() {
        let payload: serde_json::Value = serde_json::from_str(r#"{"session_id":"s"}"#).unwrap();
        assert_ne!(mint(&payload, "Bash"), mint(&payload, "Write"));
    }
}
