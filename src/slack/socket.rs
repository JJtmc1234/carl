//! The websocket half of Socket Mode.
//!
//! Dial, read envelopes, acknowledge each one straight away, reconnect when told to.
//!
//! The acknowledgement is the part with a trap in it. Slack wants one within three seconds
//! and resends the event if it does not get one. Claude takes five seconds at best, so
//! answering before acknowledging means Slack decides the event was dropped and sends it
//! again, and Carl answers the same question two or three times. That does not show up in
//! testing with one message and is extremely obvious in a channel with people in it.

use std::collections::VecDeque;
use std::time::Duration;

use tungstenite::Message;

use super::{Api, Ask, Engaged, Me, ask_from};
use crate::Result;

/// How many recent events to remember for spotting a resend.
///
/// Slack resends on any doubt, including its own timeouts, and the copy carries the same
/// `event_id`. Small because a resend follows within seconds if it comes at all.
const REMEMBERED: usize = 64;

/// Reads envelopes until the process is killed, handing questions to `on_ask`.
pub fn serve(api: &Api, me: &Me, on_ask: &mut dyn FnMut(Ask)) -> Result<()> {
    let mut seen = Recent::default();
    // Messages already answered, keyed by identity rather than by envelope.
    let mut answered = Recent::default();
    // Threads Carl has answered in. Kept here rather than in the worker because it has to be
    // updated in step with the decision to answer, and the worker is deliberately behind a
    // queue that can be several messages long.
    let mut engaged = Engaged::new();
    let mut backoff = Duration::from_secs(1);

    loop {
        // Fetched every time. The url is single use and expires within seconds, so a cached
        // one is only ever good for the reconnect that does not need it.
        let url = match api.open_socket() {
            Ok(u) => u,
            Err(e) => {
                eprintln!("cannot open a slack socket: {e}. Retrying in {backoff:?}");
                std::thread::sleep(backoff);
                backoff = (backoff * 2).min(Duration::from_secs(60));
                continue;
            }
        };

        let mut ws = match tungstenite::connect(&url) {
            Ok((ws, _)) => ws,
            Err(e) => {
                eprintln!("cannot dial slack: {e}. Retrying in {backoff:?}");
                std::thread::sleep(backoff);
                backoff = (backoff * 2).min(Duration::from_secs(60));
                continue;
            }
        };

        eprintln!("connected to slack.");
        backoff = Duration::from_secs(1);

        loop {
            let text = match ws.read() {
                Ok(Message::Text(t)) => t.to_string(),
                // Ping and pong are answered by the library. Close means go round again.
                Ok(Message::Ping(_) | Message::Pong(_) | Message::Frame(_)) => continue,
                Ok(Message::Binary(_)) => continue,
                Ok(Message::Close(_)) => break,
                Err(e) => {
                    eprintln!("slack connection dropped: {e}");
                    break;
                }
            };

            let Ok(frame) = serde_json::from_str::<serde_json::Value>(&text) else {
                continue;
            };

            match frame.get("type").and_then(|t| t.as_str()) {
                Some("hello") => continue,
                // Slack cycles connections on purpose, roughly hourly, and before deploys.
                // It is routine rather than a fault.
                Some("disconnect") => {
                    eprintln!("slack asked us to reconnect.");
                    break;
                }
                _ => {}
            }

            // Before anything else, including deciding whether it is even a question. An
            // envelope Carl ignores still has to be acknowledged or Slack keeps sending it.
            if let Some(id) = frame.get("envelope_id").and_then(|i| i.as_str())
                && let Err(e) = ws.send(Message::text(format!("{{\"envelope_id\":\"{id}\"}}")))
            {
                eprintln!("could not acknowledge an envelope: {e}");
                break;
            }

            let Some(payload) = frame.get("payload") else {
                continue;
            };

            // Printed for every envelope, not behind a flag. When nothing is happening you
            // need to know whether Slack is sending nothing or sending something Carl is
            // filtering out, and those two have completely different fixes. Same reason the
            // ear prints every transcript.
            eprintln!(
                "  <- {} / {}",
                frame.get("type").and_then(|t| t.as_str()).unwrap_or("?"),
                payload
                    .get("event")
                    .and_then(|e| e.get("type"))
                    .and_then(|t| t.as_str())
                    .unwrap_or("no event")
            );

            // A resend carries the same event id. Without this, a slow moment on Slack's side
            // turns into Carl answering the same question twice.
            if let Some(id) = payload.get("event_id").and_then(|i| i.as_str())
                && seen.seen_before(id)
            {
                eprintln!("  ignoring a resend of {id}");
                continue;
            }

            match ask_from(payload, &me.user_id, &me.bot_id, &engaged) {
                Some(ask) => {
                    // The event id is the envelope, not the message. The manifest subscribes
                    // to `app_mention` and to `message.*`, so one message that mentions Carl
                    // arrives twice with two different event ids, and both pass the check
                    // above. Two of those pairs qualify on both copies: a direct message,
                    // where `channel_type == "im"` always qualifies, and a mention inside a
                    // thread Carl is already engaged in. Both produce an identical `Ask`, so
                    // Carl asked Claude twice and posted two replies. See bug 17.
                    //
                    // Keyed on channel plus the message `ts`, which is the message's own
                    // identity, and not on `thread_ts`, which every message in a thread
                    // shares.
                    //
                    // Recorded here rather than beside the event id check, so a copy that is
                    // not a question for Carl cannot use up the entry and silence the copy
                    // that is.
                    if let Some(key) = message_key(payload)
                        && answered.seen_before(&key)
                    {
                        eprintln!("  already answered {key}, so ignoring the second copy");
                        continue;
                    }

                    engaged.join(&ask.thread_ts);
                    on_ask(ask);
                }
                // Says why rather than going quiet. "Carl ignored me" and "Carl never heard
                // me" look identical from the outside and are not the same problem.
                None => eprintln!("     ignored, not a question addressed to Carl"),
            }
        }
    }
}

/// A bounded set of recent identifiers, oldest forgotten first.
///
/// Its own type because there are now two of these, one for envelopes and one for messages,
/// and the push then trim dance was already written out twice before the second one existed.
#[derive(Default)]
struct Recent(VecDeque<String>);

impl Recent {
    /// Records `id`, reporting whether it had already been recorded.
    fn seen_before(&mut self, id: &str) -> bool {
        if self.0.iter().any(|k| k == id) {
            return true;
        }
        self.0.push_back(id.to_string());
        if self.0.len() > REMEMBERED {
            self.0.pop_front();
        }
        false
    }
}

/// A message's own identity, which is its channel and its `ts`.
///
/// The same pair `ThreadId::slack` uses, and deliberately `ts` rather than `thread_ts`, since
/// every message in a thread shares the latter and deduping on it would answer only the first
/// question anybody asked in a thread.
fn message_key(payload: &serde_json::Value) -> Option<String> {
    let event = payload.get("event")?;
    let channel = event.get("channel")?.as_str()?;
    let ts = event.get("ts")?.as_str()?;
    Some(format!("{channel}/{ts}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::slack::event::ask_from;

    const ME: &str = "U0CARL";
    const MY_BOT: &str = "B0CARL";

    /// The two envelopes Slack sends for one direct message mentioning Carl. Different
    /// `event_id`, different `type`, same message.
    fn dm_pair() -> (serde_json::Value, serde_json::Value) {
        let mention = serde_json::json!({
            "event_id": "Ev111",
            "event": {
                "type": "app_mention", "user": "U0JJ", "channel": "D01",
                "ts": "1700000000.000100", "text": "<@U0CARL> what should I research next"
            }
        });
        let message = serde_json::json!({
            "event_id": "Ev222",
            "event": {
                "type": "message", "channel_type": "im", "user": "U0JJ", "channel": "D01",
                "ts": "1700000000.000100", "text": "<@U0CARL> what should I research next"
            }
        });
        (mention, message)
    }

    /// The bug. The manifest subscribes to `app_mention` and to `message.*`, so one message
    /// arrives twice with two different event ids. The dedupe keyed on `event_id`, which is
    /// the envelope rather than the message, so both copies survived and Carl answered twice.
    ///
    /// Both halves are asserted. That both copies are genuinely questions is what makes this a
    /// bug rather than a theory, and that the event ids differ is why the old check missed it.
    #[test]
    fn one_message_arriving_twice_is_answered_once() {
        let (mention, message) = dm_pair();
        let engaged = crate::slack::Engaged::new();

        assert!(
            ask_from(&mention, ME, MY_BOT, &engaged).is_some()
                && ask_from(&message, ME, MY_BOT, &engaged).is_some(),
            "both copies have to qualify, or this test is not reproducing the bug"
        );

        let mut seen = Recent::default();
        assert!(!seen.seen_before(mention["event_id"].as_str().unwrap()));
        assert!(
            !seen.seen_before(message["event_id"].as_str().unwrap()),
            "the event ids differ, which is exactly why the old check let both through"
        );

        let mut answered = Recent::default();
        let first = message_key(&mention).expect("a key");
        let second = message_key(&message).expect("a key");
        assert_eq!(first, second, "the same message must give the same key");
        assert!(!answered.seen_before(&first));
        assert!(
            answered.seen_before(&second),
            "the second copy of one message has to be recognised"
        );
    }

    /// Two different messages in one thread share `thread_ts` and differ in `ts`. Keying on
    /// the thread would answer the first question in a thread and silently ignore every
    /// question after it, which is worse than the bug being fixed.
    #[test]
    fn two_messages_in_one_thread_are_both_answered() {
        let one = serde_json::json!({
            "event": { "type": "message", "channel": "C01",
                       "ts": "1700000000.000100", "thread_ts": "1700000000.000001" }
        });
        let two = serde_json::json!({
            "event": { "type": "message", "channel": "C01",
                       "ts": "1700000000.000200", "thread_ts": "1700000000.000001" }
        });

        let mut answered = Recent::default();
        assert!(!answered.seen_before(&message_key(&one).unwrap()));
        assert!(
            !answered.seen_before(&message_key(&two).unwrap()),
            "a second question in the same thread is a different message"
        );
    }

    /// The same `ts` in different channels is not the same message.
    #[test]
    fn the_channel_is_part_of_the_identity() {
        let here = serde_json::json!({
            "event": { "channel": "C01", "ts": "1700000000.000100" }
        });
        let there = serde_json::json!({
            "event": { "channel": "C02", "ts": "1700000000.000100" }
        });

        let mut answered = Recent::default();
        assert!(!answered.seen_before(&message_key(&here).unwrap()));
        assert!(!answered.seen_before(&message_key(&there).unwrap()));
    }

    /// Bounded, or a long lived process remembers every message it has ever seen.
    #[test]
    fn only_the_recent_past_is_remembered() {
        let mut recent = Recent::default();
        for i in 0..=REMEMBERED {
            assert!(!recent.seen_before(&format!("id{i}")));
        }
        assert_eq!(recent.0.len(), REMEMBERED);
        assert!(
            !recent.seen_before("id0"),
            "the oldest has to have been forgotten"
        );
    }
}
