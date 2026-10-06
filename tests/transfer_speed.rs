//! Transfers over a link with a long round trip: no file or folder may cost
//! a round trip of its own.

mod support;

use project_transfer::model::{Direction, ProjectId};
use project_transfer::transfer::{self, Progress, Summary};
use std::path::Path;
use std::time::{Duration, Instant};
use support::delay::{Link, open_through};
use support::*;
use tokio_util::sync::CancellationToken;

/// Each received file also costs a disk sync, which is slow on some disks.
/// The delay is long enough that round trips stay the bigger share.
const ONE_WAY: Duration = Duration::from_millis(100);
const FILES: u64 = 40;

fn fill(root: &Path) {
    for i in 0..FILES {
        write(root, &format!("part{}/file{i}.txt", i % 4), "some text");
    }
}

/// What waiting for every answer before the next request would cost.
fn one_by_one() -> Duration {
    ONE_WAY * 2 * FILES as u32
}

/// Runs the transfer through `link`, timing only what follows the preview.
async fn timed(
    a: &Instance,
    b: &Instance,
    link: &Link,
    project: ProjectId,
    d: Direction,
) -> (Summary, Duration) {
    let mut conn = open_through(a, b, link).await;
    let req = a.request(b, project, d).await;
    let preview = transfer::prepare(&mut conn, &a.shared, req).await.unwrap();
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let t = Instant::now();
    let summary = transfer::execute(&mut conn, &a.shared, preview, tx, Default::default())
        .await
        .unwrap();
    (summary, t.elapsed())
}

#[tokio::test]
async fn a_push_sends_files_without_waiting_for_each_answer() {
    let (a, b) = pair_full().await;
    let src = a.project_dir("app");
    fill(&src);
    let docs = a.project_dir("docs");
    write(&docs, "guide.md", "read me");
    let p = a
        .add_project("Garden", &[("app", &src), ("docs", &docs)])
        .await;
    let link = Link::start(b.addr, ONE_WAY).await;
    let (summary, took) = timed(&a, &b, &link, p.id, Direction::Push).await;
    assert!(summary.failures.is_empty(), "{:?}", summary.failures);
    assert_eq!(summary.files, FILES + 1);
    assert_eq!(tree(&b.dev().join("Garden/app")), tree(&src));
    assert_eq!(tree(&b.dev().join("Garden/docs")), tree(&docs));
    assert!(took < one_by_one() / 2, "took {took:?}");
}

#[tokio::test]
async fn a_pull_asks_for_files_without_waiting_for_each_one() {
    let (a, b) = pair_full().await;
    let theirs = b.project_dir("beds");
    fill(&theirs);
    let p = b.add_project("Orchard", &[("beds", &theirs)]).await;
    let link = Link::start(b.addr, ONE_WAY).await;
    let (summary, took) = timed(&a, &b, &link, p.id, Direction::Pull).await;
    assert!(summary.failures.is_empty(), "{:?}", summary.failures);
    assert_eq!(summary.files, FILES);
    assert_eq!(tree(&a.dev().join("beds")), tree(&theirs));
    assert!(took < one_by_one() / 2, "took {took:?}");
}

#[tokio::test]
async fn cancelling_with_answers_outstanding_stops_promptly() {
    let (a, b) = pair_full().await;
    let src = a.project_dir("app");
    for i in 0..200 {
        write(&src, &format!("f{i}.txt"), "x");
    }
    let p = a.add_project("Garden", &[("app", &src)]).await;
    let link = Link::start(b.addr, ONE_WAY).await;
    let mut conn = open_through(&a, &b, &link).await;
    let req = a.request(&b, p.id, Direction::Push).await;
    let preview = transfer::prepare(&mut conn, &a.shared, req).await.unwrap();
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let cancel = CancellationToken::new();
    let stop = cancel.clone();
    tokio::spawn(async move {
        while let Some(p) = rx.recv().await {
            if matches!(p, Progress::File { .. }) {
                stop.cancel();
            }
        }
    });
    let t = Instant::now();
    let err = transfer::execute(&mut conn, &a.shared, preview, tx, cancel)
        .await
        .unwrap_err();
    assert!(err.to_string().contains("cancelled"), "{err}");
    assert!(t.elapsed() < Duration::from_secs(1), "{:?}", t.elapsed());
    drop(conn);
    let dest = b.dev().join("app");
    wait_for(|| temps(&dest).is_empty()).await;
}
