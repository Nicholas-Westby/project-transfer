//! Transfers that must fail safely: interrupted, cancelled, hostile paths,
//! missing permissions and the wrong computer.

mod support;

use project_transfer::model::Direction;
use project_transfer::net::Connection;
use project_transfer::protocol::{Request, Response};
use project_transfer::transfer::{self, Progress};
use support::*;

#[tokio::test]
async fn dropped_connection_mid_file_keeps_the_old_file_and_no_temp() {
    let (a, b) = pair_full().await;
    let src = a.project_dir("app");
    write(&src, "big.bin", "original");
    let p = a.add_project("Garden", &[("app", &src)]).await;
    push(&a, &b, p.id, false).await;
    let dest = b.dev().join("app");

    let mut conn = a.open(&b).await;
    let begin = Request::BeginPush {
        project: a.project(p.id).await.unwrap(),
        folder: p.primary,
        expected_path: b.dev().join("app").display().to_string(),
        home_hints: Default::default(),
    };
    assert_eq!(conn.request(&begin).await.unwrap(), Response::Ok);
    let put = Request::PutFile {
        rel: "big.bin".into(),
        size: 1_000_000,
        mtime_ms: 5,
        exec: false,
    };
    conn.send(&put).await.unwrap();
    conn.send_raw(&[7u8; 300_000]).await.unwrap();
    // The receiver is now mid-file, writing beside the real one.
    wait_for(|| temps(&dest).len() == 1).await;
    drop(conn);

    wait_for(|| temps(&dest).is_empty()).await;
    assert_eq!(read(&dest, "big.bin"), "original");

    // A temp left by a crash is swept by the next transfer.
    write(&dest, "big.bin.abcd.pt-tmp", "stray");
    write(&src, "big.bin", "updated");
    push(&a, &b, p.id, false).await;
    assert_eq!(read(&dest, "big.bin"), "updated");
    assert!(temps(&dest).is_empty());
}

#[tokio::test]
async fn cancel_stops_the_transfer_and_reports_it() {
    let (a, b) = pair_full().await;
    let src = a.project_dir("app");
    write(&src, "f", "f");
    let p = a.add_project("Garden", &[("app", &src)]).await;
    let mut conn = a.open(&b).await;
    let req = a.request(&b, p.id, Direction::Push).await;
    let preview = transfer::prepare(&mut conn, &a.shared, req).await.unwrap();
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let cancel = tokio_util::sync::CancellationToken::new();
    cancel.cancel();
    let err = transfer::execute(&mut conn, &a.shared, preview, tx, cancel)
        .await
        .unwrap_err();
    assert!(err.to_string().contains("cancelled"), "{err}");
    let mut last = None;
    while let Ok(p) = rx.try_recv() {
        last = Some(p);
    }
    assert!(matches!(last, Some(Progress::Failed(_))), "{last:?}");
    assert!(!b.dev().join("app/f").exists());
}

#[tokio::test]
async fn paths_leaving_the_folder_are_refused() {
    let (a, b) = pair_full().await;
    let src = a.project_dir("app");
    write(&src, "f", "f");
    let p = a.add_project("Garden", &[("app", &src)]).await;
    push(&a, &b, p.id, false).await;
    let victim = b.root().join("victim.txt");
    std::fs::write(&victim, "keep").unwrap();

    let mut conn = a.open(&b).await;
    let begin = Request::BeginPush {
        project: a.project(p.id).await.unwrap(),
        folder: p.primary,
        expected_path: b.dev().join("app").display().to_string(),
        home_hints: Default::default(),
    };
    assert_eq!(conn.request(&begin).await.unwrap(), Response::Ok);
    let bad = [
        Request::Remove {
            rel: "../../victim.txt".into(),
            is_dir: false,
        },
        Request::MakeDir {
            rel: "../escape".into(),
        },
        Request::MakeDir { rel: "/abs".into() },
        Request::MakeSymlink {
            rel: "a/../../l".into(),
            target: "x".into(),
        },
        Request::SetMtime {
            rel: "..".into(),
            mtime_ms: 1,
        },
        Request::GetFile {
            project: p.id,
            folder: p.primary,
            rel: "../../victim.txt".into(),
        },
        Request::Hashes {
            project: p.id,
            folder: p.primary,
            paths: vec!["../../victim.txt".into()],
        },
    ];
    for req in bad {
        match conn.request(&req).await.unwrap() {
            Response::Refused { reason } => assert!(reason.contains("not allowed"), "{reason}"),
            Response::Hashes(h) => assert!(h.is_empty()),
            other => panic!("{req:?} answered {other:?}"),
        }
    }
    conn.send(&Request::PutFile {
        rel: "../owned".into(),
        size: 3,
        mtime_ms: 1,
        exec: false,
    })
    .await
    .unwrap();
    conn.send_raw(b"bad").await.unwrap();
    assert!(matches!(
        conn.recv().await.unwrap(),
        Response::Refused { .. }
    ));
    assert_eq!(std::fs::read_to_string(&victim).unwrap(), "keep");
    assert!(!b.dev().join("owned").exists() && !b.root().join("escape").exists());
}

#[cfg(unix)]
#[tokio::test]
async fn a_pushed_symlink_cannot_be_used_to_write_outside() {
    let (a, b) = pair_full().await;
    let src = a.project_dir("app");
    write(&src, "f", "f");
    let p = a.add_project("Garden", &[("app", &src)]).await;
    push(&a, &b, p.id, false).await;
    let outside = b.root().join("outside");
    std::fs::create_dir_all(&outside).unwrap();

    let mut conn = a.open(&b).await;
    let begin = Request::BeginPush {
        project: a.project(p.id).await.unwrap(),
        folder: p.primary,
        expected_path: b.dev().join("app").display().to_string(),
        home_hints: Default::default(),
    };
    assert_eq!(conn.request(&begin).await.unwrap(), Response::Ok);
    let link = Request::MakeSymlink {
        rel: "evil".into(),
        target: outside.display().to_string(),
    };
    assert_eq!(conn.request(&link).await.unwrap(), Response::Ok);
    conn.send(&Request::PutFile {
        rel: "evil/x".into(),
        size: 2,
        mtime_ms: 1,
        exec: false,
    })
    .await
    .unwrap();
    conn.send_raw(b"no").await.unwrap();
    assert!(matches!(
        conn.recv().await.unwrap(),
        Response::Refused { .. }
    ));
    assert!(!outside.join("x").exists());
}

#[tokio::test]
async fn push_without_permission_is_refused() {
    let (a, b) = pair_with(perms(false, true)).await;
    let src = a.project_dir("app");
    write(&src, "f", "f");
    let p = a.add_project("Garden", &[("app", &src)]).await;
    let mut conn = a.open(&b).await;
    let req = a.request(&b, p.id, Direction::Push).await;
    let err = match transfer::prepare(&mut conn, &a.shared, req).await {
        Err(e) => e,
        Ok(preview) => {
            let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
            transfer::execute(&mut conn, &a.shared, preview, tx, Default::default())
                .await
                .unwrap_err()
        }
    };
    assert!(
        err.to_string().contains("doesn't let this computer push"),
        "{err}"
    );
    assert!(!b.dev().join("app").exists());
    assert!(b.project(p.id).await.is_none());
}

#[tokio::test]
async fn pull_without_permission_is_refused() {
    let (a, b) = pair_with(perms(true, false)).await;
    let theirs = b.project_dir("beds");
    write(&theirs, "f", "f");
    let p = b.add_project("Orchard", &[("beds", &theirs)]).await;
    let mut conn = a.open(&b).await;
    let req = a.request(&b, p.id, Direction::Pull).await;
    let err = match transfer::prepare(&mut conn, &a.shared, req).await {
        Err(e) => e,
        Ok(preview) => {
            let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
            transfer::execute(&mut conn, &a.shared, preview, tx, Default::default())
                .await
                .unwrap_err()
        }
    };
    assert!(
        err.to_string().contains("doesn't let this computer pull"),
        "{err}"
    );
    assert!(!a.dev().join("beds").exists());
}

#[tokio::test]
async fn transfers_use_the_verified_peer_connection() {
    let (a, b) = pair_full().await;
    let p = a
        .add_project("Garden", &[("app", &a.project_dir("app"))])
        .await;
    let stranger = support::Instance::start(perms(true, true)).await;
    let mut conn = Connection::open(stranger.addr, &a.shared).await.unwrap();
    let req = a.request(&b, p.id, Direction::Push).await;
    let err = transfer::prepare(&mut conn, &a.shared, req)
        .await
        .unwrap_err();
    assert!(err.to_string().contains("not the computer"), "{err}");
}

#[cfg(unix)]
#[tokio::test]
async fn pull_continues_when_clearing_old_temp_files_fails() {
    use std::os::unix::fs::PermissionsExt;
    let (a, b) = pair_full().await;
    let theirs = b.project_dir("beds");
    write(&theirs, "plan.txt", "new");
    let p = b.add_project("Orchard", &[("beds", &theirs)]).await;
    let mut conn = a.open(&b).await;
    let req = a.request(&b, p.id, Direction::Pull).await;
    let preview = transfer::prepare(&mut conn, &a.shared, req).await.unwrap();
    // Appears after the preview, so only the sweep trips over it.
    let locked = a.dev().join("beds/locked");
    std::fs::create_dir_all(&locked).unwrap();
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let result = transfer::execute(&mut conn, &a.shared, preview, tx, Default::default()).await;
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).unwrap();
    let summary = result.unwrap();
    assert_eq!(summary.files, 1);
    assert_eq!(read(&a.dev().join("beds"), "plan.txt"), "new");
}

#[tokio::test]
async fn a_push_stops_when_the_receivers_folder_moved_after_the_preview() {
    let (a, b) = pair_full().await;
    let src = a.project_dir("app");
    write(&src, "a.txt", "a");
    let p = a.add_project("Garden", &[("app", &src)]).await;
    push(&a, &b, p.id, false).await;

    write(&src, "b.txt", "b");
    let mut conn = a.open(&b).await;
    let req = a.request(&b, p.id, Direction::Push).await;
    let preview = transfer::prepare(&mut conn, &a.shared, req).await.unwrap();
    // Someone on B points the folder somewhere else before A confirms.
    let moved = b.root().join("elsewhere");
    std::fs::create_dir_all(&moved).unwrap();
    let mut on_b = b.project(p.id).await.unwrap();
    on_b.folders[0].local_path = Some(moved.clone());
    b.put_project(on_b).await;

    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let err = transfer::execute(&mut conn, &a.shared, preview, tx, Default::default())
        .await
        .unwrap_err()
        .to_string();
    assert!(
        err.contains("changed since the preview. Preview again."),
        "{err}"
    );
    assert!(!moved.join("b.txt").exists());
    assert!(!moved.join("a.txt").exists());
}
