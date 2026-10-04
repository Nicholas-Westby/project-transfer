//! Paired computers: choosing one, reaching it, and keeping its status fresh.

use super::Core;
use super::state::PeerView;
use crate::model::{InstanceId, Peer, Permissions};
use crate::net::{Connection, may_ask};
use crate::protocol::{Request, Response};
use crate::transfer::projects::now_ms;
use anyhow::{Context, anyhow, bail};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::time::Duration;
use tracing::{debug, info};

/// How often the selected peer is asked for its status.
pub const POLL_EVERY: Duration = Duration::from_secs(5);

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

    /// Tries discovery's addresses first, then the last one that worked, and
    /// remembers the address and name that answered.
    pub(super) async fn connect(&self, id: InstanceId) -> anyhow::Result<Connection> {
        let peer = self
            .shared
            .peers
            .read()
            .await
            .iter()
            .find(|p| p.id == id)
            .cloned()
            .context(NOT_PAIRED)?;
        let mut addrs: Vec<SocketAddr> = self
            .ui
            .lock()
            .discovered
            .iter()
            .find(|d| d.id == id)
            .map(|d| d.addrs.clone())
            .unwrap_or_default();
        if let Some(last) = peer.last_address.filter(|a| !addrs.contains(a)) {
            addrs.push(last);
        }
        let mut last_err = None;
        for addr in addrs {
            match Connection::open_peer(addr, &self.shared, &peer).await {
                Ok(conn) => {
                    let name = conn.peer_name().to_string();
                    if peer.last_address != Some(addr) || peer.name != name {
                        self.update_peers(|all| {
                            if let Some(p) = all.iter_mut().find(|p| p.id == id) {
                                p.last_address = Some(addr);
                                p.name = name;
                            }
                            Ok(())
                        })
                        .await?;
                    }
                    self.ui.update(|s| {
                        if let Some(v) = s.peers.iter_mut().find(|v| v.peer.id == id) {
                            v.address = Some(addr);
                        }
                    });
                    return Ok(conn);
                }
                Err(e) => {
                    debug!("could not reach {} at {addr}: {e:#}", peer.name);
                    last_err = Some(e);
                }
            }
        }
        Err(last_err.unwrap_or_else(|| {
            anyhow!(
                "{} has no known address. Wait for it to appear on the network, or add it by \
                 address.",
                peer.name
            )
        }))
    }

    pub(super) async fn poll_loop(self) {
        loop {
            tokio::select! {
                _ = tokio::time::sleep(POLL_EVERY) => {}
                _ = self.poll_now.notified() => {}
            }
            let selected = self.ui.lock().selected_peer;
            if let Some(id) = selected {
                self.poll(id).await;
            }
        }
    }

    async fn poll(&self, id: InstanceId) {
        let result = async {
            let mut conn = self.connect(id).await?;
            let (allows, list) = match conn.request(&Request::Status).await? {
                Response::Status { allows, projects } => (allows, projects),
                Response::Refused { reason } => bail!("{reason}"),
                other => bail!(
                    "it answered the status request with {}. Update Project Transfer on both \
                     computers.",
                    variant(&other)
                ),
            };
            let mut remote = HashMap::new();
            for p in list {
                let ask = Request::ProjectInfo { project: p.id };
                // A peer that shares nothing with us would turn this away and
                // warn its user about it on every poll.
                if !may_ask(&ask, allows) {
                    continue;
                }
                if let Response::ProjectInfo(Some(info)) = conn.request(&ask).await? {
                    remote.insert(p.id, info);
                }
            }
            conn.close().await;
            Ok((allows, remote))
        }
        .await;
        let was_online = self.ui.lock().peer(id).is_some_and(|v| v.online);
        let name = self.ui.lock().peer(id).map(|v| v.peer.name.clone());
        let Some(name) = name else { return };
        match result {
            Ok((granted, remote)) => {
                let changed = self
                    .shared
                    .peers
                    .read()
                    .await
                    .iter()
                    .any(|p| p.id == id && p.granted != granted);
                if changed {
                    let saved = self
                        .update_peers(|all| {
                            if let Some(p) = all.iter_mut().find(|p| p.id == id) {
                                p.granted = granted;
                            }
                            Ok(())
                        })
                        .await;
                    match saved {
                        Ok(()) => self.ui.info(granted_sentence(&name, granted)),
                        Err(e) => self.ui.error(format!("{e:#}")),
                    }
                }
                self.ui.update(|s| {
                    if s.selected_peer == Some(id) {
                        s.remote_projects = remote;
                    }
                    if let Some(v) = s.peers.iter_mut().find(|v| v.peer.id == id) {
                        v.online = true;
                        v.last_seen_ms = Some(now_ms());
                    }
                });
                if !was_online {
                    info!("{name} is online");
                }
            }
            Err(e) => {
                self.ui.update(|s| {
                    if let Some(v) = s.peers.iter_mut().find(|v| v.peer.id == id) {
                        v.online = false;
                    }
                });
                if was_online {
                    self.ui.warn(format!("{name} stopped answering: {e:#}"));
                }
            }
        }
    }
}

/// Only the message kind: its contents may hold paths or project details
/// that do not belong in the activity strip or the log.
fn variant(r: &Response) -> String {
    let debug = format!("{r:?}");
    debug
        .split(|c: char| !c.is_alphanumeric())
        .next()
        .unwrap_or_default()
        .to_string()
}

const NOT_PAIRED: &str = "That computer is no longer paired.";

fn can_do(push: bool, pull: bool) -> &'static str {
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

fn granted_sentence(name: &str, g: Permissions) -> String {
    format!(
        "{name} now lets this computer {} with it.",
        can_do(g.may_push_to_me, g.may_pull_from_me)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unexpected_answers_are_named_without_their_contents() {
        let r = Response::Refused {
            reason: "/Users/me/secret".into(),
        };
        assert_eq!(variant(&r), "Refused");
        assert_eq!(variant(&Response::Ok), "Ok");
    }
}
