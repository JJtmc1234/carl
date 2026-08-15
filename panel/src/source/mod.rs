//! The one boundary between the panel and whatever is feeding it.
//!
//! Everything above this line draws. Everything below it fetches. No widget anywhere in the
//! panel knows whether the data came from a file, a socket or a script, which is the whole
//! point: Process 1 writes one implementation of this trait and the interface does not change.
//!
//! Four things, and no more than four.
//!
//! **A snapshot**, which is authoritative and complete. Taken once at startup and again after
//! every reconnect, because a UI that patches events onto state it kept across a disconnection
//! is a UI showing a version of the world nobody ever sent it.
//!
//! **Events**, polled and applied as they arrive. There is no refresh button, and the absence
//! of one is deliberate. Anything that only updates when asked is stale by definition and
//! looks exactly like something that is current.
//!
//! **Commands**, going the other way. The panel never changes authoritative state itself. It
//! asks, and it shows what came back. Marking a task stopped locally because JJ pressed stop
//! would put the panel and the army into two different versions of the truth, and the panel
//! would be the one that was wrong.
//!
//! **The link**, which is reported rather than assumed.

use crate::command::Command;
use crate::model::{Link, Snapshot};

mod live;
mod mapping;
mod mock;

pub use live::LivePanelDataSource;
pub use mock::MockPanelDataSource;

/// Where the panel gets everything.
///
/// Deliberately narrow. A wider trait would let a widget reach for something specific and tie
/// the drawing to the transport, which is what this exists to prevent.
pub trait PanelDataSource {
    /// The whole authoritative state, right now.
    ///
    /// Called at startup and again after any reconnection. Never merged with what was on
    /// screen before, always replacing it.
    fn snapshot(&mut self) -> Snapshot;

    /// Everything that has happened since the last call. Never blocks.
    fn poll(&mut self) -> Vec<PanelEvent>;

    /// Asks for something to change. The panel does not apply this itself.
    fn submit(&mut self, command: Command) -> Result<(), String>;

    fn link(&self) -> Link;

    /// A name for the source, so the panel can say what it is attached to.
    fn describe(&self) -> String;
}

/// One thing that changed.
///
/// A closed list rather than a blob, for the same reason `army::event::Event` is closed. Every
/// writer inventing its own wording means no reader can count anything, and the panel is a
/// reader whose entire job is counting.
#[derive(Debug, Clone, PartialEq)]
pub enum PanelEvent {
    /// One agent's live overlay changed. Carries the whole new view rather than a patch, so
    /// applying it twice does the same thing as applying it once.
    AgentChanged(Box<crate::model::AgentView>),
    /// A task was created or moved, as the backend describes it.
    TaskChanged(Box<carl::panel::view::TaskView>),
    /// Something was written to the army's record.
    Recorded(Box<carl::army::event::Record>),
    /// Carl said something, or is saying it.
    CarlSaid {
        text: String,
        /// True while more is coming. The panel shows a caret and does not treat it as final.
        streaming: bool,
    },
    /// JJ's own message, echoed back once the backend has it.
    JjSaid(String),
    DecisionRaised(Box<crate::model::Decision>),
    DecisionSettled {
        id: String,
    },
    Delegated(Box<crate::model::Delegation>),
    DiagnosticChanged(Box<crate::model::Diagnostic>),
    ProjectChanged(Box<crate::model::ProjectView>),
    MilestoneReached {
        project: String,
        milestone: Box<crate::model::Milestone>,
    },
    /// The whole world was replaced after a gap the stream could not bridge.
    ///
    /// Carries the new snapshot rather than a signal to go and fetch one, so applying it cannot
    /// race with whatever arrives next.
    Resynced(Box<Snapshot>),
    /// A command did not go. Shown, never swallowed, because JJ otherwise believes it did.
    CommandRefused(String),
    /// The link changed. The panel stops claiming to be live the moment this says otherwise.
    LinkChanged(Link),
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The trait has to be usable as an object, because the app holds one without knowing
    /// which it is. A method that broke that would be found here rather than at the call site.
    #[test]
    fn the_source_can_be_held_as_an_object() {
        let mut source: Box<dyn PanelDataSource> = Box::new(MockPanelDataSource::new());
        let snap = source.snapshot();
        assert!(!snap.agents.is_empty());
        assert!(source.describe().to_lowercase().contains("mock"));
    }
}
