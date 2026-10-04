//! Two app cores on 127.0.0.1, driven only through actions, as the UI would.
#![allow(dead_code)]

use project_transfer::core::{Action, AppCore, StartOptions, UiState};
use project_transfer::model::Permissions;
use project_transfer::store::Store;
use std::net::{IpAddr, Ipv4Addr};
use std::path::Path;
use std::time::{Duration, Instant};

pub const ALL: Permissions = Permissions {
    may_push_to_me: true,
    may_pull_from_me: true,
};

pub const NONE: Permissions = Permissions {
    may_push_to_me: false,
    may_pull_from_me: false,
};

pub fn start(dir: &Path) -> AppCore {
    let opts = StartOptions {
        discovery: false,
        bind_ip: IpAddr::V4(Ipv4Addr::LOCALHOST),
        preferred_port: 0,
    };
    let core = AppCore::start_with(Store::open_at(dir.join("home")).unwrap(), None, opts).unwrap();
    core.act(Action::SetProjectsFolder(dir.join("Dev")));
    core
}

pub fn wait(core: &AppCore, what: &str, f: impl Fn(&UiState) -> bool) {
    let end = Instant::now() + Duration::from_secs(20);
    while Instant::now() < end {
        if f(&core.state()) {
            return;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    let s = core.state();
    panic!(
        "timed out waiting for {what}; transfer {:?}; activity {:#?}",
        s.transfer, s.activity
    );
}

/// A and B found each other; A has started pairing and B shows the prompt.
pub fn start_pairing(da: &Path, db: &Path) -> (AppCore, AppCore) {
    start_pairing_offering(da, db, ALL)
}

/// Like `start_pairing`, with A letting B do only what `offered` allows.
pub fn start_pairing_offering(da: &Path, db: &Path, offered: Permissions) -> (AppCore, AppCore) {
    let (a, b) = (start(da), start(db));
    let b_id = b.state().me.id;
    a.act(Action::AddByAddress(format!("127.0.0.1:{}", b.port())));
    wait(&a, "B to be found", |s| {
        s.discovered.iter().any(|d| d.id == b_id)
    });
    let target = a.state().discovered[0].clone();
    a.act(Action::Pair {
        target,
        requested: ALL,
        offered,
    });
    wait(&b, "the pairing prompt", |s| s.pair_prompt.is_some());
    (a, b)
}
