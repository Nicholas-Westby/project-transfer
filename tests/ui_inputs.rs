//! Inputs through the window: the editor lists what a command will ask
//! for, and running it asks for each value first.

mod ui_support;

use egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use project_transfer::commands::command_hash;
use project_transfer::core::Action;
use project_transfer::model::{Command, Os};
use project_transfer::ui::{App, Backend, Sheet};
use std::sync::Arc;
use ui_support::{FakeBackend, ui_harness};
use uuid::Uuid;

const RELEASE: &str = "./release.sh --notes \"$INPUT_RELEASE_NOTES\"";

/// The seed with a Release command running `line`; a trusted one ran here
/// before in this text, so it needs no check.
fn with_release(line: &str, trusted: bool) -> (Arc<FakeBackend>, Command) {
    let mut c = Command {
        id: Uuid::from_u128(50),
        label: "Release".into(),
        line: line.into(),
        created_on: Os::current(),
        updated_at_ms: 1,
        deleted: false,
        last_run_hash: None,
        linked_to: None,
    };
    if trusted {
        c.last_run_hash = Some(command_hash(&c));
    }
    let fake = FakeBackend::seeded();
    fake.update(|s| s.projects[0].commands.push(c.clone()));
    (fake, c)
}

fn type_into(h: &mut Harness<'static, App>, field: &str, text: &str) {
    let f = h.get_by_role_and_label(Role::TextInput, field);
    f.focus();
    f.type_text(text);
    h.run();
}

/// The sheet's own button, drawn after anything with the same name.
fn click_last_button(h: &mut Harness<'static, App>, label: &str) {
    h.get_all_by_role_and_label(Role::Button, label)
        .last()
        .unwrap()
        .click();
    h.run();
}

#[test]
fn the_editor_lists_the_inputs_as_they_are_typed() {
    let fake = FakeBackend::seeded();
    let mut h = ui_harness(fake);
    h.get_by_label("Add command").click();
    h.run();
    h.get_by_label_contains("$INPUT_RELEASE_NOTES");
    assert!(h.query_by_label("Asks when it runs").is_none());
    type_into(
        &mut h,
        "Command",
        "./release.sh \"$INPUT_VERSION\" $INPUT_NOTES",
    );
    h.get_by_label("Asks when it runs");
    h.get_by_label("Version");
    h.get_by_label("Notes");
}

#[test]
fn running_a_command_asks_for_its_inputs_and_sends_them() {
    let (fake, c) = with_release(RELEASE, true);
    let mut h = ui_harness(fake.clone());
    h.get_by_label("▶ Release").click();
    h.run();
    assert!(fake.actions().is_empty());
    assert!(
        h.query_by_label_contains("Check that you trust it")
            .is_none(),
        "a command that ran here before needs no check"
    );
    type_into(&mut h, "Release notes", "Fixes \"quotes\" and $HOME  ");
    click_last_button(&mut h, "Run Release");
    let p = fake.snapshot().projects[0].id;
    assert_eq!(
        fake.actions(),
        vec![Action::RunCommand(
            p,
            c.id,
            Some(command_hash(&c)),
            vec![(
                "INPUT_RELEASE_NOTES".into(),
                "Fixes \"quotes\" and $HOME".into()
            )]
        )]
    );
    assert_eq!(h.state().view.sheet, Sheet::None);
}

#[test]
fn a_new_command_with_inputs_is_checked_and_filled_in_one_sheet() {
    let (fake, c) = with_release(RELEASE, false);
    let mut h = ui_harness(fake.clone());
    h.get_by_label("▶ Release").click();
    h.run();
    h.get_by_label("Run Release?");
    h.get_by_label_contains("Check that you trust it");
    type_into(&mut h, "Release notes", "First");
    click_last_button(&mut h, "Run Release");
    let sent = fake.actions();
    assert!(
        matches!(&sent[..], [Action::RunCommand(_, id, Some(hash), values)]
            if *id == c.id && *hash == command_hash(&c) && values.len() == 1),
        "{sent:?}"
    );
}

#[test]
fn a_trusted_command_without_inputs_runs_at_once() {
    let (fake, c) = with_release("./release.sh", true);
    let mut h = ui_harness(fake.clone());
    h.get_by_label("▶ Release").click();
    h.run();
    let p = fake.snapshot().projects[0].id;
    assert_eq!(
        fake.actions(),
        vec![Action::RunCommand(p, c.id, None, vec![])]
    );
}

#[test]
fn cancelling_the_run_sheet_sends_nothing() {
    let (fake, _) = with_release(RELEASE, true);
    let mut h = ui_harness(fake.clone());
    h.get_by_label("▶ Release").click();
    h.run();
    type_into(&mut h, "Release notes", "Not yet");
    click_last_button(&mut h, "Cancel");
    assert!(fake.actions().is_empty());
    assert_eq!(h.state().view.sheet, Sheet::None);
}

#[test]
fn a_change_while_the_sheet_is_open_is_pointed_out_and_the_typing_goes_on() {
    let (fake, c) = with_release(RELEASE, true);
    let mut h = ui_harness(fake.clone());
    h.get_by_label("▶ Release").click();
    h.run();
    type_into(&mut h, "Release notes", "Fixes");
    // Another computer's edit arrives while the sheet is open.
    let line = format!("{RELEASE} --push prod");
    fake.update(|s| s.projects[0].commands.last_mut().unwrap().line = line.clone());
    h.run();
    h.get_by_label_contains("changed while this was open");
    let notes = h.get_by_role_and_label(Role::TextInput, "Release notes");
    assert!(notes.is_focused(), "the box keeps the keyboard");
    notes.type_text(" and more");
    h.run();
    click_last_button(&mut h, "Run Release");
    let changed = Command { line, ..c };
    let sent = fake.actions();
    assert!(
        matches!(&sent[..], [Action::RunCommand(_, _, Some(hash), values)]
            if *hash == command_hash(&changed) && values[0].1 == "Fixes and more"),
        "{sent:?}"
    );
}
