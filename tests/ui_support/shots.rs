//! Renders each screen and dialog to PNG files for a person to look at.

use super::{FakeBackend, SIZE, sample_preview, with_pair_prompt};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use project_transfer::core::{Action, OutgoingPairView, PairState, PromptState, TransferState};
use project_transfer::model::ThemeChoice;
use project_transfer::transfer::Summary;
use project_transfer::ui::{App, FolderPicker};
use std::path::{Path, PathBuf};
use std::sync::Arc;

struct FixedPicker;

impl FolderPicker for FixedPicker {
    fn pick_folder(&self, _title: &str) -> Option<PathBuf> {
        Some(PathBuf::from("/Users/jdoe/Dev/seed-catalog"))
    }
}

fn shot(
    dir: &Path,
    name: &str,
    fake: Arc<FakeBackend>,
    setup: impl FnOnce(&mut Harness<'static, App>),
) {
    let mut h = super::build(fake, Box::new(FixedPicker), SIZE, 2.0, true);
    h.run_ok();
    setup(&mut h);
    h.run_ok();
    let img = h.render().expect("render");
    img.save(dir.join(format!("{name}.png"))).expect("save png");
}

pub fn all(dir: &Path) {
    std::fs::create_dir_all(dir).unwrap();
    let none = |_: &mut Harness<'static, App>| {};
    shot(dir, "01-main-dark", FakeBackend::seeded(), none);
    let light = FakeBackend::seeded();
    light.update(|s| s.me.theme = ThemeChoice::Light);
    shot(dir, "02-main-light", light, none);
    shot(dir, "03-empty", FakeBackend::empty(), none);

    let f = FakeBackend::seeded();
    f.update(|s| s.transfer = TransferState::Ready(sample_preview(s)));
    shot(dir, "04-preview", f, none);
    let f = FakeBackend::seeded();
    f.update(|s| s.transfer = TransferState::Ready(sample_preview(s)));
    let lf = f.clone();
    lf.update(|s| s.me.theme = ThemeChoice::Light);
    shot(dir, "04b-preview-light", lf, none);

    let f = FakeBackend::seeded();
    f.update(|s| {
        let p = sample_preview(s);
        s.transfer = TransferState::Ready(p);
    });
    shot(dir, "05-running", f.clone(), |h| {
        f.update(|s| {
            s.transfer = TransferState::Running {
                done: 4_200_000,
                total: 9_800_000,
                files: 42,
                current: "app/src/plants/tomato.rs".into(),
            }
        });
        h.run_ok();
    });
    let f = FakeBackend::seeded();
    f.update(|s| s.transfer = TransferState::Ready(sample_preview(s)));
    shot(dir, "06-finished", f.clone(), |h| {
        f.update(|s| {
            s.transfer = TransferState::Finished(Summary {
                files: 42,
                bytes: 9_800_000,
                removed: 2,
                took_ms: 3_100,
                failures: vec![("app/locked.db".into(), "The file is in use.".into())],
            })
        });
        h.run_ok();
    });

    shot(dir, "07-confirm-delete", FakeBackend::seeded(), |h| {
        h.get_by_label("Delete project").click_accesskit();
    });
    shot(dir, "08-confirm-run", FakeBackend::seeded(), |h| {
        h.get_by_label("▶ Fetch dependencies").click_accesskit();
    });
    shot(dir, "09-computers", FakeBackend::seeded(), |h| {
        super::open_computers(h);
        fixed_address(h);
    });
    let f = FakeBackend::seeded();
    f.update(super::with_relay);
    shot(dir, "09b-computers-through", f, |h| {
        super::open_computers(h);
        fixed_address(h);
    });
    shot(dir, "10-pair-setup", FakeBackend::seeded(), |h| {
        super::open_computers(h);
        h.get_by_label("Pair").click_accesskit();
    });
    let f = FakeBackend::seeded();
    f.update(|s| {
        s.pairing = Some(OutgoingPairView {
            target_name: "Mini Brisk Lynx".into(),
            code: Some("481 205".into()),
            state: PairState::Confirm,
            other_accepted: false,
        })
    });
    shot(dir, "11-pair-confirm", f, none);
    let f = FakeBackend::seeded();
    f.update(|s| {
        s.pairing = Some(OutgoingPairView {
            target_name: "Mini Brisk Lynx".into(),
            code: Some("481 205".into()),
            state: PairState::Waiting,
            other_accepted: false,
        })
    });
    shot(dir, "11b-pair-waiting", f, none);
    let f = FakeBackend::seeded();
    f.update(with_pair_prompt);
    shot(dir, "12-pair-prompt", f, none);
    let f = FakeBackend::seeded();
    f.update(|s| {
        with_pair_prompt(s);
        if let Some(p) = &mut s.pair_prompt {
            p.state = PromptState::Waiting;
        }
    });
    shot(dir, "12b-pair-prompt-waiting", f, none);
    shot(dir, "13-settings", FakeBackend::seeded(), |h| {
        h.get_by_label("Settings").click_accesskit();
        h.run_ok();
        fixed_address(h);
    });
    shot(dir, "14-new-project", FakeBackend::seeded(), |h| {
        h.get_by_label("New project").click_accesskit();
    });
    let f = FakeBackend::seeded();
    f.update(|s| s.peers[0].online = false);
    shot(dir, "15-peer-offline", f, none);
    let f = FakeBackend::seeded();
    f.update(|s| {
        s.peers[0].peer.granted = Default::default();
        s.remote_projects.clear();
    });
    shot(dir, "15b-peer-shares-nothing", f, none);
    shot(dir, "16-command-editor", FakeBackend::seeded(), |h| {
        h.get_by_label("Add command").click_accesskit();
    });
    shot(
        dir,
        "17-confirm-send-everything",
        FakeBackend::seeded(),
        |h| {
            h.get_by_label("Send everything").click_accesskit();
        },
    );
    shot(dir, "18-activity-open", FakeBackend::seeded(), |h| {
        h.get_by_label_contains("Activity").click_accesskit();
    });
    shot(dir, "19-peer-menu", FakeBackend::seeded(), |h| {
        h.get_by_label("Choose a computer").click_accesskit();
    });
    let f = FakeBackend::seeded();
    f.update(|s| {
        s.address_error = Some(project_transfer::address::NOT_LOCAL.into());
    });
    shot(dir, "20-computers-address-error", f, |h| {
        super::open_computers(h)
    });
    let _ = Action::Execute;
}

/// The real interfaces differ per machine; screenshots show a stand-in.
fn fixed_address(h: &mut Harness<'static, App>) {
    h.run_ok();
    h.state_mut().view.my_addresses = Some(vec!["192.168.1.20:47820".into()]);
}
