//! Adding, editing, unlinking and deleting commands and shared commands
//! through the window, over a hand-built state.

mod ui_support;

use egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use project_transfer::core::Action;
use project_transfer::model::{Command, Os, SharedCommand};
use project_transfer::ui::{App, Backend};
use std::sync::Arc;
use ui_support::{FakeBackend, ui_harness};
use uuid::Uuid;

fn release() -> SharedCommand {
    SharedCommand {
        id: Uuid::from_u128(40),
        label: "Release".into(),
        line: "./release.sh".into(),
        created_on: Os::MacOs,
        updated_at_ms: 1,
        deleted: false,
    }
}

/// The seeded state with Release among the shared commands.
fn with_release() -> Arc<FakeBackend> {
    let fake = FakeBackend::seeded();
    fake.update(|s| s.shared_commands = vec![release()]);
    fake
}

/// A command following Release, added to project `n` of the seed.
fn link_release(fake: &FakeBackend, n: usize, id: u128) -> Command {
    let r = release();
    let c = Command {
        id: Uuid::from_u128(id),
        label: r.label,
        line: r.line,
        created_on: r.created_on,
        updated_at_ms: r.updated_at_ms,
        deleted: false,
        last_run_hash: None,
        linked_to: Some(r.id),
    };
    fake.update(|s| s.projects[n].commands.push(c.clone()));
    c
}

fn garden(fake: &FakeBackend) -> Uuid {
    fake.snapshot().projects[0].id
}

fn open_add(h: &mut Harness<'static, App>) {
    h.get_by_label("Add command").click();
    h.run();
}

/// The sheet's own button, drawn after the one that opened it.
fn click_last_button(h: &mut Harness<'static, App>, label: &str) {
    h.get_all_by_role_and_label(Role::Button, label)
        .last()
        .unwrap()
        .click();
    h.run();
}

fn type_into(h: &mut Harness<'static, App>, field: &str, text: &str) {
    let f = h.get_by_role_and_label(Role::TextInput, field);
    f.focus();
    f.type_text(text);
    h.run();
}

#[test]
fn a_new_command_can_be_shared_as_it_is_added() {
    let fake = FakeBackend::seeded();
    let mut h = ui_harness(fake.clone());
    open_add(&mut h);
    assert!(
        h.query_by_label("Shared commands").is_none(),
        "no list to choose from yet"
    );
    type_into(&mut h, "Label", "Release");
    type_into(&mut h, "Command", "./release.sh");
    h.get_by_label("Share with other projects").click();
    h.run();
    click_last_button(&mut h, "Add command");
    assert_eq!(
        fake.actions(),
        vec![Action::AddSharedCommand(
            garden(&fake),
            "Release".into(),
            "./release.sh".into()
        )]
    );
}

#[test]
fn an_unshared_new_command_stays_in_its_project() {
    let fake = with_release();
    let mut h = ui_harness(fake.clone());
    open_add(&mut h);
    type_into(&mut h, "Label", "Build");
    type_into(&mut h, "Command", "make");
    click_last_button(&mut h, "Add command");
    assert_eq!(
        fake.actions(),
        vec![Action::AddCommand(
            garden(&fake),
            "Build".into(),
            "make".into()
        )]
    );
}

#[test]
fn a_shared_command_is_added_linked_unless_separate_is_chosen() {
    for separate in [false, true] {
        let fake = with_release();
        let mut h = ui_harness(fake.clone());
        open_add(&mut h);
        h.get_by_label("Shared commands").click();
        h.run();
        h.get_by_role_and_label(Role::RadioButton, "Release")
            .click();
        h.run();
        if separate {
            h.get_by_role_and_label(Role::RadioButton, "Separate")
                .click();
            h.run();
        }
        click_last_button(&mut h, "Add command");
        let (p, s) = (garden(&fake), release().id);
        let want = match separate {
            false => Action::LinkSharedCommand(p, s),
            true => Action::CopySharedCommand(p, s),
        };
        assert_eq!(fake.actions(), vec![want]);
    }
}

#[test]
fn a_shared_command_linked_here_already_can_only_be_copied() {
    let fake = with_release();
    link_release(&fake, 0, 41);
    let mut h = ui_harness(fake.clone());
    open_add(&mut h);
    h.get_by_label("Shared commands").click();
    h.run();
    h.get_by_role_and_label(Role::RadioButton, "Release")
        .click();
    h.run();
    let linked = h.get_by_role_and_label(Role::RadioButton, "Linked");
    assert!(linked.accesskit_node().is_disabled());
    h.get_by_label("Already linked in this project.");
    click_last_button(&mut h, "Add command");
    assert_eq!(
        fake.actions(),
        vec![Action::CopySharedCommand(garden(&fake), release().id)]
    );
}

#[test]
fn deleting_a_shared_command_asks_first_and_names_what_unlinks() {
    let fake = with_release();
    link_release(&fake, 0, 41);
    link_release(&fake, 1, 42);
    let mut h = ui_harness(fake.clone());
    open_add(&mut h);
    h.get_by_label("Shared commands").click();
    h.run();
    h.get_by_label("Delete shared command Release").click();
    h.run();
    h.get_by_label("Delete shared command Release?");
    h.get_by_label_contains("The 2 commands linked to it keep their text");
    assert!(fake.actions().is_empty());
    click_last_button(&mut h, "Delete shared command");
    assert_eq!(
        fake.actions(),
        vec![Action::DeleteSharedCommand(release().id)]
    );
}

#[test]
fn a_shared_command_is_edited_from_the_list_which_comes_back() {
    let fake = with_release();
    let mut h = ui_harness(fake.clone());
    open_add(&mut h);
    h.get_by_label("Shared commands").click();
    h.run();
    h.get_by_label("Edit shared command Release").click();
    h.run();
    h.get_by_label("Edit shared command");
    type_into(&mut h, "Command", " --notes");
    click_last_button(&mut h, "Save shared command");
    assert_eq!(
        fake.actions(),
        vec![Action::EditSharedCommand(
            release().id,
            "Release".into(),
            "./release.sh --notes".into()
        )]
    );
    h.get_by_role_and_label(Role::RadioButton, "Release");
}

#[test]
fn a_linked_command_says_shared_and_unlinks_in_one_click() {
    let fake = with_release();
    let c = link_release(&fake, 0, 41);
    let mut h = ui_harness(fake.clone());
    h.get_by_label("Shared");
    h.get_by_label("Unlink").click();
    h.run();
    assert_eq!(
        fake.actions(),
        vec![Action::UnlinkCommand(garden(&fake), c.id)]
    );
}

#[test]
fn editing_a_linked_command_names_the_projects_it_changes() {
    let fake = with_release();
    let c = link_release(&fake, 0, 41);
    link_release(&fake, 1, 42);
    let mut h = ui_harness(fake.clone());
    // Install git hooks and Fetch dependencies come first.
    h.get_all_by_label("Edit").nth(2).unwrap().click();
    h.run();
    h.get_by_label_contains("Saving changes it in garden-planner and tide-tables.");
    assert!(h.query_by_label("Share with other projects").is_none());
    type_into(&mut h, "Command", " --notes");
    click_last_button(&mut h, "Save shared command");
    assert_eq!(
        fake.actions(),
        vec![Action::EditCommand(
            garden(&fake),
            c.id,
            "Release".into(),
            "./release.sh --notes".into()
        )]
    );
}

#[test]
fn a_plain_command_is_shared_from_its_edit_sheet() {
    let fake = FakeBackend::seeded();
    let mut h = ui_harness(fake.clone());
    let hooks = fake.snapshot().projects[0].commands[0].clone();
    h.get_all_by_label("Edit").next().unwrap().click();
    h.run();
    h.get_by_label("Share with other projects").click();
    h.run();
    click_last_button(&mut h, "Save command");
    let p = garden(&fake);
    assert_eq!(
        fake.actions(),
        vec![
            Action::EditCommand(p, hooks.id, hooks.label, hooks.line),
            Action::ShareCommand(p, hooks.id),
        ]
    );
}

#[test]
fn deleting_a_linked_command_says_the_shared_one_stays() {
    let fake = with_release();
    link_release(&fake, 0, 41);
    let mut h = ui_harness(fake.clone());
    h.get_all_by_label("Delete").nth(2).unwrap().click();
    h.run();
    h.get_by_label("Delete Release?");
    h.get_by_label_contains("The shared command stays");
}
