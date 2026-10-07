use super::*;
use std::path::Path;

mod found;
mod general;
mod ignore;
mod listed;
mod paired;
mod projects;
mod reach;

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

#[derive(Clone)]
pub(super) struct Capture(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);

impl std::io::Write for Capture {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(b);
        Ok(b.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// What `f` writes to the log while it runs on this thread.
pub(super) fn logged(f: impl FnOnce()) -> String {
    let log = Capture(Default::default());
    let sink = log.clone();
    let subscriber = tracing_subscriber::fmt()
        .with_writer(move || sink.clone())
        .with_ansi(false)
        .finish();
    tracing::subscriber::with_default(subscriber, f);
    String::from_utf8(log.0.lock().unwrap().clone()).unwrap()
}
