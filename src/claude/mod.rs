//! Driving the `claude` command line in headless mode.
//!
//! There is no Rust binding for the Claude Agent SDK, so Carl runs the real `claude` binary
//! as a child process and reads its JSON. That is not a workaround. It is the same thing the
//! SDK does, minus a language runtime, and it means Carl supervises an agent as a process,
//! which is exactly what AOS is being built to do.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde::Deserialize;

use crate::{Error, Result, SessionId};

pub mod asking;
pub mod permits;
mod pool;
mod session;
mod stream;
pub use pool::{KEEP_OPEN, Pool};
pub use session::Session;
pub use stream::{Chunk, Flow, Say, chunk_of};

/// One turn: what to say, and which conversation to say it in.
pub struct Turn<'a> {
    pub session: &'a SessionId,
    /// False on the very first message of a thread, because there is nothing to resume yet.
    pub resume: bool,
    pub prompt: &'a str,
    /// Memory and standing instructions, appended to Carl's system prompt.
    pub extra_system: Option<&'a str>,
    pub workdir: &'a Path,
}

/// What `claude --output-format json` gives back.
///
/// Only the fields Carl uses are named. Serde ignores the rest, so a new field in a future
/// Claude Code release does not break the parse.
#[derive(Debug, Deserialize)]
struct Envelope {
    #[serde(default)]
    result: Option<String>,
    #[serde(default)]
    is_error: bool,
    #[serde(default)]
    session_id: Option<String>,
    #[serde(default)]
    total_cost_usd: Option<f64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Answer {
    pub text: String,
    /// True when Carl was talked over and stopped reading the rest.
    ///
    /// Worth carrying rather than dropping. Claude's own session holds the whole answer, so
    /// without this the record would claim Carl said things nobody ever heard.
    pub interrupted: bool,
    /// The session Claude Code actually used. Compared against the one we asked for, because
    /// a silent mismatch means the next turn resumes the wrong conversation.
    pub session_id: Option<String>,
    pub cost_usd: Option<f64>,
}

/// Where the `claude` binary lives, so tests can point at a stand in.
pub struct Runner {
    program: PathBuf,
    /// Tools Carl may use without being asked each time.
    ///
    /// Headless has nobody to ask, so a tool that is not listed here is simply refused and
    /// Carl explains that he cannot do the thing rather than doing it.
    allowed: Vec<String>,
    /// How much Claude decides for itself about the rest. `Ask` in headless means refuse, which
    /// is why this is worth setting per surface rather than leaving at the default everywhere.
    mode: permits::Mode,
    /// Which model this agent runs on, when it is not the CLI's default.
    ///
    /// `None` means say nothing and let the CLI choose, rather than naming a default here that
    /// would then have to agree with it forever.
    model: Option<String>,
    /// Where the panel socket is, when Carl is allowed to put a question to JJ.
    ///
    /// `None` means he is not, and `Ask` then keeps its old headless meaning of refuse. The
    /// hook is only worth installing where there is a panel that could answer it.
    ask_through: Option<(PathBuf, String)>,
}

/// Running python, which is what makes Carl able to work something out rather than guess.
///
/// Not `python3` itself. `etc/carl-python` is the same interpreter inside a namespace where
/// the home directory does not exist, the network is gone, and one directory is writable.
/// Verified: it cannot read `~/.carl/slack.json`, cannot list the home directory, cannot open
/// a socket, and can see two processes rather than the machine's.
///
/// Bare `python3` was granted first and was shell access wearing a hat. It could read any
/// file the user could read, and anybody able to message Carl in Slack could ask it to.
pub const PYTHON: &str = "Bash(carl-python:*)";

/// How a tool Carl has picked up reads on screen.
///
/// One short line, marked so it is obviously not part of the answer. It exists so a person can
/// tell a long answer from a wedged one: between the question and the first word there was only
/// a caret, and reading forty files looked identical to having stopped.
pub fn doing_line(tool: &str, detail: &str) -> String {
    const MOST: usize = 70;
    let detail: String = detail.split_whitespace().collect::<Vec<_>>().join(" ");
    let detail = match detail.char_indices().nth(MOST) {
        Some((at, _)) => format!("{}...", &detail[..at]),
        None => detail,
    };
    match detail.is_empty() {
        true => format!("\n  ... {tool}\n"),
        false => format!("\n  ... {tool}: {detail}\n"),
    }
}

/// How a refusal reads when it is put in front of a person.
///
/// Names the tool in the CLI's own syntax, because the usual fix is to paste that into the
/// allow list in `permissions.json`, and a message that describes the tool without naming it
/// makes somebody go and look it up.
///
/// It is not always the fix, which is the other half. Some tools are absent on purpose and
/// there is a sanctioned route in their place. Telling somebody to widen a permission that was
/// deliberately narrow sends them to make the system worse, so where a route exists this says
/// the route instead.
pub fn refusal_line(tool: &str, why: &str) -> String {
    format!("\n[refused: {tool}] {why}\n{}\n", advice_for(tool))
}

/// What to do about a refused tool.
///
/// The agent tools are absent by decision, not by oversight. Given one, Carl spawned a process
/// and told it who to be, which is not delegating: the thing he made had no identity, no
/// memory, no rank and no lead. `carl handoff` exists so the work goes to a real agent. A
/// refusal that pointed at `permissions.json` would invite somebody to undo that.
fn advice_for(tool: &str) -> String {
    match tool {
        "Agent" | "Task" | "ListAgents" | "SendMessage" => {
            "Work goes down the chain with `carl handoff --from <you> --to <them> \"the work\"`, \
             which runs the real agent with their own memory and tools. There is no tool for \
             listing agents or reaching one directly, and nothing here needs adding to \
             permissions.json."
                .to_string()
        }
        "ToolSearch" => {
            "The tools you hold are the ones you were given at the start of this run. There is \
             no wider set to search, so nothing here needs adding to permissions.json."
                .to_string()
        }
        _ => format!(
            "Nobody can approve this while Carl runs headless. Add {tool:?} to \
             permissions.json, or raise the mode for this surface."
        ),
    }
}

/// Where the sandboxed interpreter lives, relative to the repository.
pub const PYTHON_SCRIPT: &str = "etc/carl-python";

impl Default for Runner {
    fn default() -> Self {
        Self {
            program: PathBuf::from("claude"),
            allowed: vec![PYTHON.to_string()],
            model: None,
            mode: permits::Mode::Ask,
            ask_through: None,
        }
    }
}

impl Runner {
    pub fn at(program: impl Into<PathBuf>) -> Self {
        Self {
            program: program.into(),
            allowed: vec![PYTHON.to_string()],
            model: None,
            mode: permits::Mode::Ask,
            ask_through: None,
        }
    }

    /// Lets Carl put a tool call to JJ in the panel instead of being refused for it.
    ///
    /// Installs a `PreToolUse` hook that asks over this home's panel socket. Only meaningful
    /// under `Ask`: the other two modes decide for themselves, and asking anyway would put a
    /// question on screen for something already settled.
    ///
    /// This adds a hook rather than replacing the ones JJ has. `guard.sh` still runs on every
    /// Bash call, and a deny from either is a deny.
    pub fn asking_jj(mut self, home: impl Into<PathBuf>, surface: impl Into<String>) -> Self {
        self.ask_through = Some((home.into(), surface.into()));
        self
    }

    /// Replaces the allowed tool list. An empty list means Carl may use no tools at all.
    /// Runs this agent on a named model.
    ///
    /// Wired late, and worth saying why. `config.model` existed for a while and was only ever
    /// read to draw a label in the panel, so an agent's folder could say `claude-fable-5` while
    /// the process ran whatever the CLI defaulted to. A configuration field that changes a
    /// caption and nothing else is worse than not having one, because it reads as settled.
    pub fn running(mut self, model: impl Into<String>) -> Self {
        self.model = Some(model.into());
        self
    }

    pub fn allowing(mut self, tools: Vec<String>) -> Self {
        self.allowed = tools;
        self
    }

    /// Takes both the list and the mode from what JJ wrote down for this surface.
    pub fn permitted_by(mut self, permits: &permits::Permits) -> Self {
        self.allowed = permits.allow.clone();
        self.mode = permits.mode;
        self
    }

    pub fn program(&self) -> &Path {
        &self.program
    }
}

/// Tools no agent may ever hold, whatever else it is granted.
///
/// The chain of command is the whole design. Carl hands to a lead, the lead hands to one of its
/// own people, and work moves one step at a time. `org::check_delegation` refuses anything else
/// and the briefs say it in words.
///
/// None of that binds the built in subagent tool. Given it, Carl does not delegate to Olivia,
/// he spawns a fresh process and tells it "you are Miles, do this". That agent has no identity,
/// no memory folder, no rank, no journal entry and no lead. It looks like delegation in the
/// transcript and it is not delegation at all: nobody was asked, nobody reviewed it, and
/// Olivia never knew. JJ reported exactly this on 2026 08 29.
///
/// So it is refused at the process rather than discouraged in a brief, for the same reason the
/// mail tools that destroy are simply absent. An agent that cannot call a tool cannot be talked
/// into calling it.
pub const NEVER: &[&str] = &["Task", "Agent"];

/// Where the shared memory lives, if it is there at all.
///
/// `None` when the folder is absent, so a checkout without it does not pass a flag naming a
/// directory that does not exist.
pub fn shared_memory() -> Option<String> {
    let path = std::path::Path::new(&std::env::var("HOME").ok()?)
        .join("Projects")
        .join("MEMORY");
    path.is_dir().then(|| path.to_string_lossy().into_owned())
}

impl Runner {
    /// The allow list as arguments, or nothing at all when it is empty.
    ///
    /// Nothing rather than an empty flag, because some argument parsers read an empty allow
    /// list as allowing everything, which is the opposite of what an empty list means.
    pub(crate) fn allowed_args(&self) -> Vec<String> {
        if self.allowed.is_empty() {
            return Vec::new();
        }
        let mut args = vec!["--allowedTools".to_string()];
        args.extend(self.allowed.iter().cloned());
        args
    }

    pub fn args_for(&self, turn: &Turn<'_>) -> Vec<String> {
        self.args_with(turn, ["--print", "--output-format", "json"])
    }

    fn args_with<'b>(
        &self,
        turn: &Turn<'_>,
        head: impl IntoIterator<Item = &'b str>,
    ) -> Vec<String> {
        let mut args: Vec<String> = head.into_iter().map(str::to_owned).collect();

        if let Some(model) = &self.model {
            args.push("--model".into());
            args.push(model.clone());
        }

        // The shared memory, which every agent is told to read before it works.
        //
        // An agent runs in its own folder under Projects/army, so Projects/MEMORY is a sibling
        // it cannot reach, and reading outside the working directory needs saying so. Without
        // this the instruction is worse than absent: an agent asked to send mail read its
        // standing orders, could not open the file naming the rules, and correctly refused to
        // send anything at all.
        //
        // Before `--allowedTools`, which is variadic. Nothing goes after that list.
        if let Some(shared) = shared_memory() {
            args.push("--add-dir".into());
            args.push(shared);
        }

        // Before `--allowedTools`, which is variadic and swallows whatever follows its list.
        args.push("--disallowedTools".into());
        args.extend(NEVER.iter().map(|s| (*s).to_string()));

        if !self.allowed.is_empty() {
            args.push("--allowedTools".into());
            args.extend(self.allowed.iter().cloned());
        }

        // Only when it is not the default. Passing the default explicitly would be a second
        // place that has to agree with the CLI about what the default is.
        if let Some(mode) = self.mode.flag() {
            args.push("--permission-mode".into());
            args.push(mode.to_string());
        }

        // Under `Ask` only. The other modes have already decided, and a hook that asked anyway
        // would put a question on screen about something nobody needed to answer.
        if self.mode == permits::Mode::Ask
            && let Some((home, surface)) = &self.ask_through
            && let Some(settings) = asking::for_this_build(home, surface)
        {
            args.push("--settings".into());
            args.push(settings);
        }

        // --session-id pins a new conversation to an id we chose. --resume continues one that
        // already exists. Sending both is an error, which is why `resume` is a flag on the
        // turn rather than something guessed here.
        if turn.resume {
            args.push("--resume".into());
            args.push(turn.session.to_string());
        } else {
            args.push("--session-id".into());
            args.push(turn.session.to_string());
        }

        if let Some(system) = turn.extra_system {
            args.push("--append-system-prompt".into());
            args.push(system.to_string());
        }

        args
    }

    pub fn ask(&self, turn: &Turn<'_>) -> Result<Answer> {
        use std::io::Write;

        check(turn)?;
        std::fs::create_dir_all(turn.workdir)?;

        let mut child = Command::new(&self.program)
            .args(self.args_for(turn))
            .current_dir(turn.workdir)
            // The prompt goes on stdin rather than the argument vector. A Slack message can
            // be longer than the argument limit, and it can contain anything at all.
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| Error::Claude(format!("cannot run {}: {e}", self.program.display())))?;

        child
            .stdin
            .take()
            .ok_or_else(|| Error::Claude("no stdin on the child".into()))?
            .write_all(turn.prompt.as_bytes())?;

        let out = child.wait_with_output()?;
        let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
        let stderr = String::from_utf8_lossy(&out.stderr).into_owned();

        if !out.status.success() {
            return Err(Error::Claude(format!(
                "claude exited with {}: {}",
                out.status,
                first_line(&stderr).unwrap_or("no error output")
            )));
        }

        parse(&stdout)
    }
}

/// Refuses a turn that cannot possibly work.
///
/// An empty prompt reaches the CLI as no prompt at all, and it answers "Input must be
/// provided either through stdin or as a prompt argument", which is a true sentence that
/// tells you nothing about where the empty question came from. Better to fail here, next to
/// the caller that built it.
pub(crate) fn check(turn: &Turn<'_>) -> Result<()> {
    if turn.prompt.trim().is_empty() {
        return Err(Error::Claude(
            "refusing to ask claude an empty question".into(),
        ));
    }
    Ok(())
}

/// Pulls the answer out of the JSON envelope.
pub fn parse(stdout: &str) -> Result<Answer> {
    let envelope: Envelope = serde_json::from_str(stdout.trim()).map_err(|e| {
        Error::Claude(format!(
            "cannot read the answer as JSON ({e}): {}",
            first_line(stdout).unwrap_or("<empty>")
        ))
    })?;

    // An error envelope still parses, so checking the flag matters more than checking that
    // the JSON was well formed.
    if envelope.is_error {
        return Err(Error::Claude(
            envelope
                .result
                .unwrap_or_else(|| "claude reported an error".into()),
        ));
    }

    let text = envelope
        .result
        .ok_or_else(|| Error::Claude("the answer had no result field".into()))?;

    Ok(Answer {
        text,
        interrupted: false,
        session_id: envelope.session_id,
        cost_usd: envelope.total_cost_usd,
    })
}

fn first_line(s: &str) -> Option<&str> {
    s.lines().map(str::trim).find(|l| !l.is_empty())
}

#[cfg(test)]
mod delegation_tests {
    use super::*;

    fn args_of(runner: &Runner) -> Vec<String> {
        let session = SessionId::fresh().expect("a session id");
        runner.args_for(&Turn {
            session: &session,
            resume: false,
            prompt: "hello",
            extra_system: None,
            workdir: std::path::Path::new("/tmp"),
        })
    }

    /// The bug JJ reported. Carl did not delegate to Olivia, he spawned a subagent and told it
    /// "you are Miles, do this". That thing has no identity, no memory, no rank and no lead,
    /// and Olivia never knew. It reads as delegation in a transcript and is not delegation.
    #[test]
    fn no_agent_is_ever_handed_the_subagent_tool() {
        let args = args_of(&Runner::at("claude"));
        let at = args
            .iter()
            .position(|a| a == "--disallowedTools")
            .expect("the refusal must be passed on every invocation");
        for banned in NEVER {
            assert!(
                args[at + 1..].iter().any(|a| a == banned),
                "{banned} was not refused: {args:?}"
            );
        }
    }

    /// The field that only drew a label.
    ///
    /// `config.model` was read in exactly two places, both of which put it on a screen. An
    /// agent's folder could say `claude-fable-5` while its process ran whatever the CLI
    /// defaulted to, and the panel would confidently show the wrong answer. Found when JJ asked
    /// for Carl to be Fable 5 and the change would have been cosmetic.
    #[test]
    fn a_named_model_is_actually_passed_to_the_cli() {
        let args = args_of(&Runner::at("claude").running("claude-fable-5"));
        let at = args
            .iter()
            .position(|a| a == "--model")
            .expect("the model must reach the command line");
        assert_eq!(args[at + 1], "claude-fable-5");
    }

    /// Saying nothing is different from saying the default. Naming a default here would be a
    /// second place that has to agree with the CLI about what it is.
    #[test]
    fn no_model_means_no_flag_at_all() {
        let args = args_of(&Runner::at("claude"));
        assert!(
            !args.iter().any(|a| a == "--model"),
            "an unset model must pass no flag: {args:?}"
        );
    }

    /// `--allowedTools` is variadic, so anything after its list is read as another tool name.
    /// The refusal has to come first or it is silently swallowed into the allow list, which
    /// would grant the exact tool it exists to deny.
    #[test]
    fn the_refusal_comes_before_the_allow_list() {
        let args = args_of(&Runner::at("claude"));
        let deny = args.iter().position(|a| a == "--disallowedTools");
        let allow = args.iter().position(|a| a == "--allowedTools");
        if let (Some(deny), Some(allow)) = (deny, allow) {
            assert!(
                deny < allow,
                "the deny list is inside the allow list: {args:?}"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session() -> SessionId {
        SessionId::fresh().unwrap()
    }

    #[test]
    fn a_first_message_pins_the_session_and_does_not_resume() {
        let s = session();
        let args = Runner::default().args_for(&Turn {
            session: &s,
            resume: false,
            prompt: "hi",
            extra_system: None,
            workdir: Path::new("/tmp"),
        });
        assert!(args.contains(&"--session-id".to_string()), "{args:?}");
        assert!(!args.contains(&"--resume".to_string()), "{args:?}");
        assert!(args.contains(&s.to_string()));
    }

    /// Sending both flags is an error, so the two paths must stay exclusive.
    #[test]
    fn a_later_message_resumes_and_does_not_re_pin() {
        let s = session();
        let args = Runner::default().args_for(&Turn {
            session: &s,
            resume: true,
            prompt: "and another thing",
            extra_system: None,
            workdir: Path::new("/tmp"),
        });
        assert!(args.contains(&"--resume".to_string()), "{args:?}");
        assert!(!args.contains(&"--session-id".to_string()), "{args:?}");
    }

    /// Carl can work an answer out rather than guessing at it, which for arithmetic is the
    /// difference between right and confidently wrong.
    #[test]
    fn python_is_allowed_by_default() {
        let s = session();
        let args = Runner::default().args_for(&Turn {
            session: &s,
            resume: false,
            prompt: "what is 2 to the 64",
            extra_system: None,
            workdir: Path::new("/tmp"),
        });
        let at = args.iter().position(|a| a == "--allowedTools").unwrap();
        assert_eq!(args[at + 1], PYTHON);
    }

    /// Nothing is granted silently. An empty list must produce no flag at all rather than an
    /// empty one, which some argument parsers read as "allow everything".
    #[test]
    fn no_tools_means_no_flag() {
        let s = session();
        let args = Runner::default().allowing(vec![]).args_for(&Turn {
            session: &s,
            resume: false,
            prompt: "hi",
            extra_system: None,
            workdir: Path::new("/tmp"),
        });
        assert!(!args.contains(&"--allowedTools".to_string()), "{args:?}");
    }

    #[test]
    fn memory_is_appended_to_the_system_prompt() {
        let s = session();
        let args = Runner::default().args_for(&Turn {
            session: &s,
            resume: true,
            prompt: "hi",
            extra_system: Some("JJ is 11."),
            workdir: Path::new("/tmp"),
        });
        let at = args
            .iter()
            .position(|a| a == "--append-system-prompt")
            .unwrap();
        assert_eq!(args[at + 1], "JJ is 11.");
    }

    /// The bug that reached a real Slack channel. Somebody typed only "Carl", which is being
    /// called rather than being asked, and the empty question went all the way to the CLI.
    /// It answered "Input must be provided", which is true and says nothing about the cause.
    #[test]
    fn an_empty_question_is_refused_here_rather_than_by_the_cli() {
        let s = session();
        for prompt in ["", "   ", "\n\t "] {
            let err = check(&Turn {
                session: &s,
                resume: false,
                prompt,
                extra_system: None,
                workdir: Path::new("/tmp"),
            })
            .unwrap_err()
            .to_string();
            assert!(err.contains("empty question"), "{err}");
        }
    }

    #[test]
    fn a_real_question_passes_the_check() {
        let s = session();
        assert!(
            check(&Turn {
                session: &s,
                resume: false,
                prompt: "what should I research",
                extra_system: None,
                workdir: Path::new("/tmp"),
            })
            .is_ok()
        );
    }

    #[test]
    fn a_normal_answer_is_read() {
        let answer = parse(
            r#"{"type":"result","result":"Hello JJ","session_id":"abc","total_cost_usd":0.01}"#,
        )
        .unwrap();
        assert_eq!(answer.text, "Hello JJ");
        assert_eq!(answer.session_id.as_deref(), Some("abc"));
        assert_eq!(answer.cost_usd, Some(0.01));
    }

    /// An error envelope is valid JSON, so parsing successfully is not the same as succeeding.
    #[test]
    fn an_error_envelope_is_an_error() {
        let err = parse(r#"{"is_error":true,"result":"session not found"}"#).unwrap_err();
        assert!(err.to_string().contains("session not found"), "{err}");
    }

    #[test]
    fn unknown_fields_do_not_break_the_parse() {
        let answer =
            parse(r#"{"result":"fine","session_id":"x","some_future_field":{"nested":[1,2]}}"#)
                .unwrap();
        assert_eq!(answer.text, "fine");
    }

    #[test]
    fn non_json_output_says_what_it_actually_got() {
        let err = parse("command not found: claude").unwrap_err();
        assert!(err.to_string().contains("command not found"), "{err}");
    }

    #[test]
    fn an_empty_answer_is_an_error_rather_than_an_empty_reply() {
        assert!(parse("").is_err());
        assert!(parse(r#"{"type":"result"}"#).is_err(), "no result field");
    }

    fn a_turn<'a>(s: &'a SessionId) -> Turn<'a> {
        Turn {
            session: s,
            resume: false,
            prompt: "hi",
            extra_system: None,
            workdir: Path::new("/tmp"),
        }
    }

    /// What comes after `--settings`, if anything does.
    fn installed(args: &[String]) -> Option<serde_json::Value> {
        let at = args.iter().position(|a| a == "--settings")?;
        serde_json::from_str(args.get(at + 1)?).ok()
    }

    #[test]
    fn asking_jj_installs_a_hook_that_runs_against_that_home() {
        let s = session();
        let args = Runner::default()
            .asking_jj("/home/jj_tmc/.carl", "jj")
            .args_for(&a_turn(&s));

        let settings = installed(&args).expect("a settings argument: {args:?}");
        let command = settings["hooks"]["PreToolUse"][0]["hooks"][0]["command"]
            .as_str()
            .unwrap();
        assert!(command.contains("permit-hook"), "{command}");
        assert!(command.contains("/home/jj_tmc/.carl"), "{command}");
    }

    /// The old behaviour has to stay reachable. A surface with no panel behind it must not
    /// install a hook that asks something nobody can answer.
    #[test]
    fn without_it_nothing_is_installed_and_the_old_refusal_stands() {
        let s = session();
        let args = Runner::default().args_for(&a_turn(&s));
        assert!(installed(&args).is_none(), "{args:?}");
    }

    /// A mode that has already decided must not put a question on screen about it.
    #[test]
    fn a_mode_that_decides_for_itself_does_not_ask() {
        let s = session();
        let args = Runner::default()
            .permitted_by(&permits::Permits {
                mode: permits::Mode::BypassPermissions,
                allow: Vec::new(),
            })
            .asking_jj("/home/jj_tmc/.carl", "jj")
            .args_for(&a_turn(&s));

        assert!(
            installed(&args).is_none(),
            "bypass already answered every question: {args:?}"
        );
    }
}
