//! Reaching another computer: a paired one to talk to, or any one to pair
//! with.

use super::Core;
use super::peers::NOT_PAIRED;
use crate::discovery::Discovered;
use crate::model::{InstanceId, Peer};
use crate::net::Connection;
use anyhow::{Context, anyhow};
use std::net::SocketAddr;
use tracing::{debug, info};

impl Core {
    /// Directly first: where discovery sees the peer, then where it last
    /// answered. Then through each paired computer that may pass the
    /// connection along. Remembers the address, name or relay that worked.
    pub(super) async fn connect(&self, id: InstanceId) -> anyhow::Result<Connection> {
        let peer = self.stored(id).await.context(NOT_PAIRED)?;
        let mut failure = match self.connect_direct(&peer).await {
            Ok(conn) => return Ok(conn),
            Err(error) => Failure {
                rank: Failure::DIRECT,
                error,
            },
        };
        let tried_directly = !failure.error.is::<NoKnownAddress>();
        for r in self.relays(&peer) {
            // A relay that fails before it answers explains more than a
            // direct attempt only when it is the usual way there, or when
            // there was nowhere to try directly.
            let unreached = if peer.via == Some(r) || !tried_directly {
                Failure::RELAY_UNREACHED
            } else {
                Failure::LISTED_UNREACHED
            };
            // Only a hint at a way: a computer this one no longer trusts
            // passes nothing along.
            let Some(relay) = self.stored(r).await else {
                debug!("not asking {r} to pass {} along: not paired", peer.name);
                let why = anyhow!(
                    "{} was reached through a computer this one is no longer paired with. Pair \
                     with that computer again first.",
                    peer.name
                );
                failure.note(unreached, why);
                continue;
            };
            let conn = match self.reach_relay(&relay).await {
                Ok(conn) => conn,
                Err(e) => {
                    failure.note(unreached, e);
                    continue;
                }
            };
            match Connection::open_peer_through(conn, &self.shared, &peer).await {
                Ok(conn) => {
                    // Its address is the relay's, so it is never kept. The
                    // name comes from the peer's own hello, checked like any.
                    let name = conn.peer_name().to_string();
                    if peer.via != Some(r) || peer.name != name {
                        self.update_peers(|all| {
                            if let Some(p) = all.iter_mut().find(|p| p.id == id) {
                                p.via = Some(r);
                                p.name = name;
                            }
                            Ok(())
                        })
                        .await?;
                    }
                    if peer.via != Some(r) {
                        info!("{} is reached through {} now", peer.name, relay.name);
                    }
                    return Ok(conn);
                }
                Err(e) => {
                    debug!(
                        "could not reach {} through {}: {e:#}",
                        peer.name, relay.name
                    );
                    failure.note(Failure::RELAY_ANSWERED, e);
                }
            }
        }
        Err(failure.error)
    }

    /// Who may pass a connection to `peer` along, the likeliest first: the
    /// one that did last, then any that said they can reach it.
    fn relays(&self, peer: &Peer) -> Vec<InstanceId> {
        let reported: Vec<InstanceId> = {
            let s = self.ui.lock();
            let listed = s.discovered.iter().filter(|d| d.id == peer.id);
            listed.filter_map(|d| d.via).collect()
        };
        let mut relays = Vec::new();
        for r in peer.via.into_iter().chain(reported) {
            if r != peer.id && !relays.contains(&r) {
                relays.push(r);
            }
        }
        relays
    }

    /// Directly, never through another relay, so a connection is passed
    /// along at most once.
    async fn reach_relay(&self, relay: &Peer) -> anyhow::Result<Connection> {
        let conn = self.connect_direct(relay).await;
        conn.with_context(|| {
            format!(
                "Could not reach {}, which passes the connection along",
                relay.name
            )
        })
    }

    async fn stored(&self, id: InstanceId) -> Option<Peer> {
        let peers = self.shared.peers.read().await;
        peers.iter().find(|p| p.id == id).cloned()
    }

    /// Only where this computer itself sees `peer` or last reached it.
    async fn connect_direct(&self, peer: &Peer) -> anyhow::Result<Connection> {
        let id = peer.id;
        let mut addrs: Vec<SocketAddr> = self
            .ui
            .lock()
            .discovered
            .iter()
            .filter(|d| d.id == id && d.via.is_none())
            .flat_map(|d| d.addrs.clone())
            .collect();
        if let Some(last) = peer.last_address.filter(|a| !addrs.contains(a)) {
            addrs.push(last);
        }
        let mut last_err = None;
        for addr in addrs {
            match Connection::open_peer(addr, &self.shared, peer).await {
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
            NoKnownAddress(format!(
                "{} has no known address. Wait for it to appear on the network, or add it by \
                 address.",
                peer.name
            ))
            .into()
        }))
    }
}

/// For pairing: whoever answers may be a stranger. A computer listed through
/// a paired one is asked for through that one, reached directly.
pub(super) async fn open_any(core: &Core, target: &Discovered) -> anyhow::Result<Connection> {
    if let Some(r) = target.via {
        let relay = core.stored(r).await.with_context(|| {
            format!(
                "{} was listed by a computer this one is no longer paired with. Pair with that \
                 computer again first.",
                target.name
            )
        })?;
        let conn = core.reach_relay(&relay).await?;
        return Connection::through(conn, target.id, &core.shared).await;
    }
    let mut last = None;
    for addr in &target.addrs {
        match Connection::open(*addr, &core.shared).await {
            Ok(c) => return Ok(c),
            Err(e) => last = Some(e),
        }
    }
    Err(last.unwrap_or_else(|| anyhow!("{} has no address to connect to.", target.name)))
}

/// Nowhere to try a computer directly: discovery doesn't see it, and it
/// never answered at an address of its own.
#[derive(Debug)]
struct NoKnownAddress(String);

impl std::fmt::Display for NoKnownAddress {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for NoKnownAddress {}

/// Why a paired computer could not be reached, from the attempt that got
/// furthest: a relay's own answer names the reason, which says more than the
/// usual relay failing before it answers, which says more than a failed
/// direct attempt. A relay only listed failing that early says least. Of two
/// that got as far, the later one wins.
struct Failure {
    rank: u8,
    error: anyhow::Error,
}

impl Failure {
    const LISTED_UNREACHED: u8 = 0;
    const DIRECT: u8 = 1;
    const RELAY_UNREACHED: u8 = 2;
    const RELAY_ANSWERED: u8 = 3;

    fn note(&mut self, rank: u8, error: anyhow::Error) {
        if rank >= self.rank {
            *self = Failure { rank, error };
        }
    }
}
