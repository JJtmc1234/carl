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

mod api;
mod event;
mod socket;
mod tokens;

pub use api::{Api, Me, hint};
pub use event::{Ask, ask_from, strip_mention};
pub use socket::serve;
pub use tokens::Tokens;
