//! Inputs: the editor listing what a command asks for, and the run sheet
//! asking for it.

use super::{FakeBackend, shot};
use egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use project_transfer::commands::command_hash;
use project_transfer::model::{Command, Os, ThemeChoice};
use project_transfer::ui::App;
use std::path::Path;
use std::sync::Arc;

const RELEASE: &str = "./release.sh --version \"$INPUT_VERSION\" --notes \"$INPUT_RELEASE_NOTES\"";

/// The seed with a Release command that asks for two values; a trusted one
/// ran here before in this text, so it needs no check.
fn with_release(trusted: bool) -> Arc<FakeBackend> {
    let mut c = Command {
        id: uuid::Uuid::from_u128(50),
        label: "Release".into(),
        line: RELEASE.into(),
        created_on: Os::current(),
        updated_at_ms: 1,
        deleted: false,
        last_run_hash: None,
        linked_to: None,
    };
    if trusted {
        c.last_run_hash = Some(command_hash(&c));
    }
    let f = FakeBackend::seeded();
    f.update(|s| s.projects[0].commands.push(c));
    f
}

fn type_into(h: &mut Harness<'static, App>, field: &str, text: &str) {
    let f = h.get_by_role_and_label(Role::TextInput, field);
    f.focus();
    f.type_text(text);
    h.run_ok();
}

pub fn all(dir: &Path) {
    shot(dir, "16j-editor-inputs", FakeBackend::seeded(), |h| {
        h.get_by_label("Add command").click_accesskit();
        h.run_ok();
        type_into(h, "Label", "Release");
        type_into(h, "Command", RELEASE);
    });
    let run = |h: &mut Harness<'static, App>| {
        h.get_by_label("▶ Release").click_accesskit();
        h.run_ok();
        type_into(h, "Version", "2.4.0");
        type_into(
            h,
            "Release notes",
            "Shared commands, and commands that ask for values.",
        );
    };
    shot(dir, "16k-run-asks-inputs", with_release(true), run);
    shot(dir, "16l-run-check-and-inputs", with_release(false), run);
    let light = with_release(true);
    light.update(|s| s.me.theme = ThemeChoice::Light);
    shot(dir, "16m-run-asks-inputs-light", light, run);
    let f = with_release(true);
    let changing = f.clone();
    shot(dir, "16n-run-changed-while-open", f, move |h| {
        run(h);
        // Another computer's edit arrives while the sheet is open.
        changing.update(|s| {
            let c = s.projects[0].commands.last_mut().unwrap();
            c.line.push_str(" --push prod");
        });
        h.run_ok();
    });
}
