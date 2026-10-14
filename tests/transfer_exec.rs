//! The executable bit next to Windows, which keeps none, so every file it
//! sends says it doesn't run: the copy here keeps the bit it has, and a file
//! new here gets none. Between two Macs the bit travels as it is.
#![cfg(unix)]

mod support;

use project_transfer::ignore_rules::IgnoreSpec;
use project_transfer::model::Os;
use project_transfer::protocol::{Request, Response};
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use support::*;

fn runs(p: &Path) -> bool {
    std::fs::metadata(p).unwrap().permissions().mode() & 0o111 != 0
}

fn make_runnable(p: &Path) {
    std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o755)).unwrap();
}

/// Plays the pushing computer by hand, which says what it runs when it asks
/// for the folder scan.
#[tokio::test]
async fn a_push_from_windows_keeps_the_bit_here() {
    let (a, b) = pair_full().await;
    let src = a.project_dir("app");
    write(&src, "install", "old");
    let p = a.add_project("Garden", &[("app", &src)]).await;
    push(&a, &b, p.id, false).await;
    let dest = b.dev().join("app");
    for (from_os, kept) in [(Os::Windows, true), (Os::MacOs, false)] {
        make_runnable(&dest.join("install"));
        let new = format!("new from {from_os:?}");
        let mut conn = a.open(&b).await;
        let scan = Request::Manifest {
            project: p.id,
            folder: p.primary,
            ignore: IgnoreSpec::default(),
            folder_name: "app".into(),
            project_name: "Garden".into(),
            multi_folder: false,
            from_os: Some(from_os),
            home_hint: None,
        };
        let answer = conn.request(&scan).await.unwrap();
        assert!(matches!(answer, Response::Manifest(_)), "{answer:?}");
        let begin = Request::BeginPush {
            project: a.project(p.id).await.unwrap(),
            folder: p.primary,
            expected_path: dest.display().to_string(),
            home_hints: Default::default(),
        };
        assert_eq!(conn.request(&begin).await.unwrap(), Response::Ok);
        for rel in ["install", new.as_str()] {
            let put = Request::PutFile {
                rel: rel.into(),
                size: 3,
                mtime_ms: 5,
                exec: false,
            };
            conn.send(&put).await.unwrap();
            conn.send_raw(b"new").await.unwrap();
            assert_eq!(conn.recv().await.unwrap(), Response::Ok);
        }
        assert_eq!(read(&dest, "install"), "new");
        assert_eq!(runs(&dest.join("install")), kept, "pushed from {from_os:?}");
        assert!(!runs(&dest.join(&new)), "a file new here gets no bit");
    }
}

/// The other computer here is a Mac, so the connection is told what it runs
/// after the preview, as a Windows folder scan would have said.
#[tokio::test]
async fn a_pull_from_windows_keeps_the_bit_here() {
    use project_transfer::{model::Direction, transfer};
    let (a, b) = pair_full().await;
    let theirs = b.project_dir("app");
    write(&theirs, "install", "old");
    let p = b.add_project("Garden", &[("app", &theirs)]).await;
    pull(&a, &b, p.id).await;
    let mine = a.dev().join("app");
    for (os, kept) in [(Os::Windows, true), (Os::MacOs, false)] {
        make_runnable(&mine.join("install"));
        write(&theirs, "install", &format!("from {os:?}"));
        let mut conn = a.open(&b).await;
        let req = a.request(&b, p.id, Direction::Pull).await;
        let preview = transfer::prepare(&mut conn, &a.shared, req).await.unwrap();
        // The folder scan told the connection what the other computer runs.
        assert_eq!(conn.peer_os(), Some(Os::current()));
        conn.set_peer_os(os);
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let cancel = Default::default();
        let summary = transfer::execute(&mut conn, &a.shared, preview, tx, cancel)
            .await
            .unwrap();
        assert!(summary.failures.is_empty(), "{:?}", summary.failures);
        assert_eq!(read(&mine, "install"), format!("from {os:?}"));
        assert_eq!(runs(&mine.join("install")), kept, "pulled from {os:?}");
    }
}
