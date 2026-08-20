//! Carl answering in Slack.
//!
//! The transport lives in the library. This is the part that knows what an answer is, which
//! is the same `turn` machinery the terminal and the microphone use, so a Slack thread gets
//! the same record, the same memory and the same ordering as everything else.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{Sender, channel};

use anyhow::Result;
use carl::claude::{Pool, Runner};
use carl::slack::{self, Api, Ask, Directory, Pace, Patience, Tokens};
use carl::turn;

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

    // The token comes along because a Slack file url is private and needs it. Without it a
    // download returns a sign in page with a 200, which is the confusing kind of failure.
    let jobs = spawn_worker(
        home.to_path_buf(),
        Api::new(&tokens.bot, &tokens.app),
        tokens.bot.clone(),
    );

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
fn spawn_worker(home: PathBuf, api: Api, bot: String) -> Sender<Ask> {
    let (jobs, rx) = channel::<Ask>();

    std::thread::spawn(move || {
        // Lives in the worker rather than anywhere shared, because it is the only thing that
        // decides whether to answer and it is the last line of defence against two agents
        // talking to each other until somebody notices the bill.
        let mut patience = Patience::default();
        // Looked up once each. Slack never sends a name, only an id, and Carl greeting
        // somebody as U0BNSU5N96X is worse than not greeting them.
        let mut directory = Directory::new();

        // One process per Slack conversation, kept open. The pool closes the least recently
        // used, so a thread nobody has touched in a while pays for its own restart and the
        // ones in active use do not.
        let mut pool = Pool::new(
            Runner::default(),
            home.join("workspace"),
            carl::brief::IDENTITY,
        );

        // Reopened before anything arrives, so a restart does not make the next person to
        // speak wait for a cold process. Which threads is read from the registry, newest
        // first, and never more than the pool would keep anyway.
        match carl::Registry::open(home.join("threads.json")) {
            Ok(registry) => {
                let recent = registry.recent(carl::claude::KEEP_OPEN);
                if !recent.is_empty() {
                    eprintln!("reopening {} recent conversations", recent.len());
                    pool.warm(&recent);
                }
            }
            Err(e) => eprintln!("could not read the thread registry: {e}"),
        }

        for ask in rx {
            let who = if ask.from_agent { "agent" } else { "person" };
            eprintln!("  [{}] {who}: {}", ask.thread, ask.text);

            if !patience.allows(&ask.thread, ask.from_agent) {
                // Said once, in the thread, rather than silently. Silence looks like a crash
                // and the other agent retries, which is the loop again with extra steps.
                eprintln!("     out of agent turns in this thread, staying quiet");
                // Resolved rather than assumed, for the same reason as the reply below: the
                // sender may be a bot whose id cannot be mentioned.
                let (mention, _) = directory.sender(&api, &ask);
                let note = slack::compose(
                    mention.as_deref(),
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

            // Both at once, because a bot sender needs one lookup to answer either and the
            // mention target is not always the user id.
            let (mention, speaker) = directory.sender(&api, &ask);

            // Posted before Claude is even asked. Twenty five seconds of nothing reads as
            // being ignored, and the only way to tell that apart from Carl being broken is
            // for him to say he is on it.
            let looking = carl::needs_screen(&ask.text);
            let holding = slack::placeholder(looking);
            let live = api
                .post_returning(&ask.channel, &ask.thread_ts, holding)
                .map_err(|e| eprintln!("  could not post a placeholder: {e}"))
                .ok();

            let reply = match answer(
                &mut pool,
                &home,
                &ask,
                &speaker,
                live.as_deref(),
                &api,
                &bot,
            ) {
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
                    Some(kind) => {
                        slack::compose(mention.as_deref(), kind, incoming.reply_ttl(), &reply)
                    }
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
fn answer(
    pool: &mut Pool,
    home: &Path,
    ask: &Ask,
    speaker: &str,
    live: Option<&str>,
    api: &Api,
    bot_token: &str,
) -> Result<String> {
    // Carl needs telling that he is in Slack. Without it he reasons about whether he has a
    // Slack connector authorised, decides he has not, and explains that he cannot reply, in
    // a message that is itself posted to Slack.
    let mut context = format!("{}\n\nYou are talking to {speaker}.", slack::CONTEXT);

    // Fetched into the same working directory a screenshot lands in, so Claude reads it with
    // its own file tools and nothing downstream has to learn anything new.
    if !ask.images.is_empty() {
        let workdir = home.join("workspace");
        let mut got = Vec::new();
        for image in &ask.images {
            match slack::fetch(image, bot_token, &workdir) {
                Ok(path) => got.push(
                    path.file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default(),
                ),
                Err(e) => eprintln!("  could not fetch {}: {e}", image.name),
            }
        }
        if !got.is_empty() {
            context.push_str(&format!(
                "\n\nThey attached {} to this message, saved next to you as {}. Read the \
                 image before answering, and answer from what is actually in it rather than \
                 what you expect. If you cannot make something out, say so.",
                if got.len() == 1 {
                    "a picture"
                } else {
                    "pictures"
                },
                got.iter()
                    .map(|n| format!("./{n}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
    }
    if ask.from_agent {
        context.push_str(
            "\n\nThat is another AI agent, not a person. Answer it directly and briefly. Do \
             not be effusive and do not offer further help, because the other agent will \
             answer anything you say and neither of you gets bored. If there is nothing left \
             to settle, say so plainly.",
        );
    }

    // Nothing to rewrite, so there is no reason to pace anything, but the answer still goes
    // through the same held open process.
    let Some(ts) = live else {
        let answer = turn::stream_in(
            turn::Asking {
                pool,
                home,
                thread: &ask.thread,
                said: &ask.text,
                sent: Some(&format!("{context}\n\n---\n\n{}", ask.text)),
                said_by: Some(speaker),
            },
            &mut |_| carl::Flow::Continue,
            &mut || carl::Flow::Continue,
        )?;
        return Ok(answer.text);
    };

    let mut seen = String::new();
    let mut pace = Pace::started_with(0);

    let mut show = |chunk: &str| -> carl::Flow {
        seen.push_str(chunk);
        // Stripped before showing, not after. A half written [remember] line appearing in a
        // channel and then vanishing is worse than never streaming at all.
        let visible = carl::remember::split(&seen).text;
        if pace.should_update(visible.len())
            && let Err(e) = api.update(&ask.channel, ts, &visible)
        {
            // Not fatal, and not retried. The final rewrite carries the whole answer anyway,
            // so a dropped update costs smoothness and nothing else.
            eprintln!("  could not update the message: {e}");
        }
        carl::Flow::Continue
    };

    // The Slack context goes in front of the question rather than in the system prompt,
    // because the process is held open and a system prompt is fixed when it starts.
    let answer = if carl::needs_screen(&ask.text) {
        turn::look_in(
            turn::Asking {
                pool,
                home,
                thread: &ask.thread,
                said: &ask.text,
                sent: Some(&context),
                said_by: Some(speaker),
            },
            carl::Area::Screen,
            &mut show,
            // Nothing interrupts a Slack answer. There is no microphone, and a message
            // arriving mid answer is a new question rather than a change of mind.
            &mut || carl::Flow::Continue,
        )?
    } else {
        turn::stream_in(
            turn::Asking {
                pool,
                home,
                thread: &ask.thread,
                said: &ask.text,
                sent: Some(&format!("{context}\n\n---\n\n{}", ask.text)),
                said_by: Some(speaker),
            },
            &mut show,
            &mut || carl::Flow::Continue,
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

    // Always present here: it came from the command line, where somebody typed it.
    let text = slack::compose(Some(agent_user_id), kind, slack::START_TTL, body);
    let ts = api.announce(channel, &text)?;
    println!("opened an exchange with {agent_user_id} in {channel} at {ts}");
    Ok(())
}
