//! Push and pull from the UI's side: preview, confirm, run, cancel.
//!
//! The connection that built the preview is kept for running it, so the
//! peer's per-connection scan state matches what the user confirmed. If it
//! drops while the preview is open, running fails and asks for a new preview.

use super::pairing::files_word;
use super::state::TransferState;
use super::{Core, lock};
use crate::model::Direction;
use crate::net::Connection;
use crate::transfer::{self, Preview, Progress, Summary, TransferRequest};
use anyhow::bail;
use tokio_util::sync::CancellationToken;

/// The preview and the connection that built it live together, so a
/// confirmed plan can only ever run over its own connection. Generic so the
/// bookkeeping can be tested without a network.
pub(super) struct Slot<C = Connection> {
    /// Bumped whenever the UI moves on, so late results are dropped.
    generation: u64,
    ready: Option<(C, Preview)>,
    cancel: Option<CancellationToken>,
}

impl<C> Default for Slot<C> {
    fn default() -> Slot<C> {
        Slot {
            generation: 0,
            ready: None,
            cancel: None,
        }
    }
}

impl<C> Slot<C> {
    /// Forgets any preview and returns the generation a new one must match.
    pub(super) fn reset(&mut self) -> u64 {
        self.generation += 1;
        self.ready = None;
        self.generation
    }

    /// Keeps a finished preview only if nothing moved on since it started.
    pub(super) fn offer(&mut self, generation: u64, conn: C, preview: Preview) -> bool {
        if generation != self.generation {
            return false;
        }
        self.ready = Some((conn, preview));
        true
    }

    pub(super) fn take_ready(&mut self) -> Option<(C, Preview)> {
        self.ready.take()
    }
}

const BUSY: &str = "A transfer is already under way. Wait for it to finish or cancel it.";

impl Core {
    fn busy(&self) -> bool {
        matches!(
            self.ui.lock().transfer,
            TransferState::Preparing | TransferState::Running { .. }
        )
    }

    pub(super) fn prepare(&self, req: TransferRequest) -> anyhow::Result<()> {
        if self.busy() {
            bail!(BUSY);
        }
        let generation = {
            let mut slot = lock(&self.transfer);
            let g = slot.reset();
            self.ui.update(|s| s.transfer = TransferState::Preparing);
            g
        };
        let core = self.clone();
        tokio::spawn(async move {
            let result = async {
                let mut conn = core.connect(req.peer).await?;
                let preview = transfer::prepare(&mut conn, &core.shared, req.clone()).await?;
                anyhow::Ok((conn, preview))
            }
            .await;
            // The UI changes under the slot lock (always slot, then UI), so
            // what it shows always matches what the slot holds.
            let mut slot = lock(&core.transfer);
            if slot.generation != generation {
                return;
            }
            match result {
                Ok((conn, preview)) => {
                    let shown = preview.clone();
                    slot.offer(generation, conn, preview);
                    core.ui.update(|s| s.transfer = TransferState::Ready(shown));
                }
                Err(e) => {
                    let why = format!("{e:#}");
                    let verb = verb(req.direction, false);
                    core.ui
                        .error(format!("Could not prepare the {verb}: {why}"));
                    core.ui.update(|s| s.transfer = TransferState::Failed(why));
                }
            }
        });
        Ok(())
    }

    pub(super) fn execute(&self) -> anyhow::Result<()> {
        let cancel = CancellationToken::new();
        let (mut conn, preview, generation) = {
            let mut slot = lock(&self.transfer);
            let Some((conn, preview)) = slot.take_ready() else {
                bail!("There is no preview to confirm. Prepare the transfer again.");
            };
            slot.cancel = Some(cancel.clone());
            let (files, total) = preview.totals();
            self.ui.update(|s| {
                s.transfer = TransferState::Running {
                    done: 0,
                    total,
                    files,
                    current: String::new(),
                }
            });
            (conn, preview, slot.generation)
        };
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let ui = self.ui.clone();
        tokio::spawn(async move {
            while let Some(p) = rx.recv().await {
                // Only while running: the final state is set by the caller.
                ui.update(|s| {
                    if let TransferState::Running { done, current, .. } = &mut s.transfer
                        && let Progress::File { rel, bytes_done } = p
                    {
                        *done = bytes_done;
                        *current = rel;
                    }
                });
            }
        });
        let core = self.clone();
        tokio::spawn(async move {
            let req = preview.request.clone();
            let link = preview.link.clone();
            let peer = core
                .ui
                .lock()
                .peer(req.peer)
                .map_or_else(|| "the other computer".into(), |v| v.peer.name.clone());
            let result = transfer::execute(&mut conn, &core.shared, preview, tx, cancel).await;
            core.sync_projects().await;
            if let Some(l) = link {
                // Once the project has the new id, what the other computer
                // has for it is filed under it too, until the next poll.
                core.ui.update(|s| {
                    if s.project(l.from).is_none()
                        && s.project(l.to).is_some()
                        && let Some(r) = s.remote_projects.remove(&l.from)
                    {
                        s.remote_projects.insert(l.to, r);
                    }
                });
                core.poll_now.notify_one();
            }
            let state = match result {
                Ok(summary) => {
                    let text = done_sentence(req.direction, &peer, &summary);
                    // A warning stands out in the strip and in the log.
                    if summary.failures.is_empty() {
                        core.ui.info(text);
                    } else {
                        core.ui.warn(text);
                    }
                    TransferState::Finished(summary)
                }
                Err(e) => {
                    let why = format!("{e:#}");
                    let verb = verb(req.direction, false);
                    core.ui
                        .error(format!("The {verb} with {peer} stopped: {why}"));
                    TransferState::Failed(why)
                }
            };
            let mut slot = lock(&core.transfer);
            slot.cancel = None;
            if slot.generation == generation {
                core.ui.update(|s| s.transfer = state);
            }
        });
        Ok(())
    }

    pub(super) fn cancel_transfer(&self) {
        let running = matches!(self.ui.lock().transfer, TransferState::Running { .. });
        if running {
            if let Some(c) = &lock(&self.transfer).cancel {
                c.cancel();
            }
            self.ui
                .info("Cancelling the transfer. Files already copied stay in place.");
        } else {
            self.dismiss_transfer();
        }
    }

    pub(super) fn dismiss_transfer(&self) {
        if matches!(self.ui.lock().transfer, TransferState::Running { .. }) {
            return;
        }
        let mut slot = lock(&self.transfer);
        slot.reset();
        self.ui.update(|s| s.transfer = TransferState::Idle);
    }
}

fn verb(d: Direction, past: bool) -> &'static str {
    match (d, past) {
        (Direction::Push, false) => "push",
        (Direction::Pull, false) => "pull",
        (Direction::Push, true) => "Pushed",
        (Direction::Pull, true) => "Pulled",
    }
}

/// "Pushed 42 files to Desktop Swift Heron in 3.1 s."
pub(super) fn done_sentence(d: Direction, peer: &str, s: &Summary) -> String {
    let dir = match d {
        Direction::Push => "to",
        Direction::Pull => "from",
    };
    let mut text = format!(
        "{} {} {dir} {peer} in {:.1} s.",
        verb(d, true),
        files_word(s.files),
        s.took_ms as f64 / 1000.0
    );
    if s.removed > 0 {
        text.push_str(&format!(
            " Removed {}.",
            match s.removed {
                1 => "1 item".to_string(),
                n => format!("{n} items"),
            }
        ));
    }
    if !s.failures.is_empty() {
        text.push_str(&format!(
            " {} could not be copied; the log lists each one and why.",
            s.failures.len()
        ));
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Direction;

    fn preview(tag: u128) -> Preview {
        Preview {
            request: TransferRequest {
                peer: uuid::Uuid::from_u128(tag),
                project: uuid::Uuid::from_u128(tag),
                direction: Direction::Push,
                send_everything: false,
            },
            folders: Vec::new(),
            warnings: Vec::new(),
            link: None,
            description: false,
        }
    }

    #[test]
    fn a_stale_preview_is_dropped_and_never_paired_with_a_newer_connection() {
        let mut slot: Slot<&str> = Slot::default();
        let first = slot.reset();
        // Dismissed, then a second prepare starts before the first finishes.
        let second = slot.reset();
        assert!(slot.offer(second, "conn B", preview(2)));
        assert!(!slot.offer(first, "conn A", preview(1)));
        let (conn, p) = slot.take_ready().unwrap();
        assert_eq!((conn, p), ("conn B", preview(2)));
        assert!(slot.take_ready().is_none(), "a preview runs at most once");
    }

    #[test]
    fn dismissing_forgets_the_ready_preview() {
        let mut slot: Slot<&str> = Slot::default();
        let g = slot.reset();
        assert!(slot.offer(g, "conn", preview(1)));
        slot.reset();
        assert!(slot.take_ready().is_none());
        assert!(!slot.offer(g, "late", preview(1)));
    }
}
