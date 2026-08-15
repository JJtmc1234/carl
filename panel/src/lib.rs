//! The Command Panel, a fullscreen operations interface for Carl's army.
//!
//! Four layers, and the boundary between the last two is the point of the whole crate.
//!
//! | layer | what it is |
//! |---|---|
//! | `theme` | the visual language, defined once |
//! | `model` | what is drawn, projected from the army types and never a second copy |
//! | `app` | every rule and every piece of state, with no drawing in it |
//! | `ui` | drawing only, reading `app` and putting input back through it |
//!
//! Underneath sits `source`, the one seam. `PanelDataSource` is four methods, and swapping the
//! mock for Process 1's live implementation changes nothing above it. No widget anywhere knows
//! where its data came from.
//!
//! Keeping `app` free of egui is what makes the panel testable. Tab navigation, live updates,
//! reconnection, command generation and intervention are all checked without opening a window.

pub mod app;
pub mod command;
pub mod model;
pub mod source;
pub mod theme;
pub mod ui;

pub use app::{App, Tab};
pub use command::{Command, Intervention, InterventionKind, WorkspaceRequest};
pub use model::{Link, Snapshot};
pub use source::{LivePanelDataSource, MockPanelDataSource, PanelDataSource, PanelEvent};
