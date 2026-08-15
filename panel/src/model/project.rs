//! A project, and the milestones that are worth more than the chatter under them.
//!
//! Process 3 discovers these. The panel draws them, and the one editorial rule it holds to is
//! that a milestone is something somebody decided was a milestone. Rendering every commit as
//! one produces a wall where nothing stands out, which is the same as having no milestones at
//! all. So milestones arrive as milestones and the panel never promotes anything into one.

/// Roughly where a project has got to.
///
/// Coarse on purpose. Fine grained phases are a scheduler, and the panel is not one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Planned,
    Building,
    Verifying,
    Paused,
    Done,
    /// The backend has not said. Drawn as a gap rather than guessed at.
    Unknown,
}

impl Phase {
    pub fn label(self) -> &'static str {
        match self {
            Phase::Planned => "PLANNED",
            Phase::Building => "BUILDING",
            Phase::Verifying => "VERIFYING",
            Phase::Paused => "PAUSED",
            Phase::Done => "DONE",
            Phase::Unknown => "UNKNOWN",
        }
    }

    /// How far along, for the phase rail. `None` when nobody has said.
    pub fn step(self) -> Option<usize> {
        match self {
            Phase::Planned => Some(0),
            Phase::Building => Some(1),
            Phase::Verifying => Some(2),
            Phase::Done => Some(3),
            Phase::Paused | Phase::Unknown => None,
        }
    }

    pub const STEPS: [&'static str; 4] = ["PLANNED", "BUILDING", "VERIFYING", "DONE"];
}

/// Something that actually happened, as judged by whoever recorded it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Milestone {
    pub at: u64,
    pub title: String,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Project {
    pub name: String,
    /// One or two sentences. What this is for, not what is being typed today.
    pub goal: String,
    pub phase: Phase,
    /// A line of current status, from whoever owns it.
    pub status: Option<String>,
    /// The agent accountable, when the backend names one.
    pub owner: Option<String>,
    /// The department that owns it, which is a different fact from an accountable agent and is
    /// kept separate rather than folded into `owner`.
    pub department: Option<String>,
    pub active_agents: Vec<String>,
    /// Task ids, joined against `Snapshot::tasks` rather than held twice.
    pub active_tasks: Vec<String>,
    pub blockers: Vec<String>,
    pub milestones: Vec<Milestone>,
    pub next_objective: Option<String>,
}

impl Project {
    pub fn new(name: &str, goal: &str) -> Self {
        Self {
            name: name.to_string(),
            goal: goal.to_string(),
            phase: Phase::Unknown,
            status: None,
            owner: None,
            department: None,
            active_agents: Vec::new(),
            active_tasks: Vec::new(),
            blockers: Vec::new(),
            milestones: Vec::new(),
            next_objective: None,
        }
    }

    /// The most recent milestones, newest first.
    ///
    /// Capped, because the point of a milestone list is the last few things that mattered. A
    /// project a month old with forty of them is a history, and a history belongs somewhere a
    /// person goes looking rather than on the front of a panel.
    pub fn recent_milestones(&self, keep: usize) -> Vec<&Milestone> {
        let mut all: Vec<&Milestone> = self.milestones.iter().collect();
        all.sort_by_key(|m| std::cmp::Reverse(m.at));
        all.truncate(keep);
        all
    }

    pub fn blocked(&self) -> bool {
        !self.blockers.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(n: u64, title: &str) -> Milestone {
        Milestone {
            at: n,
            title: title.into(),
            detail: None,
        }
    }

    /// Newest first, and only the last few, because the front of a panel is not a history.
    #[test]
    fn recent_milestones_are_newest_first_and_capped() {
        let mut p = Project::new("jjtorio", "a factorio mod");
        p.milestones = vec![at(10, "one"), at(50, "three"), at(30, "two")];

        let recent = p.recent_milestones(2);
        assert_eq!(recent.len(), 2);
        assert_eq!(recent[0].title, "three");
        assert_eq!(recent[1].title, "two");
    }

    #[test]
    fn a_project_with_no_milestones_shows_none_rather_than_inventing_one() {
        let p = Project::new("jjtorio", "a factorio mod");
        assert!(p.recent_milestones(5).is_empty());
        assert_eq!(p.phase, Phase::Unknown);
        assert_eq!(p.status, None);
    }

    /// A phase nobody has stated has no position on the rail, so the rail draws no position
    /// rather than defaulting to the start.
    #[test]
    fn an_unstated_phase_has_no_place_on_the_rail() {
        assert_eq!(Phase::Unknown.step(), None);
        assert_eq!(
            Phase::Paused.step(),
            None,
            "paused is not a point of progress"
        );
        assert_eq!(Phase::Planned.step(), Some(0));
        assert_eq!(Phase::Done.step(), Some(3));
    }

    #[test]
    fn blockers_decide_whether_it_is_blocked() {
        let mut p = Project::new("jjtorio", "x");
        assert!(!p.blocked());
        p.blockers.push("no lua interpreter on the machine".into());
        assert!(p.blocked());
    }
}
