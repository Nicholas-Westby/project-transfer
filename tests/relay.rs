//! Three instances on 127.0.0.1: A and B are each paired with M, which tells
//! each about the other and passes connections between them.

mod relay_support;
mod support;

use project_transfer::model::{Direction, InstanceId, Peer};
use project_transfer::net::Connection;
use project_transfer::protocol::{Reachable, Request, Response};
use project_transfer::transfer;
use relay_support::*;
use support::{perms, read, write};

async fn reachable(conn: &mut Connection) -> Vec<Reachable> {
    match conn.request(&Request::Status).await.unwrap() {
        Response::Status { reachable, .. } => reachable,
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn the_relay_lists_the_other_paired_computers_it_sees() {
    let t = trio().await;
    let (a_id, b_id) = (t.a.id().await, t.b.id().await);
    // Paired with M, but M doesn't see it on the network.
    t.m.shared.peers.write().await.push(Peer {
        id: InstanceId::new_v4(),
        name: "Mini Brisk Lynx".into(),
        fingerprint: "ab".repeat(32),
        allows: perms(true, true),
        granted: perms(true, true),
        last_address: Some("127.0.0.1:9".parse().unwrap()),
        via: None,
    });
    // Seen on the network, but not paired with M.
    let stranger = start().await;
    let seen = vec![stranger.addr];
    t.m.shared
        .found
        .write()
        .await
        .insert(stranger.id().await, seen);

    let b = Reachable {
        id: b_id,
        name: stored(&t.m, b_id).await.name,
    };
    assert_eq!(reachable(&mut open(&t.a, &t.m).await).await, vec![b]);
    let a = Reachable {
        id: a_id,
        name: stored(&t.m, a_id).await.name,
    };
    assert_eq!(reachable(&mut open(&t.b, &t.m).await).await, vec![a]);
}

#[tokio::test]
async fn a_computer_pairs_with_another_through_one_both_are_paired_with() {
    let t = trio().await;
    let (a_id, m_id, b_id) = (t.a.id().await, t.m.id().await, t.b.id().await);
    let relay = open(&t.a, &t.m).await;
    let mut conn = Connection::through(relay, b_id, &t.a.shared).await.unwrap();
    assert_eq!(conn.peer_id(), b_id);
    assert_eq!(conn.via(), Some(m_id));
    // B's own certificate: the session runs end to end and M only copies it.
    assert_eq!(conn.peer_fingerprint(), t.b.shared.identity.fingerprint());

    let peer = pair_over(&mut conn, &t.a).await;
    assert_eq!(peer.id, b_id);
    assert_eq!(peer.via, Some(m_id));
    // The address A dialled is M's, which would never reach B.
    assert_eq!(peer.last_address, None);
    assert!(t.a.shared.store.load_peers().unwrap().contains(&peer));
    let on_b = stored(&t.b, a_id).await;
    assert_eq!(on_b.fingerprint, t.a.shared.identity.fingerprint());
    // The call reached B from M's address, which would reach M, not A.
    assert_eq!(on_b.last_address, None);
    assert_eq!(on_b.via, Some(m_id));
    assert!(t.b.shared.store.load_peers().unwrap().contains(&on_b));
}

#[tokio::test]
async fn a_push_through_the_relay_arrives() {
    let t = trio().await;
    let b = pair_through(&t).await;
    let src = t.a.project_dir("app");
    write(&src, "src/main.rs", "fn main() {}");
    // Far larger than a TLS record, so it crosses M in many pieces.
    let seeds = "radish, carrot, kale\n".repeat(20_000);
    write(&src, "seeds.txt", &seeds);
    let p = t.a.add_project("Garden", &[("app", &src)]).await;

    let relay = open(&t.a, &t.m).await;
    let mut conn = Connection::open_peer_through(relay, &t.a.shared, &b)
        .await
        .unwrap();
    let req = t.a.request(&t.b, p.id, Direction::Push).await;
    let preview = transfer::prepare(&mut conn, &t.a.shared, req)
        .await
        .unwrap();
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let summary = transfer::execute(&mut conn, &t.a.shared, preview, tx, Default::default())
        .await
        .unwrap();
    assert!(summary.failures.is_empty(), "{:?}", summary.failures);

    let dest = t.b.dev().join("app");
    assert_eq!(read(&dest, "src/main.rs"), "fn main() {}");
    assert_eq!(read(&dest, "seeds.txt"), seeds);
    let received = t.b.received.lock().unwrap().clone();
    assert_eq!(received, vec![(p.id, t.a.id().await, 2)]);
}

#[tokio::test]
async fn a_pull_through_the_relay_arrives() {
    let t = trio().await;
    let b = pair_through(&t).await;
    let theirs = t.b.project_dir("beds");
    write(&theirs, "plan.txt", "rows");
    // Far larger than a TLS record, so it crosses M in many pieces.
    let sketches = "carrot | kale | radish\n".repeat(20_000);
    write(&theirs, "sketches.txt", &sketches);
    let p = t.b.add_project("Orchard", &[("beds", &theirs)]).await;

    let relay = open(&t.a, &t.m).await;
    let mut conn = Connection::open_peer_through(relay, &t.a.shared, &b)
        .await
        .unwrap();
    let req = t.a.request(&t.b, p.id, Direction::Pull).await;
    let preview = transfer::prepare(&mut conn, &t.a.shared, req)
        .await
        .unwrap();
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let summary = transfer::execute(&mut conn, &t.a.shared, preview, tx, Default::default())
        .await
        .unwrap();
    assert!(summary.failures.is_empty(), "{:?}", summary.failures);

    let mine = t.a.dev().join("beds");
    assert_eq!(read(&mine, "plan.txt"), "rows");
    assert_eq!(read(&mine, "sketches.txt"), sketches);
    let on_a = t.a.project(p.id).await.expect("created on A");
    assert_eq!(on_a.folders[0].local_path.as_deref(), Some(mine.as_path()));
}

#[tokio::test]
async fn the_relay_dials_where_it_sees_the_target_then_where_it_last_reached_it() {
    let t = trio().await;
    let b_id = t.b.id().await;
    // B's old address now belongs to another computer; M sees B elsewhere.
    let stranger = start().await;
    set_stored_address(&t.m, b_id, Some(stranger.addr)).await;
    let relay = open(&t.a, &t.m).await;
    let conn = Connection::through(relay, b_id, &t.a.shared).await;
    assert_eq!(conn.map(|c| c.peer_id()).ok(), Some(b_id));

    // Nothing answers where M sees B now, so it falls back to the stored address.
    set_stored_address(&t.m, b_id, Some(t.b.addr)).await;
    let gone = vec![closed_port().await];
    t.m.shared.found.write().await.insert(b_id, gone);
    let relay = open(&t.a, &t.m).await;
    let conn = Connection::through(relay, b_id, &t.a.shared).await;
    assert_eq!(conn.map(|c| c.peer_id()).ok(), Some(b_id));
}

#[tokio::test]
async fn a_computer_calling_through_the_relay_is_remembered_as_reached_through_it() {
    let t = trio().await;
    let (a_id, m_id) = (t.a.id().await, t.m.id().await);
    let b = pair(&t.a, &t.b).await;
    assert_eq!(stored(&t.b, a_id).await.via, None);

    let relay = open(&t.a, &t.m).await;
    let conn = Connection::open_peer_through(relay, &t.a.shared, &b).await;
    assert_eq!(conn.map(|c| c.peer_id()).ok(), Some(b.id));
    let on_b = stored(&t.b, a_id).await;
    assert_eq!(on_b.via, Some(m_id));
    assert!(t.b.shared.store.load_peers().unwrap().contains(&on_b));

    // Calling directly later leaves the way through M as it is.
    drop(open(&t.a, &t.b).await);
    assert_eq!(stored(&t.b, a_id).await.via, Some(m_id));
}
