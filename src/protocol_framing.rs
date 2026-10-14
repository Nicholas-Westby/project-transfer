//! Messages on the wire: a u32 big-endian length, then JSON.

use super::MAX_MSG;
use anyhow::{Context, bail};
use serde::{Serialize, de::DeserializeOwned};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

/// u32 big-endian length, then JSON.
pub async fn write_msg<W: AsyncWrite + Unpin, T: Serialize>(
    w: &mut W,
    msg: &T,
) -> anyhow::Result<()> {
    let mut framed = vec![0u8; 4];
    serde_json::to_writer(&mut framed, msg)?;
    let len = framed.len() - 4;
    if len as u64 > MAX_MSG as u64 {
        bail!("message of {len} bytes is over the 64 MiB limit");
    }
    framed[..4].copy_from_slice(&(len as u32).to_be_bytes());
    // One write: over TLS a write is a record, and without Nagle a packet.
    w.write_all(&framed).await?;
    w.flush().await?;
    Ok(())
}

pub async fn read_msg<R: AsyncRead + Unpin, T: DeserializeOwned>(r: &mut R) -> anyhow::Result<T> {
    read_msg_limited(r, MAX_MSG).await
}

/// Like `read_msg`, with a lower cap for callers that expect only small messages.
pub async fn read_msg_limited<R: AsyncRead + Unpin, T: DeserializeOwned>(
    r: &mut R,
    max: u32,
) -> anyhow::Result<T> {
    let mut len = [0u8; 4];
    r.read_exact(&mut len).await?;
    let len = u32::from_be_bytes(len);
    // Checked before allocating so a bad peer cannot make us reserve gigabytes.
    if len > max.min(MAX_MSG) {
        bail!(
            "incoming message of {len} bytes is over the {} limit",
            size(max.min(MAX_MSG))
        );
    }
    let mut body = vec![0u8; len as usize];
    r.read_exact(&mut body).await?;
    serde_json::from_slice(&body).context("could not parse message")
}

fn size(bytes: u32) -> String {
    match bytes {
        b if b % (1024 * 1024) == 0 => format!("{} MiB", b / (1024 * 1024)),
        b if b % 1024 == 0 => format!("{} KiB", b / 1024),
        b => format!("{b} bytes"),
    }
}
