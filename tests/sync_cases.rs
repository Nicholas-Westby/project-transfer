//! Sync in the cases where the look it takes first could go wrong: its
//! preview must be the one its direction gives for a pull and for a
//! project matched by name, and a folder pointed at a new, empty place is
//! filled, not emptied on the other computer.

mod support;

use project_transfer::model::{Direction, Folder};
use project_transfer::transfer::sync_count::SyncChoice;
use project_transfer::transfer::{self, Preview};
use support::*;

/// A sync from A to B of `project`, as the Sync button asks for it.
async fn sync_preview(a: &Instance, b: &Instance, project: uuid::Uuid) -> (Preview, SyncChoice) {
    let mut conn = a.open(b).await;
    let req = a.request(b, project, Direction::Push).await;
    transfer::prepare_sync(&mut conn, &a.shared, req)
        .await
        .unwrap()
}

/// The preview a plain push or pull of `project` gives.
async fn prepare_only(a: &Instance, b: &Instance, project: uuid::Uuid, d: Direction) -> Preview {
    let mut conn = a.open(b).await;
    let req = a.request(b, project, d).await;
    transfer::prepare(&mut conn, &a.shared, req).await.unwrap()
}

/// Everything the two previews show, apart from the sync's reason.
fn same(synced: &Preview, plain: &Preview) {
    assert_eq!(synced.request.direction, plain.request.direction);
    assert_eq!(synced.folders, plain.folders);
    assert_eq!(synced.left_out, plain.left_out);
    assert_eq!(synced.warnings, plain.warnings);
    assert_eq!(synced.link, plain.link);
    assert_eq!(synced.description, plain.description);
}

#[tokio::test]
async fn a_matched_project_previews_what_its_direction_does() {
    let (a, b) = pair_full().await;
    let (here, there) = (a.project_dir("app"), b.project_dir("app"));
    write(&here, "a.txt", "made here");
    write(&there, "a.txt", "made on the other computer");
    let p = a.add_project("Garden", &[("app", &here)]).await;
    b.add_project("Garden", &[("app", &there)]).await;
    let (synced, choice) = sync_preview(&a, &b, p.id).await;
    assert!(synced.link.is_some(), "matched by name");
    same(&synced, &prepare_only(&a, &b, p.id, choice.direction).await);
}

#[tokio::test]
async fn a_pulled_sync_previews_what_a_pull_does() {
    let (a, b) = pair_full().await;
    let app = b.project_dir("app");
    write(&app, "main.rs", "fn main() {}");
    write(&app, "lib.rs", "pub fn grow() {}");
    let docs = b.home_dir("notes/docs");
    write(&docs, "readme.md", "# Garden");
    let p = b
        .add_project("Garden", &[("app", &app), ("docs", &docs)])
        .await;
    pull(&a, &b, p.id).await;
    let folder = |name: &str, at| Folder {
        id: uuid::Uuid::new_v4(),
        name: name.into(),
        local_path: at,
    };
    // Here: renamed, docs no longer set up, and a folder only here.
    let mut mine = a.project(p.id).await.unwrap();
    mine.name = "Garden here".into();
    mine.folders[0].name = "app-here".into();
    mine.folders[1].local_path = None;
    let scratch = a.project_dir("scratch");
    write(&scratch, "s.txt", "scratch");
    mine.folders.push(folder("scratch", Some(scratch)));
    a.put_project(mine).await;
    // There: a new folder, one not set up, and five new files.
    let mut theirs = b.project(p.id).await.unwrap();
    let assets = b.project_dir("assets");
    write(&assets, "logo.svg", "<svg/>");
    theirs.folders.push(folder("assets", Some(assets)));
    theirs.folders.push(folder("unset", None));
    b.put_project(theirs).await;
    for n in 0..5 {
        write(&app, &format!("new{n}.rs"), "// new");
    }
    let (synced, choice) = sync_preview(&a, &b, p.id).await;
    assert_eq!(choice.direction, Direction::Pull);
    same(&synced, &prepare_only(&a, &b, p.id, Direction::Pull).await);
    let left_out: Vec<&str> = synced.left_out.iter().map(|l| l.name.as_str()).collect();
    assert!(
        left_out.contains(&"unset") && left_out.contains(&"scratch"),
        "{left_out:?}"
    );
}

#[tokio::test]
async fn an_empty_folder_here_is_filled_not_emptied_there() {
    let (a, b) = pair_full().await;
    let src = a.project_dir("app");
    for n in 0..3 {
        write(&src, &format!("f{n}.txt"), "on both");
    }
    let p = a.add_project("Garden", &[("app", &src)]).await;
    push(&a, &b, p.id, false).await;
    // The folder here now points at a new, empty place.
    let mut p = a.project(p.id).await.unwrap();
    p.folders[0].local_path = Some(a.project_dir("fresh"));
    a.put_project(p.clone()).await;
    let (preview, choice) = sync_preview(&a, &b, p.id).await;
    assert_eq!(choice.direction, Direction::Pull);
    assert_eq!(preview.counts().removed_files, 0);
}
