//! The supervisor's records on disk, and the one rule about who may write them.
//!
//! ```text
//!   <home>/run/agents/a-1f2e3d4c5b6a7988.json
//! ```
//!
//! One file per agent that has ever been started, named by the durable id. Written through the
//! same staging and rename the agent folders use, so a supervisor killed mid write leaves the
//! previous record rather than half of a new one. A half written record is worse here than
//! almost anywhere else, because the thing that reads it next is the thing that decides whether
//! to start another process.
//!
//! **Reading does not write.** The panel reads this directory to answer whether an agent has a
//! process, and a panel that created `run/agents/` by looking at it would turn "no supervisor has
//! ever run here" into "a supervisor ran here and started nobody". Those are different facts and
//! only one of them is true.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::Result;
use crate::army::personnel::AgentId;

use super::record::Runtime;

/// Where the supervisor keeps its records, under a home.
pub fn dir(home: &Path) -> PathBuf {
    home.join("run").join("agents")
}

/// The file for one agent.
pub fn path(home: &Path, agent: &AgentId) -> PathBuf {
    dir(home).join(format!("{agent}.json"))
}

/// Every record the supervisor has written, read from a home.
///
/// Holds no handles and caches nothing. A supervisor's own view of what it is running lives in
/// the supervisor, in memory, because it includes pipes that cannot be written down.
#[derive(Debug, Default)]
pub struct Roll {
    records: BTreeMap<AgentId, Runtime>,
}

impl Roll {
    /// Reads every record under a home.
    ///
    /// A missing directory is an empty roll rather than an error, and it is not created. A file
    /// that will not parse is an error naming it, because a supervisor that skipped one would
    /// go on to start a second process for an agent that already has one.
    pub fn open(home: &Path) -> Result<Self> {
        let root = dir(home);
        let mut records = BTreeMap::new();

        let listing = match std::fs::read_dir(&root) {
            Ok(l) => l,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Self { records }),
            Err(e) => return Err(e.into()),
        };

        for entry in listing.flatten() {
            let path = entry.path();
            if path.extension().is_none_or(|e| e != "json") {
                continue;
            }
            let record: Runtime = crate::army::personnel::read_json(&path)?;
            records.insert(record.agent.clone(), record);
        }

        Ok(Self { records })
    }

    pub fn get(&self, agent: &AgentId) -> Option<&Runtime> {
        self.records.get(agent)
    }

    pub fn len(&self) -> usize {
        self.records.len()
    }

    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    pub fn all(&self) -> impl Iterator<Item = &Runtime> {
        self.records.values()
    }

    /// Writes one record, replacing whatever was there.
    ///
    /// The only write in this module, and the supervisor is the only caller of it. Everything
    /// else in the process of running an army reads.
    pub fn save(&mut self, home: &Path, record: Runtime) -> Result<()> {
        std::fs::create_dir_all(dir(home))?;
        crate::army::personnel::write_json(&path(home, &record.agent), &record)?;
        self.records.insert(record.agent.clone(), record);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::army::runtime::record::Lifecycle;

    fn id() -> AgentId {
        AgentId::fresh().unwrap()
    }

    #[test]
    fn a_record_survives_the_process_that_wrote_it() {
        let d = tempfile::tempdir().unwrap();
        let agent = id();

        let mut roll = Roll::open(d.path()).unwrap();
        let mut record = Runtime::never(agent.clone(), "nora", 100);
        record.lifecycle = Lifecycle::Running {
            pid: 5,
            started: 900,
            since: 100,
        };
        roll.save(d.path(), record.clone()).unwrap();
        drop(roll);

        let after = Roll::open(d.path()).unwrap();
        assert_eq!(after.get(&agent), Some(&record));
    }

    #[test]
    fn saving_the_same_agent_twice_replaces_rather_than_doubles() {
        let d = tempfile::tempdir().unwrap();
        let agent = id();
        let mut roll = Roll::open(d.path()).unwrap();

        roll.save(d.path(), Runtime::never(agent.clone(), "nora", 1))
            .unwrap();
        roll.save(d.path(), Runtime::never(agent.clone(), "nora", 2))
            .unwrap();

        let after = Roll::open(d.path()).unwrap();
        assert_eq!(after.len(), 1);
        assert_eq!(after.get(&agent).unwrap().updated_at, 2);
    }

    /// "No supervisor has ever run here" and "a supervisor ran here and started nobody" are
    /// different facts, and a reader that creates the directory destroys the first one.
    #[test]
    fn reading_a_home_with_no_supervisor_creates_nothing() {
        let d = tempfile::tempdir().unwrap();
        let roll = Roll::open(d.path()).unwrap();
        assert!(roll.is_empty());
        assert!(!dir(d.path()).exists(), "and left no directory behind");
    }

    /// Skipping an unreadable record would mean deciding this agent has never been started,
    /// and then starting a second process for one that already has one.
    #[test]
    fn a_record_that_will_not_parse_is_an_error_naming_the_file() {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir(d.path())).unwrap();
        std::fs::write(dir(d.path()).join("a-00112233445566aa.json"), "{ not json").unwrap();

        let err = Roll::open(d.path()).unwrap_err().to_string();
        assert!(err.contains("a-00112233445566aa.json"), "{err}");
    }

    /// A staging file from an interrupted write is not a record, and neither is anything else
    /// somebody drops in there.
    #[test]
    fn something_that_is_not_a_record_file_is_ignored_rather_than_read() {
        let d = tempfile::tempdir().unwrap();
        let mut roll = Roll::open(d.path()).unwrap();
        roll.save(d.path(), Runtime::never(id(), "nora", 1))
            .unwrap();
        std::fs::write(dir(d.path()).join("a-1.json.writing"), "{ half").unwrap();

        assert_eq!(Roll::open(d.path()).unwrap().len(), 1);
    }
}
