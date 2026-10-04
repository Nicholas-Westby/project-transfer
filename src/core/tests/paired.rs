//! Cores paired with each other by writing the pairings straight into each
//! one's memory and disk, for the tests of reaching one through another.

use super::*;
use crate::discovery::{Discovered, DiscoveryEvent};
use crate::model::{Peer, Permissions};
use crate::protocol::PROTOCOL_VERSION;

pub(super) const ALL: Permissions = Permissions {
    may_push_to_me: true,
    may_pull_from_me: true,
};

pub(super) fn id(f: &Fixture) -> InstanceId {
    f.core.state().me.id
}

pub(super) fn name(f: &Fixture) -> String {
    f.core.state().me.name.clone()
}

pub(super) fn at(f: &Fixture) -> SocketAddr {
    SocketAddr::from((Ipv4Addr::LOCALHOST, f.core.port()))
}

/// `on` stores `other` as paired, last reached at `last_address` or
/// through `via`.
pub(super) fn pair(
    on: &Fixture,
    other: &Fixture,
    last_address: Option<SocketAddr>,
    via: Option<InstanceId>,
) {
    let peer = Peer {
        id: id(other),
        name: name(other),
        fingerprint: other.core.core.shared.identity.fingerprint(),
        allows: ALL,
        granted: ALL,
        last_address,
        via,
    };
    {
        let mut peers = on.core.core.shared.peers.blocking_write();
        peers.retain(|p| p.id != peer.id);
        peers.push(peer);
        on.store().save_peers(&peers).unwrap();
    }
    sync(on);
}

/// The window lists what memory holds, as after a real pairing; a poll only
/// reports on a computer it lists.
fn sync(on: &Fixture) {
    let core = on.core.core.clone();
    let runtime = on.core.runtime.as_ref().unwrap();
    runtime.block_on(async move { core.sync_peers().await });
}

/// `on` sees `other` on the network, as discovery would report it.
pub(super) fn sees(on: &Fixture, other: &Fixture) {
    on.core
        .core
        .discovery_event(DiscoveryEvent::Found(Discovered {
            id: id(other),
            name: name(other),
            addrs: vec![at(other)],
            version: PROTOCOL_VERSION,
            via: None,
        }));
}

/// `on` unpairs `id`; the other computer still has `on`.
pub(super) fn forget(on: &Fixture, id: InstanceId) {
    {
        let mut peers = on.core.core.shared.peers.blocking_write();
        peers.retain(|p| p.id != id);
        on.store().save_peers(&peers).unwrap();
    }
    sync(on);
}

pub(super) fn stored(on: &Fixture, id: InstanceId) -> Peer {
    let peers = on.core.core.shared.peers.blocking_read();
    peers.iter().find(|p| p.id == id).cloned().expect("paired")
}

pub(super) fn saved(on: &Fixture, id: InstanceId) -> Peer {
    let peers = on.store().load_peers().unwrap();
    peers.into_iter().find(|p| p.id == id).expect("saved")
}

/// An address nothing answers at any more.
pub(super) fn closed_port() -> SocketAddr {
    let gone = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    gone.local_addr().unwrap()
}

/// Connects `from` to the paired computer `to`, as a poll or a transfer
/// would. Ok holds the computer that passed the connection along, if any.
pub(super) fn connect(from: &Fixture, to: InstanceId) -> anyhow::Result<Option<InstanceId>> {
    let core = from.core.core.clone();
    let runtime = from.core.runtime.as_ref().unwrap();
    runtime.block_on(async move {
        let mut conn = core.connect(to).await?;
        conn.close().await;
        Ok(conn.via())
    })
}

/// W and V, each paired directly with M, which sees both. How W stores V
/// is up to each test.
pub(super) fn trio() -> [Fixture; 3] {
    let [w, m, v] = [(); 3].map(|_| Fixture::new());
    for (on, other) in [(&w, &m), (&m, &w), (&m, &v), (&v, &m)] {
        pair(on, other, Some(at(other)), None);
    }
    sees(&m, &w);
    sees(&m, &v);
    pair(&v, &w, None, Some(id(&m)));
    [w, m, v]
}

/// How `target` is listed when `relay` reported it can reach it.
pub(super) fn listed_through(target: &Fixture, relay: InstanceId) -> Discovered {
    Discovered {
        id: id(target),
        name: name(target),
        addrs: vec![],
        version: PROTOCOL_VERSION,
        via: Some(relay),
    }
}
