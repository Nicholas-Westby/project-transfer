//! Streaming file content in 256 KiB chunks, either direction.

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
        self.tick(rel, true);
        match side {
            Side::Push(root) => self.send_file(root, rel).await,
            Side::Pull(a, project, folder) => self.fetch_file(a, *project, *folder, rel).await,
        }
    }

    /// Mid-file there is no way to stop the peer waiting for the rest, so
    /// cancelling closes the connection and the peer discards its temp file.
    async fn abort_mid_file(&mut self, why: String) -> anyhow::Error {
        self.conn.close().await;
        anyhow::anyhow!(why)
    }

    async fn send_file(&mut self, root: &Path, rel: &str) -> anyhow::Result<()> {
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
        self.conn.send(&put).await?;
        let mut buf = vec![0u8; CHUNK];
        let mut left = size;
        while left > 0 {
            if self.cancel.is_cancelled() {
                return Err(self.abort_mid_file(CANCELLED.into()).await);
            }
            let want = left.min(CHUNK as u64) as usize;
            let n = match f.read(&mut buf[..want]) {
                Ok(0) | Err(_) => {
                    let why = format!("\"{rel}\" changed while it was being sent. Try again.");
                    return Err(self.abort_mid_file(why).await);
                }
                Ok(n) => n,
            };
            self.conn.send_raw(&buf[..n]).await?;
            left -= n as u64;
            self.bytes_done += n as u64;
            self.tick(rel, false);
        }
        match self.conn.recv().await? {
            Response::Ok => {
                self.summary.files += 1;
                self.summary.bytes += size;
            }
            Response::Refused { reason } => self.fail(rel, reason),
            other => return Err(unexpected(self.conn, "a file", other)),
        }
        Ok(())
    }

    async fn fetch_file(
        &mut self,
        a: &Applier,
        project: ProjectId,
        folder: FolderId,
        rel: &str,
    ) -> anyhow::Result<()> {
        let get = Request::GetFile {
            project,
            folder,
            rel: rel.into(),
        };
        let (size, mtime_ms, exec) = match self.conn.request(&get).await? {
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
                return Err(self.abort_mid_file(CANCELLED.into()).await);
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
