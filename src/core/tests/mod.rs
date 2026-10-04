use super::*;
use std::path::Path;

mod general;
mod projects;

use std::time::{Duration, Instant};

pub(super) fn opts() -> StartOptions {
    StartOptions {
        discovery: false,
        bind_ip: IpAddr::V4(Ipv4Addr::LOCALHOST),
        preferred_port: 0,
    }
}

pub(super) struct Fixture {
    pub(super) dir: tempfile::TempDir,
    pub(super) core: AppCore,
}

impl Fixture {
    pub(super) fn new() -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let core = AppCore::start_with(store_in(dir.path()), None, opts()).unwrap();
        Fixture { dir, core }
    }

    pub(super) fn folder(&self, name: &str) -> PathBuf {
        let p = self.dir.path().join("src").join(name);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    pub(super) fn act(&self, a: Action) {
        self.core.act(a);
        self.core.settle();
    }

    pub(super) fn store(&self) -> Store {
        store_in(self.dir.path())
    }

    pub(super) fn project(&self) -> crate::model::Project {
        let s = self.core.state();
        assert_eq!(s.projects.len(), 1);
        s.projects[0].clone()
    }

    pub(super) fn last_activity(&self) -> ActivityLine {
        self.core.state().activity.last().cloned().unwrap()
    }

    pub(super) fn create(&self, name: &str, folder: &str) -> crate::model::Project {
        let folder = self.folder(folder);
        self.act(Action::CreateProject {
            name: name.into(),
            folder,
        });
        self.project()
    }
}

pub(super) fn store_in(dir: &Path) -> Store {
    Store::open_at(dir.join("home")).unwrap()
}

pub(super) fn wait_until(core: &AppCore, what: &str, f: impl Fn(&UiState) -> bool) {
    let end = Instant::now() + Duration::from_secs(10);
    while Instant::now() < end {
        if f(&core.state()) {
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    panic!("timed out waiting for {what}: {:#?}", core.state().activity);
}
