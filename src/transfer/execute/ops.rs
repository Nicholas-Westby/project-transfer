//! Small changes (folders, links, times, removals) on either side.

use super::window::{Awaiting, Counts};
use super::{Run, Side};
use crate::protocol::{Request, Response};
use crate::transfer::Op;
use crate::transfer::prepare::unexpected;

impl Run<'_> {
    pub(super) async fn expect_ok(&mut self, req: &Request, what: &str) -> anyhow::Result<()> {
        // Answers come in order; one still on its way would be read as this one's.
        debug_assert!(self.in_flight.is_empty());
        match self.conn.request(req).await? {
            Response::Ok => Ok(()),
            other => Err(unexpected(self.conn, what, other)),
        }
    }

    /// Applies one small change: here at once in a pull, or sent ahead in a
    /// push, where it is counted when its answer arrives.
    pub(super) async fn simple(
        &mut self,
        side: &Side,
        rel: &str,
        op: Op<'_>,
        counts: Counts,
    ) -> anyhow::Result<()> {
        match side {
            Side::Push(_) => {
                let req = match op {
                    Op::MakeDir => Request::MakeDir { rel: rel.into() },
                    Op::Symlink(t) => Request::MakeSymlink {
                        rel: rel.into(),
                        target: t.into(),
                    },
                    Op::SetMtime(ms) => Request::SetMtime {
                        rel: rel.into(),
                        mtime_ms: ms,
                    },
                    Op::Remove(is_dir) => Request::Remove {
                        rel: rel.into(),
                        is_dir,
                    },
                };
                self.room(side).await?;
                self.conn.send(&req).await?;
                self.in_flight.push_back(Awaiting::Answer {
                    rel: rel.into(),
                    counts,
                    what: "a change",
                });
            }
            Side::Pull(a, ..) => match op.apply(a, rel) {
                Ok(()) => self.count(counts),
                Err(reason) => self.fail(rel, reason),
            },
        }
        Ok(())
    }
}
