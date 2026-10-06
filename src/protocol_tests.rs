use super::*;
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::io::AsyncWrite;

#[tokio::test]
async fn framing_round_trips() {
    let (mut a, mut b) = tokio::io::duplex(1024);
    let req = Request::PutFile {
        rel: "a/b.txt".into(),
        size: 3,
        mtime_ms: 7,
        exec: true,
    };
    write_msg(&mut a, &req).await.unwrap();
    write_msg(&mut a, &Response::Ok).await.unwrap();
    assert_eq!(read_msg::<_, Request>(&mut b).await.unwrap(), req);
    assert_eq!(read_msg::<_, Response>(&mut b).await.unwrap(), Response::Ok);
}

#[tokio::test]
async fn limited_read_rejects_over_its_limit() {
    let (mut a, mut b) = tokio::io::duplex(64);
    a.write_all(&(64u32 * 1024 + 1).to_be_bytes())
        .await
        .unwrap();
    let err = read_msg_limited::<_, Request>(&mut b, 64 * 1024)
        .await
        .unwrap_err();
    assert!(err.to_string().contains("64 KiB"), "{err}");
}

#[tokio::test]
async fn oversize_length_is_rejected() {
    let (mut a, mut b) = tokio::io::duplex(64);
    a.write_all(&u32::MAX.to_be_bytes()).await.unwrap();
    let err = read_msg::<_, Request>(&mut b).await.unwrap_err();
    assert!(err.to_string().contains("64 MiB"));
}

/// Counts the writes a message takes.
#[derive(Default)]
struct Writes(Vec<usize>);

impl AsyncWrite for Writes {
    fn poll_write(
        mut self: Pin<&mut Self>,
        _: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        self.0.push(buf.len());
        Poll::Ready(Ok(buf.len()))
    }
    fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Poll::Ready(Ok(()))
    }
    fn poll_shutdown(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

/// Over TLS each write is a record and, with no delay, a packet; a small
/// message should be one of each.
#[tokio::test]
async fn a_message_goes_out_in_one_write() {
    let mut w = Writes::default();
    write_msg(&mut w, &Request::Status).await.unwrap();
    assert_eq!(w.0.len(), 1, "{:?}", w.0);
}
