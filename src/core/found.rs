//! Other computers this one has found: on the network with mDNS, or added by
//! address.

use super::{Core, lock};
use crate::address::parse_address;
use crate::discovery::{Discovered, Discovery, DiscoveryEvent};
use crate::model::InstanceId;
use crate::net::Connection;
use crate::protocol::{PROTOCOL_VERSION, Reachable};
use anyhow::{anyhow, bail};

impl Core {
    /// mDNS is a convenience; without it "Add by address" still works.
    pub(super) fn start_discovery(&self, name: &str, me: InstanceId, port: u16) {
        let (tx, rx) = std::sync::mpsc::channel();
        match Discovery::start(me, name, port, tx) {
            Ok(d) => *lock(&self.discovery) = Some(d),
            Err(e) => {
                self.ui.warn(format!(
                    "Could not look for other computers on this network ({e:#}). Use Add by \
                     address instead."
                ));
                return;
            }
        }
        let core = self.clone();
        std::thread::spawn(move || {
            while let Ok(ev) = rx.recv() {
                core.discovery_event(ev);
            }
        });
    }

    /// Runs on the discovery thread, outside the runtime, so it may wait for
    /// the lock on `found`.
    pub(super) fn discovery_event(&self, ev: DiscoveryEvent) {
        match &ev {
            DiscoveryEvent::Found(d) => {
                self.shared
                    .found
                    .blocking_write()
                    .insert(d.id, d.addrs.clone());
            }
            DiscoveryEvent::Lost(id) => {
                self.shared.found.blocking_write().remove(id);
            }
        }
        self.ui.update(|s| match ev {
            DiscoveryEvent::Found(d) => {
                // mDNS resolves the same record repeatedly; log only news.
                let known = s.discovered.iter().find(|x| x.id == d.id);
                if known.is_none_or(|x| x.name != d.name || x.addrs != d.addrs) {
                    tracing::info!("found {} ({}) at {:?}", d.name, d.id, d.addrs);
                }
                s.discovered.retain(|x| x.id != d.id);
                s.discovered.push(d);
            }
            DiscoveryEvent::Lost(id) => {
                tracing::info!("{id} left the network");
                s.discovered.retain(|x| x.id != id);
            }
        });
    }

    /// Lists the computers the paired `relay` says it can pass connections
    /// to, replacing what it said before. Only in the list: `found` is where
    /// this computer itself sees others, and it can't reach these.
    pub(super) fn reachable_through(&self, relay: InstanceId, reachable: Vec<Reachable>) {
        self.ui.update(|s| {
            let before = std::mem::take(&mut s.discovered);
            let (said, mut kept): (Vec<_>, Vec<_>) =
                before.into_iter().partition(|d| d.via == Some(relay));
            for r in reachable {
                let seen_here = kept.iter().any(|d| d.id == r.id && d.via.is_none());
                if r.id == s.me.id || r.id == relay || seen_here {
                    continue;
                }
                // Listed once, through whichever paired computer said so last.
                kept.retain(|d| d.id != r.id);
                // Polls repeat every few seconds; log only news.
                if !said.iter().any(|d| d.id == r.id) {
                    let by = s
                        .peer(relay)
                        .map_or(relay.to_string(), |p| p.peer.name.clone());
                    tracing::info!("found {} ({}) through {by}", r.name, r.id);
                }
                kept.push(Discovered {
                    id: r.id,
                    name: r.name,
                    addrs: Vec::new(),
                    version: PROTOCOL_VERSION,
                    via: Some(relay),
                });
            }
            s.discovered = kept;
        });
    }

    pub(super) async fn add_by_address(self, text: String) {
        let result = async {
            let addr = parse_address(&text).map_err(|e| anyhow!(e))?;
            let mut conn = Connection::open(addr, &self.shared).await?;
            conn.close().await;
            if conn.peer_id() == self.shared.settings.read().await.id {
                bail!("{addr} is this computer. Enter the other computer's address.");
            }
            Ok(Discovered {
                id: conn.peer_id(),
                name: conn.peer_name().to_string(),
                addrs: vec![addr],
                version: PROTOCOL_VERSION,
                via: None,
            })
        }
        .await;
        match result {
            Ok(d) => {
                let text = format!("Found {} at {}.", d.name, d.addrs[0]);
                self.shared
                    .found
                    .write()
                    .await
                    .insert(d.id, d.addrs.clone());
                self.ui.update(|s| {
                    s.address_error = None;
                    s.discovered.retain(|x| x.id != d.id);
                    s.discovered.push(d);
                });
                self.ui.info(text);
            }
            Err(e) => {
                let why = format!("{e:#}");
                self.ui.update(|s| s.address_error = Some(why.clone()));
                self.ui
                    .error(format!("Could not add {}: {why}", text.trim()));
            }
        }
    }
}
