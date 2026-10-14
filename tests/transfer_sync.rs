//! Sync between two paired instances on 127.0.0.1: which way it goes, and
//! that its preview is the one that direction gives.

mod support;

use project_transfer::model::{Direction, Project};
use project_transfer::net::{Connection, pair_finish, pair_start};
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

/// A's project `Garden` with `a.txt`, `b.txt` and `c.txt`, pushed to B so
/// both have the same files and the push is A's last transfer.
async fn pushed() -> (Instance, Instance, Project, i64) {
    let (a, b) = pair_full().await;
    let src = a.project_dir("app");
    for name in ["a.txt", "b.txt", "c.txt"] {
        write(&src, name, "first");
        set_mtime(&src.join(name), 1_000_000);
    }
    let p = a.add_project("Garden", &[("app", &src)]).await;
    push(&a, &b, p.id, false).await;
    let at = a.project(p.id).await.unwrap().last_transfer.unwrap().at_ms;
    (a, b, p, at)
}

#[tokio::test]
async fn the_side_with_more_newer_changes_wins() {
    let (a, b, p, at) = pushed().await;
    let (here, there) = (a.project_dir("app"), b.dev().join("app"));
    write(&here, "a.txt", "changed here");
    set_mtime(&here.join("a.txt"), at + 60_000);
    write(&here, "b.txt", "changed here");
    set_mtime(&here.join("b.txt"), at + 60_000);
    write(&there, "c.txt", "changed there");
    set_mtime(&there.join("c.txt"), at + 120_000);
    let (preview, choice) = sync_preview(&a, &b, p.id).await;
    assert_eq!(choice.direction, Direction::Push);
    assert_eq!((choice.tally.here, choice.tally.there), (2, 1));
    assert_eq!(preview.request.direction, Direction::Push);
    let c = preview.counts();
    // The push writes all three; c.txt is newer on B.
    assert_eq!((c.changed, c.dest_newer), (3, 1));

    write(&there, "d.txt", "new there");
    set_mtime(&there.join("d.txt"), at + 180_000);
    write(&there, "e.txt", "new there");
    set_mtime(&there.join("e.txt"), at + 180_000);
    let (preview, choice) = sync_preview(&a, &b, p.id).await;
    assert_eq!(choice.direction, Direction::Pull);
    assert_eq!((choice.tally.here, choice.tally.there), (2, 3));
    assert_eq!(preview.request.direction, Direction::Pull);
}

#[tokio::test]
async fn a_file_removed_there_since_the_last_transfer_counts_for_there() {
    let (a, b, p, at) = pushed().await;
    let (here, there) = (a.project_dir("app"), b.dev().join("app"));
    std::fs::remove_file(there.join("a.txt")).unwrap();
    std::fs::remove_file(there.join("b.txt")).unwrap();
    write(&here, "c.txt", "changed here");
    set_mtime(&here.join("c.txt"), at + 60_000);
    let (preview, choice) = sync_preview(&a, &b, p.id).await;
    assert_eq!((choice.tally.here, choice.tally.there), (1, 2));
    assert_eq!(choice.direction, Direction::Pull);
    // Pulling removes them here too.
    assert_eq!(preview.counts().removed_files, 2);
}

#[tokio::test]
async fn a_file_another_computer_brought_counts_for_the_computer_that_has_it() {
    let (a, b) = pair_full().await;
    // C is paired with B only.
    let c = Instance::start(perms(true, true)).await;
    let mut conn = Connection::open(b.addr, &c.shared).await.unwrap();
    let all = perms(true, true);
    let started = pair_start(&mut conn, &c.shared, all, all).await.unwrap();
    pair_finish(&mut conn, &c.shared, started, async { true }, || {})
        .await
        .unwrap();
    let src = a.project_dir("app");
    write(&src, "main.rs", "fn main() {}");
    set_mtime(&src.join("main.rs"), 1_000_000);
    let p = a.add_project("Garden", &[("app", &src)]).await;
    push(&a, &b, p.id, false).await;
    // C pulls, adds a file that keeps an old time, and pushes it to B.
    pull(&c, &b, p.id).await;
    let on_c = c.project(p.id).await.unwrap().folders[0]
        .local_path
        .clone()
        .unwrap();
    write(&on_c, "old-notes.md", "from long ago");
    set_mtime(&on_c.join("old-notes.md"), 2_000_000);
    push(&c, &b, p.id, false).await;
    let (preview, choice) = sync_preview(&a, &b, p.id).await;
    assert_eq!(choice.direction, Direction::Pull);
    assert_eq!((choice.tally.here, choice.tally.there), (0, 1));
    assert_eq!(preview.counts().removed_files, 0);
}

#[tokio::test]
async fn the_preview_is_the_one_its_direction_gives() {
    let (a, b, p, at) = pushed().await;
    let here = a.project_dir("app");
    write(&here, "a.txt", "changed here");
    set_mtime(&here.join("a.txt"), at + 60_000);
    // Same size, new time: only hashing tells it apart from a change.
    set_mtime(&here.join("b.txt"), at + 60_000);
    write(&here, "new/f.txt", "new");
    let (synced, choice) = sync_preview(&a, &b, p.id).await;
    assert_eq!(choice.direction, Direction::Push);
    assert_eq!((choice.tally.here, choice.tally.there), (2, 0));
    let (pushed, _) = push(&a, &b, p.id, false).await;
    assert_eq!(synced.folders, pushed.folders);
    assert_eq!(synced.left_out, pushed.left_out);
    assert_eq!(synced.warnings, pushed.warnings);
    assert_eq!(synced.counts().timestamp_only, 1);
}

#[tokio::test]
async fn a_project_the_other_computer_lacks_is_pushed() {
    let (a, b) = pair_full().await;
    let src = a.project_dir("app");
    write(&src, "main.rs", "fn main() {}");
    let p = a.add_project("Garden", &[("app", &src)]).await;
    let (preview, choice) = sync_preview(&a, &b, p.id).await;
    assert_eq!(choice.direction, Direction::Push);
    assert_eq!((choice.tally.here, choice.tally.there), (1, 0));
    assert_eq!(preview.counts().added, 1);
}

#[tokio::test]
async fn send_everything_does_not_turn_a_sync_around() {
    let (a, b) = pair_full().await;
    let src = a.project_dir("app");
    write(&src, "main.rs", "fn main() {}");
    set_mtime(&src.join("main.rs"), 1_000_000);
    for n in 0..3 {
        let rel = format!("node_modules/pkg{n}/index.js");
        write(&src, &rel, "module.exports = {};");
        // npm gives package files a fixed time in 1985.
        set_mtime(&src.join(&rel), 499_162_500_000);
    }
    let p = a.add_project("Garden", &[("app", &src)]).await;
    push(&a, &b, p.id, false).await;
    write(&src, "main.rs", "fn main() { grow(); }");
    let mut conn = a.open(&b).await;
    let mut req = a.request(&b, p.id, Direction::Push).await;
    req.send_everything = true;
    let (preview, choice) = transfer::prepare_sync(&mut conn, &a.shared, req)
        .await
        .unwrap();
    assert_eq!(choice.direction, Direction::Push);
    assert_eq!((choice.tally.here, choice.tally.there), (1, 0));
    // Four folders and three files arrive; nothing is removed.
    let c = preview.counts();
    assert_eq!((c.added, c.removed_files), (7, 0));
}

#[tokio::test]
async fn syncing_needs_both_permissions() {
    let missing = [
        (perms(true, false), "pull from it, which syncing needs"),
        (perms(false, true), "push to it, which syncing needs"),
        (
            perms(false, false),
            "push to it or pull from it, which syncing needs",
        ),
    ];
    for (allows, says) in missing {
        let (a, b) = pair_with(allows).await;
        let src = a.project_dir("app");
        write(&src, "main.rs", "fn main() {}");
        let p = a.add_project("Garden", &[("app", &src)]).await;
        let mut conn = a.open(&b).await;
        let req = a.request(&b, p.id, Direction::Push).await;
        let err = transfer::prepare_sync(&mut conn, &a.shared, req)
            .await
            .unwrap_err();
        let text = format!("{err:#}");
        assert!(text.contains(says), "{text}");
    }
}

/// A file made after a preview read the folders never travels with its
/// transfer, so the preview notes when it first asked for a folder scan.
#[tokio::test]
async fn a_preview_notes_when_it_first_asked_for_a_scan() {
    let (a, b, p, _) = pushed().await;
    let mut conn = a.open(&b).await;
    assert_eq!(conn.scanned_ms(), None);
    let before = transfer::projects::now_ms();
    let req = a.request(&b, p.id, Direction::Push).await;
    transfer::prepare(&mut conn, &a.shared, req).await.unwrap();
    let at = conn.scanned_ms().expect("the preview asked for a scan");
    assert!((before..=transfer::projects::now_ms()).contains(&at));
}

#[tokio::test]
async fn a_transfer_here_with_another_computer_stops_removals_being_inferred() {
    let (a, b, p, _) = pushed().await;
    // A also moves the project with C, so its last transfer is not with B.
    let c = Instance::start(perms(true, true)).await;
    let mut conn = Connection::open(c.addr, &a.shared).await.unwrap();
    let all = perms(true, true);
    let started = pair_start(&mut conn, &a.shared, all, all).await.unwrap();
    pair_finish(&mut conn, &a.shared, started, async { true }, || {})
        .await
        .unwrap();
    push(&a, &c, p.id, false).await;
    // B still has a.txt, which A no longer has; nothing says A removed it.
    std::fs::remove_file(a.project_dir("app").join("a.txt")).unwrap();
    let (preview, choice) = sync_preview(&a, &b, p.id).await;
    assert_eq!((choice.tally.here, choice.tally.there), (0, 1));
    assert_eq!(choice.direction, Direction::Pull);
    assert_eq!(preview.counts().added, 1);
}

#[tokio::test]
async fn a_file_made_while_the_last_preview_was_open_counts_where_it_was_made() {
    let (a, b, p, _) = pushed().await;
    let (here, there) = (a.project_dir("app"), b.dev().join("app"));
    // A push is previewed, a file is made here, then the push is confirmed.
    write(&here, "a.txt", "second");
    let mut conn = a.open(&b).await;
    let req = a.request(&b, p.id, Direction::Push).await;
    let preview = transfer::prepare(&mut conn, &a.shared, req).await.unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    write(&here, "notes.md", "made while the preview was open");
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    transfer::execute(&mut conn, &a.shared, preview, tx, Default::default())
        .await
        .unwrap();
    let theirs = b.project(p.id).await.unwrap().last_transfer.unwrap();
    assert!(theirs.scanned_ms.unwrap() < mtime(&here.join("notes.md")));
    write(&here, "b.txt", "changed here");
    write(&there, "c.txt", "changed there");
    let (preview, choice) = sync_preview(&a, &b, p.id).await;
    assert_eq!((choice.tally.here, choice.tally.there), (2, 1));
    assert_eq!(choice.direction, Direction::Push);
    assert_eq!(preview.counts().removed_files, 0);
}
