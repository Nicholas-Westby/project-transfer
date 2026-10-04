//! Two app cores on 127.0.0.1 driven only through actions, as the UI would.

mod core_support;

use core_support::*;
use project_transfer::core::{Action, PairState, PromptState, TransferState};
use project_transfer::model::Direction;
use project_transfer::store::Store;
use project_transfer::transfer::TransferRequest;
use std::time::Duration;

#[test]
fn pair_push_and_see_the_files_on_the_other_computer() {
    let (da, db) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let (a, b) = (start(da.path()), start(db.path()));
    let b_id = b.state().me.id;

    a.act(Action::AddByAddress(format!("127.0.0.1:{}", b.port())));
    wait(&a, "B to be found", |s| {
        s.discovered.iter().any(|d| d.id == b_id)
    });
    let target = a.state().discovered[0].clone();
    a.act(Action::Pair {
        target,
        requested: ALL,
        offered: ALL,
    });
    wait(&b, "the pairing prompt", |s| s.pair_prompt.is_some());
    let shown_on_b = b.state().pair_prompt.clone().unwrap();
    assert_eq!(shown_on_b.requested, ALL);
    wait(&a, "the code on A", |s| {
        s.pairing
            .as_ref()
            .is_some_and(|p| p.state == PairState::Confirm)
    });
    let shown_on_a = a.state().pairing.clone().unwrap();
    assert_eq!(shown_on_a.code.as_deref(), Some(shown_on_b.code.as_str()));
    b.act(Action::AnswerPair(Some(ALL)));
    wait(&b, "B to wait for A", |s| {
        s.pair_prompt
            .as_ref()
            .is_some_and(|p| p.state == PromptState::Waiting)
    });
    wait(&a, "A to see that B accepted", |s| {
        s.pairing.as_ref().is_some_and(|p| p.other_accepted)
    });
    // Nothing is stored until A's user confirms too.
    assert!(b.state().peers.is_empty());
    assert!(a.state().peers.is_empty());
    a.act(Action::ConfirmPairCode(true));
    wait(&a, "pairing to finish", |s| {
        s.pairing
            .as_ref()
            .is_some_and(|p| p.state == PairState::Done)
    });
    wait(&b, "B to list A", |s| s.peers.len() == 1);
    wait(&b, "B to show it paired", |s| {
        s.pair_prompt
            .as_ref()
            .is_some_and(|p| p.state == PromptState::Done)
    });

    // The first pairing selects the new peer; selecting it again is harmless.
    a.act(Action::SelectPeer(b_id));
    wait(&a, "B to be online with its grants", |s| {
        s.peer(b_id)
            .is_some_and(|v| v.online && v.peer.granted == ALL)
    });

    let src = da.path().join("src/app");
    std::fs::create_dir_all(src.join("empty")).unwrap();
    std::fs::write(src.join("main.rs"), "fn main() {}").unwrap();
    a.act(Action::CreateProject {
        name: "Garden".into(),
        folder: src.clone(),
    });
    a.settle();
    let project = a.state().projects[0].id;

    a.act(Action::Prepare(TransferRequest {
        peer: b_id,
        project,
        direction: Direction::Push,
        send_everything: false,
    }));
    wait(&a, "the preview", |s| {
        matches!(s.transfer, TransferState::Ready(_))
    });
    let TransferState::Ready(preview) = a.state().transfer.clone() else {
        unreachable!()
    };
    assert_eq!(preview.counts().added, 2);
    a.act(Action::Execute);
    wait(&a, "the push to finish", |s| {
        matches!(s.transfer, TransferState::Finished(_))
    });
    let TransferState::Finished(summary) = a.state().transfer.clone() else {
        unreachable!()
    };
    assert_eq!(summary.files, 1);

    let dest = db.path().join("Dev/app");
    assert_eq!(
        std::fs::read_to_string(dest.join("main.rs")).unwrap(),
        "fn main() {}"
    );
    assert!(dest.join("empty").is_dir());
    let last = a.state().activity.last().unwrap().text.clone();
    assert!(last.starts_with("Pushed 1 file to "), "{last}");
    assert!(a.state().projects[0].last_transfer.is_some());
    wait(&b, "B to take in the project", |s| {
        s.projects.iter().any(|p| p.id == project)
    });
    // Each side says who started it: A pushed; B only answered.
    let on_a = a.state().projects[0].last_transfer.clone().unwrap();
    assert!(!on_a.by_peer);
    wait(&b, "B to record the push", |s| {
        s.project(project)
            .and_then(|p| p.last_transfer.as_ref())
            .is_some_and(|t| t.by_peer && t.peer == a.state().me.id)
    });
    a.act(Action::DismissTransfer);
    a.settle();
    assert_eq!(a.state().transfer, TransferState::Idle);

    // The next status poll shows the project on B.
    a.act(Action::SelectPeer(b_id));
    wait(&a, "B's projects", |s| {
        s.remote_projects.contains_key(&project)
    });

    // A second preview finds nothing to do, and cancelling it goes back to idle.
    a.act(Action::Prepare(TransferRequest {
        peer: b_id,
        project,
        direction: Direction::Push,
        send_everything: false,
    }));
    wait(
        &a,
        "the second preview",
        |s| matches!(&s.transfer, TransferState::Ready(p) if p.is_empty()),
    );
    a.act(Action::CancelTransfer);
    a.settle();
    assert_eq!(a.state().transfer, TransferState::Idle);
}

#[test]
fn stopping_before_the_other_side_accepts_stores_nothing_on_either_side() {
    let (da, db) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let (a, b) = start_pairing(da.path(), db.path());
    // A's user confirms the code, then gives up waiting for B.
    a.act(Action::ConfirmPairCode(true));
    a.act(Action::DismissPairing);
    a.settle();
    assert!(a.state().pairing.is_none());
    wait(&b, "B's prompt to expire", |s| {
        s.pair_prompt
            .as_ref()
            .is_some_and(|p| matches!(p.state, PromptState::Failed(_)))
    });
    // Accepting now is too late.
    b.act(Action::AnswerPair(Some(ALL)));
    b.settle();
    std::thread::sleep(Duration::from_millis(200));
    for c in [&a, &b] {
        assert!(c.state().peers.is_empty());
    }
    assert!(
        Store::open_at(da.path().join("home"))
            .unwrap()
            .load_peers()
            .unwrap()
            .is_empty()
    );
    assert!(
        Store::open_at(db.path().join("home"))
            .unwrap()
            .load_peers()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn cancelling_on_the_accepting_side_stores_nothing_and_tells_the_other() {
    let (da, db) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let (a, b) = start_pairing(da.path(), db.path());
    b.act(Action::AnswerPair(Some(ALL)));
    wait(&a, "A to see that B accepted", |s| {
        s.pairing.as_ref().is_some_and(|p| p.other_accepted)
    });
    b.act(Action::DismissPairPrompt);
    wait(&a, "A to hear that B cancelled", |s| {
        s.pairing.as_ref().is_some_and(
            |p| matches!(&p.state, PairState::Failed(why) if why.contains("cancelled")),
        )
    });
    for c in [&a, &b] {
        assert!(c.state().peers.is_empty());
    }
}

#[test]
fn a_project_only_on_the_other_computer_is_listed_and_pulled_here() {
    let (da, db) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let (a, b) = start_pairing(da.path(), db.path());
    b.act(Action::AnswerPair(Some(ALL)));
    a.act(Action::ConfirmPairCode(true));
    wait(&b, "B to list A", |s| s.peers.len() == 1);
    let a_id = a.state().me.id;

    let src = da.path().join("work/herbs");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(src.join("basil.md"), "# Basil").unwrap();
    a.act(Action::CreateProject {
        name: "herbs".into(),
        folder: src,
    });
    a.settle();
    let project = a.state().projects[0].id;

    // Discovery is off in tests, so B learns where A listens by address.
    b.act(Action::AddByAddress(format!("127.0.0.1:{}", a.port())));
    wait(&b, "A to be found", |s| {
        s.discovered.iter().any(|d| d.id == a_id)
    });
    // B never had the project; its status poll of A lists it.
    b.act(Action::SelectPeer(a_id));
    wait(&b, "A's project on B", |s| {
        s.remote_projects.contains_key(&project) && s.project(project).is_none()
    });
    b.act(Action::Prepare(TransferRequest {
        peer: a_id,
        project,
        direction: Direction::Pull,
        send_everything: false,
    }));
    wait(&b, "the pull preview", |s| {
        matches!(s.transfer, TransferState::Ready(_))
    });
    b.act(Action::Execute);
    wait(&b, "the pull to finish", |s| {
        matches!(s.transfer, TransferState::Finished(_))
    });
    let dest = db.path().join("Dev/herbs/basil.md");
    assert_eq!(std::fs::read_to_string(dest).unwrap(), "# Basil");
    assert!(b.state().project(project).is_some());
}
