//! A unit of work somebody is accountable for.
//!
//! Not a `Dispatch`. A dispatch is one instruction sent to one agent and finishes when the
//! model stops talking. A task has a goal, an owner, something that has to be true before it
//! counts as done, and a status that only moves in legal directions. Finishing one task may
//! take several dispatches, a review, and a second attempt.
//!
//! Everything Process 2 drives lives on this type. It defines the states and which moves
//! between them are allowed, and refuses the rest, so a retry loop written next week cannot
//! quietly invent a sixth state or mark something accepted that nobody reviewed.
//!
//! Two decisions worth stating.
//!
//! **Verification is required to create a task, not added at review.** A task with no way to
//! tell whether it worked gets accepted on the strength of a confident summary, which is the
//! failure this whole hierarchy exists to avoid.
//!
//! **Attempts are counted on the task.** A worker who has failed the same task four times is
//! not going to succeed on the fifth, and the only place that is visible is here.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::{Error, ProjectId, Result};

/// Names one task, unique within a run.
///
/// Eight random bytes rather than a counter, because tasks are created by several agents at
/// once and a counter needs somewhere shared to live.
///
/// Serialisable because it appears in the event record, and the record is the only place the
/// history of a task survives a restart.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TaskId(String);

impl TaskId {
    pub fn fresh() -> Result<Self> {
        let mut bytes = [0u8; 8];
        use std::io::Read;
        std::fs::File::open("/dev/urandom")?.read_exact(&mut bytes)?;
        Ok(Self(bytes.iter().map(|b| format!("{b:02x}")).collect()))
    }

    /// An id as quoted back by somebody. Not validated here, because an id that was never
    /// issued has to be looked up and refused anyway.
    pub fn quoted(s: impl Into<String>) -> Self {
        Self(s.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for TaskId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// What has to be true before a task counts as done.
///
/// Written by whoever assigns the work, in their words, and checked by whoever reviews it. Not
/// a script and not a command: the point is that both sides agreed in advance what finished
/// means, and that a reviewer has something to check against other than their own impression.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Verification {
    /// What somebody must be able to observe. One line each.
    pub must: Vec<String>,
}

impl Verification {
    pub fn of(must: impl IntoIterator<Item = impl Into<String>>) -> Result<Self> {
        let must: Vec<String> = must
            .into_iter()
            .map(Into::into)
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();

        if must.is_empty() {
            return Err(Error::Refused(
                "a task needs at least one thing that must be true before it is done. Without \
                 one it gets accepted on the strength of a confident summary."
                    .into(),
            ));
        }
        Ok(Self { must })
    }
}

/// How many times a worker may submit before it goes over the lead's head.
///
/// From the governance feed: a worker gets two correction attempts after the first try, and a
/// third rejection escalates upward with the whole history.
///
/// The number lives here rather than in whoever writes the retry loop, because two people
/// each picking a sensible number is two different numbers, and the one that matters is the
/// one somebody is counting against.
pub const MAX_ATTEMPTS: u32 = 3;

/// Where a task has got to.
///
/// Deliberately small. Every extra state is another edge somebody has to get right, and the
/// ones here are the only distinctions anybody has needed so far.
///
/// Escalation is deliberately not a state. A task that has run out of attempts is still
/// somebody's problem in `ChangesRequested`, and escalating is something a lead *does* about
/// it rather than somewhere the task goes. Making it a state would mean every reader has to
/// handle a seventh case to answer questions that have nothing to do with escalation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    /// Handed to its owner, not started.
    Assigned,
    /// The owner has picked it up.
    InHand,
    /// The owner says it is done and it is waiting on review.
    Submitted,
    /// Reviewed and sent back. The owner still owns it.
    ChangesRequested,
    /// The owner cannot get on with it, and still owns it.
    ///
    /// Not a queue. A blocked task keeps its owner, which is the whole point of having the state
    /// at all: the alternative to blocking is putting the work back and letting somebody else
    /// start it, and then two agents are doing one task and the second one does not know.
    ///
    /// Reached from `InHand`, and it is the owner who says so, because nobody else can tell.
    /// A worker whose process died mid task is left here by its lead rather than by the
    /// supervisor, which owns processes and has no business touching work.
    Blocked,
    /// Reviewed and finished.
    Accepted,
    /// Given up on, by whoever assigned it.
    Abandoned,
}

impl Status {
    /// Whether nothing more will happen to a task in this state.
    pub fn settled(self) -> bool {
        matches!(self, Status::Accepted | Status::Abandoned)
    }

    /// Whether this move is allowed.
    ///
    /// The one that matters is that nothing reaches `Accepted` except from `Submitted`. A
    /// task cannot be marked done without having been offered for review, whoever is asking
    /// and however obvious it looks.
    pub fn may_become(self, next: Status) -> bool {
        use Status::*;
        match (self, next) {
            (Assigned, InHand | Abandoned) => true,
            (InHand, Submitted | Blocked | Abandoned) => true,
            (Submitted, Accepted | ChangesRequested | Abandoned) => true,
            (ChangesRequested, InHand | Abandoned) => true,
            // Back to being worked on, or given up on. Never straight to submitted: a task
            // nobody could get on with is not a task that got done while nobody was looking.
            (Blocked, InHand | Abandoned) => true,
            // Settled is settled. Reopening is a new task, so the history of the old one stays
            // true.
            (Accepted | Abandoned, _) => false,
            _ => false,
        }
    }
}

impl fmt::Display for Status {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Status::Assigned => "assigned",
            Status::InHand => "in hand",
            Status::Submitted => "submitted",
            Status::ChangesRequested => "changes requested",
            Status::Blocked => "blocked",
            Status::Accepted => "accepted",
            Status::Abandoned => "abandoned",
        })
    }
}

/// One piece of accountable work.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Task {
    pub id: TaskId,
    /// What is wanted, in one or two sentences.
    pub goal: String,
    pub verification: Verification,
    pub status: Status,
    /// The agent accountable for doing it.
    pub owner: String,
    /// The agent who assigned it, and who reviews it.
    pub created_by: String,
    /// The task this was split out of, if any.
    pub parent: Option<TaskId>,
    /// How many times the owner has submitted it.
    ///
    /// Somebody who has failed the same task four times is not going to succeed on the fifth,
    /// and this is the only place that is visible.
    pub attempts: u32,
    /// Which project this task is part of, if any.
    ///
    /// Deliberately not `workspace`. A workspace is where the files are, a project is who the
    /// work belongs to, and they answer different questions: two tasks in one worktree can serve
    /// two projects, and one project can span several worktrees. Collapsing them would make the
    /// panel unable to tell a shared checkout from shared ownership.
    ///
    /// Set when the task is created and never afterwards. There is no setter on purpose: a
    /// worker who could move her own task into another project could quietly reassign work
    /// nobody agreed to move.
    ///
    /// A `Task` is never written to disk, so this survives a restart only by riding on the
    /// `Delegated` event. That is why the event carries it too.
    pub project: Option<ProjectId>,
    /// Where the work happens, once coding tasks get their own worktree and branch.
    ///
    /// Carried now and filled later, so adding worktrees does not change this type and every
    /// caller with it.
    pub workspace: Option<String>,
}

impl Task {
    /// Creates a task, refusing anything that could not be reviewed later.
    ///
    /// The delegation check happens here rather than at the caller, so there is one place
    /// where work is created and one place where the chain of command is enforced.
    pub fn assign(
        created_by: &str,
        owner: &str,
        goal: impl Into<String>,
        verification: Verification,
    ) -> Result<Self> {
        super::org::check_delegation(created_by, owner)?;

        let goal = goal.into().trim().to_string();
        if goal.is_empty() {
            return Err(Error::Refused(
                "a task needs a goal. Nobody can do, review or refuse an empty one.".into(),
            ));
        }

        Ok(Self {
            id: TaskId::fresh()?,
            goal,
            verification,
            status: Status::Assigned,
            owner: owner.to_string(),
            created_by: created_by.to_string(),
            parent: None,
            project: None,
            attempts: 0,
            workspace: None,
        })
    }

    /// The same, split out of a task somebody already holds.
    pub fn split_from(
        parent: &Task,
        created_by: &str,
        owner: &str,
        goal: impl Into<String>,
        verification: Verification,
    ) -> Result<Self> {
        let mut task = Self::assign(created_by, owner, goal, verification)?;
        task.parent = Some(parent.id.clone());
        // Inherited rather than asked for. Work split off a project's task is that project's
        // work, and making the caller repeat it would let a lead drop a subtask out of its
        // project by forgetting, which is the failure nobody would notice.
        task.project = parent.project.clone();
        Ok(task)
    }

    /// Says which project this task belongs to.
    ///
    /// Only at creation, before anybody has been handed it. Consuming `self` rather than taking
    /// `&mut self` is what makes that true in the type: there is no way to reach a task somebody
    /// is already holding and move it into a different project.
    pub fn for_project(mut self, project: ProjectId) -> Self {
        self.project = Some(project);
        self
    }

    /// Moves the task on, refusing a move that is not allowed.
    ///
    /// `by` is who is making the move, and it is checked. A worker cannot accept her own work,
    /// which is the single most tempting shortcut in the whole design.
    pub fn advance(&mut self, by: &str, next: Status) -> Result<()> {
        if !self.status.may_become(next) {
            return Err(Error::Refused(format!(
                "a task that is {} cannot become {}",
                self.status, next
            )));
        }

        let allowed = match next {
            // Only the owner does the work.
            Status::InHand | Status::Submitted | Status::Blocked => by == self.owner,
            // Only whoever assigned it decides whether it is done.
            Status::Accepted | Status::ChangesRequested => by == self.created_by,
            // Either side may give up, and it is recorded who did.
            Status::Abandoned => by == self.owner || by == self.created_by,
            Status::Assigned => false,
        };

        if !allowed {
            return Err(Error::Refused(format!(
                "{by} cannot move this task to {next}. It is owned by {} and was assigned by \
                 {}.",
                self.owner, self.created_by
            )));
        }

        if next == Status::Submitted {
            self.attempts += 1;
        }
        self.status = next;
        Ok(())
    }

    pub fn settled(&self) -> bool {
        self.status.settled()
    }

    /// Whether this has been sent back too many times and belongs above its lead now.
    ///
    /// True only when it is actually sitting rejected. A task on its third attempt that has
    /// not been reviewed yet has not failed three times, it has been tried three times, and
    /// escalating it early takes it off the person who is about to finish it.
    pub fn must_escalate(&self) -> bool {
        self.status == Status::ChangesRequested && self.attempts >= MAX_ATTEMPTS
    }

    /// How many more goes the owner has before it goes over the lead's head.
    pub fn attempts_left(&self) -> u32 {
        MAX_ATTEMPTS.saturating_sub(self.attempts)
    }
}

/// Whether this worker may pick up another task.
///
/// From the governance feed: one active task per worker, and the next only after the last is
/// approved. The rule lives here rather than in whoever assigns work, because a worker holding
/// two tasks is the sort of thing each caller would check slightly differently.
///
/// `held` is every task that worker owns, however you are storing them.
pub fn may_take_on(worker: &str, held: &[Task]) -> Result<()> {
    let busy: Vec<&Task> = held
        .iter()
        .filter(|t| t.owner == worker && t.status == Status::InHand)
        .collect();

    match busy.first() {
        None => Ok(()),
        Some(t) => Err(Error::Refused(format!(
            "{worker} already has {} in hand. A worker takes one task at a time, and the next \
             only once that one is approved.",
            t.id
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn verification() -> Verification {
        Verification::of(["cargo test passes", "the new test fails without the fix"]).unwrap()
    }

    fn task() -> Task {
        Task::assign(
            "mason",
            "nora",
            "make the belt counter correct",
            verification(),
        )
        .unwrap()
    }

    #[test]
    fn a_task_records_who_owns_it_and_who_assigned_it() {
        let t = task();
        assert_eq!(t.owner, "nora");
        assert_eq!(t.created_by, "mason");
        assert_eq!(t.status, Status::Assigned);
        assert_eq!(t.attempts, 0);
    }

    /// The chain is enforced where work is created rather than left to whoever remembers.
    #[test]
    fn a_task_cannot_be_assigned_outside_the_chain() {
        let err = Task::assign("carl", "nora", "do it", verification())
            .unwrap_err()
            .to_string();
        assert!(err.contains("cannot hand work straight to"), "{err}");
    }

    /// A task with nothing to check gets accepted on the strength of a confident summary.
    #[test]
    fn a_task_needs_something_that_can_be_checked() {
        assert!(Verification::of(Vec::<String>::new()).is_err());
        assert!(Verification::of(["", "   "]).is_err());
        assert!(Verification::of(["it compiles"]).is_ok());
    }

    #[test]
    fn a_task_needs_a_goal() {
        assert!(Task::assign("mason", "nora", "   ", verification()).is_err());
    }

    /// The whole normal life of a task.
    #[test]
    fn the_ordinary_route_works() {
        let mut t = task();
        assert!(t.advance("nora", Status::InHand).is_ok());
        assert!(t.advance("nora", Status::Submitted).is_ok());
        assert_eq!(t.attempts, 1);
        assert!(t.advance("mason", Status::Accepted).is_ok());
        assert!(t.settled());
    }

    /// The most tempting shortcut in the design.
    #[test]
    fn a_worker_cannot_accept_her_own_work() {
        let mut t = task();
        t.advance("nora", Status::InHand).unwrap();
        t.advance("nora", Status::Submitted).unwrap();

        let err = t.advance("nora", Status::Accepted).unwrap_err().to_string();
        assert!(err.contains("cannot move this task"), "{err}");
        assert_eq!(t.status, Status::Submitted, "and nothing moved");
    }

    /// Nothing reaches accepted without having been offered for review.
    #[test]
    fn nothing_is_accepted_without_being_submitted_first() {
        for from in [Status::Assigned, Status::InHand, Status::ChangesRequested] {
            assert!(
                !from.may_become(Status::Accepted),
                "{from} should not lead straight to accepted"
            );
        }
        assert!(Status::Submitted.may_become(Status::Accepted));
    }

    #[test]
    fn a_rejection_sends_it_back_to_its_owner() {
        let mut t = task();
        t.advance("nora", Status::InHand).unwrap();
        t.advance("nora", Status::Submitted).unwrap();
        t.advance("mason", Status::ChangesRequested).unwrap();

        assert!(t.advance("nora", Status::InHand).is_ok());
        t.advance("nora", Status::Submitted).unwrap();
        assert_eq!(t.attempts, 2, "a second submission is a second attempt");
    }

    /// Reopening a settled task would make its history untrue, so it is a new task instead.
    #[test]
    fn settled_is_settled() {
        let mut t = task();
        t.advance("nora", Status::InHand).unwrap();
        t.advance("nora", Status::Submitted).unwrap();
        t.advance("mason", Status::Accepted).unwrap();

        for next in [Status::InHand, Status::Submitted, Status::ChangesRequested] {
            assert!(t.advance("nora", next).is_err(), "{next} after accepted");
        }
    }

    #[test]
    fn either_side_may_give_up_and_nobody_else_may() {
        let mut a = task();
        assert!(a.advance("nora", Status::Abandoned).is_ok());

        let mut b = task();
        assert!(b.advance("mason", Status::Abandoned).is_ok());

        let mut c = task();
        assert!(c.advance("adrian", Status::Abandoned).is_err());
    }

    /// A split task keeps a line back to what it came from, so a tree of work can be walked.
    #[test]
    fn a_split_task_remembers_its_parent() {
        let big = Task::assign("adrian", "mason", "make JJtorio faster", verification()).unwrap();
        let small = Task::split_from(
            &big,
            "mason",
            "nora",
            "cache the belt lookup",
            verification(),
        )
        .unwrap();

        assert_eq!(small.parent.as_ref(), Some(&big.id));
        assert!(big.parent.is_none());
    }

    /// The governance rule, and the reason it is a shared constant. Two people each picking a
    /// sensible number is two different numbers, and only one of them is being counted.
    #[test]
    fn a_third_rejection_goes_over_the_leads_head() {
        let mut t = task();

        for attempt in 1..=MAX_ATTEMPTS {
            t.advance("nora", Status::InHand).unwrap();
            t.advance("nora", Status::Submitted).unwrap();
            assert_eq!(t.attempts, attempt);

            // Submitted and not yet judged is not a failure.
            assert!(!t.must_escalate(), "attempt {attempt} was not rejected yet");
            t.advance("mason", Status::ChangesRequested).unwrap();
        }

        assert!(t.must_escalate(), "three rejections should go upward");
        assert_eq!(t.attempts_left(), 0);
    }

    /// Escalating early takes the task off somebody who is about to finish it.
    #[test]
    fn a_task_still_being_worked_on_is_not_escalated() {
        let mut t = task();
        t.advance("nora", Status::InHand).unwrap();
        t.advance("nora", Status::Submitted).unwrap();
        t.advance("mason", Status::ChangesRequested).unwrap();

        assert!(!t.must_escalate(), "one rejection is not three");
        assert_eq!(t.attempts_left(), MAX_ATTEMPTS - 1);
    }

    /// One active task per worker, and the next only after the last is approved.
    #[test]
    fn a_worker_takes_one_task_at_a_time() {
        let mut first = task();
        first.advance("nora", Status::InHand).unwrap();

        let err = may_take_on("nora", std::slice::from_ref(&first))
            .unwrap_err()
            .to_string();
        assert!(err.contains("already has"), "{err}");
        assert!(err.contains("one task at a time"), "{err}");

        // Submitted and waiting on review does not count as in hand, because she is not
        // working on it and is free to be given the next one once it is approved.
        first.advance("nora", Status::Submitted).unwrap();
        first.advance("mason", Status::Accepted).unwrap();
        assert!(may_take_on("nora", std::slice::from_ref(&first)).is_ok());
    }

    /// Somebody else being busy says nothing about this worker.
    #[test]
    fn another_workers_task_does_not_block_you() {
        let mut mine = Task::assign("adrian", "mason", "lead something", verification()).unwrap();
        mine.advance("mason", Status::InHand).unwrap();

        assert!(may_take_on("nora", std::slice::from_ref(&mine)).is_ok());
        assert!(may_take_on("mason", std::slice::from_ref(&mine)).is_err());
    }

    #[test]
    fn ids_do_not_repeat() {
        let mut seen = std::collections::BTreeSet::new();
        for _ in 0..500 {
            assert!(seen.insert(TaskId::fresh().unwrap()), "an id repeated");
        }
    }
}
