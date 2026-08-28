//! What the panel asks for. It never does any of it itself.
//!
//! Every one of these is a request that goes out through `PanelDataSource::submit` and changes
//! nothing on screen until the backend says it happened. That is the rule that keeps the panel
//! honest: JJ pressing "stop task" does not grey the task out, it sends a stop and waits. A
//! panel that updates optimistically is a panel that disagrees with the army whenever the
//! command fails, and it is always the panel that is wrong.

/// Something JJ wants to happen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// Ordinary conversation with Carl.
    SayToCarl(String),
    /// A new objective, which is a different thing from a message and is sent as one.
    SetObjective(String),
    /// An answer to something Carl asked.
    AnswerDecision { id: String, answer: String },
    /// Allow or refuse one tool call Carl is holding still.
    ///
    /// Its own command rather than an `AnswerDecision`, because it does not travel the same way.
    /// A decision is answered by writing to the journal. This is answered on a channel of its
    /// own, and the id is a string the hook minted rather than a sequence number.
    AnswerPermission { question: String, allow: bool },
    /// JJ going straight to an agent, around the chain. Never ordinary traffic.
    Intervene(Intervention),
    /// Open something in the contextual workspace. Process 3 does the work.
    Workspace(WorkspaceRequest),
}

/// JJ reaching past the chain of command to one agent.
///
/// Kept as its own type rather than folded into a general message, because the whole reason it
/// exists is that it is exceptional. A type that made this look like ordinary traffic would
/// make it easy to do by accident, and the chain only means anything if going around it is a
/// deliberate act.
///
/// JJ is the one authority that was never delegated, so this is allowed. It is still recorded
/// as what it is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Intervention {
    /// The agent, by `org` name.
    pub agent: String,
    pub kind: InterventionKind,
    /// JJ's words. Required for every kind, because an intervention with no reason attached is
    /// one nobody can make sense of later.
    pub body: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InterventionKind {
    /// Say something to the agent directly.
    Message,
    /// Change what it is currently working to.
    ChangeInstruction,
    /// Stop what it is doing. The task's fate is the backend's to decide.
    StopTask,
    /// Stop this and do that instead.
    ReplaceTask,
}

impl InterventionKind {
    pub fn label(self) -> &'static str {
        match self {
            InterventionKind::Message => "MESSAGE AGENT",
            InterventionKind::ChangeInstruction => "CHANGE INSTRUCTION",
            InterventionKind::StopTask => "STOP TASK",
            InterventionKind::ReplaceTask => "REPLACE TASK",
        }
    }

    /// What the field above the box is called, since the four are not asking for the same thing.
    pub fn prompt(self) -> &'static str {
        match self {
            InterventionKind::Message => "message",
            InterventionKind::ChangeInstruction => "new instruction",
            InterventionKind::StopTask => "reason for stopping",
            InterventionKind::ReplaceTask => "the task that replaces it",
        }
    }

    /// Whether this changes what an agent is accountable for, as opposed to just telling it
    /// something. The ones that do are confirmed before they are sent.
    pub fn is_forceful(self) -> bool {
        !matches!(self, InterventionKind::Message)
    }

    pub const ALL: [InterventionKind; 4] = [
        InterventionKind::Message,
        InterventionKind::ChangeInstruction,
        InterventionKind::StopTask,
        InterventionKind::ReplaceTask,
    ];
}

/// Something to open in the contextual workspace.
///
/// The panel draws the container and the tabs. Process 3 does the opening. This enum is the
/// entire seam between those two jobs, and nothing in this branch spawns a shell or reads a
/// file to satisfy one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkspaceRequest {
    /// A file, optionally at a line.
    File {
        path: String,
        line: Option<u32>,
    },
    /// What one task changed.
    Diff {
        task: String,
    },
    /// A shell, in a directory somebody already has.
    Terminal {
        cwd: String,
    },
    /// Whatever a diagnostic points at, which the collector knows and the panel does not.
    Investigate {
        component: String,
    },
    Close,
}

impl WorkspaceRequest {
    /// What to put on the workspace tab.
    pub fn title(&self) -> String {
        match self {
            WorkspaceRequest::File { path, .. } => {
                path.rsplit('/').next().unwrap_or(path.as_str()).to_string()
            }
            WorkspaceRequest::Diff { task } => format!("diff {}", short(task)),
            WorkspaceRequest::Terminal { cwd } => {
                format!("shell {}", cwd.rsplit('/').next().unwrap_or(cwd.as_str()))
            }
            WorkspaceRequest::Investigate { component } => component.clone(),
            WorkspaceRequest::Close => "closed".into(),
        }
    }
}

fn short(id: &str) -> String {
    id.chars().take(8).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Three of the four change what an agent is accountable for, and those are the ones worth
    /// asking twice about. A message is not.
    #[test]
    fn only_the_forceful_kinds_need_confirming() {
        assert!(!InterventionKind::Message.is_forceful());
        for k in [
            InterventionKind::ChangeInstruction,
            InterventionKind::StopTask,
            InterventionKind::ReplaceTask,
        ] {
            assert!(k.is_forceful(), "{k:?}");
        }
    }

    /// The four ask for different things, so they must not share one label.
    #[test]
    fn every_kind_says_what_it_wants() {
        let mut prompts: Vec<&str> = InterventionKind::ALL.iter().map(|k| k.prompt()).collect();
        prompts.sort_unstable();
        let before = prompts.len();
        prompts.dedup();
        assert_eq!(prompts.len(), before, "two kinds ask the same question");
    }

    #[test]
    fn a_workspace_tab_is_named_after_what_it_holds() {
        let f = WorkspaceRequest::File {
            path: "/home/jj/carl/src/army/org.rs".into(),
            line: Some(12),
        };
        assert_eq!(f.title(), "org.rs");

        let t = WorkspaceRequest::Terminal {
            cwd: "/home/jj/Projects/carl".into(),
        };
        assert_eq!(t.title(), "shell carl");
    }
}
