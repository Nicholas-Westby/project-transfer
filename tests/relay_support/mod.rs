//! Three instances on 127.0.0.1: A and B, each paired with M.
#![allow(dead_code)]

use crate::support::{Instance, perms};
use project_transfer::model::{InstanceId, Peer};
use project_transfer::net::{Connection, pair_finish, pair_start};
use std::net::SocketAddr;

pub struct Trio {
    pub a: Instance,
    pub m: Instance,
    pub b: Instance,
}

/// A and B, each paired with M directly, and M seeing both on the network.
pub async fn trio() -> Trio {
    let (a, m, b) = (start().await, start().await, start().await);
    pair(&a, &m).await;
    pair(&b, &m).await;
    for other in [&a, &b] {
        let id = other.id().await;
        m.shared.found.write().await.insert(id, vec![other.addr]);
    }
    Trio { a, m, b }
}

/// Lets paired computers push and pull, and accepts every pairing prompt.
pub async fn start() -> Instance {
    Instance::start(perms(true, true)).await
}

/// `from` pairs with `to` directly, both users agreeing at once.
pub async fn pair(from: &Instance, to: &Instance) -> Peer {
    let mut conn = Connection::open(to.addr, &from.shared).await.unwrap();
    pair_over(&mut conn, from).await
}

pub async fn pair_over(conn: &mut Connection, from: &Instance) -> Peer {
    let all = perms(true, true);
    let started = pair_start(conn, &from.shared, all, all).await.unwrap();
    pair_finish(conn, &from.shared, started, async { true }, || {})
        .await
        .unwrap()
}

/// A pairs with B through M. Returns what A stored for B.
pub async fn pair_through(t: &Trio) -> Peer {
    let relay = open(&t.a, &t.m).await;
    let mut conn = Connection::through(relay, t.b.id().await, &t.a.shared)
        .await
        .unwrap();
    pair_over(&mut conn, &t.a).await
}

/// What `on` stored for the paired computer `id`.
pub async fn stored(on: &Instance, id: InstanceId) -> Peer {
    let peers = on.shared.peers.read().await;
    peers.iter().find(|p| p.id == id).cloned().expect("paired")
}

/// Changes where `on` last reached the paired computer `id`.
pub async fn set_stored_address(on: &Instance, id: InstanceId, at: Option<SocketAddr>) {
    let mut peers = on.shared.peers.write().await;
    peers
        .iter_mut()
        .find(|p| p.id == id)
        .expect("paired")
        .last_address = at;
}

/// A connection from `from` to `to`, a computer it is paired with.
pub async fn open(from: &Instance, to: &Instance) -> Connection {
    let peer = stored(from, to.id().await).await;
    Connection::open_peer(to.addr, &from.shared, &peer)
        .await
        .unwrap()
}

pub async fn name(of: &Instance) -> String {
    of.shared.settings.read().await.name.clone()
}

/// An address nothing listens on any more.
pub async fn closed_port() -> SocketAddr {
    let gone = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    gone.local_addr().unwrap()
}
