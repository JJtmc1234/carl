//! One exchange, start to finish.
//!
//! Two ways to ask, sharing every rule about what gets written down. `respond` waits for the
//! whole answer, which is right for a terminal. `stream` hands over the words as they are
//! written, which is right for a voice, because Claude takes five to twenty five seconds and
//! the first sentence is usually ready in under one.

use std::path::Path;

use crate::claude::{Answer, Flow, Pool, Runner, Say, Turn};
use crate::{Area, Camera, ThreadId};
use anyhow::{Context, Result};

mod exchange;
use exchange::Exchange;

/// The full form, where what gets recorded and what gets sent can differ.
/// The runner a surface should use, built from what JJ wrote down for it.
///
/// **Every surface has to come through here.** Building a `Runner::default()` instead is how the
/// terminal, the microphone and Slack ended up ignoring `permissions.json` completely. JJ had
/// written an allow list with `python3`, `Write` and `Edit` in it and raised the mode, and Carl
/// went on refusing all of it, because the default is the sandboxed python alone at mode `Ask`.
/// Nothing reported this: a refusal looks the same whether the rule said no or nobody read the
/// rule.
///
/// The asking hook is installed as well, so `Ask` means Carl puts the call to JJ in the panel
/// rather than refusing it where nobody sees. With no panel running the hook denies, which is
/// exactly what `Ask` did before it existed, so the worst case is the old behaviour.
pub fn runner_for(home: &Path, surface: crate::claude::permits::Surface) -> Result<Runner> {
    let book = crate::claude::permits::Book::load(home)?;
    let named = match surface {
        crate::claude::permits::Surface::Jj => "jj",
        crate::claude::permits::Surface::Shared => "slack",
    };
    let permits = book.for_surface(surface);

    // Whoever JJ is talking to on these surfaces is Carl, and Carl is the chief. The chief holds
    // no tools, in the chain and everywhere else. Without this he had two sets of powers: none
    // when a lead handed him work, and Write and Edit when JJ typed at him, so the agent who is
    // never meant to implement anything was writing code.
    let narrowed = crate::claude::permits::Permits {
        mode: permits.mode,
        allow: crate::claude::permits::narrow_to_rank(
            &permits.allow,
            crate::army::org::Rank::Chief,
        ),
    };

    Ok(Runner::default()
        .permitted_by(&narrowed)
        .asking_jj(home, named))
}

pub fn respond_full(
    runner: &Runner,
    home: &Path,
    thread: &ThreadId,
    said: &str,
    sent: Option<&str>,
    author: Option<String>,
) -> Result<Answer> {
    Exchange {
        home,
        thread,
        said,
        sent,
        author,
        extra: None,
    }
    .run(|p| {
        runner.ask(&Turn {
            session: &p.session,
            resume: p.resume,
            prompt: p.prompt,
            extra_system: Some(&p.all_in_system()),
            workdir: &p.workdir,
        })
    })
}

/// Handles one message, handing each piece of the answer over as it arrives.
///
/// `on_text` decides whether to keep going. Returning `Flow::Stop` abandons the rest, which
/// is what happens when Carl is talked over. `extra` carries the voice brief when the answer
/// is going to be spoken.
pub fn stream(
    home: &Path,
    thread: &ThreadId,
    said: &str,
    sent: Option<&str>,
    extra: Option<&str>,
    on_text: &mut dyn FnMut(Say<'_>) -> Flow,
) -> Result<Answer> {
    // The panel and the terminal are JJ's own surfaces, reached through a socket in a 0700
    // directory or through his own keyboard. They read the `jj` permits. Slack reads `shared`,
    // because who can send to it is a different set of people.
    let runner = runner_for(home, crate::claude::permits::Surface::Jj)?;
    Exchange {
        home,
        thread,
        said,
        sent,
        // Only one person has this terminal, and it is the same person who has the
        // microphone. A note written from here with no source reads later as if nobody said
        // it.
        author: Some(crate::brief::OWNER.to_string()),
        extra,
    }
    .run(|p| {
        runner.ask_streaming(
            &Turn {
                session: &p.session,
                resume: p.resume,
                prompt: p.prompt,
                extra_system: Some(&p.all_in_system()),
                workdir: &p.workdir,
            },
            on_text,
        )
    })
}

/// Everything one question needs, gathered up.
///
/// A struct rather than eight arguments, because eight arguments in a row is how `said` and
/// `sent` end up swapped and the record quietly fills with Carl's own scaffolding instead of
/// what somebody actually said.
pub struct Asking<'a> {
    pub pool: &'a mut Pool,
    pub home: &'a Path,
    pub thread: &'a ThreadId,
    /// What the person said. This is what goes in the record.
    pub said: &'a str,
    /// What Carl is told, when it differs. Never recorded.
    pub sent: Option<&'a str>,
    /// Who is speaking, by name.
    ///
    /// Goes in the record and on any note written this turn. Memory is one pile and everybody
    /// who can reach Carl writes into it, so a fact with no source comes back later as if it
    /// were JJ's own.
    pub said_by: Option<&'a str>,
}

/// Handles one message through a process that is already running.
///
/// The same recording rules and the same ordering, but the conversation is held open between
/// turns. Measured, a fresh process reaches its first token in 2.8 seconds and one that is
/// already running reaches it in 0.97, which in a spoken exchange is most of the wait.
///
/// The identity is not sent here, because the pool set it when it opened the process and a
/// system prompt cannot be changed afterwards. Everything that varies goes in front of the
/// question instead.
pub fn stream_in(
    asking: Asking<'_>,
    on_text: &mut dyn FnMut(Say<'_>) -> Flow,
    // Called while nothing is arriving, so a turn can be given up on before its first word.
    while_waiting: &mut dyn FnMut() -> Flow,
) -> Result<Answer> {
    let Asking {
        pool,
        home,
        thread,
        said,
        sent,
        said_by,
    } = asking;

    Exchange {
        home,
        thread,
        said,
        sent,
        author: said_by.map(str::to_owned),
        // Static instructions belong to the process, which already has them.
        extra: None,
    }
    .run(|p| {
        pool.ask(
            thread,
            &p.session,
            p.resume,
            &p.question_with_context(),
            on_text,
            while_waiting,
        )
    })
}

/// Take a picture of the screen, then ask about it, through a running process.
pub fn look_in(
    asking: Asking<'_>,
    area: Area,
    on_text: &mut dyn FnMut(Say<'_>) -> Flow,
    while_waiting: &mut dyn FnMut() -> Flow,
) -> Result<Answer> {
    // The picture is described to Carl and never recorded, so anything the caller wanted said
    // goes in front of that description rather than in front of the question.
    let shot = shot(asking.home, asking.said, area)?;
    let sent = match asking.sent {
        Some(p) => format!("{p}\n\n---\n\n{shot}"),
        None => shot,
    };

    stream_in(
        Asking {
            sent: Some(&sent),
            ..asking
        },
        on_text,
        while_waiting,
    )
}

/// Take a picture of the screen, then ask about it.
pub fn look(home: &Path, thread: &ThreadId, question: &str, area: Area) -> Result<Answer> {
    let sent = shot(home, question, area)?;
    respond_full(
        &Runner::default(),
        home,
        thread,
        question,
        Some(&sent),
        None,
    )
}

/// Takes the picture and writes the prompt that goes with it.
///
/// The image lands inside Claude Code's working directory, so the prompt can name it by a
/// short relative path and Claude reads it with its own file tools. No image encoding here.
fn shot(home: &Path, question: &str, area: Area) -> Result<String> {
    let workdir = home.join("workspace");
    let shot = workdir.join("screen.png");

    Camera::default()
        .capture(area, &shot)
        .context("could not take a picture of the screen")?;

    let (w, h) = crate::capture::png_size(&shot).unwrap_or((0, 0));

    // Told to look first and answer second. Asking the question first invites an answer from
    // memory before the image is ever opened.
    Ok(format!(
        "Read the image at ./screen.png ({w} by {h}). It is a picture of my screen taken \
just now. Look at it before answering, and answer from what is actually on screen rather \
than from what you expect to be there. If you cannot make something out, say so.\n\n{question}"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Speaker;

    /// The question must be on disk before Claude is ever contacted, so a failure there
    /// cannot take it with it. Proven by pointing at a binary that does not exist.
    #[test]
    fn the_question_survives_a_total_failure_to_answer() {
        let home = tempfile::tempdir().unwrap();
        let thread = ThreadId::new("cli").unwrap();

        let missing = Runner::at("/nonexistent/definitely-not-claude");
        let result = respond_full(
            &missing,
            home.path(),
            &thread,
            "did you record this",
            None,
            None,
        );
        assert!(result.is_err(), "the ask should have failed");

        let entries = crate::log::read(home.path().join("conversations.jsonl")).unwrap();
        assert!(
            entries.iter().any(|e| e.text == "did you record this"),
            "the question must be recorded even when the answer never arrives: {entries:?}"
        );
        assert!(
            entries.iter().any(|e| e.speaker == Speaker::System),
            "the failure should be recorded too: {entries:?}"
        );
    }

    /// The streaming path shares the recording rules rather than reimplementing them, and
    /// this is what proves it did not drift.
    #[test]
    fn the_streaming_path_records_the_question_first_too() {
        let home = tempfile::tempdir().unwrap();
        let thread = ThreadId::new("cli").unwrap();

        let missing = Runner::at("/nonexistent/definitely-not-claude");
        let result = Exchange {
            home: home.path(),
            thread: &thread,
            said: "streamed question",
            sent: None,
            author: None,
            extra: None,
        }
        .run(|p| {
            missing.ask_streaming(
                &Turn {
                    session: &p.session,
                    resume: p.resume,
                    prompt: p.prompt,
                    extra_system: Some(&p.all_in_system()),
                    workdir: &p.workdir,
                },
                &mut |_| Flow::Continue,
            )
        });
        assert!(result.is_err());

        let entries = crate::log::read(home.path().join("conversations.jsonl")).unwrap();
        assert!(
            entries.iter().any(|e| e.text == "streamed question"),
            "{entries:?}"
        );
    }
}

#[cfg(test)]
mod surface_tests {
    use super::*;
    use crate::claude::permits::Surface;

    fn home_with(permissions: &str) -> tempfile::TempDir {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("permissions.json"), permissions).unwrap();
        d
    }

    /// The bug this exists to stop coming back.
    ///
    /// Every surface used to build a `Runner::default()`, so `permissions.json` reached none of
    /// them. JJ had written `python3`, `Write` and `Edit` into the `jj` list and raised the mode,
    /// and Carl went on refusing all three. Nothing reported it, because a refusal looks the same
    /// whether the rule said no or nobody read the rule.
    #[test]
    fn a_surface_runner_carries_what_jj_wrote_down() {
        let d = home_with(
            r#"{
                "jj": {"mode": "acceptEdits", "allow": ["Bash(python3:*)", "Write", "Edit"]},
                "shared": {"mode": "ask", "allow": ["Bash(carl-python:*)"]}
            }"#,
        );

        let session = crate::SessionId::fresh().unwrap();
        let turn = Turn {
            session: &session,
            resume: false,
            prompt: "hi",
            extra_system: None,
            workdir: std::path::Path::new("/tmp"),
        };

        // The jj surface is Carl, and Carl is the chief, so rank empties the list however
        // generous `permissions.json` is. This test used to assert the opposite, because it was
        // written before rank narrowed anything, and it was the reason Carl could write code.
        let jj = runner_for(d.path(), Surface::Jj).unwrap().args_for(&turn);
        for tool in ["Write", "Edit", "Bash(python3:*)"] {
            assert!(
                !jj.contains(&tool.to_string()),
                "the chief was handed {tool}: {jj:?}"
            );
        }
        assert!(
            !jj.contains(&"--allowedTools".to_string()),
            "an empty list must be no flag at all, since some parsers read one as allow \
             everything: {jj:?}"
        );
        assert!(
            jj.windows(2)
                .any(|w| w[0] == "--permission-mode" && w[1] == "acceptEdits"),
            "the mode JJ set did not reach the CLI: {jj:?}"
        );

        // And the narrower surface stays narrow, so this is not just passing everything through.
        let shared = runner_for(d.path(), Surface::Shared)
            .unwrap()
            .args_for(&turn);
        assert!(
            !shared.contains(&"Write".to_string()),
            "Slack was given writing, which is not in its list: {shared:?}"
        );
    }

    /// `Ask` used to mean refuse where nobody saw it. Now it means put it to JJ.
    #[test]
    fn an_asking_surface_installs_the_hook_that_asks() {
        let d = home_with(r#"{"shared": {"mode": "ask", "allow": []}}"#);
        let session = crate::SessionId::fresh().unwrap();
        let args = runner_for(d.path(), Surface::Shared)
            .unwrap()
            .args_for(&Turn {
                session: &session,
                resume: false,
                prompt: "hi",
                extra_system: None,
                workdir: std::path::Path::new("/tmp"),
            });

        let at = args.iter().position(|a| a == "--settings");
        let settings = at.and_then(|i| args.get(i + 1)).expect("a hook: {args:?}");
        assert!(settings.contains("permit-hook"), "{settings}");
        assert!(
            settings.contains("--as slack"),
            "the panel has to be told who is asking: {settings}"
        );
    }
}

#[cfg(test)]
mod rank_tests {
    use crate::army::org::Rank;
    use crate::claude::permits::narrow_to_rank;

    /// The bug JJ reported: Carl was writing code.
    ///
    /// He is the chief. In the chain `tools_for` gives a chief nothing that can change
    /// anything, which is the whole point of having one. Reached through the panel he was built
    /// from `permissions.json` instead and got `Write` and `Edit`, so the same agent had two
    /// sets of powers and the permissive one was the one JJ talked to.
    ///
    /// He may read now, because every agent is told to read Projects/MEMORY before working.
    /// Reading is not doing the work, so the invariant is unchanged in the way that matters.
    #[test]
    fn the_chief_can_never_change_anything_however_he_is_reached() {
        let generous = vec![
            "Bash(python3:*)".to_string(),
            "Write".to_string(),
            "Edit".to_string(),
            "Read".to_string(),
        ];
        let kept = narrow_to_rank(&generous, Rank::Chief);
        for forbidden in ["Write", "Edit", "Bash"] {
            assert!(
                !kept.iter().any(|t| t.contains(forbidden)),
                "the chief was left holding {forbidden}: {kept:?}"
            );
        }
        assert!(
            kept.iter().all(|t| matches!(t.as_str(), "Read" | "Grep")),
            "the chief kept something that is not reading: {kept:?}"
        );
    }

    /// Narrowing has to keep what the rank does allow, or a worker cannot work.
    #[test]
    fn a_worker_keeps_what_jj_wrote_down() {
        let allow = vec![
            "Bash(python3:*)".to_string(),
            "Write".to_string(),
            "Edit".to_string(),
            "Read".to_string(),
        ];
        let kept = narrow_to_rank(&allow, Rank::Worker);
        for wanted in ["Write", "Edit", "Read", "Bash(python3:*)"] {
            assert!(
                kept.contains(&wanted.to_string()),
                "{wanted} was dropped: {kept:?}"
            );
        }
    }

    /// A lead manages and reviews, so it may read and run things but never write them.
    #[test]
    fn a_lead_may_look_but_not_write() {
        let allow = vec!["Read".to_string(), "Write".to_string(), "Edit".to_string()];
        let kept = narrow_to_rank(&allow, Rank::Lead);
        assert!(kept.contains(&"Read".to_string()));
        assert!(
            !kept.contains(&"Write".to_string()),
            "a lead was given Write: {kept:?}"
        );
        assert!(!kept.contains(&"Edit".to_string()));
    }

    /// Rank narrows and never widens: a tool the rank allows but JJ did not grant stays absent.
    #[test]
    fn rank_never_grants_what_jj_withheld() {
        let allow = vec!["Read".to_string()];
        let kept = narrow_to_rank(&allow, Rank::Worker);
        assert_eq!(
            kept,
            vec!["Read".to_string()],
            "narrowing added something: {kept:?}"
        );
    }
}
