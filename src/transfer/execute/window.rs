//! Requests sent before the answers to earlier ones arrive, so a transfer
//! never sits idle for a round trip per file or folder. The other computer
//! answers in the order it was asked, so answers match the queue's order.

use super::{CANCELLED, Run, Side};
use crate::protocol::Response;
use crate::transfer::prepare::unexpected;

/// How many requests may wait for their answers. The computer that is not
/// reading has at most this many small messages waiting for it (answers in a
/// push, file requests in a pull), which fit in any socket buffer, so neither
/// computer blocks on a write the other is not reading.
pub(super) const WINDOW: usize = 32;

/// What a request still waiting for its answer was for.
pub(super) enum Awaiting {
    /// A pushed file or change, answered with Ok or Refused.
    Answer {
        rel: String,
        counts: Counts,
        /// Names the request when the answer is not one of those.
        what: &'static str,
    },
    /// A file asked for in a pull, answered with its header and bytes.
    File { rel: String },
}

/// What an applied change adds to the summary.
#[derive(Clone, Copy)]
pub(super) enum Counts {
    Nothing,
    /// A file with this many content bytes, or a link or a time (0 bytes).
    File(u64),
    Removal,
}

impl Run<'_> {
    /// Makes room for one more request, waiting for the oldest answer while
    /// the window is full.
    pub(super) async fn room(&mut self, side: &Side) -> anyhow::Result<()> {
        while self.in_flight.len() >= WINDOW {
            self.settle_one(side).await?;
        }
        Ok(())
    }

    /// Waits for every answer still to come.
    pub(super) async fn settle_all(&mut self, side: &Side) -> anyhow::Result<()> {
        while !self.in_flight.is_empty() {
            self.settle_one(side).await?;
        }
        Ok(())
    }

    async fn settle_one(&mut self, side: &Side) -> anyhow::Result<()> {
        match self.in_flight.pop_front() {
            None => Ok(()),
            Some(Awaiting::Answer { rel, counts, what }) => {
                match self.answer().await? {
                    Response::Ok => self.count(counts),
                    Response::Refused { reason } => self.fail(&rel, reason),
                    other => return Err(unexpected(self.conn, what, other)),
                }
                Ok(())
            }
            Some(Awaiting::File { rel }) => match side {
                Side::Pull(a, ..) => self.receive_file(a, &rel).await,
                Side::Push(_) => anyhow::bail!("a pulled file was awaited during a push"),
            },
        }
    }

    /// The next answer. A slow answer must not hold up cancelling.
    pub(super) async fn answer(&mut self) -> anyhow::Result<Response> {
        let got = tokio::select! {
            r = self.conn.recv() => Some(r),
            () = self.cancel.cancelled() => None,
        };
        match got {
            Some(r) => r,
            // Giving up a read half-way through a message leaves the stream
            // out of step, so the connection is closed rather than reused.
            None => Err(self.abort(CANCELLED.into()).await),
        }
    }

    pub(super) fn count(&mut self, c: Counts) {
        match c {
            Counts::Nothing => {}
            Counts::File(bytes) => {
                self.summary.files += 1;
                self.summary.bytes += bytes;
            }
            Counts::Removal => self.summary.removed += 1,
        }
    }
}
