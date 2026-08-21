//! The army's folders on disk.
//!
//! ```text
//!   <home>/
//!     run/events.jsonl     the journal, owned by army::event and only ever appended through it
//!     run/agents/          the supervisor's process records. Not an agent's to write.
//!     army/
//!       carl/
//!         identity.json    the id and the record that made him. Written once, never again.
//!         profile.json     department and what he does not do. Rarely changes.
//!         config.json      which model, how long. Cheap to change.
//!         state.json       what he is holding now. Written every turn.
//!         memory/          what he keeps between sessions. summary.md is the way in.
//!         README.md        the same thing in English. Generated, never read back.
//!       adrian/  mason/  nora/
//! ```
//!
//! Four files rather than one because they change at completely different rates, and because
//! it lets the file that is written constantly be the file with nothing valuable in it. The
//! order above is the order of how often each is written, and it is the reverse of how much
//! each one matters.
//!
//! JJ has no folder. He is the authority the army answers to rather than a member of it, so a
//! directory named after him is refused rather than quietly loaded.
//!
//! The README is generated from `org` and the other two files every time an agent is saved,
//! and is never parsed. A file the program reads is a file worth editing, and this one is
//! meant to be worth reading instead.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::army::Rank;
use crate::army::org::{self, Agent};
use crate::{Error, Result};

use super::config::Config;
use super::files::{read_json, write_json};
use super::identity::Identity;
use super::memory;
use super::profile::Profile;
use super::state::State;

/// Everything one agent's folder holds, next to who the table says they are.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Folder {
    /// Who this is, from the compiled table. Not stored on disk and not editable.
    pub agent: &'static Agent,
    /// The durable id, and the record that created it.
    ///
    /// Optional because a folder written before identities existed is a real folder holding
    /// real state, and refusing to load it would throw that away to enforce a field it could
    /// not have had. `None` reads as what it is: enlisted, and never given an id. Nothing that
    /// needs an id treats that as one, and the supervisor refuses to start such an agent
    /// rather than inventing one for it.
    pub identity: Option<Identity>,
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
    ///
    /// **Reading does not write.** This used to create `<home>/army` on the way in, which meant
    /// that merely opening a panel on a home where no army had been founded left one behind. A
    /// real panel did exactly that to JJ's home. An empty directory is not much damage, but it
    /// is the difference between "no army has been founded here" and "an army was founded and
    /// everybody left", and a read that changes the answer to the question it was asked is worth
    /// removing rather than working around.
    ///
    /// A home with no army reads as an army with no folders, which is what it is. Founding still
    /// creates the directory, through `empty`.
    pub fn open(home: impl Into<PathBuf>) -> Result<Self> {
        let home = home.into();
        let root = home.join("army");

        let mut records = BTreeMap::new();
        let listing = match std::fs::read_dir(&root) {
            Ok(l) => l,
            // Never founded. Not an error, and not a reason to bring it into existence.
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self { home, records });
            }
            Err(e) => return Err(e.into()),
        };

        for entry in listing {
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

    /// The durable id, for an agent that has one.
    pub fn identity(&self, name: &str) -> Option<&Identity> {
        self.get(name).and_then(|r| r.identity.as_ref())
    }

    /// Where this agent keeps what it wants to survive its session.
    pub fn memory_dir(&self, name: &str) -> PathBuf {
        memory::dir(&self.folder(name))
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
        write_json(
            &self.home.join("army").join(agent.name).join("state.json"),
            &record.state,
        )
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

    /// Every reason this folder would be refused, asked before anything is written.
    ///
    /// Separate from `install` because enlisting announces an agent in the journal before it
    /// writes the folder, and it may only do that once it knows the folder will not be
    /// refused. Announcing first is the house rule everywhere else: a crash after the record
    /// loses an outcome that can be looked up, and a crash before it loses the fact that
    /// anything was attempted at all. That trade only works if a refusal has already happened
    /// by then.
    pub(super) fn may_install(&self, record: &Folder) -> Result<()> {
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
        Ok(())
    }

    /// Writes a folder for an agent that does not have one.
    ///
    /// Everything is checked before a byte is written, so a refused agent leaves nothing on
    /// disk to half load next time.
    pub(super) fn install(&mut self, record: Folder) -> Result<()> {
        let name = record.agent.name;
        self.may_install(&record)?;

        let folder = self.folder(name);
        std::fs::create_dir_all(&folder)?;
        if let Some(identity) = &record.identity {
            write_json(&folder.join("identity.json"), identity)?;
        }
        write_json(&folder.join("profile.json"), &record.profile)?;
        write_json(&folder.join("config.json"), &record.config)?;
        write_json(&folder.join("state.json"), &record.state)?;
        memory::seed(&folder, record.agent)?;

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
    // Absent is allowed and unreadable is not. A folder from before identities existed simply
    // has no file, which is a fact. A file that is there and will not parse is a broken
    // identity, and loading past one would hand back an agent that has an id somewhere and
    // does not know it.
    let identity_path = folder.join("identity.json");
    let identity = match identity_path.exists() {
        true => Some(read_json(&identity_path)?),
        false => None,
    };

    let record = Folder {
        agent,
        identity,
        profile: read_json(&folder.join("profile.json"))?,
        config: read_json(&folder.join("config.json"))?,
        state: read_json(&folder.join("state.json"))?,
    };
    record.profile.check()?;
    record.config.check()?;
    Ok(record)
}
