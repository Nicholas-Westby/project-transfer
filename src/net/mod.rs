//! Encrypted connections between instances, pairing, and the permission gate.

use crate::identity::Identity;
use crate::model::{InstanceId, InstanceSettings, Peer, Permissions, Project, ProjectId};
use crate::store::Store;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::RwLock;

mod client;
mod gate;
mod handlers;
mod handshake;
mod pair_client;
mod pair_server;
mod relay;
mod routes;
mod server;
mod status;
mod tls;

#[cfg(test)]
mod hello_tests;
#[cfg(test)]
mod test_support;
#[cfg(test)]
mod through_tests;

pub use client::{Connection, NotThePairedComputer};
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
    /// Where this computer sees other instances right now, from discovery
    /// and Add by address; passing a connection along tries these first.
    /// Anyone on the network can announce any address, so the relay checks
    /// who answers before using one. Never holds a computer learned of
    /// through a relay, or this computer would offer to pass connections to
    /// one it can't reach itself.
    pub found: Arc<RwLock<HashMap<InstanceId, Vec<SocketAddr>>>>,
    /// This user's home folder, where folders from another computer's home
    /// land (see `transfer::home`). Tests give each instance its own.
    pub home: Option<std::path::PathBuf>,
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
        /// Entries this computer refused; the log says why for each.
        failed: u64,
    },
    Log(String),
    /// A paired computer changed this computer's projects; the text says
    /// what, for the activity strip.
    ProjectsChanged(String),
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

/// Changes one paired peer under the write lock, so a permission change made
/// meanwhile is never overwritten with an old copy.
async fn update_peer(
    shared: &Shared,
    id: InstanceId,
    fingerprint: &str,
    change: impl FnOnce(&mut Peer),
) -> anyhow::Result<()> {
    let mut peers = shared.peers.write().await;
    let mut next = peers.clone();
    let Some(p) = next
        .iter_mut()
        .find(|p| p.id == id && p.fingerprint == fingerprint)
    else {
        return Ok(());
    };
    change(p);
    shared.store.save_peers(&next)?;
    *peers = next;
    Ok(())
}

/// The paired peer with both this id and this certificate.
async fn stored_peer(shared: &Shared, id: InstanceId, fingerprint: &str) -> Option<Peer> {
    shared
        .peers
        .read()
        .await
        .iter()
        .find(|p| p.id == id && p.fingerprint == fingerprint)
        .cloned()
}
