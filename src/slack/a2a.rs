//! A2A, a protocol for two agents to talk in a channel people are reading.
//!
//! The full specification is in `docs/a2a.md`. This is the implementation of it.
//!
//! One line at the top of an ordinary Slack message:
//!
//! ```text
//! <@U0ALEX> [a2a/1 ask ttl=5]
//! How do you handle memory between conversations?
//! ```
//!
//! Deliberately tiny, because Slack already carries most of what a protocol would normally
//! have to invent. Who sent it is the message author. Who it is for is the mention. Which
//! conversation it belongs to is the thread. Inventing fields for those would mean two
//! sources of truth that can disagree, and the Slack one would win anyway.
//!
//! So only two things are left that Slack cannot express.
//!
//! `kind` is what the other agent is supposed to do with it. Without it, "thanks, that
//! helps" and "and what about X" look identical, and an agent answers both, forever.
//!
//! `ttl` is how many more hops this exchange may take. Every reply decrements it. At zero the
//! only legal move is to stop. This is the part that makes the protocol worth having, because
//! neither agent gets bored, neither runs out of things to say, and every turn costs money.
//!
//! The body underneath is ordinary prose on purpose. People are reading this channel, and a
//! protocol they cannot follow is a protocol that gets switched off.

use std::fmt;

/// What the other agent should do with a message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Are you there, and what do you speak. The body says who you are.
    Hello,
    /// A question expecting an answer.
    Ask,
    /// An answer to one.
    Reply,
    /// Nothing further needed. Never answered, which is what makes it an ending.
    Done,
    /// Not going to answer this, and the body says why.
    Decline,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Hello => "hello",
            Self::Ask => "ask",
            Self::Reply => "reply",
            Self::Done => "done",
            Self::Decline => "decline",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "hello" => Self::Hello,
            "ask" => Self::Ask,
            "reply" => Self::Reply,
            "done" => Self::Done,
            "decline" => Self::Decline,
            _ => return None,
        })
    }

    /// Whether receiving this obliges an answer.
    ///
    /// `done` and `decline` are terminal. Answering them is how a polite exchange becomes an
    /// infinite one: "thanks", "you are welcome", "no problem", forever.
    pub fn wants_an_answer(self) -> bool {
        matches!(self, Self::Hello | Self::Ask)
    }
}

impl fmt::Display for Kind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The protocol version this speaks. Sent on everything and checked on everything.
pub const VERSION: u32 = 1;

/// How many hops a fresh exchange gets.
pub const START_TTL: u32 = 6;

/// One parsed A2A message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    pub kind: Kind,
    /// Hops remaining. Zero means the next message may only be `done`.
    pub ttl: u32,
    /// Everything after the header line. Ordinary prose.
    pub body: String,
}

impl Message {
    /// What to send back, or `None` when the exchange is over.
    ///
    /// Returning `None` is the important half. An agent that always has something to add is
    /// exactly the failure this protocol exists to prevent.
    pub fn reply_kind(&self) -> Option<Kind> {
        if !self.kind.wants_an_answer() {
            return None;
        }
        if self.ttl == 0 {
            // Out of budget. Saying so is better than going silent, because silence looks
            // like a crash and invites a retry.
            return Some(Kind::Done);
        }
        Some(match self.kind {
            Kind::Hello => Kind::Hello,
            _ => Kind::Reply,
        })
    }

    /// The ttl to put on the reply. Always one less, never below zero.
    pub fn reply_ttl(&self) -> u32 {
        self.ttl.saturating_sub(1)
    }
}

/// Builds the header line that goes at the top of a message.
pub fn header(kind: Kind, ttl: u32) -> String {
    format!("[a2a/{VERSION} {kind} ttl={ttl}]")
}

/// Builds a whole message body, mention included.
pub fn compose(to_user_id: Option<&str>, kind: Kind, ttl: u32, body: &str) -> String {
    match to_user_id {
        Some(id) => format!("<@{id}> {}\n{}", header(kind, ttl), body.trim()),
        // No mention at all, rather than one Slack cannot resolve. A bot id put here renders
        // as the literal text `<@B0ALEX>` and notifies nobody, which is worse than nothing,
        // because it looks addressed and is not. The header still carries the protocol, so
        // an agent reading the thread can still act on it. See bug 21.
        None => format!("{}\n{}", header(kind, ttl), body.trim()),
    }
}

/// Reads a message. `None` means it is not A2A, which is the normal case for a person.
///
/// Anything unparseable is not A2A rather than a broken A2A message. A person writing
/// something with square brackets in it must never be mistaken for an agent, and an agent
/// speaking a future version must be ignored rather than guessed at.
pub fn parse(text: &str) -> Option<Message> {
    let (first, rest) = match text.split_once('\n') {
        Some((f, r)) => (f, r),
        None => (text, ""),
    };

    let open = first.find("[a2a/")?;
    let close = first[open..].find(']')? + open;
    let inside = &first[open + 1..close];

    let mut parts = inside.split_whitespace();

    let version = parts.next()?.strip_prefix("a2a/")?;
    // A version this does not know is not something to guess at. Newer fields could change
    // what the ones it does understand mean.
    if version.parse::<u32>().ok()? != VERSION {
        return None;
    }

    let kind = Kind::parse(parts.next()?)?;

    let mut ttl = None;
    for p in parts {
        if let Some(v) = p.strip_prefix("ttl=") {
            ttl = v.parse::<u32>().ok();
        }
        // Unknown keys are skipped rather than refused, so a later version can add one
        // without every older agent falling over.
    }

    Some(Message {
        kind,
        ttl: ttl?,
        body: rest.trim().to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_ordinary_message_reads_back() {
        let m = parse("<@U0ALEX> [a2a/1 ask ttl=5]\nHow do you handle memory?").unwrap();
        assert_eq!(m.kind, Kind::Ask);
        assert_eq!(m.ttl, 5);
        assert_eq!(m.body, "How do you handle memory?");
    }

    #[test]
    fn composing_and_parsing_are_the_same_shape() {
        let sent = compose(Some("U0ALEX"), Kind::Ask, 4, "  what is your plan?  ");
        let got = parse(&sent).unwrap();
        assert_eq!(got.kind, Kind::Ask);
        assert_eq!(got.ttl, 4);
        assert_eq!(got.body, "what is your plan?");
        assert!(sent.starts_with("<@U0ALEX> "), "the mention must be first");
    }

    /// A person is not an agent. Somebody writing about arrays must never be answered as
    /// though they had opened a protocol exchange.
    #[test]
    fn a_person_writing_brackets_is_not_speaking_the_protocol() {
        for text in [
            "hey Carl what does [a2a] mean",
            "the value is a[0] and b[1]",
            "[a2a/1]",
            "[a2a/1 ask]",
            "[a2a/1 gossip ttl=3]\nhello",
            "[a2a/1 ask ttl=notanumber]\nhello",
            "plain english with no brackets at all",
        ] {
            assert_eq!(parse(text), None, "should not have parsed: {text}");
        }
    }

    /// A future version might change what today's fields mean, so it is ignored rather than
    /// half understood.
    #[test]
    fn a_version_from_the_future_is_left_alone() {
        assert_eq!(parse("[a2a/2 ask ttl=5]\nhi"), None);
        assert_eq!(parse("[a2a/0 ask ttl=5]\nhi"), None);
    }

    /// A later version has to be able to add a field without breaking every agent already
    /// deployed, so unknown keys are skipped.
    #[test]
    fn an_unknown_field_does_not_break_it() {
        let m = parse("[a2a/1 reply ttl=2 priority=high topic=memory]\nsure").unwrap();
        assert_eq!(m.kind, Kind::Reply);
        assert_eq!(m.ttl, 2);
    }

    /// The reason the protocol exists. Every hop costs money and neither agent tires.
    #[test]
    fn the_budget_runs_down_and_then_stops_the_exchange() {
        let mut ttl = 3;
        let mut hops = 0;
        loop {
            let m = Message {
                kind: Kind::Ask,
                ttl,
                body: "and another thing".into(),
            };
            match m.reply_kind() {
                Some(Kind::Done) | None => break,
                Some(_) => {
                    hops += 1;
                    ttl = m.reply_ttl();
                }
            }
            assert!(hops < 50, "this is exactly what must not happen");
        }
        assert_eq!(hops, 3, "three hops of budget, then it ends");
    }

    /// The politeness loop. "Thanks" answered by "you are welcome" answered by "no problem"
    /// is an infinite exchange made entirely of good manners.
    /// The bug. A bot id is not a user id. Slack renders `<@B0ALEX>` as literal text and
    /// notifies nobody, so a reply addressed that way never reaches the agent the protocol
    /// says it is addressed to. No mention is better than an unresolvable one, because a
    /// message that looks addressed and is not is worse than one that plainly is not.
    #[test]
    fn a_sender_with_no_user_id_gets_no_mention_rather_than_a_broken_one() {
        let sent = compose(None, Kind::Reply, 3, "here is the answer");

        assert!(
            !sent.contains("<@"),
            "an unresolvable mention must not be invented: {sent}"
        );
        assert!(
            sent.starts_with(&header(Kind::Reply, 3)),
            "the header still has to carry the protocol: {sent}"
        );
        assert!(sent.contains("here is the answer"));
    }

    /// And the ordinary case still leads with the mention, which is what notifies the other
    /// agent at all.
    #[test]
    fn a_known_user_is_still_mentioned_first() {
        let sent = compose(Some("U0ALEX"), Kind::Reply, 3, "here is the answer");
        assert!(sent.starts_with("<@U0ALEX> "), "{sent}");
    }

    #[test]
    fn an_ending_is_never_answered() {
        for kind in [Kind::Done, Kind::Decline] {
            let m = Message {
                kind,
                ttl: 99,
                body: "thanks, that helps".into(),
            };
            assert_eq!(m.reply_kind(), None, "{kind} must end the exchange");
        }
    }

    /// Running out mid conversation should say so rather than going quiet, because silence
    /// looks like a crash and invites the other side to retry.
    #[test]
    fn running_out_says_so_instead_of_vanishing() {
        let m = Message {
            kind: Kind::Ask,
            ttl: 0,
            body: "one more thing".into(),
        };
        assert_eq!(m.reply_kind(), Some(Kind::Done));
    }

    #[test]
    fn hello_is_answered_with_hello() {
        let m = Message {
            kind: Kind::Hello,
            ttl: 6,
            body: "I am Alex".into(),
        };
        assert_eq!(m.reply_kind(), Some(Kind::Hello));
        assert_eq!(m.reply_ttl(), 5);
    }

    /// A header with no body is legal. "done" carries nothing useful.
    #[test]
    fn a_header_with_no_body_is_fine() {
        let m = parse("[a2a/1 done ttl=0]").unwrap();
        assert_eq!(m.kind, Kind::Done);
        assert_eq!(m.body, "");
    }
}
