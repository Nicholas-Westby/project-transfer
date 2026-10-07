//! The window driven through egui_kittest, over a real core with a temp
//! store and over a hand-built state where the core cannot reach (a preview).

mod ui_support;

use egui_kittest::kittest::Queryable;
use project_transfer::core::{Action, TransferState};
use project_transfer::model::Direction;
use project_transfer::ui::Confirm;
use ui_support::{FakeBackend, live, sample_preview, ui_harness};

#[test]
fn an_empty_app_invites_adding_a_project() {
    let dir = tempfile::tempdir().unwrap();
    let (_core, h) = live(dir.path());
    h.get_by_label("Add a project to start. Pick its folder on this computer first.");
    h.get_by_label("Not paired yet");
    h.get_by_label("New project");
}

#[test]
fn a_created_project_shows_its_folders() {
    let dir = tempfile::tempdir().unwrap();
    let garden = dir.path().join("garden");
    std::fs::create_dir_all(&garden).unwrap();
    let (core, mut h) = live(dir.path());
    core.act(Action::CreateProject {
        name: "garden-planner".into(),
        folder: garden.clone(),
    });
    core.settle();
    h.run();
    // The first project opens by itself.
    h.get_by_label(&garden.display().to_string());
    h.get_by_label("Primary");
}

#[test]
fn deleting_a_project_asks_first_and_cancel_keeps_it() {
    let dir = tempfile::tempdir().unwrap();
    let garden = dir.path().join("garden");
    std::fs::create_dir_all(&garden).unwrap();
    let (core, mut h) = live(dir.path());
    core.act(Action::CreateProject {
        name: "garden-planner".into(),
        folder: garden,
    });
    core.settle();
    h.run();

    h.get_by_label("Delete project").click();
    h.run();
    h.get_by_label("Delete garden-planner?");
    h.get_by_label("Cancel").click();
    h.run();
    core.settle();
    assert_eq!(core.state().projects.len(), 1);
    assert!(h.query_by_label("Delete garden-planner?").is_none());

    h.get_by_label("Delete project").click();
    h.run();
    // The view's button and the dialog's confirm share a label; the dialog's is last.
    h.get_all_by_label("Delete project").last().unwrap().click();
    h.run();
    core.settle();
    assert!(core.state().projects.is_empty());
}

#[test]
fn escape_closes_a_confirmation_without_acting() {
    let dir = tempfile::tempdir().unwrap();
    let garden = dir.path().join("garden");
    std::fs::create_dir_all(&garden).unwrap();
    let (core, mut h) = live(dir.path());
    core.act(Action::CreateProject {
        name: "garden-planner".into(),
        folder: garden,
    });
    core.settle();
    h.run();
    h.get_by_label("Delete project").click();
    h.run();
    h.key_press(egui::Key::Escape);
    h.run();
    core.settle();
    assert!(h.query_by_label("Delete garden-planner?").is_none());
    assert_eq!(core.state().projects.len(), 1);
}

#[test]
fn the_preview_shows_counts_and_names_the_push() {
    let fake = FakeBackend::seeded();
    fake.update(|s| s.transfer = TransferState::Ready(sample_preview(s)));
    let mut h = ui_harness(fake.clone());
    h.get_by_label("Push 320 file changes to Desktop Swift Heron");
    // One unit everywhere: the counts row, the list headers and the button count files.
    h.get_by_label("4 added");
    h.get_by_label("Removed (313 files)");
    h.get_by_label("Changed (2 files)");
    h.get_by_label("Skipped (1 file)");
    h.get_by_label_contains("1 file can't be copied to Desktop Swift Heron");
    assert!(
        h.query_by_label_contains("reserved name on Windows, so")
            .is_none()
    );
    h.get_by_label("2 changed");
    h.get_by_label("1 timestamp only");
    h.get_by_label("313 removed");
    h.get_by_label_contains("Remove folder old-sketches (312 files, 280 of them ignored)");
    h.get_by_label("Push 320 file changes to Desktop Swift Heron")
        .click();
    h.run();
    assert_eq!(fake.actions(), vec![Action::Execute]);
}

#[test]
fn push_is_disabled_with_a_reason_when_the_peer_is_offline() {
    let fake = FakeBackend::seeded();
    fake.update(|s| s.peers[0].online = false);
    let mut h = ui_harness(fake.clone());
    h.run();
    // The same reason for both buttons reads once.
    h.get_by_label_contains("Desktop Swift Heron is offline");
    h.get_by_label("Push to Desktop Swift Heron").click();
    h.run();
    assert!(fake.actions().is_empty());
}

#[test]
fn a_received_command_shows_its_text_before_it_runs() {
    let fake = FakeBackend::seeded();
    let mut h = ui_harness(fake.clone());
    h.get_by_label("▶ Fetch dependencies").click();
    h.run();
    h.get_by_label("Run Fetch dependencies?");
    // Once in the list, once in full in the dialog.
    assert_eq!(h.get_all_by_label("make deps").count(), 2);
    h.get_by_label_contains("created on Windows");
    assert!(fake.actions().is_empty());
    h.get_by_label("Run Fetch dependencies").click();
    h.run();
    // The action carries the hash of the text the dialog showed.
    let mut shown = None;
    fake.update(|s| shown = Some(s.projects[0].commands[1].clone()));
    let shown = shown.unwrap();
    let want = project_transfer::commands::command_hash(&shown);
    assert!(matches!(&fake.actions()[..], [Action::RunCommand(_, _, Some(h))] if *h == want));
}

/// Writes PNGs of each screen for review when `UI_SHOTS_DIR` is set.
#[test]
fn screenshots() {
    let Ok(dir) = std::env::var("UI_SHOTS_DIR") else {
        return;
    };
    ui_support::shots::all(std::path::Path::new(&dir));
}

#[test]
fn a_confirmation_whose_target_is_gone_closes() {
    let fake = FakeBackend::seeded();
    let mut h = ui_harness(fake.clone());
    let gone = uuid::Uuid::from_u128(999);
    for c in [
        Confirm::Unpair(gone),
        Confirm::DeleteCommand(gone, gone),
        Confirm::DeleteProject(gone),
    ] {
        h.state_mut().view.confirm = Some(c);
        h.run();
        assert!(h.state().view.confirm.is_none());
    }
    assert!(fake.actions().is_empty());
}

#[test]
fn a_project_only_on_the_peer_is_listed_and_pulls() {
    let fake = FakeBackend::seeded();
    let mut h = ui_harness(fake.clone());
    h.get_by_label("Only on Desktop Swift Heron");
    h.get_by_label("seed-catalog");
    h.get_by_label("Pull").click();
    h.run();
    match &fake.actions()[..] {
        [Action::Prepare(req)] => {
            assert_eq!(req.direction, Direction::Pull);
            assert_eq!(req.project, uuid::Uuid::from_u128(12));
        }
        other => panic!("expected one Prepare, got {other:?}"),
    }
}

#[test]
fn pulling_a_remote_only_project_says_why_it_cannot() {
    let fake = FakeBackend::seeded();
    fake.update(|s| s.peers[0].peer.granted.may_pull_from_me = false);
    let mut h = ui_harness(fake.clone());
    // Once under the project's own Pull button and once in the list.
    let reasons = h.get_all_by_label_contains("doesn't allow pulls to this computer");
    assert_eq!(reasons.count(), 2);
    h.get_by_label("Pull").click();
    h.run();
    assert!(fake.actions().is_empty());
}

#[test]
fn any_project_name_can_be_used() {
    let fake = FakeBackend::seeded();
    let mut h = ui_harness(fake.clone());
    let folder = std::path::PathBuf::from("/Users/jdoe/Dev/greenhouse");
    h.state_mut().view.sheet = project_transfer::ui::Sheet::NewProject {
        name: "What Next?".into(),
        folder: folder.clone(),
    };
    h.run();
    assert!(h.query_by_label_contains("can't be used").is_none());
    h.get_by_label("Create project").click();
    h.run();
    assert_eq!(
        fake.actions(),
        vec![Action::CreateProject {
            name: "What Next?".into(),
            folder,
        }]
    );
}

#[test]
fn folders_a_peer_does_not_share_are_not_called_missing() {
    let fake = FakeBackend::seeded();
    // Desktop lets this computer neither push nor pull, so the status poll
    // learns nothing about its folders.
    fake.update(|s| {
        s.peers[0].peer.granted = Default::default();
        s.remote_projects.clear();
    });
    let h = ui_harness(fake);
    // The open project has three folders.
    assert_eq!(
        h.get_all_by_label("Not shared with this computer").count(),
        3
    );
    assert!(h.query_by_label("Not set up").is_none());
}

#[test]
fn a_transfer_the_peer_started_is_described_from_this_side() {
    let fake = FakeBackend::seeded();
    let h = ui_harness(fake.clone());
    h.get_by_label_contains("Last pushed to Desktop Swift Heron");
    fake.update(|s| {
        let t = s.projects[0].last_transfer.as_mut().unwrap();
        t.by_peer = true;
    });
    let mut h = ui_harness(fake);
    h.run();
    h.get_by_label_contains("Desktop Swift Heron last pushed here");
}

#[test]
fn nothing_to_push_names_the_folders_it_compared() {
    let fake = FakeBackend::seeded();
    fake.update(|s| {
        let mut p = sample_preview(s);
        for f in &mut p.folders {
            f.plan.changes.clear();
            f.skipped.clear();
            f.replaced.clear();
        }
        p.warnings.clear();
        p.left_out.clear();
        s.transfer = TransferState::Ready(p);
    });
    let h = ui_harness(fake);
    h.get_by_label_contains("Nothing to push");
    // Once in the folders table behind the sheet, once in the sheet.
    let here = h.get_all_by_label("/Users/jdoe/Dev/garden-planner/app");
    assert_eq!(here.count(), 2);
    let there = h.get_all_by_label("D:\\dev\\garden-planner\\app");
    assert_eq!(there.count(), 2);
}

#[test]
fn the_version_shows_next_to_settings() {
    let h = ui_harness(FakeBackend::seeded());
    h.get_by_label(concat!("v", env!("CARGO_PKG_VERSION")));
}
