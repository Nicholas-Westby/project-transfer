//! What Settings says about ignore patterns, and how it checks them.

mod ui_support;

use egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use project_transfer::core::Action;
use project_transfer::ui::App;
use std::sync::Arc;
use ui_support::{FakeBackend, ui_harness};

fn open_settings(fake: Arc<FakeBackend>) -> Harness<'static, App> {
    let mut h = ui_harness(fake);
    h.get_by_label("Settings").click();
    h.run();
    h
}

#[test]
fn settings_explain_where_paths_start() {
    let h = open_settings(FakeBackend::seeded());
    h.get_by_label_contains("Paths start inside each project folder");
    h.get_by_label("/src/tmp/");
    h.get_by_label_contains("ignore files straight from a transfer's preview");
    h.get_by_label_contains("even on Windows");
    h.get_by_label_contains("except inside a folder that is left out");
}

#[test]
fn a_bad_pattern_is_explained_before_it_is_added() {
    let fake = FakeBackend::seeded();
    let mut h = open_settings(fake.clone());
    let field = h.get_by_role_and_label(Role::TextInput, "New pattern");
    field.focus();
    field.type_text("{a,b");
    h.run();
    h.get_by_label_contains("This pattern can't be used: unclosed");
    h.get_by_label("Add pattern").click();
    h.run();
    h.get_by_label("Save settings").click();
    h.run();
    assert!(fake.actions().is_empty(), "{:?}", fake.actions());
}

#[test]
fn a_pattern_typed_and_entered_at_once_is_added() {
    let fake = FakeBackend::seeded();
    let mut h = open_settings(fake.clone());
    h.get_by_role_and_label(Role::TextInput, "New pattern")
        .focus();
    h.run();
    // Typing and Enter in one frame, as when pasting and pressing Enter at
    // once; the harness's own helpers give each event a frame of its own.
    let enter = |pressed| egui::Event::Key {
        key: egui::Key::Enter,
        physical_key: None,
        pressed,
        repeat: false,
        modifiers: Default::default(),
    };
    let typed = egui::Event::Text("*.tmp".into());
    h.input_mut()
        .events
        .extend([typed, enter(true), enter(false)]);
    h.step();
    h.run();
    h.get_by_label("Save settings").click();
    h.run();
    let want = vec!["*.log".to_string(), "*.tmp".to_string()];
    assert!(
        fake.actions()
            .iter()
            .any(|a| matches!(a, Action::SetIgnores { extra, .. } if *extra == want)),
        "{:?}",
        fake.actions()
    );
}

#[test]
fn a_pattern_starting_with_a_folder_name_can_be_fixed() {
    let fake = FakeBackend::seeded();
    fake.update(|s| s.me.extra_ignores = vec!["app/src/tmp/".into()]);
    let mut h = open_settings(fake.clone());
    h.get_by_label_contains("Starts with the folder name app");
    h.get_by_label("Change to /src/tmp/").click();
    h.run();
    assert!(
        h.query_by_label_contains("Starts with the folder name")
            .is_none()
    );
    h.get_by_label("Save settings").click();
    h.run();
    let want = vec!["/src/tmp/".to_string()];
    assert!(
        fake.actions()
            .iter()
            .any(|a| matches!(a, Action::SetIgnores { extra, .. } if *extra == want)),
        "{:?}",
        fake.actions()
    );
}
