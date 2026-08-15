//! What the Command Panel needs to know, collected.
//!
//! Three providers, deliberately separable, none of which knows anything about a user
//! interface or a transport. Each is an ordinary Rust type an integrator can call.
//!
//! - `diagnostics`, split into the army read from its own records and the machine sampled from
//!   `/proc`.
//! - `projects`, the first durable notion of a project and a milestone that Carl has had.
//! - `workspace`, JJ's own terminal, editor and diff.
//!
//! **The distinction that runs through all of it.** Army state is event driven. It changes when
//! something happens, it is true until then, and it carries no timestamp of its own. Machine
//! telemetry is sampled. It is a number read at a moment, it is stale immediately, and it
//! carries the moment it was read. A panel that shows both must show them differently, so this
//! layer never lets them be confused: see `health::Kind`.
//!
//! **The other rule.** A measurement nobody could take is `unknown` and never zero. Zero disk
//! free and unmeasurable disk free look identical once flattened, and one of them is an
//! emergency. `Diagnostic::flattened` offers a lossy view for a caller whose model has no room
//! for a gap, but the canonical value always keeps the detail.
//!
//! # Component ids
//!
//! A caller groups on the component id, so the id is part of the contract rather than a label.
//! There are two families and only two, and `Diagnostic::group` returns which:
//!
//! ```text
//!   army.personnel                  the agent folders as a whole
//!   army.agent.<name>               one agent, from its folder
//!   army.journal                    the event record, including holes in it
//!   army.tasks                      what is in hand, blocked, abandoned
//!   army.latency                    measured handover times
//!   army.service.<unit>             carl-aec, carl-listen, carl-slack
//!   army.claude.processes           a count of processes, never a named agent
//!
//!   system.cpu                      utilisation and load
//!   system.memory                   memory and swap
//!   system.disk:<path>              one filesystem, by resolved path
//!   system.gpu                      the graphics card
//!   system.temperature              CPU package
//!   system.network                  interface totals
//! ```
//!
//! Two rules about them. **They are stable**, so renaming one breaks whoever grouped on it and
//! is not something to do casually. And **they are unique**: `Snapshot::duplicate_components`
//! exists so that can be asserted rather than assumed, and a test does assert it.
//!
//! An id is a lookup key and never anything else. Nothing in this module turns one into a path
//! or a command, which is what makes `Workspace::investigate` safe to hand a string that came
//! from a click.

pub mod army;
pub mod diagnostics;
pub mod health;
pub mod projects;
pub mod system;
pub mod workspace;

pub use diagnostics::{Diagnostics, Snapshot};
pub use health::{Diagnostic, Health, Kind, Metric, Reading};
