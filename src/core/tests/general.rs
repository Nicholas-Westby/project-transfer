use super::*;
use crate::model::{Peer, ThemeChoice};

// The command lines are unix shell; the runner itself is tested per OS.
#[cfg(unix)]
#[test]
fn running_a_command_uses_the_primary_folder_and_records_its_hash() {
    let f = Fixture::new();
    let p = f.create("Garden", "app");
    std::fs::write(f.folder("app").join("marker.txt"), "").unwrap();
    f.act(Action::AddCommand(p.id, "List".into(), "ls".into()));
    let c = f.project().commands[0].clone();
    assert!(crate::commands::needs_confirmation(&c));
    let hash = crate::commands::command_hash(&c);
    f.act(Action::RunCommand(p.id, c.id, Some(hash)));
    wait_until(&f.core, "the command to exit", |s| {
        s.command_runs
            .get(&c.id)
            .is_some_and(|r| r.status == RunStatus::Exited(Some(0)))
    });
    let run = f.core.state().command_runs[&c.id].clone();
    assert!(run.lines.iter().any(|l| l.text == "marker.txt"), "{run:?}");
    let saved = f.store().load_projects().unwrap()[0].commands[0].clone();
    assert_eq!(saved.last_run_hash, Some(crate::commands::command_hash(&c)));
    assert!(!crate::commands::needs_confirmation(&saved));
}

// The command lines are unix shell; the runner itself is tested per OS.
#[cfg(unix)]
#[test]
fn a_running_command_can_be_stopped() {
    let f = Fixture::new();
    let p = f.create("Garden", "app");
    f.act(Action::AddCommand(p.id, "Wait".into(), "sleep 30".into()));
    let c = f.project().commands[0].clone();
    let hash = Some(crate::commands::command_hash(&c));
    f.act(Action::RunCommand(p.id, c.id, hash.clone()));
    f.act(Action::RunCommand(p.id, c.id, hash));
    assert!(f.last_activity().text.contains("already running"));
    f.act(Action::StopCommand(c.id));
    wait_until(&f.core, "the command to stop", |s| {
        s.command_runs[&c.id].status == RunStatus::Exited(None)
    });
}

#[test]
fn a_command_runs_only_as_confirmed() {
    let f = Fixture::new();
    let p = f.create("Garden", "app");
    f.act(Action::AddCommand(p.id, "Build".into(), "make".into()));
    let c = f.project().commands[0].clone();
    // New here, so running without a confirmation is refused.
    f.act(Action::RunCommand(p.id, c.id, None));
    assert!(f.last_activity().text.contains("did not run"));
    // Confirmed text that has since been replaced is refused too.
    let old = crate::commands::command_hash(&c);
    f.act(Action::EditCommand(
        p.id,
        c.id,
        "Build".into(),
        "make all".into(),
    ));
    f.act(Action::RunCommand(p.id, c.id, Some(old)));
    let line = f.last_activity();
    assert_eq!(line.kind, ActivityKind::Error);
    assert!(
        line.text.contains("changed after you confirmed it"),
        "{}",
        line.text
    );
    assert!(f.core.state().command_runs.is_empty());
}

#[test]
fn output_keeps_only_the_newest_lines() {
    let mut run = CommandRun {
        lines: Vec::new(),
        status: RunStatus::Running,
    };
    for i in 0..OUTPUT_CAP + 5 {
        run.push(OutputLine {
            stderr: false,
            text: i.to_string(),
        });
    }
    assert_eq!(run.lines.len(), OUTPUT_CAP);
    assert_eq!(run.lines[0].text, "5");
}

#[test]
fn settings_changes_persist() {
    let f = Fixture::new();
    f.act(Action::Rename("  Studio Calm Otter ".into()));
    f.act(Action::SetTheme(ThemeChoice::Light));
    f.act(Action::SetProjectsFolder(f.dir.path().join("Dev")));
    f.act(Action::SetIgnores {
        extra: vec!["*.tmp".into(), " ".into()],
        always_include: vec!["bin".into()],
        removed_defaults: vec!["dist".into()],
    });
    let saved = f.store().load_or_create_instance().unwrap();
    assert_eq!(saved.name, "Studio Calm Otter");
    assert_eq!(saved.theme, ThemeChoice::Light);
    assert_eq!(saved.projects_folder, f.dir.path().join("Dev"));
    assert_eq!(saved.extra_ignores, ["*.tmp"]);
    assert_eq!(saved.always_include, ["bin"]);
    assert_eq!(saved.removed_default_ignores, ["dist"]);
    assert_eq!(f.core.state().me, saved);
    f.act(Action::Rename(" ".into()));
    assert_eq!(
        f.store().load_or_create_instance().unwrap().name,
        "Studio Calm Otter"
    );
}

#[test]
fn a_bad_ignore_pattern_changes_nothing() {
    let f = Fixture::new();
    f.act(Action::SetIgnores {
        extra: vec!["{a,b".into()],
        always_include: vec![],
        removed_defaults: vec![],
    });
    assert_eq!(f.last_activity().kind, ActivityKind::Error);
    assert!(
        f.store()
            .load_or_create_instance()
            .unwrap()
            .extra_ignores
            .is_empty()
    );
}

#[test]
fn a_public_address_is_refused_with_the_reason() {
    let f = Fixture::new();
    f.core.act(Action::AddByAddress("8.8.8.8".into()));
    wait_until(&f.core, "the address error", |s| s.address_error.is_some());
    assert_eq!(
        f.core.state().address_error.as_deref(),
        Some(crate::address::NOT_LOCAL)
    );
    assert!(f.core.state().discovered.is_empty());
}

#[test]
fn selecting_an_unknown_peer_is_refused() {
    let f = Fixture::new();
    f.act(Action::SelectPeer(uuid::Uuid::new_v4()));
    assert_eq!(f.core.state().selected_peer, None);
    assert_eq!(f.last_activity().kind, ActivityKind::Error);
}

#[test]
fn last_peer_is_selected_again_at_start_and_unpair_forgets_it() {
    let dir = tempfile::tempdir().unwrap();
    let store = store_in(dir.path());
    let peer = Peer {
        id: uuid::Uuid::new_v4(),
        name: "Desk".into(),
        fingerprint: "ff".into(),
        allows: Default::default(),
        granted: Default::default(),
        last_address: None,
    };
    store.save_peers(std::slice::from_ref(&peer)).unwrap();
    let mut me = store.load_or_create_instance().unwrap();
    me.last_peer = Some(peer.id);
    store.save_instance(&me).unwrap();

    let core = AppCore::start_with(store, None, opts()).unwrap();
    assert_eq!(core.state().selected_peer, Some(peer.id));
    assert!(!core.state().peers[0].online);
    core.act(Action::Unpair(peer.id));
    core.settle();
    assert_eq!(core.state().selected_peer, None);
    assert!(core.state().peers.is_empty());
    let store = store_in(dir.path());
    assert!(store.load_peers().unwrap().is_empty());
    assert_eq!(store.load_or_create_instance().unwrap().last_peer, None);
}

#[test]
fn a_busy_saved_port_is_replaced_and_saved() {
    let dir = tempfile::tempdir().unwrap();
    let busy = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let taken = busy.local_addr().unwrap().port();
    let store = store_in(dir.path());
    let mut me = store.load_or_create_instance().unwrap();
    me.port = taken;
    store.save_instance(&me).unwrap();
    let core = AppCore::start_with(store, None, opts()).unwrap();
    assert_ne!(core.port(), taken);
    assert_eq!(core.state().port, core.port());
    assert_eq!(
        store_in(dir.path()).load_or_create_instance().unwrap().port,
        core.port()
    );
    assert_eq!(core.state().activity[0].kind, ActivityKind::Warn);
}

#[test]
fn port_zero_tries_the_preferred_port_first() {
    let dir = tempfile::tempdir().unwrap();
    let free = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let preferred = free.local_addr().unwrap().port();
    drop(free);
    let o = StartOptions {
        preferred_port: preferred,
        ..opts()
    };
    let core = AppCore::start_with(store_in(dir.path()), None, o).unwrap();
    assert_eq!(core.port(), preferred);
    assert_eq!(
        store_in(dir.path()).load_or_create_instance().unwrap().port,
        preferred
    );
}

#[test]
fn repeated_activity_is_folded_into_one_line() {
    let ui = StateHandle::new(
        UiState::new(Fixture::new().core.state().me.clone(), 1),
        None,
    );
    ui.warn("Desk stopped answering.");
    ui.warn("Desk stopped answering.");
    ui.info("Desk stopped answering.");
    assert_eq!(ui.lock().activity.len(), 2);
}

#[test]
fn execute_without_a_preview_is_refused() {
    let f = Fixture::new();
    f.act(Action::Execute);
    assert_eq!(f.core.state().transfer, TransferState::Idle);
    assert!(f.last_activity().text.contains("no preview"));
}
