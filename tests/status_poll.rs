//! The status poll between paired computers that share unequally.

mod core_support;

use core_support::*;
use project_transfer::core::{Action, AppCore, UiState};
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

#[test]
fn a_paired_computer_replaced_at_its_address_is_reported_not_turned_away() {
    let (da, db) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let (a, b) = start_pairing_offering(da.path(), db.path(), ALL);
    b.act(Action::AnswerPair(Some(ALL)));
    a.act(Action::ConfirmPairCode(true));
    wait(&a, "A to list B", |s| s.peers.len() == 1);
    wait(&b, "B to list A", |s| s.peers.len() == 1);
    // As if B were set up again: a new identity answers where B was.
    drop(a);
    let peers = da.path().join("home/peers.json");
    let mut stored: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&peers).unwrap()).unwrap();
    stored[0]["fingerprint"] = "00".repeat(32).into();
    std::fs::write(&peers, stored.to_string()).unwrap();

    let a = start(da.path());
    let b_id = b.state().me.id;
    // A remembers its selection a moment after pairing, which the drop above
    // can beat, so select B again.
    a.act(Action::SelectPeer(b_id));
    // B still polls A and is rightly turned away there, so only A's own
    // words about B are counted. A compares certificates before the hello,
    // so it can't tell B set up again from another computer at B's address.
    let said = |s: &UiState| {
        s.activity
            .iter()
            .filter(|l| l.text.contains("another computer answers there"))
            .filter(|l| !l.text.starts_with("Turned away"))
            .count()
    };
    wait(&a, "A to say B is not the computer it paired with", |s| {
        said(s) == 1
    });
    // Each selection logs "Working with B." and polls again; none repeats it.
    for _ in 0..3 {
        a.act(Action::SelectPeer(b_id));
        std::thread::sleep(Duration::from_millis(300));
    }
    assert_eq!(said(&a.state()), 1, "{:#?}", a.state().activity);
    drop(b);
}
