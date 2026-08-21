//! The one thing about an agent that never changes.
//!
//! Everything else an agent has is replaceable. Its name could change, its department could
//! change, the model it runs on changes whenever somebody edits `config.json`, and the Claude
//! session behind it is expected to be thrown away and replaced. If any of those were the way
//! an agent is referred to, then replacing one would silently create a second agent, or worse,
//! quietly merge two.
//!
//! So there is an id, it is minted once, and nothing may edit it. The file holds two fields and
//! is meant to stay that small. Anything that can change belongs somewhere else, and the test
//! at the bottom of this file is what stops it drifting.
//!
//! **The second field is a reference, not a copy.** `enlisted` is the sequence number of the
//! journal record that announced this agent. The time, the actor and the wording all live in
//! that record, so this file cannot come to disagree with the record about how the agent got
//! here. A copy would be one failed write away from doing exactly that.
//!
//! Not here, on purpose: the name. An agent's name is expected to become changeable with an
//! append only history, and putting today's name in the immutable file would make the first
//! rename a lie. The name is the folder for now and the id is what outlives it.

use serde::{Deserialize, Serialize};

use crate::{Error, Result};

/// How many random bytes an id carries. Sixteen hex characters is enough that two agents
/// enlisted a second apart will never collide, and short enough to read out loud.
const BYTES: usize = 8;

/// A durable name for one agent, independent of what it is called or which session it runs.
///
/// The `a-` prefix is not decoration. These ids end up in filenames next to session ids and
/// task ids, and a bare string of hex tells whoever is reading a directory nothing about what
/// kind of thing they are looking at.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct AgentId(String);

impl AgentId {
    /// A fresh id, from the kernel.
    ///
    /// The same source `SessionId::fresh` uses, and for the same reason: it is where the uuid
    /// crates get their bytes and it keeps the dependency list short.
    pub fn fresh() -> Result<Self> {
        use std::io::Read;
        let mut b = [0u8; BYTES];
        std::fs::File::open("/dev/urandom")?.read_exact(&mut b)?;
        let hex: String = b.iter().map(|x| format!("{x:02x}")).collect();
        Self::new(format!("a-{hex}"))
    }

    /// Checks an id that came from somewhere else.
    ///
    /// Validated at construction rather than at use, because an id becomes a filename under
    /// `run/agents/`. Anything that could mean "separator" or "parent directory" to a path is
    /// refused here, once, instead of being guarded everywhere a path is built.
    pub fn new(raw: impl Into<String>) -> Result<Self> {
        let raw = raw.into();
        let body = raw.strip_prefix("a-").unwrap_or("");
        let ok = body.len() == BYTES * 2 && body.chars().all(|c| c.is_ascii_hexdigit());
        if ok {
            Ok(Self(raw))
        } else {
            Err(Error::Refused(format!(
                "an agent id is a- followed by {} hex characters, got {raw:?}",
                BYTES * 2
            )))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for AgentId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl TryFrom<String> for AgentId {
    type Error = Error;
    fn try_from(value: String) -> Result<Self> {
        Self::new(value)
    }
}

impl From<AgentId> for String {
    fn from(value: AgentId) -> Self {
        value.0
    }
}

/// The immutable core of one agent, as `identity.json` holds it.
///
/// Written once when the agent is given a folder and never rewritten. There is no function
/// here that changes one, which is the point.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Identity {
    pub id: AgentId,
    /// The sequence number of the journal record that announced this agent.
    pub enlisted: u64,
}

impl Identity {
    pub fn new(id: AgentId, enlisted: u64) -> Self {
        Self { id, enlisted }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fresh_id_is_well_formed_and_not_the_same_twice() {
        let one = AgentId::fresh().unwrap();
        let two = AgentId::fresh().unwrap();
        assert!(one.as_str().starts_with("a-"), "{one}");
        assert_ne!(one, two);
        assert_eq!(AgentId::new(one.to_string()).unwrap(), one);
    }

    /// An id becomes a filename, so the shapes that mean something to a path are refused at
    /// construction rather than guarded at every use.
    #[test]
    fn an_id_that_could_be_a_path_is_refused() {
        for bad in [
            "a-../../etc",
            "a-",
            "",
            "carl",
            "a-zzzzzzzzzzzzzzzz",
            "a-0011223344556677aa",
            "a-00112233/44556677",
        ] {
            assert!(AgentId::new(bad).is_err(), "{bad} should be refused");
        }
    }

    #[test]
    fn an_identity_round_trips() {
        let before = Identity::new(AgentId::fresh().unwrap(), 7);
        let text = serde_json::to_string_pretty(&before).unwrap();
        assert_eq!(serde_json::from_str::<Identity>(&text).unwrap(), before);
    }

    /// The file that never changes is also the file with the most to gain from a forged
    /// extra field, so it refuses anything it does not know rather than ignoring it.
    #[test]
    fn an_identity_file_cannot_carry_anything_else() {
        for field in ["rank", "name", "reports_to", "granted", "model"] {
            let mut raw =
                serde_json::to_value(Identity::new(AgentId::fresh().unwrap(), 1)).unwrap();
            raw.as_object_mut()
                .unwrap()
                .insert(field.into(), serde_json::json!("chief"));
            assert!(
                serde_json::from_value::<Identity>(raw).is_err(),
                "{field} should not load"
            );
        }
    }

    /// The immutable core is meant to stay tiny. Two fields, and adding a third should be a
    /// decision somebody argues for rather than something that happens on a Tuesday.
    #[test]
    fn the_immutable_core_is_two_fields() {
        let raw = serde_json::to_value(Identity::new(AgentId::fresh().unwrap(), 1)).unwrap();
        let keys: Vec<_> = raw.as_object().unwrap().keys().cloned().collect();
        assert_eq!(
            keys,
            ["enlisted", "id"],
            "sorted, as serde_json orders them"
        );
    }
}
