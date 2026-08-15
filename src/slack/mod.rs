//! Carl in Slack, over Socket Mode.
//!
//! Socket Mode rather than the Events API, because the Events API needs Slack to be able to
//! reach a public URL. This machine is a laptop behind a router. Socket Mode dials out over a
//! websocket instead, so nothing has to be exposed and nothing has to be hosted.
//!
//! ```text
//!   apps.connections.open  ->  a single use websocket url
//!   dial it
//!   for each envelope:
//!     ack it immediately          <- Slack retries after 3 seconds and Carl takes longer
//!     decide if it is a question  <- event.rs, pure
//!     hand it to the worker       <- so reading never stops
//!   worker: ask Claude, post the answer
//! ```
//!
//! The acknowledgement and the answer are deliberately separate. Slack wants an ack within
//! three seconds and Claude takes five or more, so answering first means Slack decides the
//! event was dropped and sends it again. Carl would answer the same question two or three
//! times, which is not a thing you notice in testing and is very obvious in a real channel.

mod a2a;
mod api;
mod engaged;
mod event;
mod files;
mod live;
mod named;
mod patience;
mod socket;
mod tokens;
mod who;

pub use a2a::{Kind, Message, START_TTL, compose, header, parse};
pub use api::{Api, Me, hint};
pub use engaged::Engaged;
pub use event::{Ask, ask_from, strip_mention};
pub use files::{MAX_BYTES, Shared, fetch, filename_for, images_in};
pub use live::{Pace, placeholder};
pub use named::{NAMES, addressed};
pub use patience::{DEFAULT_LIMIT, Patience};
pub use socket::serve;
pub use tokens::Tokens;
pub use who::{Directory, preferred};

/// What Carl is told about being in Slack, on every Slack turn.
///
/// He needs telling. Without it he reasons about whether some Slack connector is authorised,
/// concludes it is not, and carefully explains that he cannot send a message, in a message
/// that is then posted to Slack. He has a mouth already and does not know it.
pub const CONTEXT: &str = "\
You are answering in Slack right now. Whatever you write is posted to the channel \
automatically, in the thread you were asked in. You do not need a connector, a tool or a \
permission to reply, and you must not say that you cannot send messages. Writing the reply \
is sending it.

Keep it short, a few sentences. This is a chat message that people scroll past, not a \
document.

To speak to another agent, address them by their Slack mention and follow the A2A protocol \
in docs/a2a.md: a first line of [a2a/1 <kind> ttl=<n>] and then plain prose. Never answer a \
message whose kind is done or decline.";
