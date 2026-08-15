//! The little that an agent's folder knows which `army::org` does not.
//!
//! Deliberately three fields. `org::Agent` already carries the name, the display name, the
//! rank, the reporting line and the remit, and every one of those is compiled in on purpose,
//! so none of them is repeated here. Copying `rank` into a file would undo the best property
//! this layer has, which is that there is nowhere on disk to promote yourself.
//!
//! What is left is the part JJ asked for that the table genuinely does not hold: which
//! department an agent sits in, and what it does not do. The second is the half that gets
//! forgotten, and it is the half that stops an agent helpfully wandering into somebody else's
//! job.

use serde::{Deserialize, Serialize};

use crate::{Error, Result};

/// The department metadata for one agent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    /// The department this agent belongs to. Absent for the chief, who owns all of them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub department: Option<String>,
    /// The sub department, for the agents that sit inside one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sub_department: Option<String>,
    /// What this agent does not do, whatever it is asked.
    #[serde(default)]
    pub does_not: Vec<String>,
}

impl Profile {
    pub fn new(
        department: Option<&str>,
        sub_department: Option<&str>,
        does_not: &[&str],
    ) -> Self {
        Self {
            department: department.map(str::to_string),
            sub_department: sub_department.map(str::to_string),
            does_not: does_not.iter().map(|s| s.to_string()).collect(),
        }
    }

    /// A sub department sits inside a department, so one without the other is a profile
    /// nobody can place.
    pub fn check(&self) -> Result<()> {
        if self.sub_department.is_some() && self.department.is_none() {
            return Err(Error::Refused(
                "a sub department has to sit inside a department".into(),
            ));
        }
        Ok(())
    }

    /// Where this agent sits, in a sentence.
    pub fn place(&self) -> String {
        match (&self.department, &self.sub_department) {
            (Some(dept), Some(sub)) => format!("the {sub} sub department of {dept}"),
            (Some(dept), None) => format!("the {dept} department"),
            _ => "the organisation as a whole".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_profile_round_trips() {
        let before = Profile::new(Some("coding"), Some("factorio"), &["assigns work"]);
        let text = serde_json::to_string_pretty(&before).unwrap();
        assert_eq!(serde_json::from_str::<Profile>(&text).unwrap(), before);
    }

    #[test]
    fn a_sub_department_needs_a_department() {
        let orphan = Profile::new(None, Some("factorio"), &[]);
        assert!(orphan.check().is_err());
        assert!(Profile::new(Some("coding"), Some("factorio"), &[]).check().is_ok());
    }

    #[test]
    fn the_place_reads_as_a_sentence() {
        assert_eq!(
            Profile::new(Some("coding"), Some("factorio"), &[]).place(),
            "the factorio sub department of coding"
        );
        assert_eq!(
            Profile::new(Some("coding"), None, &[]).place(),
            "the coding department"
        );
        assert_eq!(Profile::default().place(), "the organisation as a whole");
    }

    /// Rank and reporting line come from the compiled table. A profile carrying either would
    /// be a second answer to a question that already has one.
    #[test]
    fn a_profile_cannot_carry_rank_or_a_reporting_line() {
        for field in ["rank", "reports_to", "granted"] {
            let mut raw = serde_json::to_value(Profile::default()).unwrap();
            raw.as_object_mut()
                .unwrap()
                .insert(field.into(), serde_json::json!("chief"));
            assert!(
                serde_json::from_value::<Profile>(raw).is_err(),
                "{field} should not load"
            );
        }
    }
}
