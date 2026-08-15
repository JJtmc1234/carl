//! A scripted army, so the interface can be built and judged before a backend exists.
//!
//! This is not filler. Every transition the panel has to handle well is in the script and
//! happens on a clock: a worker picking up a task, a review landing, a blocker appearing,
//! Carl asking JJ something, Carl streaming an answer a word at a time, a diagnostic turning
//! amber, a milestone arriving, and the link dropping and coming back.
//!
//! The reason to script it rather than randomise it is that a bug in a live panel is a bug you
//! have to catch in the two seconds it is on screen. A fixed timeline means the same second of
//! the same run shows the same thing every time, so a wrong colour or a jump in the layout can
//! be looked at twice.
//!
//! The agents are the real ones out of `army::org`. Nothing here invents a hierarchy.

use std::time::{Duration, Instant};

use carl::army::org;
use carl::army::task::{Status, Task, Verification};

use super::{PanelDataSource, PanelEvent};
use crate::command::Command;
use crate::model::{
    AgentStatus, AgentView, Delegation, Link, ProcessState, Snapshot, Speaker, Turn,
};

mod script;

/// Where the clock starts, so every timestamp in the mock is stable and readable.
const EPOCH: u64 = 1_760_000_000;

pub struct MockPanelDataSource {
    began: Instant,
    /// Which scripted beats have already been handed out.
    fired: usize,
    /// Everything submitted, so tests can assert what the UI asked for.
    pub sent: Vec<Command>,
    link: Link,
    snapshot: Snapshot,
    /// Beats queued by a submitted command rather than by the clock.
    replies: Vec<(Duration, PanelEvent)>,
}

impl Default for MockPanelDataSource {
    fn default() -> Self {
        Self::new()
    }
}

impl MockPanelDataSource {
    pub fn new() -> Self {
        Self {
            began: Instant::now(),
            fired: 0,
            sent: Vec::new(),
            link: Link::Live,
            snapshot: opening_state(),
            replies: Vec::new(),
        }
    }

    /// How long the mock has been running. Tests drive this instead of sleeping.
    pub fn elapsed(&self) -> Duration {
        self.began.elapsed()
    }

    /// Pretends the given amount of time has passed, for tests.
    pub fn advance(&mut self, by: Duration) {
        self.began -= by;
    }

    /// Everything the script has due by now and has not already given out.
    fn due(&mut self) -> Vec<PanelEvent> {
        let now = self.began.elapsed();
        let beats = script::timeline();
        let mut out = Vec::new();

        while self.fired < beats.len() && beats[self.fired].0 <= now {
            out.push(beats[self.fired].1.clone());
            self.fired += 1;
        }

        let mut still = Vec::new();
        for (at, event) in std::mem::take(&mut self.replies) {
            if at <= now {
                out.push(event);
            } else {
                still.push((at, event));
            }
        }
        self.replies = still;
        out
    }

    /// Queues something to come back a moment after a command was sent.
    fn reply_in(&mut self, delay: Duration, event: PanelEvent) {
        self.replies.push((self.began.elapsed() + delay, event));
    }
}

impl PanelDataSource for MockPanelDataSource {
    fn snapshot(&mut self) -> Snapshot {
        self.snapshot.clone()
    }

    fn poll(&mut self) -> Vec<PanelEvent> {
        let events = self.due();
        for e in &events {
            if let PanelEvent::LinkChanged(l) = e {
                self.link = l.clone();
            }
        }
        events
    }

    fn submit(&mut self, command: Command) -> Result<(), String> {
        // Refused while the link is down, and refused loudly. Queueing it silently would let
        // JJ believe an intervention had been sent when nothing received it.
        if !self.link.is_live() {
            return Err(format!("not sent, {}", self.link.label().to_lowercase()));
        }

        // The echo comes back from the backend rather than being drawn on the way out, which
        // is the same round trip the real source will have.
        match &command {
            Command::SayToCarl(text) => {
                self.reply_in(Duration::from_millis(60), PanelEvent::JjSaid(text.clone()));
                for (n, part) in script::carl_reply(text).into_iter().enumerate() {
                    self.reply_in(
                        Duration::from_millis(400 + 220 * n as u64),
                        PanelEvent::CarlSaid {
                            text: part.0,
                            streaming: part.1,
                        },
                    );
                }
            }
            Command::SetObjective(goal) => {
                self.reply_in(
                    Duration::from_millis(80),
                    PanelEvent::JjSaid(format!("New objective. {goal}")),
                );
                self.reply_in(
                    Duration::from_millis(700),
                    PanelEvent::Delegated(Box::new(Delegation {
                        at: EPOCH + 900,
                        from: "carl".into(),
                        to: "adrian".into(),
                        goal: goal.clone(),
                        task: None,
                    })),
                );
            }
            Command::AnswerDecision { id, .. } => {
                self.reply_in(
                    Duration::from_millis(120),
                    PanelEvent::DecisionSettled { id: id.clone() },
                );
            }
            Command::Intervene(i) => {
                self.reply_in(
                    Duration::from_millis(150),
                    PanelEvent::Recorded(Box::new(script::intervention_record(i))),
                );
            }
            Command::Workspace(_) => {}
        }

        self.sent.push(command);
        Ok(())
    }

    fn link(&self) -> Link {
        self.link.clone()
    }

    fn describe(&self) -> String {
        "mock source, scripted timeline".into()
    }
}

/// The state the panel opens on.
fn opening_state() -> Snapshot {
    let mut agents: Vec<AgentView> = org::everyone()
        .iter()
        .map(|a| {
            let mut v = AgentView::unknown(a.name);
            v.model = Some("claude-opus-5".into());
            v.process = Some(ProcessState::Stopped);
            v.status = AgentStatus::Idle;
            v
        })
        .collect();

    if let Some(carl) = agents.iter_mut().find(|a| a.name == "carl") {
        carl.status = AgentStatus::Working;
        carl.process = Some(ProcessState::Running);
        carl.last_activity = Some("handed the belt planner objective to adrian".into());
        carl.last_activity_at = Some(EPOCH + 40);
        carl.department = Some("office of the chief".into());
    }
    for (name, dept, sub) in [
        ("adrian", Some("coding"), None),
        ("mason", Some("coding"), Some("factorio")),
        ("nora", Some("coding"), Some("factorio")),
    ] {
        if let Some(a) = agents.iter_mut().find(|a| a.name == name) {
            a.department = dept.map(str::to_string);
            a.sub_department = sub.map(str::to_string);
        }
    }
    if let Some(jj) = agents.iter_mut().find(|a| a.name == "jj") {
        jj.status = AgentStatus::Unknown;
        jj.process = None;
        jj.model = None;
    }

    let task = seed_task();
    let nora = agents.iter_mut().find(|a| a.name == "nora").unwrap();
    nora.status = AgentStatus::Idle;
    nora.worktree = Some("/home/jj_tmc/Projects/jjtorio-belts".into());
    nora.branch = Some("belt-throughput".into());
    nora.last_activity = Some("finished the smelting ratio task".into());
    nora.last_activity_at = Some(EPOCH - 600);

    Snapshot {
        agents,
        tasks: vec![task],
        projects: script::projects(EPOCH),
        diagnostics: script::diagnostics(EPOCH),
        conversation: vec![
            Turn {
                at: EPOCH,
                from: Speaker::Jj,
                text: "The belt throughput numbers in the planner are wrong. Sort it out.".into(),
                streaming: false,
            },
            Turn {
                at: EPOCH + 40,
                from: Speaker::Carl,
                text: "Handed to Adrian as a coding objective. He is routing it to Mason, who \
                       owns the Factorio side. I will tell you when it is verified rather than \
                       when it is claimed."
                    .into(),
                streaming: false,
            },
        ],
        decisions: Vec::new(),
        delegations: vec![Delegation {
            at: EPOCH + 40,
            from: "carl".into(),
            to: "adrian".into(),
            goal: "Correct the belt throughput figures and prove it with the project's tests"
                .into(),
            task: None,
        }],
        events: Vec::new(),
        at: EPOCH + 60,
    }
}

fn seed_task() -> Task {
    let mut t = Task::assign(
        "mason",
        "nora",
        "Correct the express belt rate and prove it with run-tests.sh",
        Verification::of([
            "run-tests.sh passes with no failures",
            "the suite fails against the unfixed code",
        ])
        .expect("two conditions"),
    )
    .expect("mason may assign nora");
    t.workspace = Some("/home/jj_tmc/Projects/jjtorio-belts".into());
    let _ = t.advance("nora", Status::InHand);
    t
}

#[cfg(test)]
mod tests;
