use super::*;
use crate::core::PeerView;
use crate::model::{Peer, Permissions};

fn id(n: u128) -> uuid::Uuid {
    uuid::Uuid::from_u128(n)
}

/// Studio, with garden-planner (10), Desk (2) and Laptop (3) paired, and
/// computer `selected` chosen.
fn state(selected: u128, online: bool) -> UiState {
    let me = r#"{"id":"00000000-0000-0000-0000-000000000001","name":"Studio",
        "projects_folder":"/Users/jdoe/Dev"}"#;
    let mut s = UiState::new(serde_json::from_str(me).unwrap(), 0);
    let all = Permissions {
        may_push_to_me: true,
        may_pull_from_me: true,
    };
    for (n, name) in [(2, "Desk"), (3, "Laptop")] {
        let peer = Peer {
            id: id(n),
            name: name.into(),
            fingerprint: String::new(),
            allows: all,
            granted: all,
            last_address: None,
            via: None,
        };
        s.peers.push(PeerView {
            peer,
            online,
            address: None,
            last_seen_ms: None,
        });
    }
    s.selected_peer = Some(id(selected));
    let project = serde_json::json!({"id": id(10), "name": "garden-planner",
        "folders": [], "primary": id(20), "commands": [], "last_transfer": null});
    s.projects.push(serde_json::from_value(project).unwrap());
    s
}

/// A pull of garden-planner with computer `peer`, picked now.
fn pick(peer: u128) -> Pending {
    Pending {
        kind: TransferKind::Pull,
        project: id(10),
        peer: id(peer),
        since: Instant::now(),
        dismissed: false,
    }
}

#[test]
fn a_pick_that_runs_out_of_time_never_starts_with_another_computer() {
    let s = state(2, true);
    let p = pick(3);
    assert_eq!(step(&p, &s, Some(id(10)), p.since), Step::Wait);
    let later = p.since + Duration::from_secs(11);
    assert_eq!(step(&p, &s, Some(id(10)), later), Step::Drop);
}

#[test]
fn a_pick_that_runs_out_of_time_on_its_computer_leaves_the_reason() {
    let s = state(3, false);
    let p = pick(3);
    let later = p.since + Duration::from_secs(11);
    assert_eq!(step(&p, &s, Some(id(10)), later), Step::Try);
}

#[test]
fn a_pick_never_closes_a_result_while_another_computer_is_selected() {
    let mut s = state(2, true);
    s.transfer = TransferState::Finished(Default::default());
    let p = pick(3);
    assert_eq!(step(&p, &s, Some(id(10)), p.since), Step::Wait);
    s.selected_peer = Some(id(3));
    assert_eq!(step(&p, &s, Some(id(10)), p.since), Step::Dismiss);
}
