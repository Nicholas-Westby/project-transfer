//! Entries a transfer could not apply: the summary says which step failed
//! and why, and both computers log each one.
//!
//! Every test here captures the log. One that logs from other threads
//! (an app core) would decide for these whether a log line is wanted.

mod support;

use project_transfer::model::Direction;
use project_transfer::net::Connection;
use project_transfer::transfer::{self, Preview, Summary};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use support::*;

/// What this thread logs while the guard lives. A tokio test runs on one
/// thread, so the handlers of both computers log here too.
#[derive(Clone, Default)]
struct Log(Arc<Mutex<Vec<u8>>>);

impl std::io::Write for Log {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(b);
        Ok(b.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl Log {
    fn start() -> (Log, tracing::subscriber::DefaultGuard) {
        let log = Log::default();
        let sink = log.clone();
        let subscriber = tracing_subscriber::fmt()
            .with_writer(move || sink.clone())
            .with_ansi(false)
            .finish();
        (log, tracing::subscriber::set_default(subscriber))
    }

    fn text(&self) -> String {
        String::from_utf8_lossy(&self.0.lock().unwrap()).into_owned()
    }

    /// Whether a single warning holds every part.
    fn warned(&self, parts: &[&str]) -> bool {
        self.text()
            .lines()
            .any(|l| l.contains("WARN") && parts.iter().all(|p| l.contains(p)))
    }
}

async fn name(i: &Instance) -> String {
    i.shared.settings.read().await.name.clone()
}

async fn run(conn: &mut Connection, from: &Instance, preview: Preview) -> Summary {
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    transfer::execute(conn, &from.shared, preview, tx, Default::default())
        .await
        .unwrap()
}

/// A folder takes the place of `same.txt`, so its time cannot be fixed.
fn folder_in_the_way(root: &Path) {
    std::fs::remove_file(root.join("same.txt")).unwrap();
    std::fs::create_dir(root.join("same.txt")).unwrap();
}

fn reason(s: &Summary, rel: &str) -> String {
    let found = s.failures.iter().find(|(r, _)| r == rel);
    found.map(|(_, why)| why.clone()).unwrap_or_default()
}

#[tokio::test]
async fn a_push_says_which_step_failed_and_both_computers_log_each_refusal() {
    let (log, _guard) = Log::start();
    let (a, b) = pair_full().await;
    let src = a.project_dir("app");
    write(&src, "same.txt", "same");
    write(&src, "sub/kept.txt", "kept");
    let p = a.add_project("Garden", &[("app", &src)]).await;
    push(&a, &b, p.id, false).await;
    let dest = b.dev().join("app");

    // A time-only change and a new file, previewed while B still matches...
    set_mtime(&src.join("same.txt"), 1_600_000_000_000);
    write(&src, "sub/new.txt", "new");
    let mut conn = a.open(&b).await;
    let req = a.request(&b, p.id, Direction::Push).await;
    let preview = transfer::prepare(&mut conn, &a.shared, req).await.unwrap();
    // ...then B changes so that neither can be applied there.
    folder_in_the_way(&dest);
    std::fs::remove_dir_all(dest.join("sub")).unwrap();
    write(&dest, "sub", "a file where the folder was");
    let summary = run(&mut conn, &a, preview).await;

    assert_eq!(summary.failures.len(), 2, "{:?}", summary.failures);
    let time = reason(&summary, "same.txt");
    assert!(
        time.contains("modified time") && time.contains("is not a file"),
        "{time}"
    );
    let new = reason(&summary, "sub/new.txt");
    assert!(new.contains("write"), "{new}");

    let (a_name, b_name) = (name(&a).await, name(&b).await);
    let (src, dest) = (src.display().to_string(), dest.display().to_string());
    for rel in ["\"same.txt\"", "\"sub/new.txt\""] {
        assert!(
            log.warned(&[rel, &src, &b_name]),
            "A pushed: {}",
            log.text()
        );
        assert!(
            log.warned(&[rel, &dest, &a_name]),
            "B refused: {}",
            log.text()
        );
    }
    assert!(log.warned(&["\"same.txt\"", &src, "is not a file"]));
    // B counts what it refused, for its own activity line.
    let received = b.received.lock().unwrap().clone();
    assert_eq!(received.last(), Some(&(p.id, a.id().await, 0, 2)));
}

#[tokio::test]
async fn a_pull_logs_what_failed_here_and_what_the_other_computer_could_not_send() {
    let (log, _guard) = Log::start();
    let (a, b) = pair_full().await;
    let theirs = b.project_dir("app");
    write(&theirs, "same.txt", "same");
    write(&theirs, "gone.txt", "soon gone");
    let p = b.add_project("Garden", &[("app", &theirs)]).await;
    pull(&a, &b, p.id).await;

    set_mtime(&theirs.join("same.txt"), 1_600_000_000_000);
    write(&theirs, "gone.txt", "changed, then gone");
    let mut conn = a.open(&b).await;
    let req = a.request(&b, p.id, Direction::Pull).await;
    let preview = transfer::prepare(&mut conn, &a.shared, req).await.unwrap();
    let mine = PathBuf::from(&preview.folders[0].dest_path);
    folder_in_the_way(&mine);
    std::fs::remove_file(theirs.join("gone.txt")).unwrap();
    let summary = run(&mut conn, &a, preview).await;

    assert_eq!(summary.failures.len(), 2, "{:?}", summary.failures);
    assert!(reason(&summary, "same.txt").contains("modified time"));
    let (a_name, b_name) = (name(&a).await, name(&b).await);
    let mine = mine.display().to_string();
    // A, which pulled: its own step failed for one, B refused the other.
    assert!(
        log.warned(&["\"same.txt\"", &mine, &b_name, "is not a file"]),
        "{}",
        log.text()
    );
    assert!(
        log.warned(&["\"gone.txt\"", &mine, &b_name]),
        "{}",
        log.text()
    );
    // B, which could not send it from its folder.
    let theirs = theirs.display().to_string();
    assert!(
        log.warned(&["\"gone.txt\"", &theirs, &a_name]),
        "{}",
        log.text()
    );
}

#[cfg(unix)]
#[tokio::test]
async fn a_name_with_a_line_break_stays_on_one_log_line() {
    let (log, _guard) = Log::start();
    let (a, b) = pair_full().await;
    let src = a.project_dir("app");
    write(&src, "same.txt", "same");
    let p = a.add_project("Garden", &[("app", &src)]).await;
    push(&a, &b, p.id, false).await;
    let dest = b.dev().join("app");

    // Such names are legal here, and the other computer sends what it has.
    std::fs::rename(src.join("same.txt"), src.join("two\nlines.txt")).unwrap();
    std::fs::rename(dest.join("same.txt"), dest.join("two\nlines.txt")).unwrap();
    set_mtime(&src.join("two\nlines.txt"), 1_600_000_000_000);
    let mut conn = a.open(&b).await;
    let req = a.request(&b, p.id, Direction::Push).await;
    let preview = transfer::prepare(&mut conn, &a.shared, req).await.unwrap();
    std::fs::remove_file(dest.join("two\nlines.txt")).unwrap();
    std::fs::create_dir(dest.join("two\nlines.txt")).unwrap();
    run(&mut conn, &a, preview).await;

    // Each warning, its reason included, on a line of its own.
    let (a_name, b_name) = (name(&a).await, name(&b).await);
    for who in [&b_name, &a_name] {
        let whole = [r#""two\nlines.txt""#, who, "is not a file"];
        assert!(log.warned(&whole), "{}", log.text());
    }
}

/// Names that sort in the order they were made, so a batch keeps its order.
fn numbered(i: u32) -> String {
    format!("f{i:02}.txt")
}

/// The batch is bigger than the window, so answers are read while later
/// requests go out; each refusal must still name its own entry.
fn assert_only_failed(s: &Summary, stuck: &[u32]) {
    let failed: Vec<&str> = s.failures.iter().map(|(rel, _)| rel.as_str()).collect();
    let want: Vec<String> = stuck.iter().map(|&i| numbered(i)).collect();
    assert_eq!(failed, want);
    assert_eq!(s.files, 40 - stuck.len() as u64);
}

#[tokio::test]
async fn refusals_among_many_pushed_changes_name_the_right_entries() {
    let (log, _guard) = Log::start();
    let (a, b) = pair_full().await;
    let src = a.project_dir("app");
    for i in 0..40 {
        write(&src, &numbered(i), "same");
    }
    let p = a.add_project("Garden", &[("app", &src)]).await;
    push(&a, &b, p.id, false).await;
    let dest = b.dev().join("app");

    // Only times change, so each entry is a small request of its own.
    for i in 0..40 {
        set_mtime(
            &src.join(numbered(i)),
            1_600_000_000_000 + i64::from(i) * 1000,
        );
    }
    let mut conn = a.open(&b).await;
    let req = a.request(&b, p.id, Direction::Push).await;
    let preview = transfer::prepare(&mut conn, &a.shared, req).await.unwrap();
    // Some cannot be applied there, spread across the whole batch.
    let stuck = [3, 17, 34, 39];
    for i in stuck {
        let file = dest.join(numbered(i));
        std::fs::remove_file(&file).unwrap();
        std::fs::create_dir(&file).unwrap();
    }
    let summary = run(&mut conn, &a, preview).await;

    assert_only_failed(&summary, &stuck);
    let (src, b_name) = (src.display().to_string(), name(&b).await);
    for i in stuck {
        let rel = format!("\"{}\"", numbered(i));
        assert!(log.warned(&[&rel, &src, &b_name]), "{}", log.text());
    }
}

#[tokio::test]
async fn a_pull_names_the_right_files_among_many_the_other_computer_lost() {
    let (_log, _guard) = Log::start();
    let (a, b) = pair_full().await;
    let theirs = b.project_dir("app");
    for i in 0..40 {
        write(&theirs, &numbered(i), &format!("content {i}"));
    }
    let p = b.add_project("Garden", &[("app", &theirs)]).await;
    let mut conn = a.open(&b).await;
    let req = a.request(&b, p.id, Direction::Pull).await;
    let preview = transfer::prepare(&mut conn, &a.shared, req).await.unwrap();
    // Gone after the preview, so asking for them is refused.
    let gone = [3, 17, 34, 39];
    for i in gone {
        std::fs::remove_file(theirs.join(numbered(i))).unwrap();
    }
    let summary = run(&mut conn, &a, preview).await;

    assert_only_failed(&summary, &gone);
    // Each file that did arrive holds its own content.
    let mine = a.dev().join("app");
    for i in (0..40).filter(|i| !gone.contains(i)) {
        assert_eq!(read(&mine, &numbered(i)), format!("content {i}"));
    }
}
