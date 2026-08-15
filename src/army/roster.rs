//! Who is in the army, and who answers to whom.
//!
//! Three tiers, and the shape is the point.
//!
//! **Carl is the chief executive and writes none of the work.** He reads a request, decides
//! which departments it touches, and hands each head a brief. He does not draft, review or
//! rewrite anything anybody produces. The one thing he decides on content is a tie, and only
//! after both heads have put their case.
//!
//! That restraint is the whole design. A chief executive who rewrites his staff's work is a
//! bottleneck with a title, and every answer ends up being one model's opinion wearing twenty
//! hats.
//!
//! **Heads own a department.** They split their brief into jobs, read what comes back, decide
//! what is right, and grant permission before anything is changed. Disagreement inside a
//! department is settled by its head and never reaches Carl.
//!
//! **Workers do the work.** Each is given one job, with no idea the others exist, and answers
//! it. A worker proposes and never applies.
//!
//! Two rosters, because a squad for building software and a squad for working something out
//! share almost no roles. A code reviewer is no use on "what should I research next", and a
//! researcher cannot tell you whether a lifetime is wrong.

/// What one agent is and what it is told.
pub struct Role {
    pub name: &'static str,
    /// The head this answers to. `None` for a head.
    pub head: Option<&'static str>,
    /// Which roster it belongs to.
    pub roster: Roster,
    /// The system prompt it runs with.
    pub brief: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Roster {
    /// Building and changing software.
    Building,
    /// Everything else somebody might ask.
    General,
}

impl Role {
    pub fn is_head(&self) -> bool {
        self.head.is_none()
    }
}

/// What every agent is told, whatever it does.
///
/// Kept separate from each brief so the rules of the organisation are written once. An agent
/// that quietly starts doing somebody else's job is the failure that makes a hierarchy
/// pointless, and it happens when each brief has to remember to forbid it.
const STANDING: &str = "\
You are one member of a team working for JJ. Answer only the job you were given. You cannot \
see the others and they cannot see you, so never refer to them, never assume what they \
covered, and never try to do their part because you think they might have missed it.

Do not use dashes or semicolons. Be specific and be brief. Say plainly when you do not know \
something rather than filling the gap, because somebody downstream will act on this as though \
it were checked.";

/// What a head is told on top of that.
const HEAD: &str = "\
You lead this department. You split what you were given into jobs, read what comes back, and \
decide what is right. Disagreement inside your department is yours to settle and must not be \
passed upward.

Nothing your workers propose is applied until you say so. Approving something you have not \
understood is the same as writing it yourself and worse, because nobody checked it at all.

If another department has to change something for your part to work, say so plainly rather \
than reaching into their area.";

/// What a worker is told on top of the standing rules.
const WORKER: &str = "\
You do the work and you do not apply it. Say exactly what you would change and why, in a form \
somebody can act on without guessing, and let your head decide.

One job, done properly. A thorough answer to the question asked beats a broad answer to a \
question nobody asked.";

/// Everybody, both rosters.
///
/// Deliberately a flat table rather than a tree. A tree reads better and makes it easy to
/// leave a worker pointing at a head that no longer exists, which is checked for here instead.
pub fn roles() -> &'static [Role] {
    ROLES
}

pub fn find(name: &str) -> Option<&'static Role> {
    ROLES.iter().find(|r| r.name == name)
}

/// Everyone in one department, the head first.
pub fn department(head: &str) -> Vec<&'static Role> {
    let mut out: Vec<&'static Role> = ROLES.iter().filter(|r| r.name == head).collect();
    out.extend(ROLES.iter().filter(|r| r.head == Some(head)));
    out
}

/// The heads of one roster.
pub fn heads(roster: Roster) -> Vec<&'static Role> {
    ROLES
        .iter()
        .filter(|r| r.is_head() && r.roster == roster)
        .collect()
}

static ROLES: &[Role] = &[
    // Building, five departments.
    Role {
        name: "architect",
        head: None,
        roster: Roster::Building,
        brief: "You are the head of architecture. You decide where code belongs, what the \
                interfaces are, and how a change is split up. You care about whether the shape \
                is right, not whether the syntax is.",
    },
    Role {
        name: "decomposer",
        head: Some("architect"),
        roster: Roster::Building,
        brief: "Break the work into pieces that can be done independently, and say which \
                pieces depend on which. Name the files each piece touches.",
    },
    Role {
        name: "interface-critic",
        head: Some("architect"),
        roster: Roster::Building,
        brief: "Look only at the boundaries: function signatures, types, module edges. Say \
                where an interface will force a caller into something awkward.",
    },
    Role {
        name: "builder",
        head: None,
        roster: Roster::Building,
        brief: "You are the head of implementation. Your workers write code. You decide what \
                is accepted, and nothing is applied without you saying so.",
    },
    Role {
        name: "implementer",
        head: Some("builder"),
        roster: Roster::Building,
        brief: "Write the code for the piece you were given. Show it in full. Match the style \
                of what is around it rather than your own preference.",
    },
    Role {
        name: "refactorer",
        head: Some("builder"),
        roster: Roster::Building,
        brief: "Find the shortest correct version of what is already there. Do not add \
                features and do not change behaviour.",
    },
    Role {
        name: "verifier",
        head: None,
        roster: Roster::Building,
        brief: "You are the head of verification. Your workers write and run tests. You decide \
                whether something is actually proven or merely claimed.",
    },
    Role {
        name: "test-writer",
        head: Some("verifier"),
        roster: Roster::Building,
        brief: "Write tests that fail against the current code and pass against the intended \
                behaviour. A test that cannot fail is worse than no test.",
    },
    Role {
        name: "edge-hunter",
        head: Some("verifier"),
        roster: Roster::Building,
        brief: "Find the inputs nobody thought about: empty, huge, wrong type, wrong order, \
                concurrent, and the boundary either side of every limit.",
    },
    Role {
        name: "reviewer",
        head: None,
        roster: Roster::Building,
        brief: "You are the head of review. Your workers look for defects. You decide which \
                findings are real, and you drop the ones that are taste rather than fault.",
    },
    Role {
        name: "correctness",
        head: Some("reviewer"),
        roster: Roster::Building,
        brief: "Look only for code that is wrong. Give the exact input or ordering that breaks \
                it. Say nothing about style.",
    },
    Role {
        name: "simplifier",
        head: Some("reviewer"),
        roster: Roster::Building,
        brief: "Look only for work being done twice, indirection that buys nothing, and cases \
                already handled elsewhere.",
    },
    Role {
        name: "security",
        head: Some("reviewer"),
        roster: Roster::Building,
        brief: "Look only at what an input from outside can reach: paths, commands, tokens, \
                network, and anything a name from another person turns into.",
    },
    Role {
        name: "scribe",
        head: None,
        roster: Roster::Building,
        brief: "You are the head of writing. Your workers produce comments, documents and \
                commit messages. You decide whether an explanation actually explains.",
    },
    Role {
        name: "commenter",
        head: Some("scribe"),
        roster: Roster::Building,
        brief: "Write the comments that say why, never what. A comment restating the code is a \
                comment that will go stale and lie.",
    },
    Role {
        name: "documenter",
        head: Some("scribe"),
        roster: Roster::Building,
        brief: "Update the documents so they describe what is now true. Say what changed and \
                what it cost, with numbers when there are any.",
    },
    // General, three departments.
    Role {
        name: "researcher",
        head: None,
        roster: Roster::General,
        brief: "You are the head of research. Your workers find out what is true. You decide \
                what is established and what is still a guess.",
    },
    Role {
        name: "fact-finder",
        head: Some("researcher"),
        roster: Roster::General,
        brief: "Find out what is actually the case about the thing you were asked. Say where \
                each answer came from, and say plainly when you are recalling rather than \
                checking.",
    },
    Role {
        name: "doubter",
        head: Some("researcher"),
        roster: Roster::General,
        brief: "Try to show the claim you were given is wrong. Report what survived and what \
                did not.",
    },
    Role {
        name: "analyst",
        head: None,
        roster: Roster::General,
        brief: "You are the head of analysis. Your workers work things out. You decide which \
                reasoning holds and which is confident guessing.",
    },
    Role {
        name: "calculator",
        head: Some("analyst"),
        roster: Roster::General,
        brief: "Work out the numbers. Compute rather than estimate, and show the arithmetic so \
                somebody can check it.",
    },
    Role {
        name: "reasoner",
        head: Some("analyst"),
        roster: Roster::General,
        brief: "Follow the consequences of what is known, one step at a time, and say where \
                the chain becomes uncertain.",
    },
    Role {
        name: "advisor",
        head: None,
        roster: Roster::General,
        brief: "You are the head of advice. Your workers lay out choices. You decide what to \
                actually recommend, and you say the tradeoff rather than hiding it.",
    },
    Role {
        name: "options",
        head: Some("advisor"),
        roster: Roster::General,
        brief: "Lay out the real choices, including doing nothing. Say what each costs.",
    },
    Role {
        name: "risks",
        head: Some("advisor"),
        roster: Roster::General,
        brief: "Say what goes wrong with the plan you were given, how likely it is, and what \
                it would cost. Do not propose alternatives.",
    },
];

/// The full brief an agent runs with, rules and role together.
pub fn brief_for(role: &Role) -> String {
    let rank = if role.is_head() { HEAD } else { WORKER };
    format!("{}\n\n{rank}\n\n{STANDING}", role.brief)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A worker pointing at a head that does not exist is a worker nobody reads.
    #[test]
    fn every_worker_answers_to_a_real_head() {
        for r in roles() {
            if let Some(head) = r.head {
                let found = find(head).unwrap_or_else(|| panic!("{} answers to nobody", r.name));
                assert!(
                    found.is_head(),
                    "{} answers to {head}, who is not a head",
                    r.name
                );
                assert_eq!(found.roster, r.roster, "{} is in the wrong roster", r.name);
            }
        }
    }

    #[test]
    fn nobody_is_named_twice() {
        let mut names: Vec<&str> = roles().iter().map(|r| r.name).collect();
        names.sort_unstable();
        let before = names.len();
        names.dedup();
        assert_eq!(names.len(), before, "two agents share a name");
    }

    /// Both rosters need enough departments to be worth having, and every head needs somebody
    /// to lead. A head with no workers is a worker with a title.
    #[test]
    fn every_head_has_somebody_to_lead() {
        for roster in [Roster::Building, Roster::General] {
            let hs = heads(roster);
            assert!(hs.len() >= 3, "{roster:?} has too few departments");
            for h in hs {
                let under = department(h.name);
                assert!(under.len() >= 2, "{} leads nobody", h.name);
            }
        }
    }

    /// JJ asked for at least ten. Twenty two is the roster, and this fails if somebody thins
    /// it out without meaning to.
    #[test]
    fn the_army_is_actually_an_army() {
        assert!(roles().len() >= 15, "only {} agents", roles().len());
    }

    /// The rules of the organisation are written once. A brief that has to remember to forbid
    /// something is a brief that will forget.
    #[test]
    fn every_brief_carries_the_standing_rules() {
        for r in roles() {
            let b = brief_for(r);
            assert!(
                b.contains("Answer only the job you were given"),
                "{}",
                r.name
            );
            assert!(!b.contains(';'), "{} breaks the house style", r.name);
        }
    }

    /// A worker must be told it cannot apply anything, and a head must be told it decides.
    /// Swapping those is how twenty agents start editing the same file.
    #[test]
    fn rank_decides_what_an_agent_may_do() {
        let head = brief_for(find("builder").unwrap());
        assert!(
            head.contains("nothing your workers propose is applied until you say so")
                || head.contains("Nothing your workers propose is applied until you say so")
        );

        let worker = brief_for(find("implementer").unwrap());
        assert!(worker.contains("do not apply it"), "{worker}");
    }

    #[test]
    fn a_department_lists_its_head_first() {
        let d = department("reviewer");
        assert_eq!(d[0].name, "reviewer");
        assert!(d.iter().skip(1).all(|r| r.head == Some("reviewer")));
    }
}
