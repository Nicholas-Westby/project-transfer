//! Small changes (folders, links, times, removals) on either side.

use super::{Run, Side};
use crate::protocol::{Request, Response};
use crate::transfer::prepare::unexpected;

pub(super) enum Op<'a> {
    MakeDir,
    Symlink(&'a str),
    SetMtime(i64),
    Remove(bool),
}

impl Run<'_> {
    pub(super) async fn expect_ok(&mut self, req: &Request, what: &str) -> anyhow::Result<()> {
        match self.conn.request(req).await? {
            Response::Ok => Ok(()),
            other => Err(unexpected(self.conn, what, other)),
        }
    }

    /// Applies one small change; Ok(false) means it failed and was recorded.
    pub(super) async fn simple(
        &mut self,
        side: &Side,
        rel: &str,
        op: Op<'_>,
    ) -> anyhow::Result<bool> {
        let result = match side {
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
                match self.conn.request(&req).await? {
                    Response::Ok => Ok(()),
                    Response::Refused { reason } => Err(reason),
                    other => return Err(unexpected(self.conn, "a change", other)),
                }
            }
            Side::Pull(a, ..) => match op {
                Op::MakeDir => a.make_dir(rel),
                Op::Symlink(t) => a.make_symlink(rel, t),
                Op::SetMtime(ms) => a.set_mtime(rel, ms),
                Op::Remove(is_dir) => a.remove(rel, is_dir),
            }
            .map_err(|e| e.to_string()),
        };
        match result {
            Ok(()) => Ok(true),
            Err(reason) => {
                self.fail(rel, reason);
                Ok(false)
            }
        }
    }
}
