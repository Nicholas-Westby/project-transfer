//! The Sync button between Pull and Push, and what a sync's preview says.

mod ui_support;

use egui_kittest::kittest::Queryable;
use project_transfer::core::{Action, TransferState};
use project_transfer::model::Direction;
use project_transfer::transfer::TransferRequest;
use project_transfer::transfer::sync_count::{SyncChoice, Tally};
use ui_support::{FakeBackend, sample_preview, ui_harness};

fn id(n: u128) -> uuid::Uuid {
    uuid::Uuid::from_u128(n)
}

#[test]
fn sync_sits_between_pull_and_push_and_asks_for_a_sync() {
    let fake = FakeBackend::seeded();
    let mut h = ui_harness(fake.clone());
    let left = |label: &str| h.get_by_label(label).rect().left();
    let (pull, sync, push) = (
        left("Pull from Desktop Swift Heron"),
        left("Sync with Desktop Swift Heron"),
        left("Push to Desktop Swift Heron"),
    );
    assert!(pull < sync && sync < push, "{pull} {sync} {push}");
    h.get_by_label("Sync with Desktop Swift Heron").click();
    h.run();
    let want = TransferRequest {
        peer: id(2),
        project: id(10),
        direction: Direction::Push,
        send_everything: false,
    };
    assert_eq!(fake.actions(), vec![Action::PrepareSync(want)]);
}

#[test]
fn sync_needs_pulls_as_well_as_pushes() {
    let fake = FakeBackend::seeded();
    fake.update(|s| s.peers[0].peer.granted.may_pull_from_me = false);
    let mut h = ui_harness(fake.clone());
    h.get_by_label(
        "Desktop Swift Heron doesn't allow pulls to this computer, which syncing needs. Allow it \
         on Desktop Swift Heron.",
    );
    h.get_by_label("Sync with Desktop Swift Heron").click();
    h.run();
    assert!(fake.actions().is_empty(), "{:?}", fake.actions());
}

#[test]
fn sync_names_both_permissions_when_both_are_missing() {
    let fake = FakeBackend::seeded();
    fake.update(|s| s.peers[0].peer.granted = Default::default());
    let h = ui_harness(fake);
    h.get_by_label(
        "Desktop Swift Heron doesn't allow pushes from or pulls to this computer, which syncing \
         needs. Allow both on Desktop Swift Heron.",
    );
}

#[test]
fn a_project_with_no_folder_here_can_still_sync() {
    let fake = FakeBackend::seeded();
    fake.update(|s| {
        for f in &mut s.projects[0].folders {
            f.local_path = None;
        }
    });
    let mut h = ui_harness(fake.clone());
    h.get_by_label("None of this project's folders are on this computer yet.");
    h.get_by_label("Sync with Desktop Swift Heron").click();
    h.run();
    assert!(matches!(&fake.actions()[..], [Action::PrepareSync(_)]));
}

#[test]
fn a_sync_preview_says_which_way_it_went_and_why() {
    let fake = FakeBackend::seeded();
    fake.update(|s| {
        let mut p = sample_preview(s);
        p.sync = Some(SyncChoice::from_tally(Tally {
            here: 7,
            there: 2,
            newest_here: Some(2),
            newest_there: Some(1),
            ..Default::default()
        }));
        s.transfer = TransferState::Ready(p);
    });
    let h = ui_harness(fake);
    h.get_by_label("Push garden-planner to Desktop Swift Heron");
    h.get_by_label(
        "Sync chose to push: 7 files have newer changes here, 2 on Desktop Swift Heron.",
    );
}

#[test]
fn a_sync_that_chose_to_pull_is_titled_as_a_pull() {
    let fake = FakeBackend::seeded();
    fake.update(|s| {
        let mut p = sample_preview(s);
        p.request.direction = Direction::Pull;
        p.sync = Some(SyncChoice::from_tally(Tally {
            there: 1,
            newest_there: Some(1),
            ..Default::default()
        }));
        s.transfer = TransferState::Ready(p);
    });
    let h = ui_harness(fake);
    h.get_by_label("Pull garden-planner from Desktop Swift Heron");
    h.get_by_label(
        "Sync chose to pull: 1 file has newer changes on Desktop Swift Heron, none here.",
    );
}
