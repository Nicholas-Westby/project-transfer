//! Transfers over a link with a long round trip: no file or folder may cost
//! a round trip of its own.

mod support;

use project_transfer::model::{Direction, ProjectId};
use project_transfer::transfer::{self, Progress, Summary};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use support::delay::{Link, open_through};
use support::*;
use tokio_util::sync::CancellationToken;

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

/// Runs the transfer of `project` through a link with a long round trip and
/// that of `twin`, the same files in another project, through one without.
/// Returns the first one's summary and how much longer it took: each
/// received file costs a disk sync, which can take as long as a round trip
/// on a busy disk, and only the difference is down to round trips.
async fn delay_cost(
    a: &Instance,
    b: &Instance,
    project: ProjectId,
    twin: ProjectId,
    d: Direction,
) -> (Summary, Duration) {
    let direct = Link::start(b.addr, Duration::ZERO).await;
    let (base, without) = timed(a, b, &direct, twin, d).await;
    let link = Link::start(b.addr, ONE_WAY).await;
    let (summary, with) = timed(a, b, &link, project, d).await;
    assert_eq!(base.files, summary.files, "the twin moves as many files");
    (summary, with.saturating_sub(without))
}

/// A project of `a`'s with FILES files in its `app` folder and one in `docs`.
async fn two_folders(a: &Instance, name: &str) -> (ProjectId, PathBuf, PathBuf) {
    let src = a.project_dir(&format!("{name}/app"));
    fill(&src);
    let docs = a.project_dir(&format!("{name}/docs"));
    write(&docs, "guide.md", "read me");
    let p = a.add_project(name, &[("app", &src), ("docs", &docs)]).await;
    (p.id, src, docs)
}

#[tokio::test]
async fn a_push_sends_files_without_waiting_for_each_answer() {
    let (a, b) = pair_full().await;
    let (p, src, docs) = two_folders(&a, "Garden").await;
    let (twin, _, _) = two_folders(&a, "Meadow").await;
    let (summary, cost) = delay_cost(&a, &b, p, twin, Direction::Push).await;
    assert!(summary.failures.is_empty(), "{:?}", summary.failures);
    assert_eq!(summary.files, FILES + 1);
    assert_eq!(tree(&b.dev().join("Garden/app")), tree(&src));
    assert_eq!(tree(&b.dev().join("Garden/docs")), tree(&docs));
    assert!(cost < one_by_one() / 2, "the delay cost {cost:?}");
}

#[tokio::test]
async fn a_pull_asks_for_files_without_waiting_for_each_one() {
    let (a, b) = pair_full().await;
    let theirs = b.project_dir("beds");
    fill(&theirs);
    let p = b.add_project("Orchard", &[("beds", &theirs)]).await;
    let rows = b.project_dir("rows");
    fill(&rows);
    let twin = b.add_project("Meadow", &[("rows", &rows)]).await;
    let (summary, cost) = delay_cost(&a, &b, p.id, twin.id, Direction::Pull).await;
    assert!(summary.failures.is_empty(), "{:?}", summary.failures);
    assert_eq!(summary.files, FILES);
    assert_eq!(tree(&a.dev().join("beds")), tree(&theirs));
    assert!(cost < one_by_one() / 2, "the delay cost {cost:?}");
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
