//! Where pushed and pulled folders land, and the warnings a preview gives
//! before anything already on the destination is removed.

mod support;

use project_transfer::manifest::Change;
use project_transfer::model::{Direction, Folder};
use project_transfer::transfer;
use support::*;

#[tokio::test]
async fn replacing_a_folder_warns_about_its_ignored_files() {
    let (a, b) = pair_full().await;
    let src = a.project_dir("app");
    write(&src, "x/a.txt", "a");
    let p = a.add_project("Garden", &[("app", &src)]).await;
    push(&a, &b, p.id, false).await;
    write(&b.dev().join("app"), "x/node_modules/m.js", "m");
    std::fs::remove_dir_all(src.join("x")).unwrap();
    write(&src, "x", "now a file");
    let mut conn = a.open(&b).await;
    let preview = transfer::prepare(
        &mut conn,
        &a.shared,
        a.request(&b, p.id, Direction::Push).await,
    )
    .await
    .unwrap();
    assert_eq!(
        preview.warnings,
        ["Replacing folder `x` with a file removes 2 files inside it (1 of them ignored)."]
    );
    assert_eq!(preview.counts().removed_files, 2);
}

#[tokio::test]
async fn replacing_a_folder_of_tracked_files_shows_them_as_removed() {
    let (a, b) = pair_full().await;
    let src = a.project_dir("app");
    write(&src, "x/a.txt", "a");
    write(&src, "x/deep/b.txt", "b");
    let p = a.add_project("Garden", &[("app", &src)]).await;
    push(&a, &b, p.id, false).await;
    std::fs::remove_dir_all(src.join("x")).unwrap();
    write(&src, "x", "now a file");
    let (preview, _) = push(&a, &b, p.id, false).await;
    assert_eq!(
        preview.warnings,
        ["Replacing folder `x` with a file removes 2 files inside it."]
    );
    let c = preview.counts();
    assert_eq!((c.changed, c.removed_files), (1, 2));
    assert_eq!(read(&b.dev().join("app"), "x"), "now a file");
}

#[tokio::test]
async fn default_path_never_lands_in_another_projects_folder() {
    let (a, b) = pair_full().await;
    let src = a.project_dir("app");
    write(&src, "new.txt", "pushed");
    let p = a.add_project("Garden", &[("app", &src)]).await;
    let taken = b.dev().join("app");
    write(&taken, "theirs.txt", "keep me");
    b.add_project("Other", &[("app", &taken)]).await;

    let (preview, _) = push(&a, &b, p.id, false).await;
    let dest = b.dev().join("app 2");
    assert_eq!(preview.folders[0].dest_path, dest.display().to_string());
    assert!(
        preview.folders[0]
            .plan
            .changes
            .iter()
            .all(|c| !matches!(c, Change::RemoveFile(_)))
    );
    assert_eq!(read(&taken, "theirs.txt"), "keep me");
    assert_eq!(read(&dest, "new.txt"), "pushed");
    let on_b = b.project(p.id).await.unwrap();
    assert_eq!(on_b.folders[0].local_path, Some(dest));
}

#[tokio::test]
async fn an_unrelated_existing_folder_is_warned_about() {
    let (a, b) = pair_full().await;
    let src = a.project_dir("app");
    write(&src, "new.txt", "pushed");
    let p = a.add_project("Garden", &[("app", &src)]).await;
    let existing = b.dev().join("app");
    write(&existing, "stray.txt", "s");
    let mut conn = a.open(&b).await;
    let req = a.request(&b, p.id, Direction::Push).await;
    let preview = transfer::prepare(&mut conn, &a.shared, req).await.unwrap();
    let b_name = b.shared.settings.read().await.name.clone();
    assert_eq!(preview.folders[0].dest_path, existing.display().to_string());
    assert_eq!(
        preview.warnings,
        [format!(
            "`{}` already exists on {b_name} and isn't part of this project yet. Files there \
             that aren't in the source will be removed.",
            existing.display()
        )]
    );

    // Pulling into an unrelated folder here warns the same way.
    let theirs = b.project_dir("beds");
    write(&theirs, "plan.txt", "x");
    let q = b.add_project("Orchard", &[("beds", &theirs)]).await;
    write(&a.dev().join("beds"), "mine.txt", "m");
    let req = a.request(&b, q.id, Direction::Pull).await;
    let preview = transfer::prepare(&mut conn, &a.shared, req).await.unwrap();
    let a_name = a.shared.settings.read().await.name.clone();
    assert!(
        preview.warnings[0].starts_with(&format!(
            "`{}` already exists on {a_name}",
            a.dev().join("beds").display()
        )),
        "{:?}",
        preview.warnings
    );
}

#[tokio::test]
async fn existing_local_path_on_the_receiver_is_kept() {
    let (a, b) = pair_full().await;
    let src = a.project_dir("app");
    write(&src, "f", "f");
    let p = a.add_project("Garden", &[("app", &src)]).await;
    let elsewhere = b.root().join("Elsewhere/my-app");
    let mut theirs = p.clone();
    theirs.name = "Renamed on B".into();
    theirs.folders[0].local_path = Some(elsewhere.clone());
    b.put_project(theirs).await;

    let (preview, _) = push(&a, &b, p.id, false).await;
    assert_eq!(
        preview.folders[0].dest_path,
        elsewhere.display().to_string()
    );
    assert_eq!(read(&elsewhere, "f"), "f");
    let on_b = b.project(p.id).await.unwrap();
    assert_eq!(on_b.name, "Renamed on B");
    assert_eq!(
        on_b.folders[0].local_path.as_deref(),
        Some(elsewhere.as_path())
    );
}

#[tokio::test]
async fn a_new_folder_on_a_known_project_gets_the_default_path() {
    let (a, b) = pair_full().await;
    let app = a.project_dir("app");
    write(&app, "f", "f");
    let p = a.add_project("Garden", &[("app", &app)]).await;
    push(&a, &b, p.id, false).await;
    let docs = a.project_dir("docs");
    write(&docs, "d", "d");
    let mut more = a.project(p.id).await.unwrap();
    more.folders.push(Folder {
        id: uuid::Uuid::new_v4(),
        name: "docs".into(),
        local_path: Some(docs),
    });
    a.put_project(more).await;
    push(&a, &b, p.id, false).await;
    let on_b = b.project(p.id).await.unwrap();
    assert_eq!(on_b.folders[0].local_path, Some(b.dev().join("app")));
    assert_eq!(
        on_b.folders[1].local_path,
        Some(b.dev().join("Garden/docs"))
    );
    assert_eq!(read(&b.dev().join("Garden/docs"), "d"), "d");
}

#[tokio::test]
async fn a_project_name_is_a_label_not_a_folder_name() {
    let (a, b) = pair_full().await;
    let (app, docs) = (a.project_dir("app"), a.project_dir("docs"));
    write(&app, "a.txt", "a");
    write(&docs, "d", "d");
    let mut p = a
        .add_project("Garden", &[("app", &app), ("docs", &docs)])
        .await;
    p.name = "What: Next?".into();
    a.put_project(p.clone()).await;
    let preview = push(&a, &b, p.id, false).await.0;
    assert!(preview.warnings.is_empty(), "{:?}", preview.warnings);
    let on_b = b.project(p.id).await.unwrap();
    assert_eq!(on_b.name, "What: Next?");
    assert_eq!(read(&b.dev().join("What Next/app"), "a.txt"), "a");
}

// Only unix file systems allow a backslash in a name.
#[cfg(unix)]
#[tokio::test]
async fn a_destination_file_with_a_backslash_is_left_and_the_push_finishes() {
    let (a, b) = pair_full().await;
    let src = a.project_dir("app");
    write(&src, "keep.txt", "a");
    let p = a.add_project("Garden", &[("app", &src)]).await;
    push(&a, &b, p.id, false).await;
    write(&b.dev().join("app"), "odd\\name.txt", "b");
    write(&src, "new.txt", "n");
    let (preview, summary) = push(&a, &b, p.id, false).await;
    let skipped: Vec<&str> = preview.folders[0]
        .skipped
        .iter()
        .map(|(r, _)| r.as_str())
        .collect();
    assert_eq!(skipped, ["odd\\name.txt"]);
    assert!(summary.failures.is_empty(), "{:?}", summary.failures);
    assert_eq!(read(&b.dev().join("app"), "new.txt"), "n");
    assert_eq!(read(&b.dev().join("app"), "odd\\name.txt"), "b");
}

/// Made on each computer separately: a push goes into the folder the other
/// computer already has, never a " 2" beside it, and the ids then agree.
#[tokio::test]
async fn a_push_lands_in_the_same_named_project_made_there() {
    let (a, b) = pair_full().await;
    let src = a.project_dir("garden-planner");
    write(&src, "beds.txt", "newest");
    let mine = a
        .add_project("Garden Planner", &[("garden-planner", &src)])
        .await;
    let theirs_dir = b.dev().join("garden-planner");
    write(&theirs_dir, "beds.txt", "older");
    let theirs = b
        .add_project("garden planner", &[("garden-planner", &theirs_dir)])
        .await;

    let (preview, _) = push(&a, &b, mine.id, false).await;
    assert_eq!(preview.request.project, theirs.id);
    assert_eq!(
        preview.folders[0].dest_path,
        theirs_dir.display().to_string()
    );
    assert_eq!(read(&theirs_dir, "beds.txt"), "newest");
    assert!(!b.dev().join("garden-planner 2").exists());
    assert!(a.project(mine.id).await.is_none());
    let now = a.project(theirs.id).await.expect("took their id");
    assert_eq!(now.folders[0].id, theirs.folders[0].id);
    assert_eq!(now.folders[0].local_path, Some(src));
    assert_eq!(b.project(theirs.id).await.unwrap().folders.len(), 1);
}

#[tokio::test]
async fn a_pull_lands_in_the_same_named_project_made_here() {
    let (a, b) = pair_full().await;
    let here = a.project_dir("tide-tables");
    write(&here, "march.csv", "older");
    let mine = a
        .add_project("Tide Tables", &[("tide-tables", &here)])
        .await;
    let there = b.project_dir("tide-tables");
    write(&there, "march.csv", "newest");
    let theirs = b
        .add_project("Tide Tables", &[("tide-tables", &there)])
        .await;

    let (preview, _) = pull(&a, &b, mine.id).await;
    assert_eq!(preview.request.project, theirs.id);
    assert!(preview.warnings[0].starts_with("Matched with the project `Tide Tables`"));
    assert_eq!(read(&here, "march.csv"), "newest");
    let all = a.shared.projects.read().await.clone();
    assert_eq!(all.len(), 1, "no second project: {all:?}");
    assert_eq!(all[0].folders[0].local_path, Some(here));
    assert_eq!(all[0].folders[0].id, theirs.folders[0].id);
}

/// The match is saved only when the transfer runs.
#[tokio::test]
async fn a_cancelled_preview_leaves_the_project_as_it_was() {
    let (a, b) = pair_full().await;
    let here = a.project_dir("seed-catalog");
    let mine = a
        .add_project("Seed Catalog", &[("seed-catalog", &here)])
        .await;
    let there = b.project_dir("seed-catalog");
    b.add_project("Seed Catalog", &[("seed-catalog", &there)])
        .await;
    let mut conn = a.open(&b).await;
    let req = a.request(&b, mine.id, Direction::Push).await;
    let preview = transfer::prepare(&mut conn, &a.shared, req).await.unwrap();
    assert!(preview.link.is_some());
    assert_eq!(a.project(mine.id).await.as_ref(), Some(&mine));
    assert_eq!(a.shared.store.load_projects().unwrap(), vec![mine]);
}

/// Same project name, differently named folders: the other computer's
/// folder arrives beside this one's instead of mirroring over it.
#[tokio::test]
async fn differently_named_folders_are_never_mirrored_onto_each_other() {
    let (a, b) = pair_full().await;
    let here = a.project_dir("notes");
    write(&here, "mine.txt", "keep me");
    let mine = a.add_project("Notes", &[("notes", &here)]).await;
    let there = b.project_dir("work-notes");
    write(&there, "theirs.txt", "t");
    b.add_project("Notes", &[("work-notes", &there)]).await;

    let (preview, _) = pull(&a, &b, mine.id).await;
    assert_eq!(read(&here, "mine.txt"), "keep me");
    let dest = a.dev().join("Notes").join("work-notes");
    assert_eq!(preview.folders[0].dest_path, dest.display().to_string());
    assert_eq!(read(&dest, "theirs.txt"), "t");
}
