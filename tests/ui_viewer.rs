//! Opening a file from a transfer's preview, and the viewer over it.

mod ui_support;

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use project_transfer::core::{Action, TransferState};
use project_transfer::net::FilePart;
use project_transfer::ui::App;
use project_transfer::viewer::{Copies, FileAsk, FileView, Shown};
use std::path::PathBuf;
use std::sync::Arc;
use ui_support::{FakeBackend, sample_preview, ui_harness};

fn id(n: u128) -> uuid::Uuid {
    uuid::Uuid::from_u128(n)
}

/// The sample push preview, open.
fn preview() -> (Arc<FakeBackend>, Harness<'static, App>) {
    let fake = FakeBackend::seeded();
    fake.update(|s| s.transfer = TransferState::Ready(sample_preview(s)));
    let h = ui_harness(fake.clone());
    (fake, h)
}

/// What opening `rel` in the sample preview's first folder asks for.
fn ask(rel: &str, copies: Copies) -> FileAsk {
    FileAsk {
        peer: id(2),
        project: id(10),
        folder: id(20),
        local_root: PathBuf::from("/Users/jdoe/Dev/garden-planner/app"),
        rel: rel.into(),
        copies,
    }
}

#[test]
fn a_click_on_a_changed_file_opens_both_copies() {
    let (fake, mut h) = preview();
    h.get_by_label_contains("README.md").click();
    h.run();
    let want = ask("README.md", Copies::Both);
    assert_eq!(fake.actions(), vec![Action::OpenFile(want)]);
}

#[test]
fn an_added_file_opens_the_copy_here_and_a_removed_one_the_copy_there() {
    let (fake, mut h) = preview();
    h.get_by_label("Added (4 files)").click();
    h.run();
    h.get_by_label_contains("src/new.rs").click();
    h.run();
    h.get_by_label_contains("todo.txt").click();
    h.run();
    let want = vec![
        Action::OpenFile(ask("src/new.rs", Copies::Here)),
        Action::OpenFile(ask("todo.txt", Copies::There)),
    ];
    assert_eq!(fake.actions(), want);
}

#[test]
fn a_folder_line_opens_nothing() {
    let (fake, mut h) = preview();
    h.get_by_label_contains("Remove folder old-sketches")
        .click();
    h.run();
    h.get_by_label("Added (4 files)").click();
    h.run();
    h.get_by_label_contains(".git contents").click();
    h.run();
    assert!(fake.actions().is_empty(), "{:?}", fake.actions());
}

/// How many widgets Tab visits.
fn focus_stops(h: &mut Harness<'static, App>) -> usize {
    let mut seen = std::collections::HashSet::new();
    for _ in 0..80 {
        h.key_press(egui::Key::Tab);
        h.run();
        if let Some(f) = h.ctx.memory(|m| m.focused()) {
            seen.insert(f);
        }
    }
    seen.len()
}

#[test]
fn more_lines_add_no_keyboard_stops() {
    let (_, mut a) = preview();
    let fake = FakeBackend::seeded();
    fake.update(ui_support::with_drawings);
    let mut b = ui_harness(fake);
    assert_eq!(focus_stops(&mut a), focus_stops(&mut b));
}

/// The sample preview with README.md open in the viewer.
fn viewing() -> (Arc<FakeBackend>, Harness<'static, App>) {
    let (fake, mut h) = preview();
    let fetched = |text: &str| {
        Some(Ok(FilePart {
            size: text.len() as u64,
            mtime_ms: 0,
            bytes: text.as_bytes().to_vec(),
        }))
    };
    let shown = Shown::build(fetched("# Garden\nPlans beds.\n"), fetched("# Garden\n"));
    fake.update(|s| {
        s.file_view = Some(FileView {
            ask: ask("README.md", Copies::Both),
            opened: 1,
            shown: Some(Arc::new(shown)),
        })
    });
    h.run();
    (fake, h)
}

#[test]
fn the_viewer_shows_over_the_preview_and_close_closes_it() {
    let (fake, mut h) = viewing();
    // The folders table behind the sheet heads its column the same way.
    assert_eq!(h.get_all_by_label("On Desktop Swift Heron").count(), 2);
    h.get_by_label("Replaced");
    h.get_by_label("Plans beds.");
    h.get_by_label("README.md in app");
    h.get_by_label("Close").click();
    h.run();
    assert_eq!(fake.actions(), vec![Action::CloseFile]);
}

#[test]
fn escape_closes_the_viewer_and_leaves_the_preview_open() {
    let (fake, mut h) = viewing();
    h.key_press(egui::Key::Escape);
    h.run();
    assert_eq!(fake.actions(), vec![Action::CloseFile]);
}
