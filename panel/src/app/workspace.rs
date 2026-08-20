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

/// How much terminal text a pane keeps, matching the ring the facade holds it in.
///
/// The same number on purpose. Keeping more here than the terminal itself keeps would mean
/// paying for scrollback the shell has already forgotten, and this side has no way to refill
/// it. See bug 13.
pub const PANE_OUTPUT_BYTES: usize = carl::providers::workspace::terminal::SCROLLBACK_BYTES;

/// Appends drained output, dropping the oldest text once the pane is over its budget.
///
/// The second half of bug 13. Bounding the terminal's ring bounds what one `drain` can return,
/// and this string was still growing forever on the other side of it, so a shell that printed
/// steadily for an hour cost an hour of text either way.
///
/// Trimmed on a character boundary rather than at the byte, because the drained bytes are
/// `from_utf8_lossy` output and cutting a multi byte character in half would put a replacement
/// character on JJ's screen every time the cap was hit.
pub fn append_bounded(output: &mut String, text: &str) {
    output.push_str(text);
    if output.len() <= PANE_OUTPUT_BYTES {
        return;
    }
    let over = output.len() - PANE_OUTPUT_BYTES;
    let cut = (over..=output.len())
        .find(|i| output.is_char_boundary(*i))
        .unwrap_or(output.len());
    output.drain(..cut);
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
        /// True once it has been seen to have exited.
        ///
        /// The session is deliberately not closed at that moment. A shell that died left its
        /// scrollback behind and that is usually the only evidence of why, so it stays open and
        /// readable until the pane is dismissed, and dismissing it is what releases it.
        exited: bool,
    },
    Editor {
        /// The buffer being edited, which may differ from the file.
        buffer: String,
        read_only: bool,
        /// True once somebody else has touched the file underneath.
        changed_on_disk: bool,
        /// Set when a save was refused, and why.
        refused: Option<String>,
        /// True when the refusal was because the file moved underneath.
        ///
        /// Its own flag rather than a substring check on the message, because the pane offers a
        /// different way out for this one: the buffer is kept and reloading is a deliberate
        /// choice, where a read only file has nothing to resolve.
        conflict: bool,
        path: String,
    },
    Diff(Comparison),
    Investigating(Box<Investigation>),
    /// Opened, and the facade could not do it.
    Empty,
}

impl Workspace {
    pub fn title(&self) -> String {
        self.open.title()
    }
}

/// What a comparison came back as.
///
/// Four outcomes and not three, because "no changes" and "cannot be compared" are different
/// answers and collapsing them is the specific mistake worth avoiding: a repository with no
/// commits yet cannot be diffed at all, and showing that as a clean tree would tell somebody
/// their work is committed when nothing is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Comparison {
    /// Real differences, as text.
    Changes(String),
    /// Compared successfully, and there are none.
    Same,
    /// Git said the files differ and would not say how.
    Binary,
    /// The comparison could not be made. Never shown as cleanliness.
    Unavailable(String),
}

impl Comparison {
    /// Reads what the facade returned into one of the four.
    pub fn of(result: Result<String, String>) -> Self {
        match result {
            Err(why) => Comparison::Unavailable(why),
            Ok(text) if text.trim().is_empty() => Comparison::Same,
            // Git says this instead of a hunk, and dumping the bytes of an image into a text
            // widget is a wall of replacement characters at best.
            Ok(text) if text.contains("Binary files") && !text.contains("\n@@") => {
                Comparison::Binary
            }
            Ok(text) => Comparison::Changes(safe(&text)),
        }
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
                    exited: false,
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
                        conflict: false,
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
                Some(id) => {
                    w.pane = Pane::Diff(Comparison::of(
                        service.diff_against_head(id).map_err(|e| e.to_string()),
                    ))
                }
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

#[cfg(test)]
mod bounded_output {
    use super::*;

    /// The pane's own string must not grow forever, which was the second half of bug 13.
    ///
    /// Bounding the terminal ring bounds what one drain returns. It says nothing about what
    /// happens when a shell prints steadily for an hour and every drain is small.
    #[test]
    fn a_pane_that_never_stops_printing_stays_bounded() {
        let mut output = String::new();
        let chunk = "hello\n".repeat(1000);
        for _ in 0..200 {
            append_bounded(&mut output, &chunk);
        }

        assert!(
            output.len() <= PANE_OUTPUT_BYTES,
            "the pane held {} bytes against a {PANE_OUTPUT_BYTES} cap",
            output.len()
        );
        // The newest text is the part worth keeping, since that is what JJ is reading.
        assert!(output.ends_with("hello\n"), "the tail was not kept");
    }

    /// Trimming must cut between characters, or a cap that is hit mid character puts a
    /// replacement glyph on the screen for output that arrived perfectly intact.
    #[test]
    fn trimming_lands_on_a_character_boundary() {
        let mut output = "e".repeat(PANE_OUTPUT_BYTES);
        // Three byte characters, so almost every byte offset is inside one.
        append_bounded(&mut output, &"日".repeat(200));

        assert!(output.len() <= PANE_OUTPUT_BYTES);
        assert!(
            !output.contains('\u{fffd}'),
            "a character was cut in half: {:?}",
            &output[..40]
        );
        assert!(output.ends_with('日'));
    }
}
