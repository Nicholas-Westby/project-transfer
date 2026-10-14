//! Sync from the UI's side: the preview says why it went the way it did.

use super::paired::*;
use super::*;
use crate::model::Direction;
use crate::transfer::TransferRequest;

#[test]
fn a_sync_that_cannot_be_prepared_says_so() {
    let (a, b) = (Fixture::new(), Fixture::new());
    pair(&a, &b, Some(at(&b)), None);
    // B doesn't know A, so it refuses everything A asks.
    let p = a.create("garden-planner", "app");
    a.act(Action::PrepareSync(TransferRequest {
        peer: id(&b),
        project: p.id,
        direction: Direction::Push,
        send_everything: false,
    }));
    wait_until(&a.core, "the failure", |s| {
        matches!(s.transfer, TransferState::Failed(_))
    });
    let said = a.core.state().activity.clone();
    assert!(
        said.iter()
            .any(|l| l.text.starts_with("Could not prepare the sync: ")),
        "{said:#?}"
    );
}

#[test]
fn a_sync_compared_again_stays_a_sync_and_says_when_it_turns_around() {
    let (a, b) = (Fixture::new(), Fixture::new());
    for f in [&a, &b] {
        f.act(Action::SetProjectsFolder(f.dir.path().join("Dev")));
    }
    pair(&a, &b, Some(at(&b)), None);
    pair(&b, &a, Some(at(&a)), None);
    let p = a.create("garden-planner", "app");
    let root = p.folders[0].local_path.clone().unwrap();
    for rel in ["exports/one.svg", "exports/two.svg", "main.rs"] {
        std::fs::create_dir_all(root.join(rel).parent().unwrap()).unwrap();
        std::fs::write(root.join(rel), "first").unwrap();
        filetime::set_file_mtime(root.join(rel), filetime::FileTime::from_unix_time(1_000, 0))
            .unwrap();
    }
    let ask = TransferRequest {
        peer: id(&b),
        project: p.id,
        direction: Direction::Push,
        send_everything: false,
    };
    a.act(Action::Prepare(ask.clone()));
    wait_until(&a.core, "the preview", |s| {
        matches!(s.transfer, TransferState::Ready(_))
    });
    a.act(Action::Execute);
    wait_until(&a.core, "the push", |s| {
        matches!(s.transfer, TransferState::Finished(_))
    });
    a.act(Action::DismissTransfer);
    // Two newer changes here, one on B.
    for rel in ["exports/one.svg", "exports/two.svg"] {
        std::fs::write(root.join(rel), "changed here").unwrap();
    }
    let there = b.core.core.shared.projects.blocking_read()[0].folders[0]
        .local_path
        .clone();
    std::fs::write(there.unwrap().join("main.rs"), "changed there").unwrap();
    let going = |s: &UiState, d: Direction| match &s.transfer {
        TransferState::Ready(p) => p.sync.is_some() && p.request.direction == d,
        _ => false,
    };
    a.act(Action::PrepareSync(ask));
    wait_until(&a.core, "a sync that pushes", |s| going(s, Direction::Push));
    a.act(Action::AddIgnore("/exports/".into()));
    wait_until(&a.core, "a sync that pulls", |s| going(s, Direction::Pull));
    let TransferState::Ready(p) = a.core.state().transfer.clone() else {
        unreachable!()
    };
    let first = p.warnings.first().cloned().unwrap_or_default();
    assert!(
        first.starts_with("Sync now pulls instead of pushing"),
        "{first}"
    );
}

/// `garden-planner` with `notes.txt`, `main.rs` and `lib.rs`, pushed from
/// A to B; then `changed` are rewritten on B. Returns the sync request.
fn pushed_then_changed_there(a: &Fixture, b: &Fixture, changed: &[&str]) -> TransferRequest {
    for f in [a, b] {
        f.act(Action::SetProjectsFolder(f.dir.path().join("Dev")));
    }
    pair(a, b, Some(at(b)), None);
    pair(b, a, Some(at(a)), None);
    let p = a.create("garden-planner", "app");
    let root = p.folders[0].local_path.clone().unwrap();
    for rel in ["notes.txt", "main.rs", "lib.rs"] {
        std::fs::write(root.join(rel), "first").unwrap();
        filetime::set_file_mtime(root.join(rel), filetime::FileTime::from_unix_time(1_000, 0))
            .unwrap();
    }
    let ask = TransferRequest {
        peer: id(b),
        project: p.id,
        direction: Direction::Push,
        send_everything: false,
    };
    a.act(Action::Prepare(ask.clone()));
    wait_until(&a.core, "the preview", |s| {
        matches!(s.transfer, TransferState::Ready(_))
    });
    a.act(Action::Execute);
    wait_until(&a.core, "the push", |s| {
        matches!(s.transfer, TransferState::Finished(_))
    });
    a.act(Action::DismissTransfer);
    let there = b.core.core.shared.projects.blocking_read()[0].folders[0]
        .local_path
        .clone()
        .unwrap();
    for rel in changed {
        std::fs::write(there.join(rel), "changed there").unwrap();
    }
    ask
}

/// The sync preview going `d`, with `there` files newer on B.
fn sync_going(s: &UiState, d: Direction, there: u64) -> bool {
    match &s.transfer {
        TransferState::Ready(p) => {
            p.request.direction == d && p.sync.as_ref().is_some_and(|c| c.tally.there == there)
        }
        _ => false,
    }
}

#[test]
fn a_sync_left_with_nothing_to_do_does_not_say_it_turned_around() {
    let (a, b) = (Fixture::new(), Fixture::new());
    let ask = pushed_then_changed_there(&a, &b, &["notes.txt"]);
    a.act(Action::PrepareSync(ask));
    wait_until(&a.core, "a sync that pulls", |s| {
        sync_going(s, Direction::Pull, 1)
    });
    a.act(Action::AddIgnore("/notes.txt".into()));
    wait_until(&a.core, "a sync with nothing to do", |s| {
        sync_going(s, Direction::Push, 0)
    });
    let TransferState::Ready(p) = a.core.state().transfer.clone() else {
        unreachable!()
    };
    assert!(p.is_empty());
    assert!(p.warnings.is_empty(), "{:?}", p.warnings);
}

#[test]
fn a_sync_compared_again_the_same_way_says_nothing_about_turning() {
    let (a, b) = (Fixture::new(), Fixture::new());
    let ask = pushed_then_changed_there(&a, &b, &["notes.txt", "main.rs"]);
    a.act(Action::PrepareSync(ask));
    wait_until(&a.core, "a sync that pulls", |s| {
        sync_going(s, Direction::Pull, 2)
    });
    a.act(Action::AddIgnore("/notes.txt".into()));
    wait_until(&a.core, "the sync again", |s| {
        sync_going(s, Direction::Pull, 1)
    });
    let TransferState::Ready(p) = a.core.state().transfer.clone() else {
        unreachable!()
    };
    assert!(p.warnings.is_empty(), "{:?}", p.warnings);
}
