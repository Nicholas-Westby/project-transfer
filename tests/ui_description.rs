//! The project's description box: typing, saving on leaving it, and Esc.

mod ui_support;

use egui_kittest::kittest::Queryable;
use project_transfer::core::Action;
use project_transfer::ui::Backend;
use ui_support::{FakeBackend, ui_harness};

fn blank() -> std::sync::Arc<FakeBackend> {
    let fake = FakeBackend::seeded();
    fake.update(|s| s.projects[0].description = Default::default());
    fake
}

#[test]
fn a_description_is_saved_when_its_box_loses_focus() {
    let fake = blank();
    let mut h = ui_harness(fake.clone());
    let id = fake.snapshot().projects[0].id;
    let field = h.get_by_label("Description");
    field.focus();
    field.type_text("Plans the beds.\n");
    h.run();
    h.get_by_label("Saved when you click away. Esc undoes your changes.");
    assert!(fake.actions().is_empty(), "nothing saved while typing");
    h.get_by_label("Add folder").click();
    h.run();
    assert!(fake.actions().iter().any(
        |a| matches!(a, Action::SetDescription(p, t) if *p == id && t == "Plans the beds.\n")
    ));
}

#[test]
fn esc_drops_the_edit() {
    let fake = blank();
    let mut h = ui_harness(fake.clone());
    let field = h.get_by_label("Description");
    field.focus();
    field.type_text("Never mind");
    h.run();
    h.key_press(egui::Key::Escape);
    h.run();
    h.get_by_label("Add folder").click();
    h.run();
    assert!(
        !fake
            .actions()
            .iter()
            .any(|a| matches!(a, Action::SetDescription(..)))
    );
}

#[test]
fn moving_to_another_project_saves_the_draft() {
    let fake = blank();
    let mut h = ui_harness(fake.clone());
    let id = fake.snapshot().projects[0].id;
    let field = h.get_by_label("Description");
    field.focus();
    field.type_text("Half done");
    h.run();
    h.get_by_label("tide-tables").click();
    h.run();
    assert!(
        fake.actions()
            .iter()
            .any(|a| matches!(a, Action::SetDescription(p, t) if *p == id && t == "Half done"))
    );
}

/// A newer description arriving while the box is focused is kept when
/// the box is left without typing.
#[test]
fn leaving_an_untouched_box_never_restores_the_old_text() {
    let fake = blank();
    let mut h = ui_harness(fake.clone());
    h.get_by_label("Description").focus();
    h.run();
    fake.update(|s| s.projects[0].description.text = "Arrived from the other computer".into());
    h.run();
    h.get_by_label("Add folder").click();
    h.run();
    assert!(
        !fake
            .actions()
            .iter()
            .any(|a| matches!(a, Action::SetDescription(..)))
    );
}
