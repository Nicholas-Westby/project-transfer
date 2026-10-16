//! What a computer answers about the ignored folders in its copy: an older
//! version's folder scan still has them all counted, and a count asked for
//! a path it can't safely reach is refused.

mod support;

use project_transfer::ignore_rules::{DEFAULT_IGNORES, IgnoreSpec};
use project_transfer::model::Project;
use project_transfer::net::Connection;
use project_transfer::protocol::{FolderScan, Request, Response};
use support::*;

fn manifest_request(p: &Project) -> serde_json::Value {
    let ignore = IgnoreSpec {
        patterns: DEFAULT_IGNORES.iter().map(|s| s.to_string()).collect(),
        ..Default::default()
    };
    let ask = Request::Manifest {
        project: p.id,
        folder: p.primary,
        ignore,
        folder_name: "app".into(),
        project_name: p.name.clone(),
        multi_folder: false,
        from_os: None,
        home_hint: None,
        name_ignored_dirs: true,
    };
    serde_json::to_value(ask).unwrap()
}

/// Asks for a folder scan as a version from before ignored folders were
/// named, which leaves `name_ignored_dirs` out.
async fn older_scan(conn: &mut Connection, p: &Project) -> FolderScan {
    let mut older = manifest_request(p);
    older["Manifest"]
        .as_object_mut()
        .unwrap()
        .remove("name_ignored_dirs");
    let body = serde_json::to_vec(&older).unwrap();
    let mut framed = (body.len() as u32).to_be_bytes().to_vec();
    framed.extend(body);
    conn.send_raw(&framed).await.unwrap();
    match conn.recv().await.unwrap() {
        Response::Manifest(scan) => scan,
        other => panic!("no folder scan: {other:?}"),
    }
}

/// Older versions don't ask for ignored folders by name, and their previews
/// count on the numbers including what those folders hold.
#[tokio::test]
async fn an_older_computer_still_gets_every_ignored_folder_counted() {
    let (a, b) = pair_full().await;
    let theirs = b.project_dir("app");
    write(&theirs, "keep.txt", "k");
    write(&theirs, "node_modules/x/1.js", "1");
    write(&theirs, "node_modules/2.js", "2");
    let p = b.add_project("Garden", &[("app", &theirs)]).await;
    let mut conn = a.open(&b).await;

    let scan = older_scan(&mut conn, &p).await;
    assert_eq!(scan.manifest.ignored_in_dir.get(""), Some(&2));
    assert!(scan.manifest.ignored_dirs.is_empty());

    let newer: Request = serde_json::from_value(manifest_request(&p)).unwrap();
    let Response::Manifest(scan) = conn.request(&newer).await.unwrap() else {
        panic!("no folder scan");
    };
    assert_eq!(scan.manifest.ignored_in_dir.get(""), None);
    assert_eq!(scan.manifest.ignored_dirs, ["node_modules"]);
}

/// A top-level name that starts like a drive can't travel in a request, so
/// the folder is counted as it is scanned, for an older computer too.
#[cfg(unix)]
#[tokio::test]
async fn an_older_computer_gets_an_ignored_folder_counted_whose_path_cannot_travel() {
    let (a, b) = pair_full().await;
    let theirs = b.project_dir("app");
    write(&theirs, "c:old/node_modules/x.js", "x");
    let p = b.add_project("Garden", &[("app", &theirs)]).await;
    let mut conn = a.open(&b).await;
    let scan = older_scan(&mut conn, &p).await;
    assert_eq!(scan.manifest.ignored_in_dir.get("c:old"), Some(&1));
    assert!(scan.manifest.ignored_dirs.is_empty());
}

#[tokio::test]
async fn counting_files_refuses_a_path_that_leaves_the_folder() {
    let (a, b) = pair_full().await;
    let theirs = b.project_dir("app");
    write(&theirs, "keep.txt", "k");
    let p = b.add_project("Garden", &[("app", &theirs)]).await;
    let mut conn = a.open(&b).await;
    let ask = Request::CountFiles {
        project: p.id,
        folder: p.primary,
        dirs: vec!["../app".into()],
    };
    match conn.request(&ask).await.unwrap() {
        Response::Refused { reason } => assert!(reason.contains("not allowed"), "{reason}"),
        other => panic!("{other:?}"),
    }
}

/// A link in the receiving copy could lead the count out of the folder.
#[cfg(unix)]
#[tokio::test]
async fn counting_files_refuses_a_folder_reached_through_a_link() {
    let (a, b) = pair_full().await;
    let theirs = b.project_dir("app");
    let elsewhere = b.project_dir("elsewhere");
    write(&elsewhere, "inner/x.js", "x");
    std::os::unix::fs::symlink(&elsewhere, theirs.join("link")).unwrap();
    let p = b.add_project("Garden", &[("app", &theirs)]).await;
    let mut conn = a.open(&b).await;
    let ask = Request::CountFiles {
        project: p.id,
        folder: p.primary,
        dirs: vec!["link/inner".into()],
    };
    match conn.request(&ask).await.unwrap() {
        Response::Refused { reason } => assert!(reason.contains("symbolic link"), "{reason}"),
        other => panic!("{other:?}"),
    }
}

/// The asking computer shows the refusal, so it names the computer that
/// hasn't scanned the folder rather than saying "this computer".
#[tokio::test]
async fn counting_files_in_a_folder_never_scanned_names_the_computer_asked() {
    let (a, b) = pair_full().await;
    let mut conn = a.open(&b).await;
    let ask = Request::CountFiles {
        project: uuid::Uuid::new_v4(),
        folder: uuid::Uuid::new_v4(),
        dirs: vec!["old/node_modules".into()],
    };
    let named = b.shared.settings.read().await.name.clone();
    match conn.request(&ask).await.unwrap() {
        Response::Refused { reason } => assert_eq!(
            reason,
            format!("{named} hasn't looked at that folder yet. Preview again.")
        ),
        other => panic!("{other:?}"),
    }
}
