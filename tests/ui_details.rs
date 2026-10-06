//! Sending a project's details without files, and changing where a folder
//! lives on the other computer, over a hand-built state.

mod ui_support;

use egui::accesskit::Role;
use egui_kittest::kittest::Queryable;
use project_transfer::core::{Action, TransferState};
use ui_support::{FakeBackend, ui_harness};
use uuid::Uuid;

const SYNC: &str = "Sync details with Desktop Swift Heron";

type Setup = fn(&mut project_transfer::core::UiState);

#[test]
fn sync_details_sends_the_selected_project() {
    let fake = FakeBackend::seeded();
    let mut h = ui_harness(fake.clone());
    h.get_by_label_contains("No files are copied.");
    h.get_by_label(SYNC).click();
    h.run();
    assert_eq!(
        fake.actions(),
        vec![Action::SyncDetails(Uuid::from_u128(10))]
    );
}

#[test]
fn sync_details_says_why_it_cannot_run() {
    let cases: [(Setup, &str); 3] = [
        (|s| s.peers[0].online = false, "is offline"),
        (
            |s| s.transfer = TransferState::Failed("stopped".into()),
            "Another transfer is open",
        ),
        (
            |s| s.peers[0].peer.granted.may_push_to_me = false,
            "which syncing details needs",
        ),
    ];
    for (setup, says) in cases {
        let fake = FakeBackend::seeded();
        fake.update(setup);
        let mut h = ui_harness(fake.clone());
        assert!(
            h.query_all_by_label_contains(says).next().is_some(),
            "{says}"
        );
        h.get_by_label(SYNC).click();
        h.run();
        // A click outside an open transfer's sheet may close it; that's all.
        let synced = fake
            .actions()
            .into_iter()
            .any(|a| matches!(a, Action::SyncDetails(_)));
        assert!(!synced, "{says}");
    }
}

#[test]
fn the_folder_menu_changes_where_the_peer_keeps_it() {
    let fake = FakeBackend::seeded();
    let mut h = ui_harness(fake.clone());
    h.get_by_label("More for app").click();
    h.run();
    h.get_by_label("Change folder on Desktop Swift Heron…")
        .click();
    h.run();
    h.get_by_label("Change where app lives on Desktop Swift Heron");
    let field = h.get_by_role_and_label(Role::TextInput, "Folder on Desktop Swift Heron");
    field.focus();
    field.type_text("2");
    h.run();
    h.get_by_label("Save on Desktop Swift Heron").click();
    h.run();
    let want = Action::SetPeerFolderPath(
        Uuid::from_u128(10),
        Uuid::from_u128(20),
        "D:\\dev\\garden-planner\\app2".into(),
    );
    assert_eq!(fake.actions(), vec![want]);
    assert!(
        h.query_by_label("Change where app lives on Desktop Swift Heron")
            .is_none()
    );
}

#[test]
fn cancel_leaves_the_peer_folder_alone() {
    let fake = FakeBackend::seeded();
    let mut h = ui_harness(fake.clone());
    h.get_by_label("More for app").click();
    h.run();
    h.get_by_label("Change folder on Desktop Swift Heron…")
        .click();
    h.run();
    h.get_by_label("Cancel").click();
    h.run();
    assert!(fake.actions().is_empty());
    assert!(
        h.query_by_label("Change where app lives on Desktop Swift Heron")
            .is_none()
    );
}

#[test]
fn the_peer_shows_its_folders_under_home_with_a_tilde() {
    let fake = FakeBackend::seeded();
    fake.update(|s| {
        let r = s.remote_projects.get_mut(&Uuid::from_u128(10)).unwrap();
        r.folders[0].shown = Some("~/Dev/garden-planner/app".into());
    });
    let h = ui_harness(fake);
    h.get_by_label("~/Dev/garden-planner/app");
}
