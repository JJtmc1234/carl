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

use super::{Engaged, named};
use crate::ThreadId;

/// Something Carl should reply to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ask {
    pub thread: ThreadId,
    /// True when another bot sent this, rather than a person.
    ///
    /// Not a reason to ignore it. Talking to Hunter's agent is the point. It is the reason to
    /// count how long it has been since a person was involved.
    pub from_agent: bool,
    pub channel: String,
    /// The thread to reply in. Slack threads a reply when this is the parent's timestamp.
    pub thread_ts: String,
    pub text: String,
    pub user: String,
}

/// Reads one Socket Mode payload. `None` for everything Carl should stay out of.
///
/// `me` is Carl's own user id and `my_bot` his own bot id, both from `auth.test`.
pub fn ask_from(
    payload: &serde_json::Value,
    me: &str,
    my_bot: &str,
    engaged: &Engaged,
) -> Option<Ask> {
    let event = payload.get("event")?;
    let kind = event.get("type")?.as_str()?;

    // A message Carl posted comes back as an event like any other, and answering it is an
    // infinite loop in a channel with people in it. Checked two ways, because a bot message
    // does not always carry a user field and a user message does not carry a bot id.
    let bot = event.get("bot_id").and_then(|b| b.as_str());
    if bot.is_some() && bot == Some(my_bot) {
        return None;
    }
    let user = event
        .get("user")
        .and_then(|u| u.as_str())
        .or(bot)
        .unwrap_or_default();
    if user.is_empty() || user == me {
        return None;
    }

    // Another agent. Heard, deliberately, because talking to Alex is the whole point. What
    // stops that running away is patience.rs and the protocol ttl, not deafness.
    let from_agent = bot.is_some();
    // Edits, deletions, joins, topic changes. Only a plain new message is a question.
    // bot_message is allowed through, because that is how another agent's words arrive.
    if let Some(sub) = event.get("subtype").and_then(|s| s.as_str())
        && !matches!(sub, "file_share" | "bot_message")
    {
        return None;
    }

    let channel = event.get("channel")?.as_str()?;
    let ts = event.get("ts")?.as_str()?;

    let raw = event.get("text")?.as_str()?;
    let channel_type = event
        .get("channel_type")
        .and_then(|c| c.as_str())
        .unwrap_or("");

    let text = match kind {
        // Mentioned in a channel. Unambiguous, answer it.
        "app_mention" => strip_mention(raw, me),
        // A direct message. Every message in a DM is addressed to Carl by definition.
        "message" if channel_type == "im" => strip_mention(raw, me),
        // An ordinary message in a channel Carl is in. Most of these are none of his
        // business, so his name has to be used the way you use a name when you are speaking
        // to somebody rather than about them. named.rs decides, and it is deliberately strict.
        //
        // Except in a thread Carl is already part of. Making somebody use his name on every
        // message of a conversation they already started is not a conversation, it is a run
        // of unrelated questions sitting next to each other.
        "message" if matches!(channel_type, "channel" | "group" | "mpim") => {
            let said = strip_mention(raw, me);
            let in_our_thread = event
                .get("thread_ts")
                .and_then(|t| t.as_str())
                .is_some_and(|t| engaged.contains(t));

            match named::addressed(&said) {
                Some(text) => text,
                None if in_our_thread => said,
                None => return None,
            }
        }
        _ => return None,
    };

    // A bare name is being called rather than being asked, which still deserves an answer.
    // An empty message that never had a name in it does not.
    if text.is_empty() && kind != "message" {
        return None;
    }
    if text.is_empty() && channel_type == "im" {
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
        from_agent,
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
    const MY_BOT: &str = "B0CARL";

    fn nothing() -> Engaged {
        Engaged::new()
    }

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
            MY_BOT,
            &nothing(),
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
                "type": "message", "channel_type": "im", "subtype": "bot_message",
                "bot_id": MY_BOT, "channel": "D01", "ts": "1.1",
                "text": "an answer Carl posted, arriving with no user field at all"
            }),
        ] {
            assert_eq!(ask_from(&payload(own), ME, MY_BOT, &nothing()), None);
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
            MY_BOT,
            &nothing(),
        )
        .expect("a dm is a question");
        assert_eq!(ask.text, "hello Carl");
    }

    /// A message in a channel that has nothing to do with Carl is left alone.
    #[test]
    fn ordinary_channel_chatter_is_left_alone() {
        for text in [
            "morning everyone",
            "I asked Carl yesterday and he said no",
            "Carl's memory design is the good bit",
        ] {
            assert_eq!(
                ask_from(
                    &payload(serde_json::json!({
                        "type": "message", "channel_type": "channel", "user": "U0JJ",
                        "channel": "C01", "ts": "1.1", "text": text
                    })),
                    ME,
                    MY_BOT,
                    &nothing()
                ),
                None,
                "should have stayed out of: {text}"
            );
        }
    }

    /// Using his name the way you use a name when speaking to somebody works without an at
    /// sign, which is the point.
    #[test]
    fn his_name_in_a_channel_is_enough() {
        let ask = ask_from(
            &payload(serde_json::json!({
                "type": "message", "channel_type": "channel", "user": "U0JJ",
                "channel": "C01", "ts": "1700000000.000400",
                "text": "carl what should I research next"
            })),
            ME,
            MY_BOT,
            &nothing(),
        )
        .expect("his own name should reach him");
        assert_eq!(ask.text, "what should I research next");
        assert!(!ask.from_agent);
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
                    ME,
                    MY_BOT,
                    &nothing()
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
            MY_BOT,
            &nothing(),
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
                ME,
                MY_BOT,
                &nothing()
            ),
            None
        );
    }

    /// Another agent is heard, deliberately. Talking to Hunter's Alex is the point, and it
    /// is the one case the old blanket "ignore every bot" rule got wrong.
    #[test]
    fn another_agent_is_heard_and_marked_as_an_agent() {
        let ask = ask_from(
            &payload(serde_json::json!({
                "type": "app_mention", "subtype": "bot_message", "bot_id": "B0ALEX",
                "channel": "C01", "ts": "1700000000.000300",
                "text": "<@U0CARL> [a2a/1 hello ttl=6]"
            })),
            ME,
            MY_BOT,
            &nothing(),
        )
        .expect("Alex must be heard");
        assert!(ask.from_agent, "and must be known to be an agent");
    }

    /// A person must never be counted as an agent, or the loop guard would throttle JJ.
    #[test]
    fn a_person_is_not_an_agent() {
        let ask = ask_from(
            &payload(serde_json::json!({
                "type": "message", "channel_type": "im", "user": "U0JJ",
                "channel": "D01", "ts": "1.1", "text": "hello"
            })),
            ME,
            MY_BOT,
            &nothing(),
        )
        .unwrap();
        assert!(!ask.from_agent);
    }

    /// What JJ hit. He said "carl how are you", got an answer, then asked a follow up in the
    /// same thread without the name and was ignored.
    #[test]
    fn a_follow_up_in_carls_own_thread_needs_no_name() {
        let mut engaged = Engaged::new();
        engaged.join("1700000000.000100");

        let event = serde_json::json!({
            "type": "message", "channel_type": "channel", "user": "U0JJ",
            "channel": "C01", "ts": "1700000009.000900",
            "thread_ts": "1700000000.000100",
            "text": "and what about coal"
        });

        assert_eq!(
            ask_from(&payload(event.clone()), ME, MY_BOT, &nothing()),
            None,
            "without the thread being his, this is somebody else's conversation"
        );

        let ask = ask_from(&payload(event), ME, MY_BOT, &engaged)
            .expect("in his own thread it is for him");
        assert_eq!(ask.text, "and what about coal");
    }

    /// Being in one thread must not make Carl answer everything in the channel.
    #[test]
    fn being_in_a_thread_does_not_open_the_whole_channel() {
        let mut engaged = Engaged::new();
        engaged.join("1700000000.000100");

        assert_eq!(
            ask_from(
                &payload(serde_json::json!({
                    "type": "message", "channel_type": "channel", "user": "U0JJ",
                    "channel": "C01", "ts": "1700000050.000000",
                    "text": "morning everyone"
                })),
                ME,
                MY_BOT,
                &engaged
            ),
            None,
            "a top level message is not part of his thread"
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
