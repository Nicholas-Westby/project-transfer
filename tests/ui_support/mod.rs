//! Harnesses and seeded states for the UI tests and screenshots.

#![allow(dead_code)]

pub mod seed;
pub mod shots;

use seed::seeded_state;
pub use seed::{sample_preview, with_pair_prompt};

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use project_transfer::core::{Action, AppCore, StartOptions, TransferState, UiState};
use project_transfer::discovery::Discovered;
use project_transfer::manifest::{Change, Entry, Kind};
use project_transfer::protocol::PROTOCOL_VERSION;
use project_transfer::store::Store;
use project_transfer::ui::{App, Backend, FolderPicker};
use std::net::{IpAddr, Ipv4Addr};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

pub const SIZE: [f32; 2] = [1100.0, 720.0];
/// Tests click by position, so everything they touch must be on screen.
pub const TEST_SIZE: [f32; 2] = [1300.0, 1100.0];

/// Never opens a window; tests that need a folder set one first.
#[derive(Default)]
pub struct NoPicker;

impl FolderPicker for NoPicker {
    fn pick_folder(&self, _title: &str) -> Option<PathBuf> {
        None
    }
}

pub fn harness(backend: Arc<dyn Backend>) -> Harness<'static, App> {
    build(backend, Box::new(NoPicker), TEST_SIZE, 1.0, false)
}

/// The app as eframe runs it, so panels fill the whole window.
pub fn build(
    backend: Arc<dyn Backend>,
    picker: Box<dyn FolderPicker>,
    size: [f32; 2],
    ppp: f32,
    wgpu: bool,
) -> Harness<'static, App> {
    let mut b = Harness::builder()
        .with_size(size)
        .with_pixels_per_point(ppp);
    if wgpu {
        b = b.wgpu();
    }
    let mut h = b.build_eframe(move |_cc| App::new(backend, picker));
    // The first frame only loads fonts, and sheets settle their size over
    // a few more; clicks go by position, so let the layout come to rest.
    h.run_steps(6);
    h
}

pub fn live(dir: &Path) -> (Arc<AppCore>, Harness<'static, App>) {
    let opts = StartOptions {
        discovery: false,
        bind_ip: IpAddr::V4(Ipv4Addr::LOCALHOST),
        preferred_port: 0,
    };
    let core = Arc::new(
        AppCore::start_with(Store::open_at(dir.join("home")).unwrap(), None, opts).unwrap(),
    );
    core.act(Action::SetProjectsFolder(dir.join("Dev")));
    core.settle();
    let h = harness(core.clone());
    (core, h)
}

pub fn ui_harness(fake: Arc<FakeBackend>) -> Harness<'static, App> {
    harness(fake)
}

/// Opens the sample preview with three drawings changed in its first
/// folder: two in the top `exports` folder and one deeper, so the ignore
/// dialog has folders to offer.
pub fn with_drawings(s: &mut UiState) {
    let mut p = sample_preview(s);
    for rel in [
        "exports/2026/plot-01.svg",
        "exports/2026/plot-02.svg",
        "src/exports/beds.svg",
    ] {
        p.folders[0].plan.changes.push(changed(rel));
    }
    s.transfer = TransferState::Ready(p);
}

/// A file a preview rewrites.
pub fn changed(rel: &str) -> Change {
    let kind = Kind::File {
        size: 2_400,
        mtime_ms: 0,
        exec: false,
    };
    let entry = Entry {
        rel: rel.into(),
        kind,
    };
    Change::Update {
        entry,
        dest_newer: false,
    }
}

/// Adds a computer listed through Desktop Swift Heron, which Laptop Quiet
/// Finch is reached through too.
pub fn with_relay(s: &mut UiState) {
    let desktop = s.peers[0].peer.id;
    s.discovered.push(Discovered {
        id: uuid::Uuid::from_u128(5),
        name: "Tower Calm Wren".into(),
        addrs: vec![],
        version: PROTOCOL_VERSION,
        via: Some(desktop),
    });
    s.peers[1].peer.via = Some(desktop);
}

pub fn open_computers(h: &mut Harness<'static, App>) {
    h.get_by_label("Choose a computer").click();
    h.run();
    h.get_by_label("Manage paired computers…").click();
    h.run();
}

/// A state the test sets directly, recording the actions the UI sends.
pub struct FakeBackend {
    state: Mutex<UiState>,
    acts: Mutex<Vec<Action>>,
}

impl Backend for FakeBackend {
    fn snapshot(&self) -> UiState {
        self.state.lock().unwrap().clone()
    }

    fn act(&self, a: Action) {
        self.acts.lock().unwrap().push(a);
    }
}

impl FakeBackend {
    pub fn seeded() -> Arc<FakeBackend> {
        Arc::new(FakeBackend {
            state: Mutex::new(seeded_state()),
            acts: Mutex::new(Vec::new()),
        })
    }

    pub fn empty() -> Arc<FakeBackend> {
        let mut s = seeded_state();
        s.projects.clear();
        s.peers.clear();
        s.selected_peer = None;
        s.remote_projects.clear();
        s.command_runs.clear();
        s.activity.truncate(1);
        Arc::new(FakeBackend {
            state: Mutex::new(s),
            acts: Mutex::new(Vec::new()),
        })
    }

    pub fn update(&self, f: impl FnOnce(&mut UiState)) {
        f(&mut self.state.lock().unwrap());
    }

    pub fn actions(&self) -> Vec<Action> {
        self.acts.lock().unwrap().clone()
    }
}
