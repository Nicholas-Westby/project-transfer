//! Reading a file part, against pretend computers over in-memory pipes.

use super::*;
use crate::net::test_support::addr;
use crate::protocol::{read_msg, write_msg};
use tokio::io::{AsyncWriteExt, DuplexStream, duplex};

/// A connection to a paired computer called Studio, over `stream`.
fn studio(stream: DuplexStream) -> Connection {
    Connection {
        stream: Box::new(stream),
        addr: addr("10.20.30.1:47820"),
        via: None,
        peer_id: uuid::Uuid::new_v4(),
        peer_name: "Studio".into(),
        peer_fingerprint: "ab".repeat(32),
        peer_os: None,
        scanned_ms: None,
    }
}

async fn read(conn: &mut Connection, max: u64) -> Result<FilePart, PartError> {
    let id = uuid::Uuid::from_u128(1);
    conn.read_part(id, id, "notes.txt", max).await
}

#[tokio::test]
async fn the_part_and_the_whole_size_arrive() {
    let (ours, mut theirs) = duplex(64 * 1024);
    tokio::spawn(async move {
        let _: Request = read_msg(&mut theirs).await.unwrap();
        let part = Response::FilePart {
            size: 18,
            mtime_ms: 7,
            len: 4,
        };
        write_msg(&mut theirs, &part).await.unwrap();
        theirs.write_all(b"line").await.unwrap();
    });
    let part = read(&mut studio(ours), 4).await.unwrap();
    assert_eq!((part.size, part.mtime_ms), (18, 7));
    assert_eq!(part.bytes, b"line");
}

#[tokio::test]
async fn an_older_version_hangs_up_and_is_named_as_such() {
    let (ours, mut theirs) = duplex(64 * 1024);
    tokio::spawn(async move {
        // It can't parse the request, so it ends the session.
        let _: serde_json::Value = read_msg(&mut theirs).await.unwrap();
    });
    let err = read(&mut studio(ours), 4).await.unwrap_err();
    assert!(matches!(err, PartError::Unanswered), "{err:?}");
    assert!(err.sentence("Studio").contains("older version"));
}

#[tokio::test(start_paused = true)]
async fn a_computer_that_stops_answering_is_given_up_on() {
    let (ours, mut theirs) = duplex(64 * 1024);
    tokio::spawn(async move {
        let _: Request = read_msg(&mut theirs).await.unwrap();
        // It keeps the connection open and never answers.
        std::future::pending::<()>().await;
    });
    let err = read(&mut studio(ours), 4).await.unwrap_err();
    assert!(matches!(err, PartError::TimedOut), "{err:?}");
    assert!(err.sentence("Studio").contains("stopped answering"));
}

#[tokio::test]
async fn an_oversized_answer_is_not_read() {
    let (ours, mut theirs) = duplex(64 * 1024);
    tokio::spawn(async move {
        let _: Request = read_msg(&mut theirs).await.unwrap();
        // Announces a megabyte of answer, which no header needs.
        theirs.write_all(&1_000_000u32.to_be_bytes()).await.unwrap();
    });
    let err = read(&mut studio(ours), 4).await.unwrap_err();
    let PartError::Failed(e) = &err else {
        panic!("expected a failure, got {err:?}");
    };
    assert!(e.to_string().contains("64 KiB"), "{e}");
}

#[tokio::test]
async fn a_refusal_is_passed_on_as_worded() {
    let (ours, mut theirs) = duplex(64 * 1024);
    tokio::spawn(async move {
        let _: Request = read_msg(&mut theirs).await.unwrap();
        let no = Response::Refused {
            reason: "Studio doesn't let this computer pull from it.".into(),
        };
        write_msg(&mut theirs, &no).await.unwrap();
    });
    let err = read(&mut studio(ours), 4).await.unwrap_err();
    assert_eq!(
        err.sentence("Studio"),
        "Studio doesn't let this computer pull from it."
    );
}

#[tokio::test]
async fn more_than_was_asked_for_is_not_read() {
    let (ours, mut theirs) = duplex(64 * 1024);
    tokio::spawn(async move {
        let _: Request = read_msg(&mut theirs).await.unwrap();
        let part = Response::FilePart {
            size: 100,
            mtime_ms: 0,
            len: 50,
        };
        write_msg(&mut theirs, &part).await.unwrap();
    });
    let err = read(&mut studio(ours), 4).await.unwrap_err();
    assert!(matches!(err, PartError::Failed(_)), "{err:?}");
}
