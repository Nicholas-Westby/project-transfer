use super::*;
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::io::{AsyncWrite, AsyncWriteExt};

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

#[tokio::test]
async fn reading_a_file_part_round_trips() {
    let (mut a, mut b) = tokio::io::duplex(1024);
    let ask = Request::ReadFile {
        project: uuid::Uuid::from_u128(1),
        folder: uuid::Uuid::from_u128(2),
        rel: "src/main.rs".into(),
        max: MAX_PART,
    };
    let part = Response::FilePart {
        size: 3_000_000,
        mtime_ms: 1_700_000_000_000,
        len: MAX_PART,
    };
    write_msg(&mut a, &ask).await.unwrap();
    write_msg(&mut a, &part).await.unwrap();
    assert_eq!(read_msg::<_, Request>(&mut b).await.unwrap(), ask);
    assert_eq!(read_msg::<_, Response>(&mut b).await.unwrap(), part);
    assert_eq!(ask.kind(), "ReadFile");
}

#[test]
fn a_project_from_an_older_version_was_not_last_moved_with_us() {
    let older = r#"{"name":"garden","folders":[],
        "primary":"00000000-0000-0000-0000-000000000001"}"#;
    let info: RemoteProject = serde_json::from_str(older).unwrap();
    assert!(!info.last_with_you);
}

#[test]
fn a_scan_from_an_older_version_names_no_ignored_folders() {
    let older = r#"{"Manifest":{"manifest":{"entries":[],"ignored_in_dir":{"":3}},
        "path":"/x/app","exists":true,"set_up":true,"os":"MacOs"}}"#;
    let Response::Manifest(scan) = serde_json::from_str(older).unwrap() else {
        panic!("not a folder scan");
    };
    assert!(scan.manifest.ignored_dirs.is_empty());
    assert_eq!(scan.manifest.ignored_in_dir.get(""), Some(&3));
}
