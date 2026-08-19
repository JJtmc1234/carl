//! JJ going straight to one agent, around the chain.
//!
//! The chain is the whole design of the army, so reaching past it has to be a deliberate act
//! rather than a convenient one. That shows up in three places here.
//!
//! It is composed against one named agent and thrown away if JJ moves to another, because an
//! instruction written for Nora must never be sent to Mason because a selection changed under
//! it.
//!
//! Anything that changes what an agent is accountable for is confirmed first. A message is not,
//! because talking to somebody is not an intervention in what they are doing.
//!
//! And it carries JJ's words always. An intervention with no reason attached is one nobody can
//! make sense of a week later, including JJ.

use crate::command::{Command, Intervention, InterventionKind};

use super::{App, Intervening};

impl App {
    /// Starts composing an intervention against whichever agent is selected.
    pub fn begin_intervention(&mut self, kind: InterventionKind) {
        let Some(agent) = self.agent.clone() else {
            return;
        };
        // Keep whatever was typed when only the kind changed, since the words usually still
        // apply and retyping them is the fastest way to make somebody avoid the safe path.
        let body = self
            .intervening
            .as_ref()
            .filter(|i| i.agent == agent)
            .map(|i| i.body.clone())
            .unwrap_or_default();

        self.intervening = Some(Intervening {
            agent,
            kind,
            body,
            confirming: false,
        });
    }

    pub fn cancel_intervention(&mut self) {
        self.intervening = None;
    }

    /// Whether what is composed could be sent as it stands.
    pub fn intervention_ready(&self) -> bool {
        self.intervening
            .as_ref()
            .is_some_and(|i| !i.body.trim().is_empty())
    }

    /// Asks to send it. Forceful kinds come back asking to be confirmed first.
    ///
    /// Returns true when something was actually sent.
    pub fn send_intervention(&mut self) -> bool {
        let Some(current) = self.intervening.clone() else {
            return false;
        };
        if current.body.trim().is_empty() {
            self.notice = Some(("say what you want done first".into(), false));
            return false;
        }

        if current.kind.is_forceful() && !current.confirming {
            if let Some(i) = self.intervening.as_mut() {
                i.confirming = true;
            }
            return false;
        }

        self.submit(Command::Intervene(Intervention {
            agent: current.agent,
            kind: current.kind,
            body: current.body.trim().to_string(),
        }));
        self.intervening = None;
        true
    }

    /// The command that would be sent, without sending it. Used by the confirmation line, so
    /// what JJ is shown and what goes out are built by the same code.
    pub fn pending_intervention(&self) -> Option<Intervention> {
        self.intervening.as_ref().map(|i| Intervention {
            agent: i.agent.clone(),
            kind: i.kind,
            body: i.body.trim().to_string(),
        })
    }

    /// One line saying exactly what is about to happen, for the confirmation.
    pub fn intervention_warning(&self) -> Option<String> {
        let i = self.intervening.as_ref()?;
        if !i.confirming {
            return None;
        }
        Some(match i.kind {
            InterventionKind::Message => format!("Send this to {} directly.", i.agent),
            InterventionKind::ChangeInstruction => format!(
                "This changes what {} is working to, around {}.",
                i.agent,
                boss_of(&i.agent)
            ),
            InterventionKind::StopTask => format!(
                "This stops what {} is doing now. {} decides what happens to the task.",
                i.agent, "The backend"
            ),
            InterventionKind::ReplaceTask => format!(
                "This stops {}'s current task and gives a different one, around {}.",
                i.agent,
                boss_of(&i.agent)
            ),
        })
    }
}

/// Who is being gone around, taken from the real hierarchy rather than assumed.
fn boss_of(agent: &str) -> String {
    carl::army::org::find(agent)
        .and_then(|a| a.reports_to)
        .unwrap_or("the chain")
        .to_string()
}
