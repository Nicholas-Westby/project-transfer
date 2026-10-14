//! The Dock menu's items, and what picking one does in the window.

mod ui_support;

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use project_transfer::core::{Action, TransferState, UiState};
use project_transfer::model::{Direction, LastStarted, TransferKind};
use project_transfer::transfer::TransferRequest;
use project_transfer::ui::{App, Sheet};
use std::sync::Arc;
use ui_support::{FakeBackend, nothing_to_do, sample_preview, ui_harness};

fn id(n: u128) -> uuid::Uuid {
    uuid::Uuid::from_u128(n)
}

/// The seeded window with `kind` of garden-planner with computer `peer`
/// remembered as the last transfer started here.
fn after(kind: TransferKind, peer: u128) -> (Arc<FakeBackend>, Harness<'static, App>) {
    let fake = FakeBackend::seeded();
    fake.update(|s| {
        s.me.last_started = Some(LastStarted {
            kind,
            project: id(10),
            peer: id(peer),
        })
    });
    let h = ui_harness(fake.clone());
    (fake, h)
}

fn request(peer: u128, direction: Direction) -> TransferRequest {
    TransferRequest {
        peer: id(peer),
        project: id(10),
        direction,
        send_everything: false,
    }
}

#[test]
fn the_last_transfer_comes_first_and_then_its_counterparts() {
    let (_, h) = after(TransferKind::Push, 2);
    assert_eq!(
        h.state().dock.items(),
        [
            "Push garden-planner to Desktop Swift Heron",
            "Pull garden-planner from Desktop Swift Heron",
        ]
    );
    let (_, h) = after(TransferKind::Sync, 2);
    assert_eq!(
        h.state().dock.items(),
        [
            "Sync garden-planner with Desktop Swift Heron",
            "Pull garden-planner from Desktop Swift Heron",
            "Push garden-planner to Desktop Swift Heron",
        ]
    );
}

#[test]
fn after_a_pull_the_pull_comes_first() {
    let (_, h) = after(TransferKind::Pull, 2);
    assert_eq!(
        h.state().dock.items(),
        [
            "Pull garden-planner from Desktop Swift Heron",
            "Push garden-planner to Desktop Swift Heron",
        ]
    );
}

#[test]
fn the_items_use_the_current_names() {
    let (fake, mut h) = after(TransferKind::Push, 2);
    fake.update(|s| {
        s.projects[0].name = "seed-bank".into();
        s.peers[0].peer.name = "Desk Calm Robin".into();
    });
    h.run();
    assert_eq!(
        h.state().dock.items(),
        [
            "Push seed-bank to Desk Calm Robin",
            "Pull seed-bank from Desk Calm Robin",
        ]
    );
}

#[test]
fn nothing_remembered_or_a_project_or_computer_gone_offers_nothing() {
    let h = ui_harness(FakeBackend::seeded());
    assert!(h.state().dock.items().is_empty());
    let (fake, mut h) = after(TransferKind::Pull, 2);
    fake.update(|s| s.projects.retain(|p| p.id != id(10)));
    h.run();
    assert!(h.state().dock.items().is_empty());
    let (fake, mut h) = after(TransferKind::Pull, 2);
    fake.update(|s| s.peers.retain(|p| p.peer.id != id(2)));
    h.run();
    assert!(h.state().dock.items().is_empty());
}

#[test]
fn picking_an_item_selects_its_project_and_starts_what_its_button_does() {
    let (fake, mut h) = after(TransferKind::Push, 2);
    h.state_mut().view.selected = Some(id(11));
    h.state()
        .dock
        .click("Pull garden-planner from Desktop Swift Heron");
    h.run();
    assert_eq!(h.state().view.selected, Some(id(10)));
    let want = Action::Prepare(request(2, Direction::Pull));
    assert_eq!(fake.actions(), vec![want]);
}

#[test]
fn a_pick_still_means_the_item_on_screen_after_a_rename() {
    let (fake, mut h) = after(TransferKind::Push, 2);
    h.state()
        .dock
        .click("Pull garden-planner from Desktop Swift Heron");
    fake.update(|s| s.peers[0].peer.name = "Desk Calm Robin".into());
    h.run();
    let want = Action::Prepare(request(2, Direction::Pull));
    assert_eq!(fake.actions(), vec![want]);
}

#[test]
fn picking_sync_asks_for_a_sync() {
    let (fake, mut h) = after(TransferKind::Sync, 2);
    h.state()
        .dock
        .click("Sync garden-planner with Desktop Swift Heron");
    h.run();
    let want = Action::PrepareSync(request(2, Direction::Push));
    assert_eq!(fake.actions(), vec![want]);
}

#[test]
fn a_pick_for_another_computer_selects_it_and_starts_once_it_answers() {
    // Laptop Quiet Finch is offline, and lets this computer pull only.
    let (fake, mut h) = after(TransferKind::Pull, 3);
    h.state()
        .dock
        .click("Pull garden-planner from Laptop Quiet Finch");
    // While it waits it looks again every so often, so the window never
    // settles; a few frames are enough.
    h.run_steps(3);
    h.get_by_label("Starting the pull once Laptop Quiet Finch answers…");
    assert_eq!(fake.actions(), vec![Action::SelectPeer(id(3))]);
    fake.update(|s| {
        s.selected_peer = Some(id(3));
        s.peers[1].online = true;
    });
    h.run();
    let want = Action::Prepare(request(3, Direction::Pull));
    assert_eq!(fake.actions(), vec![Action::SelectPeer(id(3)), want]);
}

/// Picks the Laptop item with project 11 selected; nothing may change. A
/// few frames each time: a running transfer never stops repainting.
fn pick_changes_nothing(fake: &FakeBackend, h: &mut Harness<'static, App>) {
    h.state_mut().view.selected = Some(id(11));
    h.run_steps(3);
    h.state()
        .dock
        .click("Pull garden-planner from Laptop Quiet Finch");
    h.run_steps(3);
    assert!(fake.actions().is_empty(), "{:?}", fake.actions());
    assert_eq!(h.state().view.selected, Some(id(11)));
}

#[test]
fn a_pick_while_a_preview_is_open_changes_nothing() {
    let (fake, mut h) = after(TransferKind::Pull, 3);
    fake.update(|s| s.transfer = TransferState::Ready(sample_preview(s)));
    pick_changes_nothing(&fake, &mut h);
}

#[test]
fn a_pick_over_an_open_sheet_changes_nothing() {
    let (fake, mut h) = after(TransferKind::Pull, 3);
    h.state_mut().view.sheet = Sheet::Computers;
    pick_changes_nothing(&fake, &mut h);
}

#[test]
fn a_pick_while_a_transfer_runs_changes_nothing() {
    let (fake, mut h) = after(TransferKind::Pull, 3);
    fake.update(|s| {
        s.transfer = TransferState::Running {
            done: 0,
            total: 10,
            files: 1,
            current: "notes.txt".into(),
            started: std::time::Instant::now(),
            left: None,
        }
    });
    pick_changes_nothing(&fake, &mut h);
}

#[test]
fn a_pick_is_forgotten_once_another_project_is_selected() {
    let (fake, mut h) = after(TransferKind::Pull, 3);
    h.state()
        .dock
        .click("Pull garden-planner from Laptop Quiet Finch");
    h.run_steps(3);
    h.state_mut().view.selected = Some(id(11));
    fake.update(|s| {
        s.selected_peer = Some(id(3));
        s.peers[1].online = true;
    });
    h.run_steps(3);
    assert_eq!(fake.actions(), vec![Action::SelectPeer(id(3))]);
}

#[test]
fn a_pick_while_the_last_result_is_open_closes_it_and_starts() {
    let results: [fn(&mut UiState); 3] = [
        |s| s.transfer = TransferState::Finished(Default::default()),
        |s| s.transfer = TransferState::Failed("Desktop Swift Heron went away.".into()),
        nothing_to_do,
    ];
    for result in results {
        let (fake, mut h) = after(TransferKind::Pull, 2);
        fake.update(result);
        h.run_steps(2);
        h.state()
            .dock
            .click("Pull garden-planner from Desktop Swift Heron");
        h.run_steps(3);
        assert_eq!(fake.actions(), vec![Action::DismissTransfer]);
        fake.update(|s| s.transfer = TransferState::Idle);
        h.run_steps(3);
        let started = Action::Prepare(request(2, Direction::Pull));
        assert_eq!(fake.actions(), vec![Action::DismissTransfer, started]);
    }
}

#[test]
fn a_pick_that_cannot_start_leaves_the_reason_under_the_buttons() {
    let (fake, mut h) = after(TransferKind::Push, 3);
    fake.update(|s| {
        s.selected_peer = Some(id(3));
        s.peers[1].online = true;
    });
    h.run();
    h.state()
        .dock
        .click("Push garden-planner to Laptop Quiet Finch");
    h.run();
    assert!(fake.actions().is_empty(), "{:?}", fake.actions());
    h.get_by_label(
        "Laptop Quiet Finch doesn't allow pushes from this computer. Allow it on Laptop Quiet \
         Finch.",
    );
}

#[test]
fn a_waiting_pick_leaves_a_result_it_did_not_start_alone() {
    let (fake, mut h) = after(TransferKind::Pull, 3);
    h.state()
        .dock
        .click("Pull garden-planner from Laptop Quiet Finch");
    h.run_steps(3);
    fake.update(|s| s.selected_peer = Some(id(3)));
    h.run_steps(3);
    // Meanwhile the user goes back to Desktop Swift Heron and compares.
    fake.update(|s| {
        s.selected_peer = Some(id(2));
        nothing_to_do(s);
    });
    h.run_steps(3);
    assert_eq!(fake.actions(), vec![Action::SelectPeer(id(3))]);
}

#[test]
fn choosing_another_computer_forgets_a_waiting_pick() {
    let (fake, mut h) = after(TransferKind::Pull, 3);
    h.state()
        .dock
        .click("Pull garden-planner from Laptop Quiet Finch");
    h.run_steps(3);
    fake.update(|s| s.selected_peer = Some(id(3)));
    h.run_steps(3);
    h.get_by_label("Choose a computer").click();
    h.run_steps(3);
    h.get_by_label("Desktop Swift Heron").click();
    h.run_steps(3);
    let waiting = "Starting the pull once Laptop Quiet Finch answers…";
    assert!(h.query_by_label(waiting).is_none());
    let want = vec![Action::SelectPeer(id(3)), Action::SelectPeer(id(2))];
    assert_eq!(fake.actions(), want);
}
