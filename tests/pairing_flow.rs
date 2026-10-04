//! The pairing exchange itself: commit-reveal nonces, both users confirming,
//! and every way it can stop without storing anything.

mod pairing_support;

use pairing_support::*;
use project_transfer::identity::{commitment, hex, nonce};
use project_transfer::model::Peer;
use project_transfer::net::{Connection, pair_finish, pair_start};
use project_transfer::protocol::{Request, Response};
use tokio::sync::oneshot;

async fn nothing_stored(i: &Instance) {
    assert!(i.shared.store.load_peers().unwrap().is_empty());
    assert!(i.shared.peers.read().await.is_empty());
}

#[tokio::test]
async fn both_users_confirm_and_both_store_with_the_same_code() {
    let a = instance(None).await;
    let b = instance(Some(perms(true, false))).await;
    let mut conn = Connection::open(b.addr, &a.shared).await.unwrap();
    assert_eq!(conn.peer_id(), id_of(&b).await);
    assert_eq!(conn.peer_fingerprint(), b.shared.identity.fingerprint());
    let started = pair_start(&mut conn, &a.shared, perms(true, true), perms(false, true))
        .await
        .unwrap();
    let code_on_a = started.code.clone();
    let (accepted_tx, accepted_rx) = oneshot::channel();
    // A's user answers only after B accepted, so B waits for A's final word.
    let confirm = async { accepted_rx.await.is_ok() };
    let peer = pair_finish(&mut conn, &a.shared, started, confirm, move || {
        let _ = accepted_tx.send(());
    })
    .await
    .unwrap();
    assert_eq!(*b.codes.lock().unwrap(), vec![code_on_a]);

    // A stores B: what A allows is what it offered; what B granted is B's choice.
    assert_eq!(peer.id, id_of(&b).await);
    assert_eq!(peer.fingerprint, b.shared.identity.fingerprint());
    assert_eq!(peer.allows, perms(false, true));
    assert_eq!(peer.granted, perms(true, false));
    assert_eq!(a.shared.store.load_peers().unwrap(), vec![peer.clone()]);
    assert_eq!(*a.shared.peers.read().await, vec![peer]);

    let on_b: Vec<Peer> = b.shared.store.load_peers().unwrap();
    assert_eq!(on_b.len(), 1);
    assert_eq!(on_b[0].id, id_of(&a).await);
    assert_eq!(on_b[0].fingerprint, a.shared.identity.fingerprint());
    assert_eq!(on_b[0].allows, perms(true, false));
    assert_eq!(on_b[0].granted, perms(false, true));
    assert_eq!(*b.shared.peers.read().await, on_b);
}

#[tokio::test]
async fn codes_differ_between_pairings_of_the_same_computers() {
    let a = instance(None).await;
    let b = instance(Some(perms(false, true))).await;
    let mut codes = Vec::new();
    for _ in 0..3 {
        let mut conn = Connection::open(b.addr, &a.shared).await.unwrap();
        let started = pair_start(&mut conn, &a.shared, perms(false, true), perms(false, true))
            .await
            .unwrap();
        codes.push(started.code.clone());
        pair_finish(&mut conn, &a.shared, started, async { true }, || {})
            .await
            .unwrap();
    }
    // Fresh nonces each time; three equal codes would be a one in 10^12 chance.
    assert!(codes[0] != codes[1] || codes[1] != codes[2], "{codes:?}");
}

#[tokio::test]
async fn cancelling_on_the_starting_side_after_the_other_accepted_stores_nothing() {
    let a = instance(None).await;
    let b = instance(Some(perms(true, true))).await;
    let mut conn = Connection::open(b.addr, &a.shared).await.unwrap();
    let started = pair_start(&mut conn, &a.shared, perms(true, true), perms(true, true))
        .await
        .unwrap();
    let (accepted_tx, accepted_rx) = oneshot::channel();
    let confirm = async {
        let _ = accepted_rx.await;
        false
    };
    let err = pair_finish(&mut conn, &a.shared, started, confirm, move || {
        let _ = accepted_tx.send(());
    })
    .await
    .unwrap_err();
    assert!(err.to_string().contains("cancelled"), "{err}");
    until("B to hear that A cancelled", || {
        !b.ended.lock().unwrap().is_empty()
    })
    .await;
    assert!(b.ended.lock().unwrap()[0].contains("did not confirm the code"));
    nothing_stored(&a).await;
    nothing_stored(&b).await;
}

#[tokio::test]
async fn declined_pairing_stores_nothing() {
    let a = instance(None).await;
    let b = instance(None).await;
    let mut conn = Connection::open(b.addr, &a.shared).await.unwrap();
    let err = pair(&mut conn, &a.shared, perms(true, true), perms(true, true))
        .await
        .unwrap_err();
    assert!(err.to_string().contains("declined"), "{err}");
    assert_eq!(b.codes.lock().unwrap().len(), 1);
    nothing_stored(&a).await;
    nothing_stored(&b).await;
}

#[tokio::test]
async fn a_reveal_that_does_not_match_the_commitment_is_refused() {
    let a = instance(None).await;
    let b = instance(Some(perms(true, true))).await;
    let mut conn = Connection::open(b.addr, &a.shared).await.unwrap();
    let commit = Request::PairCommit {
        commitment: commitment(&nonce()),
        requested: perms(true, true),
        offered: perms(true, true),
    };
    let reply = conn.request(&commit).await.unwrap();
    assert!(matches!(reply, Response::PairNonce { .. }), "{reply:?}");
    let reveal = Request::PairReveal {
        nonce: hex(&nonce()),
    };
    let reason = refusal(conn.request(&reveal).await.unwrap());
    assert!(reason.contains("pairing check failed"), "{reason}");
    assert!(b.codes.lock().unwrap().is_empty(), "no prompt is shown");
    nothing_stored(&a).await;
    nothing_stored(&b).await;
}

#[tokio::test]
async fn a_stranger_answering_is_not_stored_unless_the_user_confirms() {
    // A means to pair with B, but C answers at the address and accepts at once.
    let a = instance(None).await;
    let c = instance(Some(perms(true, true))).await;
    let mut conn = Connection::open(c.addr, &a.shared).await.unwrap();
    let started = pair_start(&mut conn, &a.shared, perms(true, true), perms(true, true))
        .await
        .unwrap();
    // A's user sees a code B does not show, and says so.
    let err = pair_finish(&mut conn, &a.shared, started, async { false }, || {})
        .await
        .unwrap_err();
    assert!(err.to_string().contains("cancelled"), "{err}");
    until("C to hear that A cancelled", || {
        !c.ended.lock().unwrap().is_empty()
    })
    .await;
    nothing_stored(&a).await;
    nothing_stored(&c).await;
}

#[tokio::test]
async fn reveal_and_final_out_of_order_are_refused() {
    let a = instance(None).await;
    let b = instance(Some(perms(true, true))).await;
    let mut conn = Connection::open(b.addr, &a.shared).await.unwrap();
    let final_ = Request::PairFinal { confirmed: true };
    let reason = refusal(conn.request(&final_).await.unwrap());
    assert!(reason.contains("out of order"), "{reason}");
    nothing_stored(&b).await;
}

#[tokio::test]
async fn responder_stores_the_initiators_listening_address() {
    let (a, b) = paired(perms(true, true)).await;
    let a_id = id_of(&a).await;
    let stored = b
        .shared
        .peers
        .read()
        .await
        .iter()
        .find(|p| p.id == a_id)
        .cloned()
        .unwrap();
    assert_eq!(stored.last_address, Some(a.addr));
    // With discovery off, the stored address is all the responder has.
    let conn = Connection::open_peer(stored.last_address.unwrap(), &b.shared, &stored)
        .await
        .unwrap();
    assert_eq!(conn.peer_id(), a_id);
}

#[tokio::test]
async fn a_paired_peer_connecting_refreshes_its_stored_address() {
    let (a, b) = paired(perms(true, true)).await;
    let a_id = id_of(&a).await;
    // Pretend A moved: B remembers a stale port.
    {
        let mut peers = b.shared.peers.write().await;
        let p = peers.iter_mut().find(|p| p.id == a_id).unwrap();
        p.last_address = Some("127.0.0.1:1".parse().unwrap());
    }
    let mut conn = Connection::open(b.addr, &a.shared).await.unwrap();
    conn.close().await;
    let stored = b.shared.store.load_peers().unwrap();
    let p = stored.iter().find(|p| p.id == a_id).unwrap();
    assert_eq!(p.last_address, Some(a.addr));
    let live = b.shared.peers.read().await;
    assert_eq!(
        live.iter().find(|p| p.id == a_id).unwrap().last_address,
        Some(a.addr)
    );
}
