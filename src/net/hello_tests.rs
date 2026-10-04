//! What a hello tells this computer: which version the caller runs, and how
//! it can be called back, at its own address or through a relay.

use super::server::{PairSlot, session};
use super::test_support::{addr, shared};
use super::{NetEvent, Shared};
use crate::identity::{commitment, hex, nonce};
use crate::model::{InstanceId, Peer, Permissions};
use crate::protocol::{PROTOCOL_VERSION, Request, Response, read_msg, write_msg};
use tokio::io::DuplexStream;
use tokio::sync::mpsc::UnboundedReceiver;

/// Runs a session for a call arriving from `remote`; returns the caller's end.
fn answer(s: &Shared, remote: &str) -> DuplexStream {
    let (client, mut server) = tokio::io::duplex(64 * 1024);
    let (s, remote) = (s.clone(), addr(remote));
    tokio::spawn(
        async move { session(&s, &PairSlot::new(), &mut server, "ff".into(), remote).await },
    );
    client
}

/// Says hello as a computer that listens on port 4242, has the addresses
/// `own`, and was passed along by `via`, if anyone.
async fn hello(client: &mut DuplexStream, id: InstanceId, own: &[&str], via: Option<InstanceId>) {
    let hello = Request::Hello {
        id,
        name: "Laptop".into(),
        version: PROTOCOL_VERSION,
        port: 4242,
        addrs: own.iter().map(|ip| ip.parse().unwrap()).collect(),
        via,
    };
    write_msg(client, &hello).await.unwrap();
    let reply: Response = read_msg(client).await.unwrap();
    assert!(matches!(reply, Response::Hello { .. }), "{reply:?}");
}

fn laptop(id: InstanceId, at: &str) -> Peer {
    Peer {
        id,
        name: "Laptop".into(),
        fingerprint: "ff".into(),
        allows: Permissions::default(),
        granted: Permissions::default(),
        last_address: Some(addr(at)),
        via: None,
    }
}

#[tokio::test]
async fn a_hello_from_the_previous_version_is_told_to_update() {
    let dir = tempfile::tempdir().unwrap();
    let (s, _rx) = shared(dir.path());
    let mut client = answer(&s, "10.0.0.9:50000");
    // Protocol 2 sent no addresses of its own.
    let old = serde_json::json!({ "Hello": {
        "id": uuid::Uuid::new_v4(), "name": "Laptop", "version": 2, "port": 4242
    } });
    write_msg(&mut client, &old).await.unwrap();
    let reply: Response = read_msg(&mut client).await.unwrap();
    assert!(
        matches!(&reply, Response::Refused { reason } if reason.contains("different versions")),
        "{reply:?}"
    );
}

#[tokio::test]
async fn a_paired_computer_calling_from_its_own_address_is_remembered_there() {
    let dir = tempfile::tempdir().unwrap();
    let (s, _rx) = shared(dir.path());
    let id = uuid::Uuid::new_v4();
    s.peers.write().await.push(laptop(id, "10.0.0.7:4242"));
    let mut client = answer(&s, "10.0.0.9:50000");
    hello(&mut client, id, &["127.0.0.1", "10.0.0.9"], None).await;
    let stored = s.peers.read().await[0].last_address;
    assert_eq!(stored, Some(addr("10.0.0.9:4242")));
}

/// A call passed along by another computer, or through a router that
/// rewrites addresses, comes from an address that would reach someone else.
#[tokio::test]
async fn a_call_from_an_address_the_caller_does_not_have_leaves_its_address_alone() {
    let dir = tempfile::tempdir().unwrap();
    let (s, _rx) = shared(dir.path());
    let id = uuid::Uuid::new_v4();
    s.peers.write().await.push(laptop(id, "10.0.0.7:4242"));
    let mut client = answer(&s, "192.168.64.1:50000");
    hello(&mut client, id, &["127.0.0.1", "10.0.0.7"], None).await;
    let stored = s.peers.read().await[0].last_address;
    assert_eq!(stored, Some(addr("10.0.0.7:4242")));
}

/// Pairs as a computer that listens on port 4242 and has the addresses
/// `own`, calling from `remote`. Returns what this computer stored.
async fn pair_from(remote: &str, own: &[&str]) -> Peer {
    let dir = tempfile::tempdir().unwrap();
    let (s, mut rx) = shared(dir.path());
    pair_with(&s, &mut rx, remote, own, None).await
}

/// Pairs with `s` as `pair_from` does, passed along by `via`, if anyone.
async fn pair_with(
    s: &Shared,
    rx: &mut UnboundedReceiver<NetEvent>,
    remote: &str,
    own: &[&str],
    via: Option<InstanceId>,
) -> Peer {
    let id = uuid::Uuid::new_v4();
    let mut client = answer(s, remote);
    hello(&mut client, id, own, via).await;
    let mine = nonce();
    let commit = Request::PairCommit {
        commitment: commitment(&mine),
        requested: Permissions::default(),
        offered: Permissions::default(),
    };
    write_msg(&mut client, &commit).await.unwrap();
    let _nonce: Response = read_msg(&mut client).await.unwrap();
    let reveal = Request::PairReveal { nonce: hex(&mine) };
    write_msg(&mut client, &reveal).await.unwrap();
    // This computer's user accepts.
    while let Some(ev) = rx.recv().await {
        if let NetEvent::PairPrompt { reply, .. } = ev {
            reply.send(Some(Permissions::default())).unwrap();
            break;
        }
    }
    let _accepted: Response = read_msg(&mut client).await.unwrap();
    let confirmed = Request::PairFinal { confirmed: true };
    write_msg(&mut client, &confirmed).await.unwrap();
    assert_eq!(
        read_msg::<_, Response>(&mut client).await.unwrap(),
        Response::Ok
    );
    let peers = s.peers.read().await;
    peers.iter().find(|p| p.id == id).cloned().expect("stored")
}

#[tokio::test]
async fn pairing_with_a_computer_calling_from_its_own_address_remembers_it() {
    let peer = pair_from("10.0.0.9:50000", &["127.0.0.1", "10.0.0.9"]).await;
    assert_eq!(peer.last_address, Some(addr("10.0.0.9:4242")));
}

#[tokio::test]
async fn pairing_with_a_call_from_an_address_the_caller_does_not_have_remembers_none() {
    let peer = pair_from("192.168.64.1:50000", &["127.0.0.1", "10.0.0.7"]).await;
    assert_eq!(peer.last_address, None);
}

/// A paired computer that passes calls along.
fn studio() -> Peer {
    Peer {
        id: uuid::Uuid::new_v4(),
        name: "Studio".into(),
        fingerprint: "ab".repeat(32),
        allows: Permissions::default(),
        granted: Permissions::default(),
        last_address: Some(addr("192.168.64.1:47820")),
        via: None,
    }
}

#[tokio::test]
async fn a_paired_computer_passed_along_by_another_is_remembered_as_reached_through_it() {
    let dir = tempfile::tempdir().unwrap();
    let (s, _rx) = shared(dir.path());
    let (id, relay) = (uuid::Uuid::new_v4(), studio());
    s.peers
        .write()
        .await
        .extend([laptop(id, "10.0.0.7:4242"), relay.clone()]);
    let mut client = answer(&s, "192.168.64.1:50000");
    hello(&mut client, id, &[], Some(relay.id)).await;
    let live = s.peers.read().await[0].clone();
    assert_eq!(live.via, Some(relay.id));
    assert_eq!(live.last_address, Some(addr("10.0.0.7:4242")));
    assert_eq!(s.store.load_peers().unwrap()[0], live);
}

#[tokio::test]
async fn a_call_passed_along_by_a_computer_this_one_is_not_paired_with_changes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let (s, _rx) = shared(dir.path());
    let id = uuid::Uuid::new_v4();
    s.peers.write().await.push(laptop(id, "10.0.0.7:4242"));
    // Nor can a caller name itself as the way to reach it.
    for via in [uuid::Uuid::new_v4(), id] {
        let mut client = answer(&s, "192.168.64.1:50000");
        hello(&mut client, id, &[], Some(via)).await;
        assert_eq!(s.peers.read().await[0].via, None);
    }
}

#[tokio::test]
async fn a_direct_call_leaves_the_way_through_a_relay_alone() {
    let dir = tempfile::tempdir().unwrap();
    let (s, _rx) = shared(dir.path());
    let (id, relay) = (uuid::Uuid::new_v4(), studio());
    let mut caller = laptop(id, "10.0.0.7:4242");
    caller.via = Some(relay.id);
    s.peers.write().await.extend([caller, relay.clone()]);
    let mut client = answer(&s, "10.0.0.9:50000");
    hello(&mut client, id, &["10.0.0.9"], None).await;
    let live = s.peers.read().await[0].clone();
    assert_eq!(live.via, Some(relay.id));
    assert_eq!(live.last_address, Some(addr("10.0.0.9:4242")));
}

#[tokio::test]
async fn pairing_through_a_paired_computer_remembers_it_as_the_way_back() {
    let dir = tempfile::tempdir().unwrap();
    let (s, mut rx) = shared(dir.path());
    let relay = studio();
    s.peers.write().await.push(relay.clone());
    let peer = pair_with(&s, &mut rx, "192.168.64.1:50000", &[], Some(relay.id)).await;
    assert_eq!(peer.via, Some(relay.id));
    assert_eq!(peer.last_address, None);
}

#[tokio::test]
async fn pairing_through_a_computer_this_one_is_not_paired_with_remembers_no_way_back() {
    let dir = tempfile::tempdir().unwrap();
    let (s, mut rx) = shared(dir.path());
    let stranger = Some(uuid::Uuid::new_v4());
    let peer = pair_with(&s, &mut rx, "192.168.64.1:50000", &[], stranger).await;
    assert_eq!(peer.via, None);
}
