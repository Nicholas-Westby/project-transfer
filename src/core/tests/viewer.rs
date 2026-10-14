//! The file viewer from the UI's side: both copies read and lined up, and
//! a copy the other computer won't share explained.

use super::paired::*;
use super::*;
use crate::model::{Direction, Project};
use crate::transfer::TransferRequest;
use crate::viewer::text::Body;
use crate::viewer::{Copies, FileAsk, Pane, Shown};

/// A and B paired, A's `garden-planner` pushed to B, and `main.rs` changed
/// on A since.
fn pushed_then_changed() -> (Fixture, Fixture, Project) {
    let (a, b) = (Fixture::new(), Fixture::new());
    for f in [&a, &b] {
        f.act(Action::SetProjectsFolder(f.dir.path().join("Dev")));
    }
    pair(&a, &b, Some(at(&b)), None);
    pair(&b, &a, Some(at(&a)), None);
    let p = a.create("garden-planner", "app");
    let root = p.folders[0].local_path.clone().unwrap();
    std::fs::write(root.join("main.rs"), "fn main() {}\n").unwrap();
    std::fs::write(root.join("other.rs"), "fn other() {}\n").unwrap();
    a.act(Action::Prepare(TransferRequest {
        peer: id(&b),
        project: p.id,
        direction: Direction::Push,
        send_everything: false,
    }));
    wait_until(&a.core, "the preview", |s| {
        matches!(s.transfer, TransferState::Ready(_))
    });
    a.act(Action::Execute);
    wait_until(&a.core, "the push", |s| {
        matches!(s.transfer, TransferState::Finished(_))
    });
    a.act(Action::DismissTransfer);
    std::fs::write(root.join("main.rs"), "fn main() { grow(); }\n").unwrap();
    (a, b, p)
}

fn ask(b: &Fixture, p: &Project) -> FileAsk {
    FileAsk {
        peer: id(b),
        project: p.id,
        folder: p.folders[0].id,
        local_root: p.folders[0].local_path.clone().unwrap(),
        rel: "main.rs".into(),
        copies: Copies::Both,
    }
}

fn shown(a: &Fixture) -> std::sync::Arc<Shown> {
    wait_until(&a.core, "both copies", |s| {
        let view = s.file_view.as_ref();
        view.is_some_and(|v| v.shown.as_ref().is_some_and(|s| !s.waiting))
    });
    a.core.state().file_view.clone().unwrap().shown.unwrap()
}

fn lines(pane: &Option<Result<Pane, String>>) -> Vec<String> {
    match pane {
        Some(Ok(Pane {
            body: Body::Text(t),
            ..
        })) => t.lines().iter().map(|l| l.to_string()).collect(),
        other => panic!("expected text, got {other:?}"),
    }
}

#[test]
fn both_copies_are_read_and_lined_up() {
    let (a, b, p) = pushed_then_changed();
    a.act(Action::OpenFile(ask(&b, &p)));
    let s = shown(&a);
    assert_eq!(lines(&s.here), ["fn main() { grow(); }"]);
    assert_eq!(lines(&s.there), ["fn main() {}"]);
    assert_eq!(s.rows.len(), 1);
    assert!(s.rows[0].differs);
    a.act(Action::CloseFile);
    assert!(a.core.state().file_view.is_none());
}

#[test]
fn a_copy_the_other_computer_wont_share_says_why_and_the_other_still_shows() {
    let (a, b, p) = pushed_then_changed();
    {
        let mut peers = a.core.core.shared.peers.blocking_write();
        peers[0].granted.may_pull_from_me = false;
    }
    a.act(Action::OpenFile(ask(&b, &p)));
    let s = shown(&a);
    assert_eq!(lines(&s.here), ["fn main() { grow(); }"]);
    let why = s.there.clone().unwrap().unwrap_err();
    assert!(
        why.contains("doesn't let this computer pull from it"),
        "{why}"
    );
    assert!(s.rows.is_empty());
}

#[test]
fn the_file_opened_last_is_the_one_shown() {
    let (a, b, p) = pushed_then_changed();
    a.act(Action::OpenFile(ask(&b, &p)));
    a.act(Action::CloseFile);
    let mut other = ask(&b, &p);
    other.rel = "other.rs".into();
    other.copies = Copies::Here;
    a.act(Action::OpenFile(other));
    let s = shown(&a);
    let view = a.core.state().file_view.clone().unwrap();
    assert_eq!((view.ask.rel.as_str(), view.opened), ("other.rs", 2));
    assert_eq!(lines(&s.here), ["fn other() {}"]);
}

/// A computer that takes a connection and never answers; the receiver
/// hears once the connection is closed from this side.
fn silent() -> (std::net::SocketAddr, std::sync::mpsc::Receiver<()>) {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let (mut s, _) = listener.accept().unwrap();
        let mut buf = [0u8; 4096];
        while matches!(std::io::Read::read(&mut s, &mut buf), Ok(n) if n > 0) {}
        let _ = tx.send(());
    });
    (addr, rx)
}

#[test]
fn the_copy_here_shows_while_the_other_waits_and_closing_lets_go() {
    let (a, b) = (Fixture::new(), Fixture::new());
    let (addr, hung_up) = silent();
    pair(&a, &b, Some(addr), None);
    let p = a.create("garden-planner", "app");
    let root = p.folders[0].local_path.clone().unwrap();
    std::fs::write(root.join("main.rs"), "fn main() {}\n").unwrap();
    a.act(Action::OpenFile(ask(&b, &p)));
    wait_until(&a.core, "the copy here", |s| {
        let shown = s.file_view.as_ref().and_then(|v| v.shown.as_ref());
        shown.is_some_and(|s| s.waiting)
    });
    let s = a.core.state().file_view.clone().unwrap().shown.unwrap();
    assert_eq!(lines(&s.here), ["fn main() {}"]);
    assert!(s.there.is_none());
    a.act(Action::CloseFile);
    // Well inside the 10 s the handshake may take: closing itself lets go.
    hung_up
        .recv_timeout(std::time::Duration::from_secs(2))
        .expect("closing the viewer ends its connection");
}

#[test]
fn a_late_read_of_an_earlier_opening_is_dropped() {
    let (a, b, p) = pushed_then_changed();
    let mut here = ask(&b, &p);
    here.copies = Copies::Here;
    a.act(Action::OpenFile(here.clone()));
    a.act(Action::OpenFile(here));
    let _ = shown(&a);
    let late = Shown::build(Some(Err("late".into())), None);
    a.core.core.show(1, late);
    let view = a.core.state().file_view.clone().unwrap();
    assert_eq!(view.opened, 2);
    assert_eq!(lines(&view.shown.unwrap().here), ["fn main() { grow(); }"]);
}
