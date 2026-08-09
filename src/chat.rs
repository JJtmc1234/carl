//! Carl answering in Slack.
//!
//! The transport lives in the library. This is the part that knows what an answer is, which
//! is the same `turn` machinery the terminal and the microphone use, so a Slack thread gets
//! the same record, the same memory and the same ordering as everything else.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{Sender, channel};

use anyhow::Result;
use carl::slack::{self, Api, Ask, Directory, Pace, Patience, Tokens};

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

    carl::slack::serve(&api, &me, &mut |ask| {
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
        // Lives in the worker rather than anywhere shared, because it is the only thing that
        // decides whether to answer and it is the last line of defence against two agents
        // talking to each other until somebody notices the bill.
        let mut patience = Patience::default();
        // Looked up once each. Slack never sends a name, only an id, and Carl greeting
        // somebody as U0BNSU5N96X is worse than not greeting them.
        let mut directory = Directory::new();

        for ask in rx {
            let who = if ask.from_agent { "agent" } else { "person" };
            eprintln!("  [{}] {who}: {}", ask.thread, ask.text);

            if !patience.allows(&ask.thread, ask.from_agent) {
                // Said once, in the thread, rather than silently. Silence looks like a crash
                // and the other agent retries, which is the loop again with extra steps.
                eprintln!("     out of agent turns in this thread, staying quiet");
                let note = slack::compose(
                    &ask.user,
                    slack::Kind::Done,
                    0,
                    "Out of turns for this exchange. A person can start it again.",
                );
                let _ = api.post(&ask.channel, &ask.thread_ts, &note);
                continue;
            }

            // Being called by name is not being asked anything. Answered here rather than by
            // Claude, because five seconds and a model call to say "Yes?" is absurd, and
            // because an empty question is what broke this the first time.
            if ask.text.trim().is_empty() {
                let _ = api.post(&ask.channel, &ask.thread_ts, "Yes?");
                continue;
            }

            let speaker = directory.name_of(&api, &ask.user);

            // Posted before Claude is even asked. Twenty five seconds of nothing reads as
            // being ignored, and the only way to tell that apart from Carl being broken is
            // for him to say he is on it.
            let looking = carl::needs_screen(&ask.text);
            let holding = slack::placeholder(looking);
            let live = api
                .post_returning(&ask.channel, &ask.thread_ts, holding)
                .map_err(|e| eprintln!("  could not post a placeholder: {e}"))
                .ok();

            let reply = match answer(&home, &ask, &speaker, live.as_deref(), &api) {
                Ok(text) if !text.trim().is_empty() => text,
                Ok(_) => "I had nothing to say to that, which is probably a bug.".to_string(),
                // Said out loud rather than only logged. Somebody is waiting in a channel,
                // and silence is indistinguishable from Carl ignoring them.
                Err(e) => {
                    eprintln!("  failed: {e:#}");
                    format!("Sorry, that went wrong on my end. {e}")
                }
            };

            // An agent gets a protocol header back. A person gets plain text, because a
            // person reading a channel does not want to see the wiring.
            let out = match slack::parse(&ask.text) {
                Some(incoming) => match incoming.reply_kind() {
                    Some(kind) => slack::compose(&ask.user, kind, incoming.reply_ttl(), &reply),
                    // done and decline are endings. Answering them is how good manners
                    // become an infinite exchange.
                    None => {
                        eprintln!("     that was an ending, so nothing to send back");
                        continue;
                    }
                },
                None => reply,
            };

            // The placeholder becomes the answer. A separate message would leave the
            // thinking line sitting above every reply forever.
            let posted = match &live {
                Some(ts) => api.update(&ask.channel, ts, &out),
                None => api.post(&ask.channel, &ask.thread_ts, &out),
            };
            if let Err(e) = posted {
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
fn answer(home: &Path, ask: &Ask, speaker: &str, live: Option<&str>, api: &Api) -> Result<String> {
    // Carl needs telling that he is in Slack. Without it he reasons about whether he has a
    // Slack connector authorised, decides he has not, and explains that he cannot reply, in
    // a message that is itself posted to Slack.
    let mut context = format!("{}\n\nYou are talking to {speaker}.", slack::CONTEXT);
    if ask.from_agent {
        context.push_str(
            "\n\nThat is another AI agent, not a person. Answer it directly and briefly. Do \
             not be effusive and do not offer further help, because the other agent will \
             answer anything you say and neither of you gets bored. If there is nothing left \
             to settle, say so plainly.",
        );
    }

    // Nothing to rewrite, so there is no reason to stream.
    let Some(ts) = live else {
        let answer = turn::respond_extra(
            home,
            &ask.thread,
            &ask.text,
            Some(ask.user.clone()),
            Some(&context),
        )?;
        return Ok(answer.text);
    };

    let mut seen = String::new();
    let mut pace = Pace::started_with(0);

    let mut show = |chunk: &str| -> carl::Flow {
        seen.push_str(chunk);
        // Stripped before showing, not after. A half written [remember] line appearing in a
        // channel and then vanishing is worse than never streaming at all.
        let (visible, _) = carl::remember::split(&seen);
        if pace.should_update(visible.len())
            && let Err(e) = api.update(&ask.channel, ts, &visible)
        {
            // Not fatal, and not retried. The final rewrite carries the whole answer anyway,
            // so a dropped update costs smoothness and nothing else.
            eprintln!("  could not update the message: {e}");
        }
        carl::Flow::Continue
    };

    let answer = if carl::needs_screen(&ask.text) {
        turn::look_streaming(
            home,
            &ask.thread,
            &ask.text,
            carl::Area::Screen,
            Some(&context),
            &mut show,
        )?
    } else {
        turn::stream(
            home,
            &ask.thread,
            &ask.text,
            None,
            Some(&context),
            &mut show,
        )?
    };
    Ok(answer.text)
}

/// Says something in a channel without being asked.
pub fn say(home: &Path, channel: &str, message: &str) -> Result<()> {
    let tokens = Tokens::load(home)?;
    let api = Api::new(&tokens.bot, &tokens.app);
    let ts = api.announce(channel, message)?;
    println!("posted to {channel} at {ts}");
    Ok(())
}

/// Opens an A2A exchange with another agent.
pub fn greet(home: &Path, channel: &str, agent_user_id: &str, message: &str) -> Result<()> {
    let tokens = Tokens::load(home)?;
    let api = Api::new(&tokens.bot, &tokens.app);

    let kind = if message.trim().is_empty() {
        slack::Kind::Hello
    } else {
        slack::Kind::Ask
    };
    let body = if message.trim().is_empty() {
        "I am Carl, JJ's assistant. Rust, driving the claude command line. I speak a2a/1, \
         see docs/a2a.md."
    } else {
        message
    };

    let text = slack::compose(agent_user_id, kind, slack::START_TTL, body);
    let ts = api.announce(channel, &text)?;
    println!("opened an exchange with {agent_user_id} in {channel} at {ts}");
    Ok(())
}
