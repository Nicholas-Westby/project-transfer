//! Push and pull from the UI's side: preview, confirm, run, cancel.
//!
//! The connection that built the preview is kept for running it, so the
//! peer's per-connection scan state matches what the user confirmed. If it
//! drops while the preview is open, running fails and asks for a new preview.

use super::progress;
use super::state::TransferState;
use super::transfer_words::{done_sentence, verb};
use super::{Core, lock};
use crate::net::Connection;
use crate::transfer::{self, Preview, TransferRequest};
use anyhow::bail;
use std::time::Instant;
use tokio_util::sync::CancellationToken;

/// The preview and the connection that built it live together, so a
/// confirmed plan can only ever run over its own connection. Generic so the
/// bookkeeping can be tested without a network.
pub(super) struct Slot<C = Connection> {
    /// Bumped whenever the UI moves on, so late results are dropped.
    generation: u64,
    /// The latest request as the user made it, and whether it was a sync,
    /// for comparing again. A matched project's preview names the other
    /// computer's id, which this computer only takes on when the transfer
    /// runs.
    asked: Option<(TransferRequest, bool)>,
    ready: Option<(C, Preview)>,
    cancel: Option<CancellationToken>,
}

impl<C> Default for Slot<C> {
    fn default() -> Slot<C> {
        Slot {
            generation: 0,
            asked: None,
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

pub(super) const BUSY: &str =
    "A transfer is already under way. Wait for it to finish or cancel it.";

impl Core {
    pub(super) fn busy(&self) -> bool {
        matches!(
            self.ui.lock().transfer,
            TransferState::Preparing | TransferState::Running { .. }
        )
    }

    /// A sync looks at both sides first and previews whichever direction
    /// it picks; `req.direction` is then ignored.
    pub(super) fn prepare(&self, req: TransferRequest, sync: bool) -> anyhow::Result<()> {
        if self.busy() {
            bail!(BUSY);
        }
        // The way the open sync preview went, so comparing it again can say
        // when it turns around.
        let shown = match &self.ui.lock().transfer {
            TransferState::Ready(p) if sync && p.sync.is_some() => Some(p.request.direction),
            _ => None,
        };
        let generation = {
            let mut slot = lock(&self.transfer);
            let g = slot.reset();
            slot.asked = Some((req.clone(), sync));
            self.ui.update(|s| s.transfer = TransferState::Preparing);
            g
        };
        let core = self.clone();
        tokio::spawn(async move {
            let result = async {
                let mut conn = core.connect(req.peer).await?;
                let preview = if sync {
                    let (mut p, choice) =
                        transfer::prepare_sync(&mut conn, &core.shared, req.clone()).await?;
                    // With nothing to do, neither way moves anything.
                    if shown.is_some_and(|d| d != choice.direction) && !p.is_empty() {
                        p.warnings.insert(0, choice.turned_around());
                    }
                    p.sync = Some(choice);
                    p
                } else {
                    transfer::prepare(&mut conn, &core.shared, req.clone()).await?
                };
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
                    let verb = if sync {
                        "sync"
                    } else {
                        verb(req.direction, false)
                    };
                    core.ui
                        .error(format!("Could not prepare the {verb}: {why}"));
                    core.ui.update(|s| s.transfer = TransferState::Failed(why));
                }
            }
        });
        Ok(())
    }

    /// Builds the open preview again from the request behind it, so a
    /// changed ignore list shows at once.
    pub(super) fn compare_again(&self) -> anyhow::Result<()> {
        if !matches!(self.ui.lock().transfer, TransferState::Ready(_)) {
            return Ok(());
        }
        let asked = lock(&self.transfer).asked.clone();
        asked.map_or(Ok(()), |(req, sync)| self.prepare(req, sync))
    }

    pub(super) fn execute(&self) -> anyhow::Result<()> {
        let started = Instant::now();
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
                    started,
                    left: None,
                }
            });
            (conn, preview, slot.generation)
        };
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        progress::follow(self.ui.clone(), rx, started);
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

#[cfg(test)]
#[path = "transfers_tests.rs"]
mod tests;
