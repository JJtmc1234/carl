//! Deciding what Carl should answer, and what he must ignore.
//!
//! Pure. A JSON payload in, a decision out, no network and no Claude, so every rule about
//! what gets a reply is testable without a workspace.
//!
//! The rule that matters most is that Carl must never answer himself. He posts into the same
//! channel he listens to, so his own message comes straight back as an event. Left alone that
//! is an infinite loop that costs real money and floods a real channel other people are in.
//! It is the same shape as the microphone hearing the speakers, and it is worse here, because
//! the room was only Carl talking to himself and this one has an audience.

use crate::ThreadId;

/// Something Carl should reply to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ask {
    pub thread: ThreadId,
    pub channel: String,
    /// The thread to reply in. Slack threads a reply when this is the parent's timestamp.
    pub thread_ts: String,
    pub text: String,
    pub user: String,
}

/// Reads one Socket Mode payload. `None` for everything Carl should stay out of.
///
/// `me` is Carl's own user id from `auth.test`.
pub fn ask_from(payload: &serde_json::Value, me: &str) -> Option<Ask> {
    let event = payload.get("event")?;
    let kind = event.get("type")?.as_str()?;

    // A message Carl posted comes back as an event like any other. Three separate ways to
    // spot it, because any one of them missing is an infinite loop in a channel with people
    // in it.
    if event.get("bot_id").is_some() {
        return None;
    }
    let user = event.get("user")?.as_str()?;
    if user == me {
        return None;
    }
    // Edits, deletions, joins, topic changes. Only a plain new message is a question.
    if let Some(sub) = event.get("subtype").and_then(|s| s.as_str())
        && sub != "file_share"
    {
        return None;
    }

    let channel = event.get("channel")?.as_str()?;
    let ts = event.get("ts")?.as_str()?;

    match kind {
        // Mentioned in a channel. Answer it.
        "app_mention" => {}
        // A direct message. Every message in a DM is addressed to Carl by definition.
        "message" if event.get("channel_type").and_then(|c| c.as_str()) == Some("im") => {}
        _ => return None,
    }

    let raw = event.get("text")?.as_str()?;
    let text = strip_mention(raw, me);
    if text.is_empty() {
        return None;
    }

    // Replies land in the thread they were asked in, and a question asked at top level starts
    // one. Carrying the same thread id means Carl remembers the rest of that conversation and
    // nothing from any other.
    let thread_ts = event
        .get("thread_ts")
        .and_then(|t| t.as_str())
        .unwrap_or(ts)
        .to_string();

    Some(Ask {
        thread: ThreadId::slack(channel, &thread_ts).ok()?,
        channel: channel.to_string(),
        thread_ts,
        text,
        user: user.to_string(),
    })
}

/// Removes the `<@U123>` that mentioning Carl puts in the text.
///
/// Left in, it reaches Claude as a raw id, which is noise at best and at worst gets answered
/// as though it were part of the question.
pub fn strip_mention(text: &str, me: &str) -> String {
    text.replace(&format!("<@{me}>"), " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    const ME: &str = "U0CARL";

    fn payload(event: serde_json::Value) -> serde_json::Value {
        serde_json::json!({ "event": event })
    }

    #[test]
    fn a_mention_in_a_channel_is_a_question() {
        let ask = ask_from(
            &payload(serde_json::json!({
                "type": "app_mention",
                "user": "U0JJ",
                "channel": "C01",
                "ts": "1700000000.000100",
                "text": "<@U0CARL> what should I research next"
            })),
            ME,
        )
        .expect("should be a question");

        assert_eq!(ask.text, "what should I research next");
        assert_eq!(ask.thread_ts, "1700000000.000100");
        assert_eq!(ask.thread.as_str(), "slack-C01-1700000000.000100");
    }

    /// The one that would cost money and flood a channel. Carl posts into the room he is
    /// listening to, so his own message arrives back as an event.
    #[test]
    fn carl_never_answers_himself() {
        for own in [
            serde_json::json!({
                "type": "message", "channel_type": "im", "user": ME,
                "channel": "D01", "ts": "1.1", "text": "an answer Carl posted"
            }),
            serde_json::json!({
                "type": "message", "channel_type": "im", "bot_id": "B01", "user": "U0X",
                "channel": "D01", "ts": "1.1", "text": "posted as a bot"
            }),
        ] {
            assert_eq!(ask_from(&payload(own), ME), None);
        }
    }

    /// Every message in a direct message is addressed to Carl, so no mention is needed.
    #[test]
    fn a_direct_message_needs_no_mention() {
        let ask = ask_from(
            &payload(serde_json::json!({
                "type": "message", "channel_type": "im", "user": "U0JJ",
                "channel": "D01", "ts": "1700000000.000200", "text": "hello Carl"
            })),
            ME,
        )
        .expect("a dm is a question");
        assert_eq!(ask.text, "hello Carl");
    }

    /// A message in a channel Carl happens to be in, with no mention, is not for him.
    #[test]
    fn ordinary_channel_chatter_is_left_alone() {
        assert_eq!(
            ask_from(
                &payload(serde_json::json!({
                    "type": "message", "channel_type": "channel", "user": "U0JJ",
                    "channel": "C01", "ts": "1.1", "text": "morning everyone"
                })),
                ME
            ),
            None
        );
    }

    /// An edit arrives as a message with a subtype. Answering it would reply twice to one
    /// question, once to the original and once to the correction.
    #[test]
    fn edits_and_joins_are_not_questions() {
        for sub in ["message_changed", "message_deleted", "channel_join"] {
            assert_eq!(
                ask_from(
                    &payload(serde_json::json!({
                        "type": "message", "channel_type": "im", "user": "U0JJ",
                        "subtype": sub, "channel": "D01", "ts": "1.1", "text": "x"
                    })),
                    ME
                ),
                None,
                "{sub} should be ignored"
            );
        }
    }

    /// A reply inside a thread stays in that thread, so Carl remembers that conversation and
    /// nothing from any other one in the same channel.
    #[test]
    fn a_threaded_reply_keeps_its_own_thread() {
        let ask = ask_from(
            &payload(serde_json::json!({
                "type": "app_mention", "user": "U0JJ", "channel": "C01",
                "ts": "1700000009.000900", "thread_ts": "1700000000.000100",
                "text": "<@U0CARL> and what after that"
            })),
            ME,
        )
        .unwrap();
        assert_eq!(ask.thread_ts, "1700000000.000100", "must join the parent");
        assert_eq!(ask.thread.as_str(), "slack-C01-1700000000.000100");
    }

    /// A bare mention with nothing after it is not a question.
    #[test]
    fn a_mention_with_no_words_is_not_a_question() {
        assert_eq!(
            ask_from(
                &payload(serde_json::json!({
                    "type": "app_mention", "user": "U0JJ", "channel": "C01",
                    "ts": "1.1", "text": "<@U0CARL>"
                })),
                ME
            ),
            None
        );
    }

    #[test]
    fn the_mention_is_taken_out_of_the_question() {
        assert_eq!(
            strip_mention("hey <@U0CARL> what is  this", ME),
            "hey what is this"
        );
    }
}
