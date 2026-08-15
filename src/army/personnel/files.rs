//! Reading and writing one small JSON file, carefully.
//!
//! Two habits, kept in one place so every file in an agent's folder gets both.
//!
//! **Say which file it was.** A serde error names a line and a column, which is no help when
//! four folders hold three files each and the process that opened them has already exited.
//!
//! **Write somewhere else first, then rename.** A crash halfway through a write would
//! otherwise leave an agent with half a folder, which is an agent that will not load next time
//! and takes the run down at the worst moment.

use std::path::Path;

use crate::{Error, Result};

/// Reads one file, saying which file it was when it will not parse.
pub fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| Error::Refused(format!("cannot read {}: {e}", path.display())))?;
    serde_json::from_str(&text)
        .map_err(|e| Error::Refused(format!("{} is not valid: {e}", path.display())))
}

/// Writes to a neighbouring temporary file and renames.
pub fn write_json<T: serde::Serialize>(path: &Path, value: &T) -> Result<()> {
    let staging = path.with_extension("json.writing");
    std::fs::write(&staging, format!("{}\n", serde_json::to_string_pretty(value)?))?;
    std::fs::rename(&staging, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_value_survives_a_write_and_a_read() {
        let d = tempfile::tempdir().unwrap();
        let path = d.path().join("thing.json");
        write_json(&path, &vec!["a", "b"]).unwrap();
        assert_eq!(read_json::<Vec<String>>(&path).unwrap(), ["a", "b"]);
    }

    #[test]
    fn writing_leaves_no_staging_file_behind() {
        let d = tempfile::tempdir().unwrap();
        write_json(&d.path().join("thing.json"), &1).unwrap();
        let left: Vec<_> = std::fs::read_dir(d.path()).unwrap().collect();
        assert_eq!(left.len(), 1, "only the file itself");
    }

    #[test]
    fn a_file_that_will_not_parse_says_which_one_it_was() {
        let d = tempfile::tempdir().unwrap();
        let path = d.path().join("broken.json");
        std::fs::write(&path, "{ not json").unwrap();
        let err = read_json::<Vec<String>>(&path).unwrap_err().to_string();
        assert!(err.contains("broken.json"), "{err}");
    }

    #[test]
    fn a_file_that_is_not_there_says_so() {
        let err = read_json::<u8>(Path::new("/nonexistent/thing.json")).unwrap_err().to_string();
        assert!(err.contains("cannot read"), "{err}");
    }
}
