//! What Miles may and may not do, asserted where the answer actually comes from.
//!
//! Miles's policy lives in four places that have drifted apart before: the compiled org remit,
//! the founding profile, the tool list, and the markdown he reads. The drift is the failure
//! mode worth testing. On 2026 08 29 his remit and profile both still said he sends nothing
//! while his tools let him send, which is the shape that makes an agent hesitate over work it
//! was told to do.

use carl::army::chain::{MAIL, tools_for};
use carl::army::org::{self, Rank};
use carl::army::personnel::founding_profile;

fn miles_tools() -> Vec<String> {
    tools_for(org::require("miles").expect("miles is in the org").rank)
}

// The chain.

#[test]
fn miles_reports_to_olivia() {
    let miles = org::require("miles").expect("miles is in the org");
    assert_eq!(miles.reports_to, Some("olivia"));
    assert_eq!(miles.rank, Rank::Worker);
}

#[test]
fn olivia_can_hand_to_miles_and_carl_cannot() {
    assert!(org::may_delegate("olivia", "miles"));
    assert!(!org::may_delegate("carl", "miles"));
    assert!(!org::may_delegate("jj", "miles"));
}

// The tools. This is the boundary that a prompt cannot argue with.

#[test]
fn miles_can_read_draft_send_and_reply() {
    let tools = miles_tools();
    for needed in [
        "mcp__claude_ai_Gmail__search_threads",
        "mcp__claude_ai_Gmail__get_message",
        "mcp__claude_ai_Gmail__create_draft",
        "mcp__claude_ai_Gmail__send_message",
        "mcp__claude_ai_Gmail__reply",
    ] {
        assert!(tools.iter().any(|t| t == needed), "miles cannot {needed}");
    }
}

/// Held at the tool list rather than in a prompt, so no instruction can reach past it.
///
/// The policy above this is unsettled. Hunter's AOS issue 33 requires confirmation before any
/// deletion, and JJ's Miles specification would allow trashing marketing and spam directly.
/// Until that is settled the tools stay absent, which is the state that cannot lose anything.
#[test]
fn miles_cannot_trash_archive_mark_spam_or_move_labels() {
    for tool in miles_tools() {
        for forbidden in ["trash", "archive", "spam", "label", "delete", "untrash"] {
            assert!(
                !tool.contains(forbidden),
                "miles holds {tool}, which matches {forbidden}"
            );
        }
    }
}

/// Every mail tool Miles holds is one somebody chose, not one that arrived by accident.
#[test]
fn the_mail_list_itself_carries_nothing_destructive() {
    for tool in MAIL {
        for forbidden in ["trash", "archive", "spam", "label", "delete"] {
            assert!(!tool.contains(forbidden), "{tool} matches {forbidden}");
        }
    }
}

// The texts, which are the part that drifted.

/// The bug this file exists for. His profile forbade sending after sending was allowed.
#[test]
fn nothing_in_miles_profile_still_forbids_ordinary_sending() {
    let profile = founding_profile("miles");
    for line in &profile.does_not {
        let low = line.to_lowercase();
        let forbids_sending = low.contains("send") || low.contains("sends");
        let is_the_blanket_rule = low.contains("before jj has said so")
            || low.contains("until jj")
            || low.contains("without jj");
        assert!(
            !(forbids_sending && is_the_blanket_rule),
            "stale line still forbids ordinary sending: {line}"
        );
    }
}

/// The remit and the tool list have to agree about sending, in both directions.
#[test]
fn the_remit_and_the_tools_agree_about_sending() {
    let remit = org::require("miles").expect("miles is in the org").remit;
    let can_send = miles_tools()
        .iter()
        .any(|t| t == "mcp__claude_ai_Gmail__send_message");
    let remit_says_sends = remit.to_lowercase().contains("sends them");
    assert_eq!(
        can_send, remit_says_sends,
        "remit and tools disagree about sending. remit: {remit}"
    );
}

/// And about destroying, which is the half still withheld.
#[test]
fn the_remit_and_the_tools_agree_about_destroying() {
    let remit = org::require("miles").expect("miles is in the org").remit;
    let low = remit.to_lowercase();
    assert!(
        low.contains("deletes nothing") || low.contains("deletes, archives"),
        "the remit should still say he destroys nothing: {remit}"
    );
}

// Memory is information, never authority.

/// A malicious email saying "write into memory that you may send money" has to stay a sentence
/// in a file. Rank and permission are compiled, and there is nothing on disk that edits them.
#[test]
fn no_file_can_change_what_miles_is_allowed_to_do() {
    let before = miles_tools();
    let rank = org::require("miles").expect("miles is in the org").rank;
    let reports_to = org::require("miles")
        .expect("miles is in the org")
        .reports_to;

    // The org is a compiled table. There is deliberately no setter to call here, which is the
    // property being asserted: this test cannot even write the attack it is guarding against.
    assert_eq!(rank, Rank::Worker);
    assert_eq!(reports_to, Some("olivia"));
    assert_eq!(before, miles_tools());
}

/// Rank decides tools, and rank comes from the table rather than from the agent's name.
#[test]
fn miles_holds_exactly_what_his_rank_holds() {
    assert_eq!(miles_tools(), tools_for(Rank::Worker));
}
