//! Carl answering in Slack.
//!
//! The transport lives in the library. This is the part that knows what an answer is, which
//! is the same `turn` machinery the terminal and the microphone use, so a Slack thread gets
//! the same record, the same memory and the same ordering as everything else.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{Sender, channel};

use anyhow::Result;
use carl::slack::{Api, Ask, Tokens};

use crate::turn;

/// Runs until interrupted.
pub fn run(home: &Path) -> Result<()> {
    let tokens = Tokens::load(home)?;
    let api = Api::new(&tokens.bot, &tokens.app);

    // Fatal on purpose. Without knowing his own id Carl cannot tell his own messages from
    // anybody else's, and the failure is answering himself forever in a channel with people
    // in it. Guessing is not an option that exists here.
    let me = api.whoami()?;
    eprintln!("carl is {} in team {}", me.user_id, me.team);
    eprintln!("mention @Carl in a channel he is in, or send him a direct message.");

    let jobs = spawn_worker(home.to_path_buf(), Api::new(&tokens.bot, &tokens.app));

    carl::slack::serve(&api, &me.user_id, &mut |ask| {
        // Never blocks. The socket has to keep reading, both to acknowledge the next envelope
        // inside Slack's three second window and to answer its pings.
        if let Err(e) = jobs.send(ask) {
            eprintln!("the worker is gone: {e}");
        }
    })?;
    Ok(())
}

/// Answers questions one at a time, off the socket thread.
///
/// One worker rather than a thread per message, on purpose. Two answers at once would both be
/// writing the thread registry, and two in the same Slack thread would interleave. Slack
/// holds the queue meanwhile, which is what the queue is for.
fn spawn_worker(home: PathBuf, api: Api) -> Sender<Ask> {
    let (jobs, rx) = channel::<Ask>();

    std::thread::spawn(move || {
        for ask in rx {
            eprintln!("  [{}] {}", ask.thread, ask.text);

            let reply = match answer(&home, &ask) {
                Ok(text) if !text.trim().is_empty() => text,
                Ok(_) => "I had nothing to say to that, which is probably a bug.".to_string(),
                // Posted rather than only logged. Somebody is waiting in a channel, and
                // silence is indistinguishable from Carl ignoring them.
                Err(e) => {
                    eprintln!("  failed: {e:#}");
                    format!("Sorry, that went wrong on my end. {e}")
                }
            };

            if let Err(e) = api.post(&ask.channel, &ask.thread_ts, &reply) {
                eprintln!("  could not post the reply: {e}");
            }
        }
    });

    jobs
}

/// One Slack question, through the same path as everything else.
///
/// No spoken brief. Slack is read, not heard, and the two sentence rule that makes a good
/// spoken answer makes a uselessly thin written one.
fn answer(home: &Path, ask: &Ask) -> Result<String> {
    let answer = turn::respond(home, &ask.thread, &ask.text, Some(ask.user.clone()))?;
    Ok(answer.text)
}
