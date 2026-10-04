//! Reaching another computer through a paired one.

use super::paired::*;
use super::*;
use crate::discovery::DiscoveryEvent;
use crate::protocol::Reachable;

#[test]
fn pairing_never_goes_through_a_computer_this_one_is_no_longer_paired_with() {
    let [w, x, v] = [(); 3].map(|_| Fixture::new());
    // W unpaired X, which still has W, sees V and is paired with it.
    pair(&x, &w, Some(at(&w)), None);
    pair(&x, &v, Some(at(&v)), None);
    pair(&v, &x, Some(at(&x)), None);
    sees(&x, &v);
    sees(&w, &x);
    w.act(Action::Pair {
        target: listed_through(&v, id(&x)),
        requested: ALL,
        offered: ALL,
    });
    wait_until(&w.core, "pairing to fail", |s| {
        s.pairing
            .as_ref()
            .is_some_and(|p| matches!(p.state, PairState::Failed(_)))
    });
    let why = format!("{:?}", w.core.state().pairing.clone().unwrap().state);
    assert!(why.contains("no longer paired"), "{why}");
    assert!(v.core.state().pair_prompt.is_none());
}

#[test]
fn a_connection_passed_along_never_stores_the_relays_address() {
    let [w, m, v] = trio();
    pair(&w, &v, None, Some(id(&m)));
    assert_eq!(connect(&w, id(&v)).unwrap(), Some(id(&m)));
    // W called M's address, which would never reach V.
    assert_eq!(stored(&w, id(&v)).last_address, None);
    assert_eq!(saved(&w, id(&v)).last_address, None);
}

#[test]
fn the_computer_that_passed_it_along_becomes_the_way_there() {
    let [w, m, v] = trio();
    // W reached V directly before; now only M, which says it sees V, can.
    let gone = closed_port();
    pair(&w, &v, Some(gone), None);
    let seen = Reachable {
        id: id(&v),
        name: name(&v),
    };
    w.core.core.reachable_through(id(&m), vec![seen]);
    assert_eq!(connect(&w, id(&v)).unwrap(), Some(id(&m)));
    let on_w = stored(&w, id(&v));
    assert_eq!(on_w.via, Some(id(&m)));
    assert_eq!(on_w.last_address, Some(gone));
    assert_eq!(saved(&w, id(&v)), on_w);
}

#[test]
fn a_computer_this_one_is_no_longer_paired_with_passes_nothing_along() {
    let [w, m, v] = trio();
    // W unpaired M, which still has W and would pass it along.
    forget(&w, id(&m));
    sees(&w, &m);
    pair(&w, &v, None, Some(id(&m)));
    let err = format!("{:#}", connect(&w, id(&v)).unwrap_err());
    let want = format!(
        "{} was reached through a computer this one is no longer paired with. Pair with that \
         computer again first.",
        name(&v)
    );
    assert_eq!(err, want);
}

#[test]
fn a_relay_is_reached_directly_never_through_another_relay() {
    let [w, m, v] = trio();
    // W reaches M only through N, which sees M and would pass W along.
    let n = Fixture::new();
    for (on, other) in [(&w, &n), (&n, &w), (&n, &m), (&m, &n)] {
        pair(on, other, Some(at(other)), None);
    }
    sees(&n, &m);
    pair(&w, &m, None, Some(id(&n)));
    pair(&w, &v, None, Some(id(&m)));
    let err = format!("{:#}", connect(&w, id(&v)).unwrap_err());
    let want = format!(
        "Could not reach {}, which passes the connection along",
        name(&m)
    );
    assert!(err.starts_with(&want), "{err}");
}

#[test]
fn a_relays_refusal_is_the_reason_given() {
    let [w, m, v] = trio();
    // M is paired with V but no longer sees it or knows where it was.
    pair(&m, &v, None, None);
    m.core.core.discovery_event(DiscoveryEvent::Lost(id(&v)));
    // X also says it can reach V, but nothing answers where X was.
    let x = Fixture::new();
    pair(&w, &x, Some(at(&x)), None);
    let x_id = id(&x);
    drop(x);
    let seen = Reachable {
        id: id(&v),
        name: name(&v),
    };
    w.core.core.reachable_through(x_id, vec![seen]);
    pair(&w, &v, None, Some(id(&m)));
    let err = format!("{:#}", connect(&w, id(&v)).unwrap_err());
    let m = name(&m);
    let want = format!(
        "{m}, which passes the connection along, says: {} isn't reachable from {m} right now.",
        name(&v)
    );
    assert_eq!(err, want);
}

/// Only listed, not the usual way there: its failure says less than a real
/// direct attempt, but more than having nowhere to try directly.
#[test]
fn a_relay_that_only_listed_it_is_blamed_only_when_nothing_else_was_tried() {
    let [w, v] = [(); 2].map(|_| Fixture::new());
    // X listed V, but nothing answers where X was.
    let x = Fixture::new();
    pair(&w, &x, Some(at(&x)), None);
    let (x_id, x_name) = (id(&x), name(&x));
    drop(x);
    let seen = Reachable {
        id: id(&v),
        name: name(&v),
    };
    w.core.core.reachable_through(x_id, vec![seen]);
    // V was reached directly before, where nothing answers now.
    let gone = closed_port();
    pair(&w, &v, Some(gone), None);
    let err = format!("{:#}", connect(&w, id(&v)).unwrap_err());
    assert!(
        err.starts_with(&format!("Could not connect to {gone}")),
        "{err}"
    );
    pair(&w, &v, None, None);
    let err = format!("{:#}", connect(&w, id(&v)).unwrap_err());
    let want = format!("Could not reach {x_name}, which passes the connection along");
    assert!(err.starts_with(&want), "{err}");
}

/// The usual way there explains more than an old address of its own.
#[test]
fn the_usual_relay_is_blamed_over_an_old_address() {
    let [w, v] = [(); 2].map(|_| Fixture::new());
    let x = Fixture::new();
    pair(&w, &x, Some(at(&x)), None);
    let (x_id, x_name) = (id(&x), name(&x));
    drop(x);
    pair(&w, &v, Some(closed_port()), Some(x_id));
    let err = format!("{:#}", connect(&w, id(&v)).unwrap_err());
    let want = format!("Could not reach {x_name}, which passes the connection along");
    assert!(err.starts_with(&want), "{err}");
}

/// W and V are still paired; it is M that turns W away.
#[test]
fn a_relay_that_turns_this_computer_away_is_named() {
    let [w, m, v] = trio();
    pair(&w, &v, None, Some(id(&m)));
    forget(&m, id(&w));
    let err = format!("{:#}", connect(&w, id(&v)).unwrap_err());
    let want = format!(
        "{}, which passes the connection along, says: These computers aren't paired. Pair \
         them, then try again.",
        name(&m)
    );
    assert_eq!(err, want);
}

/// Its name comes from its own hello, checked like any other; only the
/// address is the relay's.
#[test]
fn a_new_name_heard_through_a_relay_is_kept() {
    let [w, m, v] = trio();
    pair(&w, &v, None, Some(id(&m)));
    {
        let mut peers = w.core.core.shared.peers.blocking_write();
        let on_w = peers.iter_mut().find(|p| p.id == id(&v)).unwrap();
        on_w.name = "Laptop Quiet Finch".into();
    }
    connect(&w, id(&v)).unwrap();
    let on_w = stored(&w, id(&v));
    assert_eq!(on_w.name, name(&v));
    assert_eq!(on_w.last_address, None);
    assert_eq!(saved(&w, id(&v)), on_w);
}
