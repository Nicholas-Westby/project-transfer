//! A file the viewer asks for and is refused is logged on the computer that
//! refused it.
//!
//! Alone in its binary: tracing keeps whether a log line is wanted from the
//! first thread that reaches it, so a parallel test that reaches the same
//! line without capturing the log would decide it for this one.

mod support;

use std::sync::{Arc, Mutex};
use support::*;

/// What this thread logs while the guard lives. A tokio test runs on one
/// thread, so both computers log here.
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

#[tokio::test]
async fn a_refused_read_is_logged_where_it_was_refused() {
    let log = Log::default();
    let sink = log.clone();
    let subscriber = tracing_subscriber::fmt()
        .with_writer(move || sink.clone())
        .with_ansi(false)
        .finish();
    let _guard = tracing::subscriber::set_default(subscriber);
    let (a, b) = pair_full().await;
    let theirs = b.project_dir("app");
    write(&theirs, "notes.txt", "line one");
    let p = b.add_project("Garden", &[("app", &theirs)]).await;
    let mut conn = a.open(&b).await;
    let folder = p.folders[0].id;
    let read = conn.read_part(p.id, folder, "missing.txt", 4).await;
    assert!(read.is_err());
    let text = String::from_utf8(log.0.lock().unwrap().clone()).unwrap();
    let want = "could not look at \"missing.txt\": \"missing.txt\" is not a file on ";
    assert!(text.contains(want), "{text}");
}
