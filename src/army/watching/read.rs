//! Reading the notes back, for somebody looking rather than something parsing.

use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use super::{Line, Note, path};
use crate::Result;

/// The newest notes, oldest first, optionally about one agent.
///
/// A line that will not parse is skipped rather than fatal, the same way the journal reads. A
/// file with one torn line in it is still worth reading, and somebody looking at this file is
/// often looking precisely because something went wrong.
pub fn read(home: &Path, agent: Option<&str>, most: usize) -> Result<Vec<Line>> {
    let text = match std::fs::read_to_string(path(home)) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e.into()),
    };

    let mut lines: Vec<Line> = text
        .lines()
        .filter_map(|l| serde_json::from_str::<Line>(l).ok())
        .filter(|l| agent.is_none_or(|who| l.agent == who))
        .collect();

    if lines.len() > most {
        lines.drain(..lines.len() - most);
    }
    Ok(lines)
}

/// Whatever was written after byte `from`, and where to read from next.
///
/// For following the file. Only whole lines are ever written and only under the lock, so the
/// bytes after a point this has already seen are whole lines. A file that got shorter was
/// trimmed under us, so it is read from the start again rather than from an offset that now
/// means something else.
pub fn since(home: &Path, from: u64, agent: Option<&str>) -> Result<(Vec<Line>, u64)> {
    let file = match std::fs::File::open(path(home)) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok((Vec::new(), 0)),
        Err(e) => return Err(e.into()),
    };

    let len = file.metadata()?.len();
    let start = match len < from {
        true => 0,
        false => from,
    };

    let mut text = String::new();
    let mut file = file;
    file.seek(SeekFrom::Start(start))?;
    file.read_to_string(&mut text)?;

    // Only up to the last newline. A read that caught a writer mid line would otherwise hand
    // back half an object and then start the next read in the middle of it.
    let whole = match text.rfind('\n') {
        Some(at) => &text[..=at],
        None => return Ok((Vec::new(), start)),
    };

    let lines = whole
        .lines()
        .filter_map(|l| serde_json::from_str::<Line>(l).ok())
        .filter(|l| agent.is_none_or(|who| l.agent == who))
        .collect();
    Ok((lines, start + whole.len() as u64))
}

/// One note as a row. Nothing here prints a struct: a line nobody can read is a line nobody
/// reads, and this file is only worth keeping if somebody looks at it.
///
/// `now` is passed in rather than read here, the same way the supervisor takes its clock, so a
/// test can say what time it is instead of racing one.
pub fn line_of(line: &Line, now: u64) -> String {
    let when = ago(line.at, now);
    let who = &line.agent;
    let what = match &line.note {
        Note::Asked { chars } => format!("was asked, {chars} characters"),
        // The size, because the words are redacted at the source. Saying "thinking" with no
        // number would be indistinguishable from a stuck process.
        Note::Thinking { text, tokens } => match (text.is_empty(), tokens) {
            (false, _) => format!("thinking: {text}"),
            (true, Some(n)) => format!("thinking, about {n} tokens so far"),
            (true, None) => "thinking, size not given".to_string(),
        },
        Note::Doing { tool, detail } => match detail.is_empty() {
            true => tool.clone(),
            false => format!("{tool} {detail}"),
        },
        Note::Refused { tool, why } => format!("refused {tool}, {why}"),
        Note::Answered {
            chars,
            interrupted: false,
        } => format!("answered, {chars} characters"),
        Note::Answered {
            chars,
            interrupted: true,
        } => format!("ran out of time after {chars} characters"),
    };
    format!("{when:>5}  {who:<7} {what}")
}

/// How long ago, rather than a wall clock.
///
/// There is no time crate here, so a wall clock would be UTC while the person reading it is
/// not, and a row stamped an hour off is worse than no stamp at all. How long ago is the same
/// number in every timezone, and while following a live file it is the number you actually
/// want.
fn ago(at: u64, now: u64) -> String {
    let seconds = now.saturating_sub(at);
    match seconds {
        s if s < 60 => format!("{s}s"),
        s if s < 3600 => format!("{}m", s / 60),
        s if s < 86400 => format!("{}h", s / 3600),
        s => format!("{}d", s / 86400),
    }
}
