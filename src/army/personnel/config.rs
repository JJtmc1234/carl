//! How an agent's process is started, as opposed to who the agent is.
//!
//! Who an agent is comes from `army::org` and is compiled in. This is the part that is
//! genuinely per agent, genuinely changes, and genuinely has to survive a restart: which model
//! it runs on, how long it may take, and any standing rule the harness puts in front of it.
//!
//! There is deliberately nothing here about what an agent is allowed to do. Every permission
//! question is answered by `org`, off the table, so a config file cannot widen one.

use serde::{Deserialize, Serialize};

use crate::{Error, Result};

/// Which model an agent runs on.
///
/// A closed set rather than a free string, because a typo in a model name is a process that
/// fails at its first call rather than at load, and by then it is one bad report inside a run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Model {
    #[default]
    #[serde(rename = "claude-opus-5")]
    Opus,
    #[serde(rename = "claude-sonnet-5")]
    Sonnet,
    #[serde(rename = "claude-haiku-4-5")]
    Haiku,
}

impl Model {
    /// What to pass to `claude --model`.
    pub fn id(self) -> &'static str {
        match self {
            Self::Opus => "claude-opus-5",
            Self::Sonnet => "claude-sonnet-5",
            Self::Haiku => "claude-haiku-4-5",
        }
    }
}

impl std::fmt::Display for Model {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.id())
    }
}

/// The longest any one agent may take before it is abandoned.
pub const DEFAULT_DEADLINE: u64 = 600;

/// How one agent's process is started.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub model: Model,
    /// How long this agent may take before it is abandoned.
    pub deadline_secs: u64,
    /// Standing rules the harness puts in front of this agent, beyond its remit.
    #[serde(default)]
    pub rules: Vec<String>,
}

impl Default for Config {
    /// Everybody starts on the strongest model.
    ///
    /// Which agents are worth running cheaper is a judgement about cost that belongs to JJ, so
    /// the default does not quietly make it for him. Change one folder when there is a reason.
    fn default() -> Self {
        Self {
            model: Model::Opus,
            deadline_secs: DEFAULT_DEADLINE,
            rules: Vec::new(),
        }
    }
}

impl Config {
    /// A deadline of zero is an agent abandoned before it starts, and an unbounded one is an
    /// agent that can hold up everything waiting on it. Both are refused at load.
    pub fn check(&self) -> Result<()> {
        if self.deadline_secs == 0 || self.deadline_secs > 3600 {
            return Err(Error::Refused(format!(
                "a deadline must be between 1 and 3600 seconds, got {}",
                self.deadline_secs
            )));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_model_is_written_as_the_id_the_cli_wants() {
        assert_eq!(
            serde_json::to_string(&Model::Opus).unwrap(),
            "\"claude-opus-5\""
        );
        assert_eq!(Model::Opus.id(), "claude-opus-5");
    }

    #[test]
    fn a_model_nobody_ships_is_refused() {
        assert!(serde_json::from_str::<Model>("\"claude-4-turbo\"").is_err());
    }

    #[test]
    fn a_config_round_trips() {
        let before = Config::default();
        let text = serde_json::to_string_pretty(&before).unwrap();
        assert_eq!(serde_json::from_str::<Config>(&text).unwrap(), before);
    }

    #[test]
    fn an_unusable_deadline_is_refused() {
        let at = |deadline_secs| Config {
            deadline_secs,
            ..Config::default()
        };
        assert!(at(0).check().is_err(), "abandoned before it starts");
        assert!(at(3601).check().is_err(), "holds up everything behind it");
        assert!(at(DEFAULT_DEADLINE).check().is_ok());
    }

    /// Config says how an agent runs and nothing about what it may do. A field of that shape
    /// here would be one an agent could widen by editing its own folder, so the file refuses
    /// to load at all rather than loading with it ignored.
    #[test]
    fn a_config_cannot_carry_a_permission() {
        for field in ["granted", "rank", "reports_to", "may_delegate_to"] {
            let mut raw = serde_json::to_value(Config::default()).unwrap();
            raw.as_object_mut()
                .unwrap()
                .insert(field.into(), serde_json::json!(true));
            assert!(
                serde_json::from_value::<Config>(raw).is_err(),
                "{field} should not load"
            );
        }
    }
}
