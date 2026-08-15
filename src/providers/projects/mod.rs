//! Projects and what has actually been achieved on them.
//!
//! ```text
//!   <home>/projects/
//!     jjtorio/
//!       project.json        name, goal, phase, status, owner, path, blockers
//!       milestones.jsonl    accepted history, appended, never rewritten
//!       suggested.jsonl     proposals nobody has accepted. A different file and a different
//!                           type, so nothing can drift from here into history.
//! ```
//!
//! **How this differs from `recent_changes.json`.** That file, at the repository root, is the
//! organisation's governance and context feed: who outranks whom, what the escalation rule is,
//! that agents get no elevated rights. It is written for agents to read when they start, it is
//! about the organisation rather than about any project, and nothing in the Rust code reads it,
//! which was checked rather than assumed. Milestones are the opposite on every count: they are
//! about one project, they record something that was achieved rather than a rule that now
//! applies, and they are written by code. The two do not overlap and neither should grow into
//! the other. If a governance change ever needs to appear on a project timeline, the honest
//! move is to record a milestone that cites it, not to merge the stores.
//!
//! **Reading does not create.** `Projects::open` will not make the directory. A panel looking
//! at a machine with no projects should see none, not quietly bring the store into existence.

pub mod model;

use std::path::{Path, PathBuf};

use crate::army::task::TaskId;
use crate::providers::system::process;
use crate::{Error, Result};

pub use model::{Achievement, Milestone, Project, ProjectId, Source, Status, Suggestion};

/// How many milestones a view carries unless asked otherwise.
pub const RECENT: usize = 5;

/// Everything needed to record a milestone except the identity the store assigns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewMilestone {
    pub project: ProjectId,
    pub at: u64,
    pub title: String,
    pub detail: Option<String>,
    pub evidence: Option<String>,
    pub achievement: Achievement,
    pub source: Source,
}

/// The project store on disk.
pub struct Projects {
    root: PathBuf,
}

impl Projects {
    /// Opens the store under `<home>/projects`, without creating anything.
    pub fn open(home: impl AsRef<Path>) -> Self {
        Self {
            root: home.as_ref().join("projects"),
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn folder(&self, id: &ProjectId) -> PathBuf {
        self.root.join(id.as_str())
    }

    /// Every project, by id.
    ///
    /// A folder that will not load is an error naming the file rather than a project silently
    /// missing from the list, because a project that vanishes from a panel is worse than one
    /// that shows up broken.
    pub fn list(&self) -> Result<Vec<Project>> {
        let Ok(entries) = std::fs::read_dir(&self.root) else {
            return Ok(Vec::new());
        };

        let mut out = Vec::new();
        for entry in entries.flatten() {
            if !entry.path().is_dir() {
                continue;
            }
            let id = ProjectId::new(entry.file_name().to_string_lossy().into_owned())?;
            if let Some(p) = self.get(&id)? {
                out.push(p);
            }
        }
        out.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(out)
    }

    pub fn get(&self, id: &ProjectId) -> Result<Option<Project>> {
        let path = self.folder(id).join("project.json");
        if !path.exists() {
            return Ok(None);
        }
        let text = std::fs::read_to_string(&path)?;
        let project: Project = serde_json::from_str(&text)
            .map_err(|e| Error::Refused(format!("{} is not valid: {e}", path.display())))?;
        Ok(Some(project))
    }

    /// Writes a project, creating its folder.
    pub fn save(&self, project: &Project) -> Result<()> {
        project.check()?;
        let folder = self.folder(&project.id);
        std::fs::create_dir_all(&folder)?;
        write_atomic(
            &folder.join("project.json"),
            &format!("{}\n", serde_json::to_string_pretty(project)?),
        )
    }

    /// Accepted milestones, oldest first.
    ///
    /// Append order, which is also time order because the store only ever appends. Two
    /// milestones recorded in the same second keep the order they were written in, so this is
    /// deterministic rather than merely usually right.
    pub fn milestones(&self, id: &ProjectId) -> Result<Vec<Milestone>> {
        Ok(read_lines(&self.folder(id).join("milestones.jsonl"))?.0)
    }

    /// How many lines of the milestone file could not be read.
    ///
    /// A line that will not parse is skipped so that one bad append cannot hide every
    /// milestone after it. That is the right behaviour and it also means a caller never
    /// learns it happened, so the count is available separately rather than being swallowed.
    pub fn milestone_gaps(&self, id: &ProjectId) -> Result<usize> {
        Ok(read_lines::<Milestone>(&self.folder(id).join("milestones.jsonl"))?.1)
    }

    /// The most recent milestones, newest first.
    pub fn recent_milestones(&self, id: &ProjectId, how_many: usize) -> Result<Vec<Milestone>> {
        let mut all = self.milestones(id)?;
        all.reverse();
        all.truncate(how_many);
        Ok(all)
    }

    /// Records a milestone against a project that exists.
    ///
    /// Refuses a milestone for a project nobody has created, because a timeline for a thing
    /// with no goal and no owner is a row of text nobody can act on.
    pub fn record(&self, new: NewMilestone) -> Result<Milestone> {
        if self.get(&new.project)?.is_none() {
            return Err(Error::Refused(format!(
                "there is no project called {}",
                new.project
            )));
        }

        // Taken from the highest id already there rather than from a count. A count would
        // repeat an id the moment one line failed to parse, and two milestones with the same
        // id is a timeline that cannot be pointed at.
        let existing = self.milestones(&new.project)?;
        let seq = next_sequence(&new.project, &existing);
        let id = format!("{}-{seq}", new.project);
        if existing.iter().any(|m| m.id == id) {
            return Err(Error::Refused(format!("{id} already exists")));
        }

        let milestone = Milestone {
            id,
            project: new.project.clone(),
            at: new.at,
            title: new.title,
            detail: new.detail,
            evidence: new.evidence,
            achievement: new.achievement,
            source: new.source,
        };
        milestone.check()?;

        append_line(
            &self.folder(&new.project).join("milestones.jsonl"),
            &serde_json::to_string(&milestone)?,
        )?;
        Ok(milestone)
    }

    /// Files a proposal. It does not become history and this layer never promotes it.
    pub fn suggest(&self, suggestion: &Suggestion) -> Result<()> {
        if self.get(&suggestion.project)?.is_none() {
            return Err(Error::Refused(format!(
                "there is no project called {}",
                suggestion.project
            )));
        }
        append_line(
            &self.folder(&suggestion.project).join("suggested.jsonl"),
            &serde_json::to_string(suggestion)?,
        )
    }

    pub fn suggestions(&self, id: &ProjectId) -> Result<Vec<Suggestion>> {
        Ok(read_lines(&self.folder(id).join("suggested.jsonl"))?.0)
    }

    /// A project with its recent history, ready to render.
    pub fn view(&self, id: &ProjectId) -> Result<Option<ProjectView>> {
        let Some(project) = self.get(id)? else {
            return Ok(None);
        };
        Ok(Some(ProjectView {
            milestones: self.recent_milestones(id, RECENT)?,
            milestone_gaps: self.milestone_gaps(id)?,
            project,
            active_tasks: Vec::new(),
            active_agents: Vec::new(),
        }))
    }
}

/// One project as the panel wants it, history and live work together.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectView {
    pub project: Project,
    /// Newest first.
    pub milestones: Vec<Milestone>,
    /// Milestone lines that could not be read, so a hole in the history is visible.
    pub milestone_gaps: usize,
    pub active_tasks: Vec<TaskId>,
    pub active_agents: Vec<String>,
}

impl ProjectView {
    /// Attaches the work currently going on for this project.
    ///
    /// Supplied by the caller rather than derived here, and that is a gap rather than a design
    /// choice. **Nothing in the shared army types links a task to a project.** `Task` has a
    /// `workspace: Option<String>` field that is never set, which is the natural place for it.
    /// Until that link exists, guessing which task belongs to which project by matching words
    /// in a goal would be inventing a connection that nobody asserted, so this stays empty
    /// unless an integrator who knows fills it in.
    pub fn with_work(mut self, tasks: Vec<TaskId>, agents: Vec<String>) -> Self {
        self.active_tasks = tasks;
        self.active_agents = agents;
        self
    }

    /// Whether anything is happening on this project right now.
    pub fn is_busy(&self) -> bool {
        !self.active_tasks.is_empty()
    }
}

/// Whether a path looks like a git repository, for a project that names one.
pub fn is_repository(path: &Path) -> bool {
    path.join(".git").exists()
}

/// Whether the process at `pid` is still alive, used by callers tracking work.
pub fn still_running(pid: u32) -> bool {
    process::read(pid).is_some()
}

/// Reads a JSON lines file, returning what parsed and how many lines did not.
fn read_lines<T: serde::de::DeserializeOwned>(path: &Path) -> Result<(Vec<T>, usize)> {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok((Vec::new(), 0)),
        Err(e) => return Err(e.into()),
    };

    let mut parsed = Vec::new();
    let mut unreadable = 0;
    for line in text.lines().filter(|l| !l.trim().is_empty()) {
        match serde_json::from_str(line) {
            Ok(value) => parsed.push(value),
            Err(_) => unreadable += 1,
        }
    }
    Ok((parsed, unreadable))
}

/// The next milestone number for a project, one past the highest already recorded.
fn next_sequence(project: &ProjectId, existing: &[Milestone]) -> usize {
    let prefix = format!("{project}-");
    existing
        .iter()
        .filter_map(|m| m.id.strip_prefix(&prefix))
        .filter_map(|n| n.parse::<usize>().ok())
        .max()
        .unwrap_or(0)
        + 1
}

fn append_line(path: &Path, line: &str) -> Result<()> {
    use std::io::Write;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    // One write call, so two appenders cannot interleave a partial line.
    file.write_all(format!("{line}\n").as_bytes())?;
    file.flush()?;
    Ok(())
}

fn write_atomic(path: &Path, text: &str) -> Result<()> {
    let staging = path.with_extension("json.writing");
    std::fs::write(&staging, text)?;
    std::fs::rename(&staging, path)?;
    Ok(())
}

#[cfg(test)]
mod tests;
