//! Sending the start of a file to a paired computer that shows it.

use super::pull::open_in_folder;
use super::{Ctx, refused};
use crate::logging::one_line;
use crate::manifest::mtime_ms;
use crate::model::{FolderId, ProjectId};
use crate::protocol::{MAX_PART, Response, write_msg};
use anyhow::bail;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tracing::info;

/// Replies `FilePart` and then exactly its `len` raw bytes, or `Refused`.
/// The file is checked the way `GetFile` checks it.
pub async fn read_file<S: AsyncRead + AsyncWrite + Unpin>(
    ctx: &Ctx<'_>,
    stream: &mut S,
    project: ProjectId,
    folder: FolderId,
    rel: &str,
    max: u64,
) -> anyhow::Result<()> {
    let (root, opened) = open_in_folder(ctx, project, folder, rel).await;
    let (file, meta) = match opened {
        Ok(x) => x,
        Err(reason) => {
            // Info, not a warning: the viewer only looks, and a file can go
            // between the preview and the click.
            let (peer, why) = (&ctx.peer.name, one_line(&reason));
            info!("{peer} could not look at {rel:?}: {why}");
            return write_msg(stream, &refused(reason)).await;
        }
    };
    let size = meta.len();
    let len = size.min(max).min(MAX_PART);
    if let Some(root) = &root {
        info!(
            "{} looked at {rel:?} in {} to show it",
            ctx.peer.name,
            root.display()
        );
    }
    let part = Response::FilePart {
        size,
        mtime_ms: mtime_ms(&meta),
        len,
    };
    write_msg(stream, &part).await?;
    let sent = tokio::io::copy(&mut file.take(len), stream).await?;
    stream.flush().await?;
    if sent != len {
        // The answer promised more bytes than there are; the stream cannot
        // be resynchronised, so end the connection.
        bail!("\"{rel}\" shrank while it was being read");
    }
    Ok(())
}

#[cfg(test)]
#[path = "peek_tests.rs"]
mod tests;
