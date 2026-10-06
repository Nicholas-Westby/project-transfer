//! Streaming file content in 256 KiB chunks, either direction.

use super::window::{Awaiting, Counts};
use super::{CANCELLED, Run, Side};
use crate::manifest::{is_exec, mtime_ms};
use crate::model::{FolderId, ProjectId};
use crate::protocol::{Request, Response};
use crate::transfer::prepare::unexpected;
use crate::transfer::{Applier, Progress, safe_join};
use anyhow::bail;
use std::io::Read;
use std::path::Path;
use std::time::{Duration, Instant};

const CHUNK: usize = 256 * 1024;
const PROGRESS_EVERY: Duration = Duration::from_millis(100);

impl Run<'_> {
    pub(super) fn tick(&mut self, rel: &str, force: bool) {
        if force || self.last_progress.elapsed() >= PROGRESS_EVERY {
            self.last_progress = Instant::now();
            let _ = self.progress.send(Progress::File {
                rel: rel.to_string(),
                bytes_done: self.bytes_done,
            });
        }
    }

    pub(super) async fn file(&mut self, side: &Side, rel: &str) -> anyhow::Result<()> {
        match side {
            Side::Push(root) => self.send_file(side, root, rel).await,
            Side::Pull(_, project, folder) => self.ask_for_file(side, *project, *folder, rel).await,
        }
    }

    /// The peer works through everything already sent, and mid-file it waits
    /// for the rest, so stopping early closes the connection; the peer then
    /// discards any file it has not finished.
    pub(super) async fn abort(&mut self, why: String) -> anyhow::Error {
        self.conn.close().await;
        anyhow::anyhow!(why)
    }

    async fn send_file(&mut self, side: &Side, root: &Path, rel: &str) -> anyhow::Result<()> {
        self.tick(rel, true);
        let opened = safe_join(root, rel).and_then(|p| {
            let f = std::fs::File::open(&p)?;
            let meta = f.metadata()?;
            Ok((f, meta))
        });
        let (mut f, meta) = match opened {
            Ok(x) if x.1.is_file() => x,
            Ok(_) => {
                self.fail(rel, "It is no longer a file here.".into());
                return Ok(());
            }
            Err(e) => {
                self.fail(rel, format!("Could not read it: {e}"));
                return Ok(());
            }
        };
        let size = meta.len();
        let put = Request::PutFile {
            rel: rel.into(),
            size,
            mtime_ms: mtime_ms(&meta),
            exec: is_exec(&meta),
        };
        self.room(side).await?;
        self.conn.send(&put).await?;
        let mut buf = vec![0u8; CHUNK];
        let mut left = size;
        while left > 0 {
            if self.cancel.is_cancelled() {
                return Err(self.abort(CANCELLED.into()).await);
            }
            let want = left.min(CHUNK as u64) as usize;
            let n = match f.read(&mut buf[..want]) {
                Ok(0) | Err(_) => {
                    let why = format!("\"{rel}\" changed while it was being sent. Try again.");
                    return Err(self.abort(why).await);
                }
                Ok(n) => n,
            };
            self.conn.send_raw(&buf[..n]).await?;
            left -= n as u64;
            self.bytes_done += n as u64;
            self.tick(rel, false);
        }
        self.in_flight.push_back(Awaiting::Answer {
            rel: rel.into(),
            counts: Counts::File(size),
            what: "a file",
        });
        Ok(())
    }

    /// Asks for a file without waiting for it; it is written when its turn
    /// in the answers comes.
    async fn ask_for_file(
        &mut self,
        side: &Side,
        project: ProjectId,
        folder: FolderId,
        rel: &str,
    ) -> anyhow::Result<()> {
        self.room(side).await?;
        let get = Request::GetFile {
            project,
            folder,
            rel: rel.into(),
        };
        self.conn.send(&get).await?;
        self.in_flight.push_back(Awaiting::File { rel: rel.into() });
        Ok(())
    }

    /// Writes the next file the peer sends: the oldest one asked for.
    pub(super) async fn receive_file(&mut self, a: &Applier, rel: &str) -> anyhow::Result<()> {
        self.tick(rel, true);
        let (size, mtime_ms, exec) = match self.answer().await? {
            Response::FileHeader {
                size,
                mtime_ms,
                exec,
            } => (size, mtime_ms, exec),
            Response::Refused { reason } => {
                self.fail(rel, reason);
                return Ok(());
            }
            other => return Err(unexpected(self.conn, "a file request", other)),
        };
        let mut pending = match a.begin_file(rel) {
            Ok(p) => Some(p),
            Err(e) => {
                self.fail(rel, format!("Could not write it: {e}"));
                None
            }
        };
        let mut buf = vec![0u8; CHUNK];
        let mut left = size;
        while left > 0 {
            if self.cancel.is_cancelled() {
                return Err(self.abort(CANCELLED.into()).await);
            }
            let want = left.min(CHUNK as u64) as usize;
            let n = self.conn.read_raw(&mut buf[..want]).await?;
            if n == 0 {
                bail!(
                    "{} closed the connection in the middle of \"{rel}\".",
                    self.conn.peer_name()
                );
            }
            if let Some(p) = &mut pending
                && let Err(e) = p.write(&buf[..n])
            {
                self.fail(rel, format!("Could not write it: {e}"));
                pending = None;
            }
            left -= n as u64;
            self.bytes_done += n as u64;
            self.tick(rel, false);
        }
        if let Some(p) = pending {
            match p.finish(mtime_ms, exec) {
                Ok(()) => {
                    self.summary.files += 1;
                    self.summary.bytes += size;
                }
                Err(e) => self.fail(rel, format!("Could not write it: {e}")),
            }
        }
        Ok(())
    }
}
