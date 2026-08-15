//! JJ's own tooling: a terminal, a small editor and a way to see what changed.
//!
//! **This is the part of the panel that belongs to the person rather than to the army.** It is
//! manual tooling that runs as JJ, with his permissions, on things he chose. It is not an agent
//! capability and must not become one. Agents get the sandbox in `etc/carl-python` and a tool
//! allow list, and that boundary is the whole reason the army is safe to leave running.
//!
//! Nothing in this module is registered as a tool, named in an allow list, or reachable from
//! `claude::Runner`. Keeping it that way is a review question for any change here, not a
//! property that maintains itself.
//!
//! Three pieces, separable on purpose:
//!
//! - `terminal`, a real pseudoterminal running JJ's shell.
//! - `editor`, opening and saving one file without losing anybody's work.
//! - `diff`, read only git plus a plain text comparison for an unsaved buffer.
//! - `service`, one handle over the three so a user interface can hold sessions across frames.

pub mod diff;
pub mod editor;
pub mod service;
pub mod terminal;

pub use diff::Change;
pub use editor::{Mode, OpenFile};
pub use service::{FileInfo, Investigation, SessionId, Workspace};
pub use terminal::{Size, Terminal};
