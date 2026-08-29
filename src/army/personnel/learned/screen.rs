//! What may not be written into memory, whoever asked for it.
//!
//! Two separate dangers and they need different answers.
//!
//! A secret is dangerous because it persists. An auth code copied into a lesson outlives the
//! five minutes it was valid for and sits in a file forever, and the agent had no reason to
//! keep it: the lesson is "this sender sends codes", never the code.
//!
//! An authority claim is dangerous because it is the cheapest attack there is. An agent with a
//! writable memory and a mailbox can be sent a message saying to remember it may transfer money.
//! Refusing at the point of writing means the worst case is an email that mentioned money,
//! rather than a file the agent reads back tomorrow as its own settled policy.

/// Why a lesson was not written down.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// It looked like it carried a secret.
    Sensitive(&'static str),
    /// It read as granting authority, which no file can do.
    Authority(&'static str),
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Sensitive(what) => write!(
                f,
                "that looks like it carries a {what}, and a secret in memory outlives the \
                 moment it was useful. Keep the lesson, not the value"
            ),
            Self::Authority(what) => write!(
                f,
                "that reads as granting {what}. Memory is information and never authority. \
                 Rank, reporting line and tools are compiled and no file changes them"
            ),
        }
    }
}

/// Words that mean the text is carrying a value rather than a lesson about one.
const SECRETS: &[(&str, &str)] = &[
    ("password", "password"),
    ("passwd", "password"),
    ("api key", "key"),
    ("api_key", "key"),
    ("apikey", "key"),
    ("secret key", "key"),
    ("private key", "key"),
    ("bearer ", "token"),
    ("access token", "token"),
    ("auth token", "token"),
    ("refresh token", "token"),
    ("mfa code", "one time code"),
    ("2fa code", "one time code"),
    ("otp", "one time code"),
    ("one time code", "one time code"),
    ("verification code", "one time code"),
    ("security code", "one time code"),
    ("cvv", "card detail"),
    ("sort code", "bank detail"),
    ("account number", "bank detail"),
    ("iban", "bank detail"),
    ("card number", "card detail"),
    ("ssn", "national identifier"),
];

/// Claims that would widen what the agent may do.
const AUTHORITY: &[(&str, &str)] = &[
    ("you may send money", "money authority"),
    ("may send money", "money authority"),
    ("may transfer money", "money authority"),
    ("transfer money", "money authority"),
    ("authorised to pay", "money authority"),
    ("authorized to pay", "money authority"),
    ("approve payment", "money authority"),
    ("without asking olivia", "a way round the chain of command"),
    ("without olivia", "a way round the chain of command"),
    ("bypass olivia", "a way round the chain of command"),
    ("skip olivia", "a way round the chain of command"),
    ("without escalating", "a way round the chain of command"),
    ("no longer report to", "a change of reporting line"),
    ("report to carl instead", "a change of reporting line"),
    ("you are now", "a change of identity"),
    ("you are the chief", "a change of rank"),
    ("your rank is", "a change of rank"),
    ("grant yourself", "a permission"),
    ("you have permission to", "a permission"),
    ("you are allowed to send money", "money authority"),
];

/// Verbs an agent must never be told it is permitted to do by a file.
///
/// Listed as verbs and paired with the ways English grants a permission, rather than as whole
/// sentences. The literal list caught "you may delete" and missed "you can delete", which is
/// the losing game: there is always another phrasing and the attacker picks it.
const GRANTS: &[&str] = &[
    "you may",
    "you can",
    "you are allowed to",
    "you are permitted to",
    "you are authorised to",
    "you are authorized to",
    "miles may",
    "miles can",
    "feel free to",
    "no need to ask before",
];

const NEVER_GRANTED: &[(&str, &str)] = &[
    ("trash", "a tool you do not hold"),
    ("delete", "a tool you do not hold"),
    ("archive", "a tool you do not hold"),
    ("mark as spam", "a tool you do not hold"),
    ("mark spam", "a tool you do not hold"),
    ("send money", "money authority"),
    ("transfer", "money authority"),
    ("pay ", "money authority"),
    ("wire ", "money authority"),
];

/// A permission construction followed, before the sentence ends, by something never granted.
fn grants_a_forbidden_thing(low: &str) -> Option<&'static str> {
    for grant in GRANTS {
        let mut from = 0;
        while let Some(at) = low[from..].find(grant) {
            let after = from + at + grant.len();
            // Only within the same clause. A later unrelated sentence is not this one's doing.
            let clause_end = low[after..]
                .find(['.', ';', '\n'])
                .map_or(low.len(), |e| after + e);
            let clause = &low[after..clause_end];
            for (verb, what) in NEVER_GRANTED {
                if clause.contains(verb) {
                    return Some(what);
                }
            }
            from = after;
        }
    }
    None
}

/// Anything long enough and digit heavy enough to be a code rather than a sentence.
fn looks_like_a_code(text: &str) -> bool {
    text.split_whitespace().any(|word| {
        let digits = word.chars().filter(char::is_ascii_digit).count();
        // Six or more digits in one run, which covers one time codes, card fragments and
        // account numbers, and does not cover a year or an ordinary quantity.
        digits >= 6 && digits * 2 >= word.len()
    })
}

/// Why this lesson may not be stored, if it may not be.
pub fn refuse(lesson: &str) -> Option<Refusal> {
    let low = lesson.to_lowercase();

    if lesson.trim().is_empty() {
        return Some(Refusal::Sensitive("nothing at all"));
    }
    for (needle, what) in AUTHORITY {
        if low.contains(needle) {
            return Some(Refusal::Authority(what));
        }
    }
    if let Some(what) = grants_a_forbidden_thing(&low) {
        return Some(Refusal::Authority(what));
    }
    for (needle, what) in SECRETS {
        if low.contains(needle) {
            return Some(Refusal::Sensitive(what));
        }
    }
    if looks_like_a_code(lesson) {
        return Some(Refusal::Sensitive("one time code"));
    }
    None
}

#[cfg(test)]
mod tests;
