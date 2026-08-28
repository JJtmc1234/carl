//! Drawing, and nothing else.
//!
//! Every function here reads `App` and puts input back through its methods. None of them hold
//! state, decide anything, or touch a data source. That is what keeps the rules in one place
//! where they can be tested without a window.
//!
//! Two exceptions, both deliberate and both about this window rather than about the army:
//! which subtrees of the chain are folded, and which act the composer is set to. Those live in
//! egui's own context memory, because putting them in `App` would give the state the backend
//! feeds opinions about how somebody has arranged their screen.

pub mod agents;
pub mod carl;
pub mod detail;
pub mod diagnostics;
pub mod overview;
pub mod projects;
pub mod shell;
pub mod vitals;
pub mod widgets;
pub mod workspace;

#[cfg(test)]
pub mod probe;

pub use shell::draw;
