//! The tree and its geometry, checked without painting anything.

use super::*;
use std::collections::BTreeMap;

/// A made up organisation with twenty two agents in it, because the real one has five and the
/// requirement is that this screen still works at twenty plus. Five agents will pass any
/// layout; the test that matters is the one the real chain cannot provide yet.
fn big_org() -> BTreeMap<String, Vec<String>> {
    let mut org: BTreeMap<String, Vec<String>> = BTreeMap::new();
    org.insert(
        "carl".into(),
        vec!["adrian".into(), "brenna".into(), "cass".into()],
    );
    for lead in ["adrian", "brenna", "cass"] {
        let subs: Vec<String> = (0..2).map(|s| format!("{lead}-sub{s}")).collect();
        org.insert(lead.into(), subs.clone());
        for sub in subs {
            let workers: Vec<String> = (0..2).map(|w| format!("{sub}-w{w}")).collect();
            org.insert(sub, workers);
        }
    }
    org
}

fn reports_from(org: &BTreeMap<String, Vec<String>>) -> impl Fn(&str) -> Vec<String> + '_ {
    move |name: &str| org.get(name).cloned().unwrap_or_default()
}

fn nothing() -> BTreeSet<String> {
    BTreeSet::new()
}

/// The tree must be the real chain, in order, at the right depths. Hardcoding a second
/// hierarchy in the UI is the thing this test exists to catch.
#[test]
fn the_tree_is_the_real_chain() {
    let nodes = arrange(&army_roots(), &reports_of, &nothing());
    let names: Vec<&str> = nodes.iter().map(|n| n.name.as_str()).collect();

    // Every agent in the table except JJ, who is not in the army. Compared against the table
    // rather than a written out list, because the whole point of this test is that the UI does
    // not keep a second hierarchy of its own.
    let mut expected: Vec<&str> = carl::army::org::everyone()
        .iter()
        .filter(|a| a.rank != carl::army::org::Rank::Human)
        .map(|a| a.name)
        .collect();
    let mut got = names.clone();
    expected.sort_unstable();
    got.sort_unstable();
    assert_eq!(got, expected);

    // Three layers below JJ, so the deepest indent is the worker's. Carl sits at the root
    // because JJ is outside the operational army and is not drawn in it.
    let depths: Vec<usize> = nodes.iter().map(|n| n.depth).collect();
    assert_eq!(*depths.iter().max().unwrap(), 2, "one indent per level");

    for node in &nodes {
        let agent = carl::army::org::require(&node.name).unwrap();
        let want = match agent.rank {
            carl::army::org::Rank::Chief => 0,
            carl::army::org::Rank::Lead => 1,
            _ => 2,
        };
        assert_eq!(
            node.depth, want,
            "{} is drawn at the wrong depth",
            node.name
        );
    }
}

/// JJ is not in the army. He is drawn apart from it, so he must not appear inside the tree
/// and the tree must start at whoever he delegates to.
#[test]
fn the_human_is_outside_the_operational_tree() {
    assert_eq!(command_authority(), vec!["jj".to_string()]);
    assert_eq!(army_roots(), vec!["carl".to_string()]);

    let nodes = arrange(&army_roots(), &reports_of, &nothing());
    assert!(
        !nodes.iter().any(|n| n.name == "jj"),
        "JJ must never be drawn as one of the agents"
    );
    assert_eq!(
        nodes.len() + command_authority().len(),
        carl::army::org::everyone().len(),
        "everybody is drawn exactly once, on one side of the line or the other"
    );
}

/// Everybody in the organisation appears exactly once, so nobody is invisible and nobody
/// is drawn twice.
#[test]
fn everybody_appears_once() {
    let nodes = arrange(&army_roots(), &reports_of, &nothing());
    let mut names: Vec<&str> = nodes.iter().map(|n| n.name.as_str()).collect();
    let before = names.len();
    names.sort_unstable();
    names.dedup();
    assert_eq!(names.len(), before, "somebody is drawn twice");
}

/// Folding a department away must hide everything under it and say how much it hid. A folded
/// department that looks like an empty one is a lie about the size of the organisation.
#[test]
fn collapsing_a_department_hides_its_subtree_and_says_how_much() {
    let org = big_org();
    let reports = reports_from(&org);
    let all = arrange(&["carl".into()], &reports, &nothing());
    assert_eq!(all.len(), 22, "three leads, six subs, twelve workers, carl");

    let mut folded = BTreeSet::new();
    folded.insert("adrian".to_string());
    let some = arrange(&["carl".into()], &reports, &folded);

    assert_eq!(some.len(), all.len() - 6, "adrian's six went away");
    let adrian = some
        .iter()
        .find(|n| n.name == "adrian")
        .expect("still shown");
    assert!(adrian.collapsed);
    assert_eq!(adrian.hidden, 6, "two subs and four workers");
    assert!(!some.iter().any(|n| n.name.starts_with("adrian-")));

    // The others are untouched, so folding one department is not folding the screen.
    assert!(some.iter().any(|n| n.name == "brenna-sub0-w1"));
}

/// A worker has nobody under them, so offering to fold them would be a control that does
/// nothing. And folding somebody with no reports must not claim to have hidden anything.
#[test]
fn only_somebody_with_reports_can_be_folded() {
    let org = big_org();
    let reports = reports_from(&org);
    let mut folded = BTreeSet::new();
    folded.insert("adrian-sub0-w0".to_string());

    let nodes = arrange(&["carl".into()], &reports, &folded);
    let leaf = nodes
        .iter()
        .find(|n| n.name == "adrian-sub0-w0")
        .expect("still there");
    assert!(!leaf.can_collapse());
    assert!(!leaf.collapsed, "a leaf cannot be folded");
    assert_eq!(leaf.hidden, 0);
    assert_eq!(nodes.len(), 22, "nothing was hidden");
}

/// Folding every lead has to bring a twenty two card screen down to something that fits, which
/// is the whole reason the control exists.
#[test]
fn folding_the_leads_shrinks_the_screen_to_the_departments() {
    let org = big_org();
    let reports = reports_from(&org);
    let folded: BTreeSet<String> = ["adrian", "brenna", "cass"]
        .iter()
        .map(|s| s.to_string())
        .collect();

    let nodes = arrange(&["carl".into()], &reports, &folded);
    assert_eq!(nodes.len(), 4, "carl and three departments");
    assert_eq!(nodes.iter().map(|n| n.hidden).sum::<usize>(), 18);
}

/// A blocked agent is given the room to say why. Everybody else gets the compact card, or
/// twenty of them do not fit on any screen.
#[test]
fn a_stopped_agent_gets_a_bigger_card_and_nobody_else_does() {
    let worker = card_height(Some(Rank::Worker), false);
    let lead = card_height(Some(Rank::Lead), false);
    let chief = card_height(Some(Rank::Chief), false);
    let stopped = card_height(Some(Rank::Worker), true);

    assert!(worker < lead && lead < chief, "rank buys a little height");
    assert!(stopped > chief, "a blocker needs the most room of all");
    assert!(
        chief - worker < 24.0,
        "the difference between ranks must stay a nudge rather than a gigantic card"
    );
    assert!(
        worker * 20.0 < 2000.0,
        "twenty workers have to fit inside a tall screen without scrolling"
    );
}

/// Placement has to indent by depth, never overlap two cards, and hand back a height that
/// actually covers everything it placed.
#[test]
fn cards_are_placed_in_order_without_touching_each_other() {
    let org = big_org();
    let nodes = arrange(&["carl".into()], &reports_from(&org), &nothing());
    let heights: Vec<f32> = nodes
        .iter()
        .map(|n| card_height(Some(Rank::Worker), n.name.ends_with("w0")))
        .collect();

    let (placed, total) = place(&nodes, &heights, pos2(100.0, 50.0), 900.0);
    assert_eq!(placed.len(), nodes.len());

    for pair in placed.windows(2) {
        assert!(
            pair[0].rect.bottom() <= pair[1].rect.top(),
            "two cards overlap vertically"
        );
        assert!(
            pair[1].rect.top() - pair[0].rect.bottom() >= GAP - 0.01,
            "two cards are closer than the gap"
        );
    }
    for (p, n) in placed.iter().zip(&nodes) {
        assert_eq!(p.rect.left(), 100.0 + n.depth as f32 * INDENT);
        assert!(
            p.rect.right() <= 100.0 + 900.0 + 0.01,
            "a card overflows its column"
        );
        assert!(
            p.rect.width() >= 120.0,
            "a deep card was squeezed to nothing"
        );
    }
    let last = placed.last().expect("cards");
    assert!(
        (total - (last.rect.bottom() - 50.0)).abs() < 0.01,
        "the reported height does not reach the last card"
    );
}

/// Every card except a root has to know which card it hangs off, or there is nothing to draw
/// a connector between.
#[test]
fn every_card_below_a_root_knows_its_parent() {
    let org = big_org();
    let nodes = arrange(&["carl".into()], &reports_from(&org), &nothing());
    let heights = vec![84.0; nodes.len()];
    let (placed, _) = place(&nodes, &heights, pos2(0.0, 0.0), 800.0);

    for p in &placed {
        let node = &nodes[p.at];
        match node.depth {
            0 => assert!(p.parent.is_none(), "a root reports to nobody on screen"),
            _ => {
                let parent = p.parent.expect("a connector needs two ends");
                assert_eq!(nodes[parent].depth, node.depth - 1);
                assert!(parent < p.at, "a parent is always drawn above its reports");
            }
        }
    }
}

/// The spine of a connector runs in the parent's left margin, so it never crosses the parent's
/// card and never lands inside the child's text.
#[test]
fn the_connector_spine_runs_between_the_two_cards_it_joins() {
    let left = 40.0;
    for depth in 1..5 {
        let x = spine_x(left, depth);
        let parent_left = left + (depth as f32 - 1.0) * INDENT;
        let child_left = left + depth as f32 * INDENT;
        assert!(x > parent_left, "the spine cuts through the parent card");
        assert!(x < child_left, "the spine cuts through the child card");
    }
}
