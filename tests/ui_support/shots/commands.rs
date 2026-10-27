//! The shared commands: their rows, the add sheet's list, and the sheets
//! and confirmation that edit and delete them.

use super::{FakeBackend, FixedPicker, shot};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use project_transfer::model::ThemeChoice;
use project_transfer::ui::App;
use std::path::Path;
use std::sync::Arc;

/// The seeded state with two shared commands, Release linked in both
/// projects, so the rows, the add sheet and the edit sheets show them.
fn with_shared_commands() -> Arc<FakeBackend> {
    use project_transfer::model::{Command, Os, SharedCommand};
    let release = SharedCommand {
        id: uuid::Uuid::from_u128(40),
        label: "Release".into(),
        line: "./release.sh --notes \"$INPUT_RELEASE_NOTES\"".into(),
        created_on: Os::MacOs,
        updated_at_ms: 1,
        deleted: false,
    };
    let deps = SharedCommand {
        id: uuid::Uuid::from_u128(43),
        label: "Restore packages".into(),
        line: "dotnet restore".into(),
        created_on: Os::Windows,
        ..release.clone()
    };
    let f = FakeBackend::seeded();
    f.update(|s| {
        for (n, p) in s.projects.iter_mut().enumerate() {
            p.commands.push(Command {
                id: uuid::Uuid::from_u128(41 + n as u128),
                label: release.label.clone(),
                line: release.line.clone(),
                created_on: release.created_on,
                updated_at_ms: release.updated_at_ms,
                deleted: false,
                last_run_hash: None,
                linked_to: Some(release.id),
            });
        }
        s.shared_commands = vec![release, deps];
    });
    f
}

pub fn all(dir: &Path) {
    let open_list = |h: &mut Harness<'static, App>| {
        h.get_by_label("Add command").click_accesskit();
        h.run_ok();
        h.get_by_label("Shared commands").click_accesskit();
        h.run_ok();
    };
    // Tall enough that the linked row, last in the list, is on screen.
    let mut h = crate::ui_support::build(
        with_shared_commands(),
        Box::new(FixedPicker),
        [1100.0, 1000.0],
        2.0,
        true,
    );
    h.run_ok();
    h.get_by_label("Shared").hover();
    h.run_ok();
    let img = h.render().expect("render");
    img.save(dir.join("16b-command-rows-shared.png"))
        .expect("save png");
    shot(dir, "16c-add-from-shared", with_shared_commands(), |h| {
        open_list(h);
        h.get_by_label("Restore packages").click_accesskit();
    });
    shot(
        dir,
        "16d-add-shared-linked-here",
        with_shared_commands(),
        |h| {
            open_list(h);
            h.get_by_label("Release").click_accesskit();
        },
    );
    shot(
        dir,
        "16e-edit-shared-from-list",
        with_shared_commands(),
        |h| {
            open_list(h);
            h.get_by_label("Edit shared command Release")
                .click_accesskit();
        },
    );
    shot(dir, "16f-delete-shared", with_shared_commands(), |h| {
        open_list(h);
        h.get_by_label("Delete shared command Release")
            .click_accesskit();
    });
    shot(
        dir,
        "16g-edit-linked-command",
        with_shared_commands(),
        |h| {
            h.get_all_by_label("Edit").nth(2).unwrap().click_accesskit();
        },
    );
    shot(dir, "16h-new-command-share", with_shared_commands(), |h| {
        h.get_by_label("Add command").click_accesskit();
        h.run_ok();
        h.get_by_label("Share with other projects")
            .click_accesskit();
    });
    let light = with_shared_commands();
    light.update(|s| s.me.theme = ThemeChoice::Light);
    shot(dir, "16i-add-from-shared-light", light, |h| {
        open_list(h);
        h.get_by_label("Restore packages").click_accesskit();
    });
}
