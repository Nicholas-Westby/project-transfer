//! Encrypted connections between instances, pairing, and the permission gate.

use crate::identity::Identity;
use crate::model::{InstanceId, InstanceSettings, Peer, Permissions, Project, ProjectId};
use crate::store::Store;
use std::sync::Arc;
use tokio::sync::RwLock;

mod client;
mod gate;
mod handlers;
mod pair_client;
mod pair_server;
mod server;
mod tls;

pub use client::Connection;
pub use gate::may_ask;
pub use pair_client::{PairStarted, pair_finish, pair_start};
pub use server::serve;

/// What the server needs, shared with the app core.
#[derive(Clone)]
pub struct Shared {
    pub settings: Arc<RwLock<InstanceSettings>>,
    pub peers: Arc<RwLock<Vec<Peer>>>,
    pub projects: Arc<RwLock<Vec<Project>>>,
    pub store: Arc<Store>,
    pub identity: Arc<Identity>,
    pub events: tokio::sync::mpsc::UnboundedSender<NetEvent>,
}

#[derive(Debug)]
pub enum NetEvent {
    PairPrompt {
        from_id: InstanceId,
        from_name: String,
        code: String,
        requested: Permissions,
        offered: Permissions,
        /// None declines.
        reply: tokio::sync::oneshot::Sender<Option<Permissions>>,
        /// Notified to withdraw after accepting, while the other side has
        /// not confirmed yet.
        cancel: std::sync::Arc<tokio::sync::Notify>,
    },
    /// A pairing another computer started ended without pairing.
    PairEnded {
        from_id: InstanceId,
        reason: String,
    },
    Paired(Peer),
    Refused {
        peer_name: String,
        reason: String,
    },
    Received {
        project: ProjectId,
        peer: InstanceId,
        files: u64,
    },
    Log(String),
}

/// Adds or replaces the peer by id, saving before the in-memory list changes
/// so memory never claims a pairing the disk does not have.
async fn remember(shared: &Shared, peer: Peer) -> anyhow::Result<()> {
    let mut peers = shared.peers.write().await;
    let mut next: Vec<Peer> = peers.iter().filter(|p| p.id != peer.id).cloned().collect();
    next.push(peer);
    shared.store.save_peers(&next)?;
    *peers = next;
    Ok(())
}

/// Changes only the stored address of a paired peer, under the write lock, so
/// a permission change made meanwhile is never overwritten with an old copy.
async fn set_last_address(
    shared: &Shared,
    id: InstanceId,
    fingerprint: &str,
    addr: std::net::SocketAddr,
) -> anyhow::Result<()> {
    let mut peers = shared.peers.write().await;
    let mut next = peers.clone();
    let Some(p) = next
        .iter_mut()
        .find(|p| p.id == id && p.fingerprint == fingerprint)
    else {
        return Ok(());
    };
    p.last_address = Some(addr);
    shared.store.save_peers(&next)?;
    *peers = next;
    Ok(())
}
