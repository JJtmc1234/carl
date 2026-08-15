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
//! emergency.

pub mod army;
pub mod diagnostics;
pub mod health;
pub mod projects;
pub mod system;
pub mod workspace;

pub use diagnostics::{Diagnostics, Snapshot};
pub use health::{Diagnostic, Health, Kind, Metric, Reading};
