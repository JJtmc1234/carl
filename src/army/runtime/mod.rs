//! Keeping agents running, which is a different job from giving them work.
//!
//! This is the layer that makes the army a thing that is up rather than a thing that is invoked.
//! Today a chain run starts four `claude` processes, uses them and drops them, so an agent has no
//! continuity beyond what it wrote down. A supervised agent has a process that stays, a
//! conversation that survives that process being replaced, and an id that survives both.
//!
//! **Carl controls work. The supervisor controls process existence.** Worth stating first because
//! everything here is shaped by it. Carl deciding that Nora should stop working on something and
//! Nora's process exiting are different acts. A design that ran them together would give Carl a
//! kill switch he was never meant to have, and would give the supervisor opinions about work it
//! is in no position to judge. So nothing in this module knows what a task is, and the only thing
//! it ever says to an agent is where its memory folder is.
//!
//! ```text
//!   army/nora/identity.json     who this is, forever          personnel
//!   run/agents/a-....json       which session, which process  here
//!   the claude process          the thing doing the work      here, and replaceable
//! ```
//!
//! Three lifetimes, longest first, and each one outlives the one below it. That column is the
//! whole design. The id never changes. The session is thrown away only when it is the thing that
//! is broken. The process is expected to die and be replaced, and doing so costs the agent
//! nothing, because the conversation it was serving is resumed into the next one.
//!
//! Six modules, split by what would break if they were one.
//!
//! | module | what it is | why it is separate |
//! |---|---|---|
//! | `record` | what is known about one agent's process | it is a file, and files outlive processes |
//! | `policy` | what to do next about it | pure, so a fourth crash is a test rather than an afternoon |
//! | `continuity` | how much of what an agent had is still with it | three things that fail separately |
//! | `store` | the records on disk | one writer, and it is not the agent |
//! | `lock` | one supervisor per home | two would spend the night ending each other's processes |
//! | `supervisor` | the part that spawns, kills, wakes and carries a sentence | the only part that cannot be reasoned about without a process |
//!
//! Not built here, on purpose: the timetable that would decide when an agent sleeps, compaction,
//! the work queue, and anything that hands an agent a task. Stopping an agent and waking one are
//! here, because both are about whether a process exists. Deciding when to is not.

mod continuity;
mod lock;
mod policy;
mod record;
mod store;
mod supervisor;

#[cfg(test)]
mod tests;

pub use continuity::{Continuity, Memory, Process, Session};
pub use lock::Lock;
pub use policy::{
    BACKOFF_MAX, GIVE_UP_AFTER, HEALTHY_FOR, Next, RENEW_AFTER, Start, backoff, decide, was_healthy,
};
pub use record::{Lifecycle, Runtime};
pub use store::{Roll, dir, path};
pub use supervisor::{Outcome, Supervisor, Tick};
