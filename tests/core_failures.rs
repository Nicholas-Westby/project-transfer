//! Two app cores whose push leaves an entry that cannot be applied: both
//! activity strips warn. Apart from the tests that capture the log, since
//! these cores log from threads of their own.

mod core_support;

use core_support::*;
use project_transfer::core::{Action, ActivityKind, AppCore, TransferState};
use project_transfer::model::Direction;
use project_transfer::transfer::TransferRequest;
use std::path::Path;

/// Previews a push, runs `meanwhile`, then confirms it and waits for the end.
fn push_through(a: &AppCore, req: &TransferRequest, meanwhile: impl FnOnce()) {
    a.act(Action::Prepare(req.clone()));
    wait(a, "the preview", |s| {
        matches!(s.transfer, TransferState::Ready(_))
    });
    meanwhile();
    a.act(Action::Execute);
    wait(a, "the push to finish", |s| {
        matches!(s.transfer, TransferState::Finished(_))
    });
    a.act(Action::DismissTransfer);
}

/// A folder takes the place of `same.txt`, so its time cannot be fixed.
fn folder_in_the_way(root: &Path) {
    std::fs::remove_file(root.join("same.txt")).unwrap();
    std::fs::create_dir(root.join("same.txt")).unwrap();
}

#[test]
fn both_activity_strips_warn_when_entries_could_not_be_applied() {
    let (da, db) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let (a, b) = start_pairing(da.path(), db.path());
    b.act(Action::AnswerPair(Some(ALL)));
    a.act(Action::ConfirmPairCode(true));
    let b_id = b.state().me.id;
    wait(&a, "B to be online", |s| {
        s.peer(b_id).is_some_and(|v| v.online)
    });
    let src = da.path().join("src/app");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(src.join("same.txt"), "same").unwrap();
    a.act(Action::CreateProject {
        name: "Garden".into(),
        folder: src.clone(),
    });
    a.settle();
    let req = TransferRequest {
        peer: b_id,
        project: a.state().projects[0].id,
        direction: Direction::Push,
        send_everything: false,
    };
    push_through(&a, &req, || {});

    let older = filetime::FileTime::from_unix_time(1_600_000_000, 0);
    filetime::set_file_mtime(src.join("same.txt"), older).unwrap();
    push_through(&a, &req, || folder_in_the_way(&db.path().join("Dev/app")));

    let s = a.state();
    let line = s
        .activity
        .iter()
        .rev()
        .find(|l| l.text.starts_with("Pushed"));
    let line = line.expect("a line for the push");
    assert_eq!(line.kind, ActivityKind::Warn, "{}", line.text);
    assert!(line.text.contains("1 could not be copied"), "{}", line.text);
    assert!(line.text.contains("log"), "{}", line.text);
    wait(&b, "B to warn about what it refused", |s| {
        s.activity.iter().any(|l| {
            l.kind == ActivityKind::Warn
                && l.text.contains("1 could not be written")
                && l.text.contains("log")
        })
    });
}
