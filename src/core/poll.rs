//! Keeping the selected peer's status fresh: online or not, what it lets
//! this computer do, and what its projects look like.

use super::Core;
use super::peers::can_do;
use crate::model::{InstanceId, Permissions};
use crate::net::{NotThePairedComputer, may_ask};
use crate::protocol::{Request, Response};
use crate::transfer::link::as_seen_here;
use crate::transfer::projects::now_ms;
use anyhow::bail;
use std::collections::HashMap;
use std::time::Duration;
use tracing::info;

/// How often the selected peer is asked for its status.
pub const POLL_EVERY: Duration = Duration::from_secs(5);

impl Core {
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
            let (allows, list, reachable) = match conn.request(&Request::Status).await? {
                Response::Status {
                    allows,
                    projects,
                    reachable,
                } => (allows, projects, reachable),
                Response::Refused { reason } => bail!("{reason}"),
                other => bail!(
                    "it answered the status request with {}. Update Project Transfer on both \
                     computers.",
                    variant(&other)
                ),
            };
            // A connection is passed along at most once, so what a computer
            // reached through another one can reach is out of reach here.
            // Still reported empty, to drop what it listed before.
            let reachable = if conn.via().is_some() {
                Vec::new()
            } else {
                reachable
            };
            let mut remote = HashMap::new();
            for p in &list {
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
            let local = self.shared.projects.read().await;
            let remote = as_seen_here(&local, &list, remote);
            Ok((allows, remote, reachable))
        }
        .await;
        let was_online = self.ui.lock().peer(id).is_some_and(|v| v.online);
        let name = self.ui.lock().peer(id).map(|v| v.peer.name.clone());
        let Some(name) = name else { return };
        match result {
            Ok((granted, remote, reachable)) => {
                self.reachable_through(id, reachable);
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
                super::lock(&self.poll_problems).remove(&id);
            }
            Err(e) => {
                self.ui.update(|s| {
                    if let Some(v) = s.peers.iter_mut().find(|v| v.peer.id == id) {
                        v.online = false;
                    }
                });
                let shown = super::lock(&self.poll_problems).get(&id).cloned();
                if let Some(line) = poll_warning(&name, was_online, &e, shown.as_deref()) {
                    super::lock(&self.poll_problems).insert(id, format!("{e:#}"));
                    self.ui.warn(line);
                }
            }
        }
    }
}

/// What the user hears about a failed poll, if anything: that a peer which
/// was online stopped answering, or, once, that another computer answers at
/// a paired one's address. Plain unreachability while offline stays quiet;
/// the dot already says it, and a poll runs every few seconds.
fn poll_warning(
    name: &str,
    was_online: bool,
    e: &anyhow::Error,
    shown: Option<&str>,
) -> Option<String> {
    let text = format!("{e:#}");
    if shown == Some(text.as_str()) {
        return None;
    }
    if was_online {
        return Some(format!("{name} stopped answering: {text}"));
    }
    e.chain()
        .any(|c| c.is::<NotThePairedComputer>())
        .then_some(text)
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

fn granted_sentence(name: &str, g: Permissions) -> String {
    format!(
        "{name} now lets this computer {} with it.",
        can_do(g.may_push_to_me, g.may_pull_from_me)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::anyhow;

    #[test]
    fn a_failed_poll_is_reported_once_and_only_when_it_tells_something() {
        let gone = anyhow!("Could not connect to 10.0.0.2:47820");
        assert_eq!(
            poll_warning("Desk", true, &gone, None).unwrap(),
            "Desk stopped answering: Could not connect to 10.0.0.2:47820"
        );
        assert_eq!(poll_warning("Desk", false, &gone, None), None);
        let other = anyhow::Error::new(NotThePairedComputer("Desk isn't at 10.0.0.2".into()));
        assert_eq!(
            poll_warning("Desk", false, &other, None).unwrap(),
            "Desk isn't at 10.0.0.2"
        );
        assert_eq!(
            poll_warning("Desk", false, &other, Some("Desk isn't at 10.0.0.2")),
            None
        );
    }

    #[test]
    fn unexpected_answers_are_named_without_their_contents() {
        let r = Response::Refused {
            reason: "/Users/me/secret".into(),
        };
        assert_eq!(variant(&r), "Refused");
        assert_eq!(variant(&Response::Ok), "Ok");
    }
}
