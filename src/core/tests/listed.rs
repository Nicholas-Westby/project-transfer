//! What this computer lists under "On this network" through paired ones.

use super::paired::*;
use super::*;
use crate::discovery::Discovered;
use crate::protocol::Reachable;

/// Has `on` ask `peer` for its status now, and waits for the answer.
fn poll(on: &Fixture, peer: InstanceId) {
    let since = crate::transfer::projects::now_ms();
    on.core.act(Action::SelectPeer(peer));
    wait_until(&on.core, "a fresh status poll", |s| {
        s.peer(peer)
            .is_some_and(|v| v.online && v.last_seen_ms.is_some_and(|t| t >= since))
    });
}

fn listed(on: &Fixture, id: InstanceId) -> Vec<Discovered> {
    let s = on.core.state();
    s.discovered
        .iter()
        .filter(|d| d.id == id)
        .cloned()
        .collect()
}

/// A connection is passed along at most once, and W reaches V only through
/// M, so W could never ask V to pass one along.
#[test]
fn what_a_computer_reached_through_another_can_reach_is_not_listed() {
    let [w, m, v] = trio();
    pair(&w, &v, None, Some(id(&m)));
    let x = Fixture::new();
    pair(&v, &x, Some(at(&x)), None);
    sees(&v, &x);
    // M listed X before, and V listed another computer back when W still
    // reached V directly.
    let seen = Reachable {
        id: id(&x),
        name: name(&x),
    };
    w.core.core.reachable_through(id(&m), vec![seen]);
    let earlier = Reachable {
        id: uuid::Uuid::new_v4(),
        name: "Tower Calm Wren".into(),
    };
    let earlier_id = earlier.id;
    w.core.core.reachable_through(id(&v), vec![earlier]);
    poll(&w, id(&v));
    assert_eq!(listed(&w, id(&x)), vec![listed_through(&x, id(&m))]);
    assert_eq!(listed(&w, earlier_id), vec![]);
}

/// What it listed could only be reached through it, and pairing that way
/// needs it paired.
#[test]
fn what_an_unpaired_computer_listed_is_no_longer_listed() {
    let [w, m, v] = trio();
    let seen = Reachable {
        id: id(&v),
        name: name(&v),
    };
    w.core.core.reachable_through(id(&m), vec![seen]);
    assert_eq!(listed(&w, id(&v)), vec![listed_through(&v, id(&m))]);
    w.act(Action::Unpair(id(&m)));
    assert_eq!(listed(&w, id(&v)), vec![]);
}
