//! The live backend behind the AOS Command Panel.
//!
//! A window onto the army, and nothing more than a window. Everything the panel shows is read
//! from somewhere that already knew it: `org` for who exists, the personnel folders for what
//! each agent holds, the journal for what happened, and `turn` for talking to Carl. This module
//! adds no store, no second organisation model and no second event log.
//!
//! That constraint is not decoration. A panel with its own task table would, the first time the
//! two disagreed, force somebody to decide which was right, and the honest answer would always
//! be "the army", which means the table was never worth having. So there is no table. Views are
//! folded from the record on demand and thrown away.
//!
//! ```text
//!   view.rs      what the panel is allowed to know, and how it says it does not know
//!   wire.rs      the frames, one JSON object per line, both directions
//!   command.rs   what the panel may ask for, and how a JJ intervention is written down
//!   tasks.rs     tasks rebuilt from the record, because they exist nowhere else
//!   facts.rs     the provider backed half, and how projects join to tasks
//!   snapshot.rs  everything at one moment, from the three places that actually know
//!   listen.rs    the socket, owner only, which is the authentication
//!   client.rs    the typed client, so nothing above it deals in bytes
//!   live.rs      staying connected, and being honest when it is not
//!   serve.rs     the accept loop and the tail of the journal
//! ```
//!
//! **Where the authority comes from.** The socket lives in a 0700 directory and is itself 0600,
//! so only JJ's own processes can reach it. That is why a command carries no actor field: there
//! is nothing to claim, because the kernel answered the question before the first byte arrived.
//! Agents cannot reach it at all, since they run with no home directory bound and the path does
//! not exist for them.
//!
//! **What is not built here.** No diagnostics collectors and no project scanning: the shapes
//! exist and the lists come back empty, because Carl measures neither yet and a made up
//! milestone is worse than a missing one. No editor and no terminal. Those belong to the other
//! two processes, and `docs/panel-v1.md` is the contract they build against.

pub mod client;
pub mod command;
pub mod facts;
pub mod listen;
pub mod live;
pub mod serve;
pub mod snapshot;
pub mod tasks;
pub mod view;
pub mod wire;

#[cfg(test)]
mod tests;

pub use client::{Done, Events, Incoming, PanelClient};
pub use command::{PanelCommand, Recorded};
pub use facts::Facts;
pub use listen::{Bound, bind, hold, socket_path};
pub use live::{Health as Link, LivePanel, Update};
pub use serve::Server;
pub use snapshot::build;
pub use view::{
    AgentView, CarlView, DiagnosticView, Health, Maybe, PanelSnapshot, ProcessState, ProjectView,
    TaskView,
};
pub use wire::{Ask, Frame, PanelEvent, Reply, Request, VERSION};
