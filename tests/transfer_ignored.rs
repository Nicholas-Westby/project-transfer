//! Ignored folders cost a transfer nothing to look at: no scan reads inside
//! one, yet a preview still counts the files a removed folder takes along.

mod support;

use project_transfer::manifest::Change;
use project_transfer::transfer::Preview;
use std::path::Path;
use support::*;

fn removals(p: &Preview) -> Vec<Change> {
    let changes = p.folders.iter().flat_map(|f| &f.plan.changes);
    changes
        .filter(|c| matches!(c, Change::RemoveDir { .. }))
        .cloned()
        .collect()
}

fn old_folder(rel: &str, files: u64, ignored: u64) -> Change {
    Change::RemoveDir {
        rel: rel.into(),
        files,
        ignored,
    }
}

/// A folder only the receiving copy has, with three ignored files in it:
/// two inside `node_modules` and one beside it.
fn only_here(root: &Path) {
    write(root, "old/notes.txt", "n");
    write(root, "old/node_modules/x/1.js", "1");
    write(root, "old/node_modules/2.js", "2");
    write(root, "old/.DS_Store", "d");
}

#[tokio::test]
async fn a_push_counts_the_ignored_files_a_removed_folder_takes() {
    let (a, b) = pair_full().await;
    let src = a.project_dir("app");
    write(&src, "keep.txt", "k");
    let p = a.add_project("Garden", &[("app", &src)]).await;
    push(&a, &b, p.id, false).await;
    let dest = b.dev().join("app");
    only_here(&dest);
    let (preview, _) = push(&a, &b, p.id, false).await;
    assert_eq!(removals(&preview), [old_folder("old", 1, 3)]);
    assert_eq!(preview.counts().removed_files, 4);
    assert!(!dest.join("old").exists());
}

#[tokio::test]
async fn a_pull_counts_the_ignored_files_a_removed_folder_takes() {
    let (a, b) = pair_full().await;
    let theirs = b.project_dir("app");
    write(&theirs, "keep.txt", "k");
    let p = b.add_project("Garden", &[("app", &theirs)]).await;
    pull(&a, &b, p.id).await;
    let mine = a.dev().join("app");
    only_here(&mine);
    let (preview, _) = pull(&a, &b, p.id).await;
    assert_eq!(removals(&preview), [old_folder("old", 1, 3)]);
    assert!(!mine.join("old").exists());
}

/// A folder both copies have keeps its ignored folders, so nothing looks
/// inside them: here one can't be read at all.
#[cfg(unix)]
#[tokio::test]
async fn ignored_folders_in_a_folder_that_stays_are_never_read() {
    use std::os::unix::fs::PermissionsExt;
    let (a, b) = pair_full().await;
    let src = a.project_dir("app");
    write(&src, "old/notes.txt", "n");
    let p = a.add_project("Garden", &[("app", &src)]).await;
    push(&a, &b, p.id, false).await;
    let dest = b.dev().join("app");
    write(&dest, "old/node_modules/locked/2.js", "2");
    let locked = dest.join("old/node_modules/locked");
    let mode = |m| std::fs::Permissions::from_mode(m);
    std::fs::set_permissions(&locked, mode(0o000)).unwrap();
    write(&src, "old/notes.txt", "changed");
    let (preview, _) = push(&a, &b, p.id, false).await;
    std::fs::set_permissions(&locked, mode(0o755)).unwrap();
    assert!(removals(&preview).is_empty());
    assert_eq!(read(&dest, "old/notes.txt"), "changed");
    assert_eq!(read(&dest, "old/node_modules/locked/2.js"), "2");
}

/// A Mac or Linux disk takes names no request can carry, such as one with
/// a backslash, so the receiving computer counts such a folder as it scans.
#[cfg(unix)]
#[tokio::test]
async fn a_push_counts_an_ignored_folder_whose_path_cannot_travel() {
    let (a, b) = pair_full().await;
    let src = a.project_dir("app");
    write(&src, "keep.txt", "k");
    let p = a.add_project("Garden", &[("app", &src)]).await;
    push(&a, &b, p.id, false).await;
    let dest = b.dev().join("app");
    write(&dest, "gone/odd\\name/node_modules/x.js", "x");
    let (preview, summary) = push(&a, &b, p.id, false).await;
    assert_eq!(removals(&preview), [old_folder("gone", 0, 1)]);
    assert!(summary.failures.is_empty(), "{:?}", summary.failures);
    assert!(!dest.join("gone").exists());
}

/// A pull counts the ignored folders in its own copy, whose names never
/// travel in a request, so it takes any name, one with a backslash too.
#[cfg(unix)]
#[tokio::test]
async fn a_pull_counts_an_ignored_folder_whose_path_cannot_travel() {
    let (a, b) = pair_full().await;
    let theirs = b.project_dir("app");
    write(&theirs, "keep.txt", "k");
    let p = b.add_project("Garden", &[("app", &theirs)]).await;
    pull(&a, &b, p.id).await;
    let mine = a.dev().join("app");
    write(&mine, "gone/odd\\name/node_modules/x.js", "x");
    let (preview, summary) = pull(&a, &b, p.id).await;
    assert_eq!(removals(&preview), [old_folder("gone", 0, 1)]);
    assert!(summary.failures.is_empty(), "{:?}", summary.failures);
    assert!(!mine.join("gone").exists());
}

/// Nothing reads the counts of the copy a push sends from, so its scan
/// doesn't look inside an ignored folder even when no request can name it:
/// here one can't be read at all.
#[cfg(unix)]
#[tokio::test]
async fn a_push_never_reads_inside_the_ignored_folders_it_sends_from() {
    use std::os::unix::fs::PermissionsExt;
    let (a, b) = pair_full().await;
    let src = a.project_dir("app");
    write(&src, "keep.txt", "k");
    write(&src, "odd\\name/node_modules/x.js", "x");
    let locked = src.join("odd\\name/node_modules");
    let mode = |m| std::fs::Permissions::from_mode(m);
    std::fs::set_permissions(&locked, mode(0o000)).unwrap();
    let p = a.add_project("Garden", &[("app", &src)]).await;
    let (_, summary) = push(&a, &b, p.id, false).await;
    std::fs::set_permissions(&locked, mode(0o755)).unwrap();
    assert!(summary.failures.is_empty(), "{:?}", summary.failures);
    assert_eq!(read(&b.dev().join("app"), "keep.txt"), "k");
}

/// A folder nobody can read inside `old/node_modules`, which goes with
/// `old`, stops the preview. The error names that folder, not the readable
/// `old/node_modules` around it, and says what to do.
#[cfg(unix)]
mod unreadable {
    use super::*;
    use project_transfer::model::{Direction, ProjectId};
    use project_transfer::transfer;
    use std::os::unix::fs::PermissionsExt;

    /// Previews a transfer into `dest` with that folder there. Returns the
    /// error it stops with.
    async fn preview(
        a: &Instance,
        b: &Instance,
        id: ProjectId,
        d: Direction,
        dest: &Path,
    ) -> String {
        write(dest, "old/node_modules/locked/2.js", "2");
        let locked = dest.join("old/node_modules/locked");
        let mode = |m| std::fs::Permissions::from_mode(m);
        std::fs::set_permissions(&locked, mode(0o000)).unwrap();
        let mut conn = a.open(b).await;
        let req = a.request(b, id, d).await;
        let got = transfer::prepare(&mut conn, &a.shared, req).await;
        std::fs::set_permissions(&locked, mode(0o755)).unwrap();
        format!("{:#}", got.unwrap_err())
    }

    /// The pushing computer shows the refusal, so it names the computer the
    /// folder is on as well as the folder.
    #[tokio::test]
    async fn a_push_names_the_folder_and_the_computer_it_is_on() {
        let (a, b) = pair_full().await;
        let src = a.project_dir("app");
        write(&src, "keep.txt", "k");
        let p = a.add_project("Garden", &[("app", &src)]).await;
        push(&a, &b, p.id, false).await;
        let dest = b.dev().join("app");
        let err = preview(&a, &b, p.id, Direction::Push, &dest).await;
        let named = b.shared.settings.read().await.name.clone();
        let folder = dest.join("old/node_modules/locked");
        let what = format!("Could not read {}: ", folder.display());
        let todo = format!(". Make it readable on {named}, or remove it, then preview again.");
        assert!(err.starts_with(&what) && err.ends_with(&todo), "{err}");
    }

    #[tokio::test]
    async fn a_pull_names_the_folder_here() {
        let (a, b) = pair_full().await;
        let theirs = b.project_dir("app");
        write(&theirs, "keep.txt", "k");
        let p = b.add_project("Garden", &[("app", &theirs)]).await;
        pull(&a, &b, p.id).await;
        let mine = a.dev().join("app");
        let err = preview(&a, &b, p.id, Direction::Pull, &mine).await;
        let folder = mine.join("old/node_modules/locked");
        let what = format!("Could not read {}: ", folder.display());
        let todo = ". Make it readable on this computer, or remove it, then preview again.";
        assert!(err.starts_with(&what) && err.ends_with(todo), "{err}");
    }
}

#[tokio::test]
async fn a_pull_clears_a_temp_file_an_unfinished_transfer_left() {
    let (a, b) = pair_full().await;
    let theirs = b.project_dir("app");
    write(&theirs, "src/main.rs", "fn main() {}");
    let p = b.add_project("Garden", &[("app", &theirs)]).await;
    pull(&a, &b, p.id).await;
    let mine = a.dev().join("app");
    write(&mine, "src/main.rs.0a1b2c3d.pt-tmp", "half");
    pull(&a, &b, p.id).await;
    assert!(temps(&mine).is_empty(), "{:?}", temps(&mine));
}

/// Only a transfer that sends everything writes inside an ignored folder,
/// so only such a transfer clears temp files there.
#[tokio::test]
async fn a_transfer_leaves_temp_files_inside_ignored_folders_alone() {
    let (a, b) = pair_full().await;
    let src = a.project_dir("app");
    write(&src, "keep.txt", "k");
    write(&src, "node_modules/x.js", "x");
    let p = a.add_project("Garden", &[("app", &src)]).await;
    push(&a, &b, p.id, true).await;
    let dest = b.dev().join("app");
    write(&dest, "node_modules/x.js.0a1b2c3d.pt-tmp", "half");
    write(&dest, "keep.txt.0a1b2c3d.pt-tmp", "half");
    push(&a, &b, p.id, false).await;
    assert_eq!(temps(&dest), ["node_modules/x.js.0a1b2c3d.pt-tmp"]);
    push(&a, &b, p.id, true).await;
    assert!(temps(&dest).is_empty(), "{:?}", temps(&dest));
}
