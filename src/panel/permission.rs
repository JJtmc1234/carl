//! A permission request, waiting for JJ.
//!
//! Carl runs headless, so the CLI has nobody to prompt and refuses anything not on the allow
//! list. A `PreToolUse` hook is the one place a decision can be made from outside the model, so
//! that is where this sits: the hook holds the tool call still, this asks the panel, and the
//! answer becomes the `permissionDecision` the CLI acts on.
//!
//! **The default is deny, and every path that fails takes it.** No panel running, no answer in
//! time, a socket that will not open, a reply that will not parse. A design that allowed on
//! failure would be a permission system that grants everything the moment nobody is watching,
//! which is the opposite of the point and worse than having none, because it would look like
//! there was one.
//!
//! This does not replace the guard. Both hooks run, and a deny from either is a deny.

use std::time::Duration;

use serde::{Deserialize, Serialize};

/// How long the hook waits for a person before giving up and refusing.
///
/// Ten minutes. It was ninety seconds, which is roughly the time it takes to notice a band has
/// appeared, read what it is asking, and decide. JJ pressed Allow and nothing happened, because
/// the question had already expired and been refused: the button was fine and the window was
/// not. A prompt nobody can reach in time is the same as no prompt, except that it also looks
/// broken.
///
/// The cost is that one tool call can sit still for ten minutes. That is the honest price of
/// asking a person, and it is bounded: the default is still deny, so a forgotten question ends
/// by itself rather than holding the process for ever.
pub const WAIT: Duration = Duration::from_secs(600);

/// One thing Carl wants to do.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Request {
    /// Minted by the hook, so an answer cannot land on the wrong question.
    pub id: String,
    /// As the CLI names it: `Bash`, `Write`, `Read`.
    pub tool: String,
    /// The part worth reading before deciding: the command, or the path.
    pub detail: String,
    /// Which surface asked, so the panel can say whether this came from JJ or from Slack.
    pub surface: String,
    pub at: u64,
}

/// What JJ said.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    Allow,
    Deny,
}

impl Verdict {
    /// The word the CLI expects in `permissionDecision`.
    pub fn word(self) -> &'static str {
        match self {
            Verdict::Allow => "allow",
            Verdict::Deny => "deny",
        }
    }
}

/// The JSON a `PreToolUse` hook prints to decide a call.
///
/// Shaped by the CLI rather than by this program, so it is built in one place and the shape
/// lives next to a test that pins it.
pub fn decision(verdict: Verdict, why: &str) -> String {
    serde_json::json!({
        "hookSpecificOutput": {
            "hookEventName": "PreToolUse",
            "permissionDecision": verdict.word(),
            "permissionDecisionReason": why,
        }
    })
    .to_string()
}

/// What the hook was handed on stdin, reduced to the parts a person needs to decide.
///
/// Unknown shapes are not an error. A hook that refused to parse an unfamiliar payload would
/// fail closed on every tool the CLI adds after this was written, and failing closed on
/// everything is indistinguishable from being broken.
pub fn read_call(payload: &serde_json::Value) -> (String, String) {
    let tool = payload
        .get("tool_name")
        .and_then(|t| t.as_str())
        .unwrap_or("a tool")
        .to_string();

    let input = payload.get("tool_input");
    let detail = input
        .and_then(|i| {
            i.get("command")
                .or_else(|| i.get("file_path"))
                .or_else(|| i.get("path"))
                .or_else(|| i.get("pattern"))
        })
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .unwrap_or_else(|| match input {
            Some(i) => {
                let text = i.to_string();
                if text.len() > 200 {
                    format!("{}...", &text[..200])
                } else {
                    text
                }
            }
            None => "no detail given".to_string(),
        });

    (tool, detail)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The shape the CLI reads. Pinned, because getting it wrong means every decision is
    /// ignored and everything silently falls back to the old behaviour.
    #[test]
    fn a_decision_is_the_shape_the_cli_expects() {
        let text = decision(Verdict::Allow, "JJ said yes");
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        let out = &v["hookSpecificOutput"];
        assert_eq!(out["hookEventName"], "PreToolUse");
        assert_eq!(out["permissionDecision"], "allow");
        assert_eq!(out["permissionDecisionReason"], "JJ said yes");

        let denied = decision(Verdict::Deny, "no answer");
        let v: serde_json::Value = serde_json::from_str(&denied).unwrap();
        assert_eq!(v["hookSpecificOutput"]["permissionDecision"], "deny");
    }

    #[test]
    fn a_bash_call_is_read_as_its_command() {
        let payload = serde_json::json!({
            "tool_name": "Bash",
            "tool_input": { "command": "python3 -c 'print(1)'" }
        });
        let (tool, detail) = read_call(&payload);
        assert_eq!(tool, "Bash");
        assert_eq!(detail, "python3 -c 'print(1)'");
    }

    #[test]
    fn a_write_is_read_as_its_path() {
        let payload = serde_json::json!({
            "tool_name": "Write",
            "tool_input": { "file_path": "/home/jj/thing.py", "content": "..." }
        });
        let (tool, detail) = read_call(&payload);
        assert_eq!(tool, "Write");
        assert_eq!(detail, "/home/jj/thing.py");
    }

    /// A tool nobody anticipated still reaches a person with something to read, because a hook
    /// that fails closed on anything unfamiliar refuses everything the CLI adds later.
    #[test]
    fn an_unfamiliar_tool_still_says_something() {
        let payload = serde_json::json!({ "tool_name": "Whatever", "tool_input": { "odd": 1 } });
        let (tool, detail) = read_call(&payload);
        assert_eq!(tool, "Whatever");
        assert!(detail.contains("odd"), "{detail}");

        let (tool, detail) = read_call(&serde_json::json!({}));
        assert_eq!(tool, "a tool");
        assert_eq!(detail, "no detail given");
    }

    #[test]
    fn a_huge_input_is_cut_rather_than_pasted_whole() {
        let payload = serde_json::json!({
            "tool_name": "Write",
            "tool_input": { "content": "x".repeat(5000) }
        });
        let (_, detail) = read_call(&payload);
        assert!(detail.len() < 260, "{}", detail.len());
        assert!(detail.ends_with("..."));
    }
}
