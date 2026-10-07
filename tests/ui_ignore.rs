//! The ignore dialog over a transfer's preview, and the buttons that open it.

mod ui_support;

use egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use project_transfer::core::{Action, TransferState};
use project_transfer::manifest::Change;
use project_transfer::transfer::Replaced;
use project_transfer::ui::App;
use std::sync::Arc;
use ui_support::{
    FakeBackend, NoPicker, build, changed, sample_preview, ui_harness, with_drawings,
};

fn preview_harness() -> (Arc<FakeBackend>, Harness<'static, App>) {
    let fake = FakeBackend::seeded();
    fake.update(|s| s.transfer = TransferState::Ready(sample_preview(s)));
    let h = ui_harness(fake.clone());
    (fake, h)
}

#[test]
fn folders_head_their_lines_instead_of_prefixing_them() {
    let (_, mut h) = preview_harness();
    h.get_by_label_contains("Remove folder old-sketches (");
    assert!(h.query_by_label_contains("app/old-sketches").is_none());
    // The second folder's only change is under Added, which starts closed.
    let before = h.get_all_by_label("plant-data").count();
    h.get_by_label("Added (4 files)").click();
    h.run();
    assert_eq!(h.get_all_by_label("plant-data").count(), before + 1);
    h.get_by_label_contains("tomato.csv");
    assert!(h.query_by_label_contains("plant-data/tomato.csv").is_none());
}

#[test]
fn pointing_at_a_line_offers_to_ignore_it() {
    let (_, mut h) = preview_harness();
    assert!(h.query_by_label("Ignore…").is_none());
    h.get_by_label_contains("README.md").hover();
    h.run();
    h.get_by_label("Ignore…").click();
    h.run();
    let d = h.state().view.ignore.clone().expect("the dialog opens");
    assert_eq!(d.pattern, "/README.md");
    assert_eq!(d.options[0].label, "This file only");
}

#[test]
fn ignore_files_opens_the_dialog_with_nothing_chosen() {
    let (_, mut h) = preview_harness();
    h.get_by_label("Ignore files…").click();
    h.run();
    let d = h.state().view.ignore.clone().expect("the dialog opens");
    assert!(d.options.is_empty() && d.pattern.is_empty());
}

#[test]
fn sending_everything_offers_no_ignore() {
    let fake = FakeBackend::seeded();
    fake.update(|s| {
        let mut p = sample_preview(s);
        p.request.send_everything = true;
        s.transfer = TransferState::Ready(p);
    });
    let mut h = ui_harness(fake);
    assert!(h.query_by_label("Ignore files…").is_none());
    h.get_by_label_contains("README.md").hover();
    h.run();
    assert!(h.query_by_label("Ignore…").is_none());
}

/// The dialog opened from the first of the sample drawings.
fn dialog_for_a_drawing() -> (Arc<FakeBackend>, Harness<'static, App>) {
    let fake = FakeBackend::seeded();
    fake.update(with_drawings);
    let mut h = ui_harness(fake.clone());
    h.get_by_label_contains("exports/2026/plot-01.svg").hover();
    h.run();
    h.get_by_label("Ignore…").click();
    h.run();
    (fake, h)
}

#[test]
fn each_choice_says_what_it_leaves_out_before_it_is_added() {
    let (fake, mut h) = dialog_for_a_drawing();
    h.get_by_label("Ignore files");
    // The path the choices are about, as patterns see it.
    h.get_by_label("exports/2026/plot-01.svg");
    h.get_by_label("Leaves out 1 file of this transfer:");
    h.get_by_label("Everything in exports").click();
    h.run();
    h.get_by_label("Leaves out 2 files of this transfer:");
    h.get_by_label("Every .svg file").click();
    h.run();
    h.get_by_label("Leaves out 3 files of this transfer:");
    assert!(fake.actions().is_empty());
    h.get_by_label("Add to ignore list").click();
    h.run();
    assert_eq!(fake.actions(), vec![Action::AddIgnore("*.svg".into())]);
    assert!(h.state().view.ignore.is_none());
}

#[test]
fn a_typed_pattern_shows_what_it_leaves_out_or_that_nothing_matches() {
    let (fake, mut h) = preview_harness();
    h.get_by_label("Ignore files…").click();
    h.run();
    h.get_by_label("/src/tmp/");
    let field = h.get_by_role_and_label(Role::TextInput, "Pattern");
    field.focus();
    field.type_text("*.json");
    h.run();
    h.get_by_label("Leaves out 1 file of this transfer:");
    h.get_by_role_and_label(Role::TextInput, "Pattern")
        .type_text("x");
    h.run();
    h.get_by_label_contains("Nothing in this transfer matches.");
    h.get_by_role_and_label(Role::TextInput, "Pattern")
        .type_text("{");
    h.run();
    h.get_by_label_contains("This pattern can't be used: unclosed");
    // Escape closes only the dialog, even from inside the field.
    h.key_press(egui::Key::Escape);
    h.run();
    assert!(h.state().view.ignore.is_none());
    h.get_by_label("Push 320 file changes to Desktop Swift Heron");
    assert!(fake.actions().is_empty(), "{:?}", fake.actions());
}

#[test]
fn the_dialog_keeps_its_buttons_in_the_smallest_window() {
    let fake = FakeBackend::seeded();
    fake.update(|s| {
        let mut p = sample_preview(s);
        p.folders.truncate(1);
        p.folders[0].skipped.clear();
        p.left_out.clear();
        let deep = (0..40).map(|i| changed(&format!("a/b/c/d/e/plot-{i:02}.svg")));
        p.folders[0].plan.changes = std::iter::once(changed("a/b/c/d/e/f.svg"))
            .chain(deep)
            .collect();
        s.transfer = TransferState::Ready(p);
    });
    let mut h = build(fake, Box::new(NoPicker), [900.0, 600.0], 1.0, false);
    h.get_by_label_contains("a/b/c/d/e/f.svg").hover();
    h.run();
    h.get_by_label("Ignore…").click();
    h.run();
    h.get_by_label("Every .svg file").click();
    h.run_steps(6);
    h.get_by_label("Leaves out 41 files of this transfer:");
    let add = h.get_by_label("Add to ignore list").rect();
    assert!(add.bottom() <= 600.0, "{add:?}");
}

/// The sample preview where a file here takes the place of a folder of
/// three files on the other computer.
fn file_over_folder() -> Arc<FakeBackend> {
    let fake = FakeBackend::seeded();
    fake.update(|s| {
        let mut p = sample_preview(s);
        let f = &mut p.folders[0];
        let Change::Update { entry, .. } = changed("build") else {
            unreachable!()
        };
        f.plan.changes.push(Change::Replace { entry });
        f.replaced.push(Replaced {
            rel: "build".into(),
            files: 3,
            ignored: 0,
            by: "a file".into(),
            was_folder: true,
        });
        s.transfer = TransferState::Ready(p);
    });
    fake
}

#[test]
fn a_folder_a_file_replaces_is_ignored_through_the_file() {
    let mut h = ui_harness(file_over_folder());
    // Leaving out only the folder would let the file replace it unseen.
    h.get_by_label_contains("Replace folder build with a file")
        .hover();
    h.run();
    assert!(h.query_by_label("Ignore…").is_none());
    h.get_by_label_contains("build (replaces a different kind of entry)")
        .hover();
    h.run();
    h.get_by_label("Ignore…").click();
    h.run();
    // The file and the folder of three files are both left alone.
    h.get_by_label("Leaves out 4 files of this transfer:");
    let field = h.get_by_role_and_label(Role::TextInput, "Pattern");
    field.focus();
    field.type_text("/");
    h.run();
    h.get_by_label_contains("build would still be replaced");
    // The warning says why; the hint about paths and capitals would mislead.
    assert!(
        h.query_by_label_contains("Nothing in this transfer matches")
            .is_none()
    );
}

#[test]
fn a_long_match_list_shows_its_first_lines_and_counts_the_rest() {
    let fake = FakeBackend::seeded();
    fake.update(|s| {
        let mut p = sample_preview(s);
        p.folders.truncate(1);
        p.folders[0].plan.changes = (0..250)
            .map(|i| changed(&format!("renders/frame-{i:03}.png")))
            .collect();
        s.transfer = TransferState::Ready(p);
    });
    let mut h = ui_harness(fake);
    h.get_by_label_contains("renders/frame-000.png").hover();
    h.run();
    h.get_by_label("Ignore…").click();
    h.run();
    h.get_by_label("Everything in renders").click();
    h.run();
    h.get_by_label("Leaves out 250 files of this transfer:");
    h.get_by_label("And 50 more");
}
