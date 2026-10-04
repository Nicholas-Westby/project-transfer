//! Paired computers: choosing one and changing what it may do.

use super::Core;
use super::state::PeerView;
use crate::model::{InstanceId, Peer, Permissions};
use anyhow::{Context, anyhow};

pub(super) fn offline_view(p: &Peer) -> PeerView {
    PeerView {
        peer: p.clone(),
        online: false,
        address: p.last_address,
        last_seen_ms: None,
    }
}

impl Core {
    /// Saves before memory changes, like the other stores.
    pub(super) async fn update_peers<T>(
        &self,
        f: impl FnOnce(&mut Vec<Peer>) -> Result<T, String>,
    ) -> anyhow::Result<T> {
        let out = {
            let mut guard = self.shared.peers.write().await;
            let mut next = guard.clone();
            let out = f(&mut next).map_err(|e| anyhow!(e))?;
            self.shared.store.save_peers(&next)?;
            *guard = next;
            out
        };
        self.sync_peers().await;
        Ok(out)
    }

    /// Rebuilds the UI's peer list, keeping what it knew about reachability.
    pub(super) async fn sync_peers(&self) {
        let peers = self.shared.peers.read().await.clone();
        self.ui.update(|s| {
            let old = std::mem::take(&mut s.peers);
            s.peers = peers
                .iter()
                .map(|p| match old.iter().find(|v| v.peer.id == p.id) {
                    Some(v) => PeerView {
                        peer: p.clone(),
                        ..v.clone()
                    },
                    None => offline_view(p),
                })
                .collect();
            if s.selected_peer.is_some_and(|id| s.peer(id).is_none()) {
                s.selected_peer = None;
                s.remote_projects.clear();
            }
        });
    }

    pub(super) async fn select_peer(&self, id: InstanceId) -> anyhow::Result<()> {
        let name = self
            .shared
            .peers
            .read()
            .await
            .iter()
            .find(|p| p.id == id)
            .map(|p| p.name.clone())
            .context("That computer is not paired. Pair with it first.")?;
        self.update_settings(|s| s.last_peer = Some(id)).await?;
        self.ui.update(|s| {
            s.selected_peer = Some(id);
            s.remote_projects.clear();
        });
        self.poll_now.notify_one();
        self.ui.info(format!("Working with {name}."));
        Ok(())
    }

    pub(super) async fn unpair(&self, id: InstanceId) -> anyhow::Result<()> {
        let name = self
            .update_peers(|all| {
                let at = all.iter().position(|p| p.id == id).ok_or(NOT_PAIRED)?;
                Ok(all.remove(at).name)
            })
            .await?;
        // What it listed could only be reached through it.
        self.ui
            .update(|s| s.discovered.retain(|d| d.via != Some(id)));
        if self.shared.settings.read().await.last_peer == Some(id) {
            self.update_settings(|s| s.last_peer = None).await?;
        }
        self.ui
            .info(format!("Unpaired {name}. Pair again to transfer with it."));
        Ok(())
    }

    pub(super) async fn set_allows(
        &self,
        id: InstanceId,
        allows: Permissions,
    ) -> anyhow::Result<()> {
        let name = self
            .update_peers(|all| {
                let p = all.iter_mut().find(|p| p.id == id).ok_or(NOT_PAIRED)?;
                p.allows = allows;
                Ok(p.name.clone())
            })
            .await?;
        self.ui.info(allows_sentence(&name, allows));
        Ok(())
    }
}

pub(super) const NOT_PAIRED: &str = "That computer is no longer paired.";

pub(super) fn can_do(push: bool, pull: bool) -> &'static str {
    match (push, pull) {
        (true, true) => "push and pull",
        (true, false) => "push but not pull",
        (false, true) => "pull but not push",
        (false, false) => "neither push nor pull",
    }
}

fn allows_sentence(name: &str, a: Permissions) -> String {
    format!(
        "{name} may now {} with this computer.",
        can_do(a.may_push_to_me, a.may_pull_from_me)
    )
}
