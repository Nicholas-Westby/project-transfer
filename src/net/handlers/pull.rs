//! The source side of a pull.

use super::{Ctx, refused, stored_root};
use crate::manifest::{is_exec, mtime_ms};
use crate::model::{Direction, FolderId, ProjectId};
use crate::protocol::{Response, write_msg};
use crate::transfer::projects::{StartedBy, record_transfer};
use crate::transfer::safe_join;
use anyhow::bail;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tracing::info;

/// Replies `FileHeader` then exactly `size` raw bytes, or `Refused`.
pub async fn get_file<S: AsyncRead + AsyncWrite + Unpin>(
    ctx: &Ctx<'_>,
    stream: &mut S,
    project: ProjectId,
    folder: FolderId,
    rel: &str,
) -> anyhow::Result<()> {
    let me = ctx.shared.settings.read().await.name.clone();
    let opened = async {
        let root = stored_root(ctx.shared, project, folder)
            .await
            .ok_or_else(|| format!("{me} has no folder set up for this project."))?;
        let path = safe_join(&root, rel).map_err(|e| e.to_string())?;
        let not_file = || format!("\"{rel}\" is not a file on {me}.");
        let meta = std::fs::symlink_metadata(&path).map_err(|_| not_file())?;
        if !meta.is_file() {
            return Err(not_file());
        }
        let file = tokio::fs::File::open(&path)
            .await
            .map_err(|e| format!("Could not open \"{rel}\" on {me}: {e}"))?;
        let meta = file.metadata().await.map_err(|e| e.to_string())?;
        Ok((file, meta))
    }
    .await;
    let (file, meta) = match opened {
        Ok(x) => x,
        Err(reason) => return write_msg(stream, &refused(reason)).await,
    };
    let size = meta.len();
    let header = Response::FileHeader {
        size,
        mtime_ms: mtime_ms(&meta),
        exec: is_exec(&meta),
    };
    write_msg(stream, &header).await?;
    let sent = tokio::io::copy(&mut file.take(size), stream).await?;
    stream.flush().await?;
    if sent != size {
        // The header promised more bytes than there are; the stream cannot
        // be resynchronised, so end the connection.
        bail!("\"{rel}\" shrank while it was being sent");
    }
    Ok(())
}

pub async fn end_pull(ctx: &Ctx<'_>, project: ProjectId, files: u64) -> Response {
    let started = StartedBy::Peer;
    match record_transfer(
        ctx.shared,
        project,
        ctx.peer.id,
        Direction::Pull,
        files,
        started,
    )
    .await
    {
        Ok(()) => {
            info!("{} pulled {files} files", ctx.peer.name);
            Response::Ok
        }
        Err(e) => refused(format!("{e:#}")),
    }
}
