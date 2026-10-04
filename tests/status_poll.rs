//! The status poll between paired computers that share unequally.

mod core_support;

use core_support::*;
use project_transfer::core::{Action, AppCore};
use project_transfer::model::InstanceId;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}

/// Asks `b` to poll `peer` now and waits until a poll that started after
/// this call has finished.
fn poll_once(b: &AppCore, peer: InstanceId) {
    let since = now_ms();
    b.act(Action::SelectPeer(peer));
    wait(b, "a fresh status poll", |s| {
        s.peer(peer)
            .is_some_and(|v| v.online && v.last_seen_ms.is_some_and(|t| t >= since))
    });
}

#[test]
fn a_computer_allowed_nothing_is_not_turned_away_by_its_own_status_poll() {
    let (da, db) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    // A lets B do nothing; B lets A push and pull.
    let (a, b) = start_pairing_offering(da.path(), db.path(), NONE);
    b.act(Action::AnswerPair(Some(ALL)));
    a.act(Action::ConfirmPairCode(true));
    wait(&b, "B to list A", |s| s.peers.len() == 1);
    wait(&a, "A to list B", |s| s.peers.len() == 1);
    let a_id = a.state().me.id;

    // A's status now lists a project, which B may not look into.
    let src = da.path().join("work/garden");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(src.join("plan.txt"), "rows").unwrap();
    a.act(Action::CreateProject {
        name: "garden".into(),
        folder: src,
    });
    a.settle();

    poll_once(&b, a_id);
    // A refusal from the first poll reaches A's activity well before a
    // second poll finishes.
    poll_once(&b, a_id);
    std::thread::sleep(Duration::from_millis(200));

    let activity = a.state().activity.clone();
    assert!(
        activity.iter().all(|l| !l.text.starts_with("Turned away")),
        "{activity:#?}"
    );
    assert!(b.state().remote_projects.is_empty());
}
