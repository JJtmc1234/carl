//! The contextual workspace, driven through Process 3's facade.
//!
//! The panel owns what it looks like and how it behaves. `providers::workspace::Workspace`
//! owns every operation: it opens the pty, reads the file, runs the comparison. Nothing in the
//! interface spawns a process, opens a path or shells out, and the one place that could is
//! this file, which calls the facade and never `Command`.
//!
//! The state kept here is presentation state and nothing else. Which session a pane is showing,
//! what has been typed into the terminal, what is in the editor buffer. The authority for the
//! file on disk and the process on the end of the pty is the facade, always, and where the two
//! disagree the facade wins and the pane says so.

use carl::providers::workspace::editor::Mode;
use carl::providers::workspace::service::{Investigation, SessionId, Workspace as Service};
use carl::providers::workspace::terminal::Size;

use crate::command::WorkspaceRequest;

/// What the pane is showing, and the presentation state that goes with it.
///
/// Not `Eq`, because an investigation carries a reading that can be a float.
#[derive(Debug, Clone, PartialEq)]
pub struct Workspace {
    pub open: WorkspaceRequest,
    /// The facade's handle, once it opened something. `None` while it could not.
    pub session: Option<SessionId>,
    /// What went wrong, when something did. Shown rather than swallowed.
    pub trouble: Option<String>,
    pub pane: Pane,
}

/// The four things the pane can be.
#[derive(Debug, Clone, PartialEq)]
pub enum Pane {
    Terminal {
        /// Everything the pty has said, as text.
        output: String,
        /// What JJ is typing and has not sent.
        input: String,
        cwd: Option<String>,
        alive: bool,
    },
    Editor {
        /// The buffer being edited, which may differ from the file.
        buffer: String,
        read_only: bool,
        /// True once somebody else has touched the file underneath.
        changed_on_disk: bool,
        /// Set when a save was refused, and why.
        refused: Option<String>,
        path: String,
    },
    Diff {
        text: String,
    },
    Investigating(Box<Investigation>),
    /// Opened, and the facade could not do it.
    Empty,
}

impl Workspace {
    pub fn title(&self) -> String {
        self.open.title()
    }
}

/// Opens whatever was asked for, through the facade.
///
/// Every failure comes back as `trouble` on a pane that still draws. An unborn repository, a
/// file that is not there, a directory that has gone: all of those are things JJ should be
/// told about in the pane he opened, rather than by nothing happening.
pub fn open(service: &mut Service, request: WorkspaceRequest, rows: u16, cols: u16) -> Workspace {
    let mut w = Workspace {
        open: request.clone(),
        session: None,
        trouble: None,
        pane: Pane::Empty,
    };

    match &request {
        WorkspaceRequest::Terminal { cwd } => match service.open_terminal(cwd, Size { rows, cols })
        {
            Ok(id) => {
                w.session = Some(id);
                w.pane = Pane::Terminal {
                    output: String::new(),
                    input: String::new(),
                    cwd: service.cwd(id).map(|p| p.display().to_string()),
                    alive: true,
                };
            }
            Err(e) => w.trouble = Some(e.to_string()),
        },

        WorkspaceRequest::File { path, .. } => {
            // Read write, and the facade refuses a save on a read only file rather than
            // silently doing nothing. The pane shows that refusal.
            match service.open_file(path, Mode::ReadWrite) {
                Ok(id) => {
                    let info = service.file_info(id);
                    w.session = Some(id);
                    w.pane = Pane::Editor {
                        buffer: service.contents(id).unwrap_or_default().to_string(),
                        read_only: info.as_ref().is_some_and(|i| i.read_only),
                        changed_on_disk: false,
                        refused: None,
                        path: path.clone(),
                    };
                }
                Err(e) => w.trouble = Some(e.to_string()),
            }
        }

        WorkspaceRequest::Diff { task } => {
            // Nothing here turns a task id into a path. A diff needs a file the workspace
            // already has open, so with none this says so rather than guessing at one.
            match service.files().first().copied() {
                Some(id) => match service.diff_against_head(id) {
                    Ok(text) if text.trim().is_empty() => {
                        w.pane = Pane::Diff {
                            text: "no difference against HEAD".into(),
                        }
                    }
                    Ok(text) => w.pane = Pane::Diff { text: safe(&text) },
                    Err(e) => w.trouble = Some(e.to_string()),
                },
                None => {
                    w.trouble = Some(format!(
                        "nothing is open to compare. Task {task} does not name a file, and the \
                         panel will not guess at one."
                    ))
                }
            }
        }

        WorkspaceRequest::Investigate { .. } => {
            // Filled by the caller, which is the only place holding the diagnostics snapshot.
        }

        WorkspaceRequest::Close => {}
    }

    w
}

/// Diff text that is safe to put in a label.
///
/// A comparison can contain a binary file, and a binary blob in a text widget is a wall of
/// replacement characters at best. Bounded and stripped of control bytes, with the fact that it
/// was trimmed said out loud rather than left for somebody to wonder about.
pub fn safe(text: &str) -> String {
    const LIMIT: usize = 40_000;

    let cleaned: String = text
        .chars()
        .map(|c| {
            if c == '\n' || c == '\t' || !c.is_control() {
                c
            } else {
                '\u{fffd}'
            }
        })
        .collect();

    if cleaned.len() <= LIMIT {
        return cleaned;
    }
    let mut cut = LIMIT;
    while cut > 0 && !cleaned.is_char_boundary(cut) {
        cut -= 1;
    }
    format!(
        "{}\n\n[{} more bytes not shown]",
        &cleaned[..cut],
        cleaned.len() - cut
    )
}
