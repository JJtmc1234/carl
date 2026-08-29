//! Reading and writing the file, which is Markdown because a person is the reader.
//!
//! Round tripping matters more than it looks. The agent writes this file and JJ edits it by
//! hand, so a parse that loses a line loses something somebody wrote on purpose. Both lists are
//! plain bullets, and the only thing separating them is which heading they sit under.

use super::Learned;

const RULES: &str = "## Rules";
const WATCHING: &str = "## Watching";

impl Learned {
    /// The file as it will be written.
    pub(super) fn render(&self) -> String {
        let mut out = String::from(
            "# What I have learned\n\n\
             _Mine, and JJ reads it. Promoted rules only. Everything here is something I worked \
             out or was corrected on._\n\n\
             Nothing in this file grants me anything. My rank, who I report to and which tools \
             I hold come from the organisation, not from anything written here or anywhere \
             else.\n\n",
        );

        out.push_str(RULES);
        out.push_str("\n\n");
        if self.rules.is_empty() {
            out.push_str("Nothing yet.\n");
        } else {
            for rule in &self.rules {
                out.push_str(&format!("- {rule}\n"));
            }
        }

        out.push('\n');
        out.push_str(WATCHING);
        out.push_str(&format!(
            "\n\nNot rules yet. A pattern becomes a rule on the {} separate sighting. \
             A correction from JJ or Olivia becomes one at once.\n\n",
            ordinal(super::PROMOTE_AFTER)
        ));
        if self.watching.is_empty() {
            out.push_str("Nothing yet.\n");
        } else {
            for (seen, what) in &self.watching {
                out.push_str(&format!("- ({seen}) {what}\n"));
            }
        }
        out
    }

    /// Reads a file back. Anything it does not recognise is ignored rather than guessed at.
    pub(super) fn parse(text: &str) -> Self {
        #[derive(PartialEq)]
        enum In {
            Nothing,
            Rules,
            Watching,
        }
        let mut section = In::Nothing;
        let mut me = Self::default();

        for line in text.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with(RULES) {
                section = In::Rules;
                continue;
            }
            if trimmed.starts_with(WATCHING) {
                section = In::Watching;
                continue;
            }
            if trimmed.starts_with("##") {
                section = In::Nothing;
                continue;
            }
            let Some(item) = trimmed.strip_prefix("- ") else {
                continue;
            };
            let item = item.trim();
            if item.is_empty() || item.eq_ignore_ascii_case("nothing yet.") {
                continue;
            }
            match section {
                In::Rules => me.rules.push(item.to_string()),
                In::Watching => {
                    let (seen, what) = count_of(item);
                    me.watching.push((seen, what.to_string()));
                }
                In::Nothing => {}
            }
        }
        me
    }
}

/// `(2) something` becomes `(2, "something")`. A bullet with no count has been seen once.
fn count_of(item: &str) -> (usize, &str) {
    let Some(rest) = item.strip_prefix('(') else {
        return (1, item);
    };
    let Some((digits, tail)) = rest.split_once(')') else {
        return (1, item);
    };
    match digits.trim().parse::<usize>() {
        Ok(n) if n > 0 => (n, tail.trim()),
        _ => (1, item),
    }
}

fn ordinal(n: usize) -> String {
    match n {
        1 => "first".into(),
        2 => "second".into(),
        3 => "third".into(),
        other => format!("{other}th"),
    }
}
