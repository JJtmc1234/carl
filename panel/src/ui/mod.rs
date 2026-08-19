//! Drawing, and nothing else.
//!
//! Every function here reads `App` and puts input back through its methods. None of them hold
//! state, decide anything, or touch a data source. That is what keeps the rules in one place
//! where they can be tested without a window.

pub mod agents;
pub mod carl;
pub mod detail;
pub mod diagnostics;
pub mod projects;
pub mod shell;
pub mod widgets;
pub mod workspace;

pub use shell::draw;
