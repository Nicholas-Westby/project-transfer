//! How to reach a caller again, learned from its hello: at the address it
//! called from when that address is its own, or through the paired computer
//! that passed the call along.

use super::{Shared, stored_peer, update_peer};
use crate::model::{InstanceId, Peer};
use std::net::{IpAddr, SocketAddr};
use tracing::warn;

/// Where the caller can be called back: the address it called from, with the
/// port it listens on. Only when it says that address is its own: a call
/// passed along by another computer, or through a router that rewrites
/// addresses, arrives from an address that would reach someone else.
pub(super) fn callback(remote: SocketAddr, port: u16, own: &[IpAddr]) -> Option<SocketAddr> {
    (port != 0 && own.contains(&remote.ip())).then(|| SocketAddr::new(remote.ip(), port))
}

/// The computer the caller says passed its call along, if this computer is
/// paired with it too. Only a hint at a way back: every way back is checked
/// again when it is used.
pub(super) async fn relay_of(
    shared: &Shared,
    caller: InstanceId,
    via: Option<InstanceId>,
) -> Option<InstanceId> {
    let via = via.filter(|r| *r != caller)?;
    let paired = shared.peers.read().await.iter().any(|p| p.id == via);
    paired.then_some(via)
}

/// A paired peer calling us shows where it is now; keeping that makes the
/// stored address follow it when its IP or port changes.
pub(super) async fn refresh_address(
    shared: &Shared,
    id: InstanceId,
    fingerprint: &str,
    addr: Option<SocketAddr>,
) {
    let Some(addr) = addr else { return };
    let stale = stored_peer(shared, id, fingerprint)
        .await
        .is_some_and(|p| p.last_address != Some(addr));
    if !stale {
        return;
    }
    let change = |p: &mut Peer| p.last_address = Some(addr);
    if let Err(e) = update_peer(shared, id, fingerprint, change).await {
        warn!("could not save the new address of {id}: {e:#}");
    }
}

/// Likewise for a paired peer calling through a relay: that is the way back.
/// A direct call says nothing about it, so it changes nothing.
pub(super) async fn refresh_via(
    shared: &Shared,
    id: InstanceId,
    fingerprint: &str,
    via: Option<InstanceId>,
) {
    let Some(via) = via else { return };
    let stale = stored_peer(shared, id, fingerprint)
        .await
        .is_some_and(|p| p.via != Some(via));
    if !stale {
        return;
    }
    if let Err(e) = update_peer(shared, id, fingerprint, |p| p.via = Some(via)).await {
        warn!("could not save which computer passes {id}'s calls along: {e:#}");
    }
}
