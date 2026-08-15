//! The army's folders on disk.
//!
//! ```text
//!   <home>/
//!     run/events.jsonl     the journal, owned by army::event and only ever appended through it
//!     army/
//!       carl/
//!         profile.json     department and what he does not do. Rarely changes.
//!         config.json      which model, how long. Cheap to change.
//!         state.json       what he is holding now. Written every turn.
//!         README.md        the same thing in English. Generated, never read back.
//!       adrian/  mason/  nora/
//! ```
//!
//! Three files rather than one because they change at completely different rates, and because
//! it lets the file that is written constantly be the file with nothing valuable in it.
//!
//! JJ has no folder. He is the authority the army answers to rather than a member of it, so a
//! directory named after him is refused rather than quietly loaded.
//!
//! The README is generated from `org` and the other two files every time an agent is saved,
//! and is never parsed. A file the program reads is a file worth editing, and this one is
//! meant to be worth reading instead.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::army::org::{self, Agent};
use crate::army::Rank;
use crate::{Error, Result};

use super::config::Config;
use super::files::{read_json, write_json};
use super::profile::Profile;
use super::state::State;

/// Everything one agent's folder holds, next to who the table says they are.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Folder {
    /// Who this is, from the compiled table. Not stored on disk and not editable.
    pub agent: &'static Agent,
    pub profile: Profile,
    pub config: Config,
    pub state: State,
}

/// The army's folders, loaded and checked.
#[derive(Debug)]
pub struct Personnel {
    home: PathBuf,
    records: BTreeMap<&'static str, Folder>,
}

impl Personnel {
    /// Reads every agent folder under `<home>/army/`.
    ///
    /// An agent with no folder is not an error. The hierarchy is compiled in, so a missing
    /// folder means that agent has no state yet rather than that the organisation is broken.
    /// `missing` is how you ask.
    pub fn open(home: impl Into<PathBuf>) -> Result<Self> {
        let home = home.into();
        let root = home.join("army");
        std::fs::create_dir_all(&root)?;

        let mut records = BTreeMap::new();
        for entry in std::fs::read_dir(&root)? {
            let entry = entry?;
            if !entry.file_type()?.is_dir() {
                continue;
            }

            // Every boundary goes through `require`, so a folder named after nobody is an
            // error naming who does exist rather than a directory silently skipped.
            let folder = entry.file_name().to_string_lossy().into_owned();
            let agent = org::require(&folder)?;
            if agent.rank == Rank::Human {
                return Err(Error::Refused(format!(
                    "{} is not an agent and has no folder",
                    agent.name
                )));
            }

            records.insert(agent.name, load(agent, &entry.path())?);
        }

        Ok(Self { home, records })
    }

    /// An army with no folders written yet. Only founding uses this.
    pub(super) fn empty(home: impl Into<PathBuf>) -> Result<Self> {
        let home = home.into();
        std::fs::create_dir_all(home.join("army"))?;
        Ok(Self {
            home,
            records: BTreeMap::new(),
        })
    }

    pub fn home(&self) -> &Path {
        &self.home
    }

    pub fn army_root(&self) -> PathBuf {
        self.home.join("army")
    }

    /// Where the journal lives.
    ///
    /// Outside `army/` on purpose, so every directory under `army/` is an agent and anything
    /// else in there is a mistake worth refusing.
    pub fn journal_path(&self) -> PathBuf {
        self.home.join("run").join("events.jsonl")
    }

    pub fn folder(&self, name: &str) -> PathBuf {
        self.army_root().join(name)
    }

    pub fn names(&self) -> Vec<&'static str> {
        self.records.keys().copied().collect()
    }

    pub fn len(&self) -> usize {
        self.records.len()
    }

    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    pub fn get(&self, name: &str) -> Option<&Folder> {
        self.records.get(name)
    }

    pub fn profile(&self, name: &str) -> Option<&Profile> {
        self.get(name).map(|r| &r.profile)
    }

    pub fn config(&self, name: &str) -> Option<&Config> {
        self.get(name).map(|r| &r.config)
    }

    pub fn state(&self, name: &str) -> Option<&State> {
        self.get(name).map(|r| &r.state)
    }

    /// Everybody in the organisation who has no folder yet. JJ is not one of them.
    pub fn missing(&self) -> Vec<&'static Agent> {
        org::everyone()
            .iter()
            .filter(|a| a.rank != Rank::Human && !self.records.contains_key(a.name))
            .collect()
    }

    /// Writes one agent's state, and nothing else.
    ///
    /// The only write an agent's own run ever needs. It cannot reach the profile or the config
    /// because this function does not open those files, and it could not reach the rank or the
    /// reporting line even if it tried, because neither is on disk.
    pub fn save_state(&mut self, name: &str, state: State) -> Result<()> {
        let agent = org::require(name)?;
        let Some(record) = self.records.get_mut(agent.name) else {
            return Err(Error::Refused(format!("{name} has no folder yet")));
        };
        record.state = state;
        write_json(&self.home.join("army").join(agent.name).join("state.json"), &record.state)
    }

    /// Changes one agent's state in place and writes it.
    pub fn update_state(&mut self, name: &str, change: impl FnOnce(&mut State)) -> Result<()> {
        let agent = org::require(name)?;
        let Some(record) = self.records.get(agent.name) else {
            return Err(Error::Refused(format!("{name} has no folder yet")));
        };
        let mut state = record.state.clone();
        change(&mut state);
        self.save_state(agent.name, state)
    }

    /// Writes a folder for an agent that does not have one.
    ///
    /// Everything is checked before a byte is written, so a refused agent leaves nothing on
    /// disk to half load next time.
    pub(super) fn install(&mut self, record: Folder) -> Result<()> {
        let name = record.agent.name;
        if self.records.contains_key(name) {
            return Err(Error::Refused(format!("{name} already has a folder")));
        }
        if record.agent.rank == Rank::Human {
            return Err(Error::Refused(format!(
                "{name} is not an agent and has no folder"
            )));
        }
        record.profile.check()?;
        record.config.check()?;

        let folder = self.folder(name);
        std::fs::create_dir_all(&folder)?;
        write_json(&folder.join("profile.json"), &record.profile)?;
        write_json(&folder.join("config.json"), &record.config)?;
        write_json(&folder.join("state.json"), &record.state)?;

        self.records.insert(name, record);
        self.write_readme(name)
    }

    /// Rewrites the English summary of one agent from what is now true.
    pub fn write_readme(&self, name: &str) -> Result<()> {
        let Some(record) = self.records.get(name) else {
            return Err(Error::Refused(format!("{name} has no folder yet")));
        };
        std::fs::write(self.folder(name).join("README.md"), super::describe(record))?;
        Ok(())
    }
}

/// Reads one agent's folder.
fn load(agent: &'static Agent, folder: &Path) -> Result<Folder> {
    let record = Folder {
        agent,
        profile: read_json(&folder.join("profile.json"))?,
        config: read_json(&folder.join("config.json"))?,
        state: read_json(&folder.join("state.json"))?,
    };
    record.profile.check()?;
    record.config.check()?;
    Ok(record)
}

