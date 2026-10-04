//! Two paired instances on 127.0.0.1 pushing and pulling real folders.

mod support;

use project_transfer::manifest::Change;
use project_transfer::model::{Command, Direction};
use project_transfer::protocol::{Request, Response};
use project_transfer::transfer;
use support::*;

#[tokio::test]
async fn first_push_creates_everything_including_empty_dirs() {
    let (a, b) = pair_full().await;
    let src = a.project_dir("app");
    write(&src, "src/main.rs", "fn main() {}");
    write(&src, "README.md", "hi");
    std::fs::create_dir_all(src.join("empty/nested")).unwrap();
    let p = a.add_project("Garden", &[("app", &src)]).await;

    let (preview, summary) = push(&a, &b, p.id, false).await;
    let fp = &preview.folders[0];
    let dest = b.dev().join("app");
    assert_eq!(fp.dest_path, dest.display().to_string());
    assert!(fp.dest_will_be_created);
    // src, src/main.rs, README.md, empty, empty/nested
    assert_eq!(preview.counts().added, 5);
    assert_eq!(summary.files, 2);
    assert_eq!(summary.bytes, 14);
    assert!(summary.failures.is_empty(), "{:?}", summary.failures);

    assert_eq!(read(&dest, "src/main.rs"), "fn main() {}");
    assert_eq!(read(&dest, "README.md"), "hi");
    assert!(dest.join("empty/nested").is_dir());
    assert_eq!(
        mtime(&dest.join("README.md")),
        mtime(&src.join("README.md"))
    );

    // B now knows the project by the same ids, at its default path.
    let on_b = b.project(p.id).await.expect("project created on B");
    assert_eq!(on_b.name, "Garden");
    assert_eq!(on_b.primary, p.primary);
    assert_eq!(on_b.folders[0].id, p.folders[0].id);
    assert_eq!(on_b.folders[0].local_path.as_deref(), Some(dest.as_path()));
    assert_eq!(b.shared.store.load_projects().unwrap()[0], on_b);

    let rec = on_b.last_transfer.unwrap();
    assert_eq!(
        (rec.peer, rec.direction, rec.files),
        (a.id().await, Direction::Push, 2)
    );
    let rec = a.project(p.id).await.unwrap().last_transfer.unwrap();
    assert_eq!(
        (rec.peer, rec.direction, rec.files),
        (b.id().await, Direction::Push, 2)
    );
    let received = b.received.lock().unwrap().clone();
    assert_eq!(received, vec![(p.id, a.id().await, 2)]);
}

#[tokio::test]
async fn edit_add_and_delete_mirror_on_the_next_push() {
    let (a, b) = pair_full().await;
    let src = a.project_dir("app");
    write(&src, "keep.txt", "same");
    write(&src, "edit.txt", "v1");
    write(&src, "gone.txt", "bye");
    write(&src, "olddir/x.txt", "x");
    let p = a.add_project("Garden", &[("app", &src)]).await;
    push(&a, &b, p.id, false).await;

    write(&src, "edit.txt", "version 2");
    write(&src, "new/added.txt", "new");
    std::fs::remove_file(src.join("gone.txt")).unwrap();
    std::fs::remove_dir_all(src.join("olddir")).unwrap();
    let (preview, summary) = push(&a, &b, p.id, false).await;
    let c = preview.counts();
    assert_eq!((c.added, c.changed, c.removed_files), (2, 1, 2));
    assert_eq!(summary.removed, 2);

    let dest = b.dev().join("app");
    assert_eq!(tree(&dest), tree(&src));
    assert_eq!(read(&dest, "edit.txt"), "version 2");

    let (again, _) = push(&a, &b, p.id, false).await;
    assert!(again.is_empty(), "{:?}", again.folders[0].plan);
}

#[tokio::test]
async fn pull_mirrors_into_a_new_local_project() {
    let (a, b) = pair_full().await;
    let theirs = b.project_dir("beds");
    write(&theirs, "plan.txt", "rows");
    write(&theirs, "sketches/a.txt", "draft");
    std::fs::create_dir_all(theirs.join("photos")).unwrap();
    let docs = b.project_dir("docs");
    write(&docs, "guide.md", "read me");
    let p = b
        .add_project("Orchard", &[("beds", &theirs), ("docs", &docs)])
        .await;

    let (preview, summary) = pull(&a, &b, p.id).await;
    let beds = a.dev().join("Orchard/beds");
    assert_eq!(preview.folders[0].dest_path, beds.display().to_string());
    assert!(summary.failures.is_empty(), "{:?}", summary.failures);
    assert_eq!(tree(&beds), tree(&theirs));
    assert_eq!(tree(&a.dev().join("Orchard/docs")), tree(&docs));

    let mine = a.project(p.id).await.expect("created on A");
    assert_eq!(mine.primary, p.primary);
    assert_eq!(mine.folders.len(), 2);
    assert_eq!(mine.folders[0].local_path.as_deref(), Some(beds.as_path()));
    let rec = b.project(p.id).await.unwrap().last_transfer.unwrap();
    assert_eq!((rec.peer, rec.direction), (a.id().await, Direction::Pull));

    // A deletion on the source spreads on the next pull.
    std::fs::remove_file(theirs.join("sketches/a.txt")).unwrap();
    pull(&a, &b, p.id).await;
    assert!(!beds.join("sketches/a.txt").exists());
    assert!(beds.join("sketches").is_dir());
}

#[tokio::test]
async fn pull_leaves_out_a_folder_missing_on_the_source() {
    let (a, b) = pair_full().await;
    let theirs = b.project_dir("beds");
    let p = b.add_project("Orchard", &[("beds", &theirs)]).await;
    std::fs::remove_dir(&theirs).unwrap();
    let mine = a.project_dir("beds");
    write(&mine, "precious.txt", "do not delete");
    let mut local = p.clone();
    local.folders[0].local_path = Some(mine.clone());
    a.put_project(local).await;

    let (preview, _) = pull(&a, &b, p.id).await;
    assert!(preview.folders.is_empty());
    assert!(
        preview.warnings[0].contains("beds"),
        "{:?}",
        preview.warnings
    );
    assert_eq!(read(&mine, "precious.txt"), "do not delete");
}

#[tokio::test]
async fn timestamp_only_fixes_the_time_without_sending_bytes() {
    let (a, b) = pair_full().await;
    let src = a.project_dir("app");
    write(&src, "f.txt", "same content");
    let p = a.add_project("Garden", &[("app", &src)]).await;
    push(&a, &b, p.id, false).await;
    let dest = b.dev().join("app/f.txt");
    set_mtime(&dest, 1_000_000_000_000);

    let (preview, summary) = push(&a, &b, p.id, false).await;
    assert!(matches!(
        preview.folders[0].plan.changes[..],
        [Change::TimestampOnly(_)]
    ));
    assert_eq!(summary.bytes, 0);
    assert_eq!(summary.files, 1);
    assert_eq!(mtime(&dest), mtime(&src.join("f.txt")));
}

#[tokio::test]
async fn file_and_folder_swap_both_ways() {
    let (a, b) = pair_full().await;
    let src = a.project_dir("app");
    write(&src, "x", "file x");
    write(&src, "y/inner.txt", "in y");
    let p = a.add_project("Garden", &[("app", &src)]).await;
    push(&a, &b, p.id, false).await;

    std::fs::remove_file(src.join("x")).unwrap();
    write(&src, "x/now.txt", "x is a folder");
    std::fs::remove_dir_all(src.join("y")).unwrap();
    write(&src, "y", "y is a file");
    let (_, summary) = push(&a, &b, p.id, false).await;
    assert!(summary.failures.is_empty(), "{:?}", summary.failures);
    let dest = b.dev().join("app");
    assert_eq!(tree(&dest), tree(&src));
    assert_eq!(read(&dest, "y"), "y is a file");
}

#[tokio::test]
async fn removed_folder_goes_with_its_ignored_files_but_surviving_ignored_stay() {
    let (a, b) = pair_full().await;
    let src = a.project_dir("app");
    write(&src, "tool/index.js", "t");
    write(&src, "web/index.js", "w");
    let p = a.add_project("Garden", &[("app", &src)]).await;
    push(&a, &b, p.id, false).await;
    let dest = b.dev().join("app");
    write(&dest, "tool/node_modules/dep/i.js", "dep");
    write(&dest, "web/node_modules/dep/i.js", "dep");
    write(&dest, "web/.DS_Store", "ds");

    std::fs::remove_dir_all(src.join("tool")).unwrap();
    let (preview, _) = push(&a, &b, p.id, false).await;
    assert_eq!(
        preview.folders[0].plan.changes,
        vec![Change::RemoveDir {
            rel: "tool".into(),
            files: 1,
            ignored: 1
        }]
    );
    assert!(!dest.join("tool").exists());
    assert_eq!(read(&dest, "web/node_modules/dep/i.js"), "dep");
    assert_eq!(read(&dest, "web/.DS_Store"), "ds");
}

#[tokio::test]
async fn send_everything_copies_ignored_folders() {
    let (a, b) = pair_full().await;
    let src = a.project_dir("app");
    write(&src, "index.js", "i");
    write(&src, "node_modules/dep/i.js", "dep");
    let p = a.add_project("Garden", &[("app", &src)]).await;
    push(&a, &b, p.id, false).await;
    let dest = b.dev().join("app");
    assert!(!dest.join("node_modules").exists());
    push(&a, &b, p.id, true).await;
    assert_eq!(read(&dest, "node_modules/dep/i.js"), "dep");
}

#[tokio::test]
async fn commands_merge_on_both_sides_and_run_hashes_stay_local() {
    let (a, b) = pair_full().await;
    let src = a.project_dir("app");
    write(&src, "f", "f");
    let mut p = a.add_project("Garden", &[("app", &src)]).await;
    let mine = command("Build", 1, Some("hash-a"));
    p.commands = vec![mine.clone()];
    a.put_project(p.clone()).await;
    let mut theirs = p.clone();
    theirs.folders[0].local_path = Some(b.dev().join("app"));
    let other = command("Test", 1, Some("hash-b"));
    theirs.commands = vec![other.clone()];
    b.put_project(theirs).await;

    push(&a, &b, p.id, false).await;
    for (side, own, foreign) in [(&a, &mine, &other), (&b, &other, &mine)] {
        let cmds = side.project(p.id).await.unwrap().commands;
        assert_eq!(cmds.len(), 2);
        let get = |id| cmds.iter().find(|c: &&Command| c.id == id).unwrap();
        assert_eq!(get(own.id).last_run_hash, own.last_run_hash);
        assert_eq!(get(foreign.id).last_run_hash, None);
        let saved = side.shared.store.load_projects().unwrap();
        assert_eq!(saved[0].commands, cmds);
    }

    // The reply over the wire carries no run hashes either.
    let mut conn = a.open(&b).await;
    let ex = Request::ExchangeCommands {
        project: p.id,
        commands: vec![],
    };
    match conn.request(&ex).await.unwrap() {
        Response::Commands(c) => {
            assert_eq!(c.len(), 2);
            assert!(c.iter().all(|c| c.last_run_hash.is_none()), "{c:?}");
        }
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn progress_reports_start_each_file_and_done() {
    let (a, b) = pair_full().await;
    let src = a.project_dir("app");
    write(&src, "one.txt", "1");
    write(&src, "two.txt", "22");
    let p = a.add_project("Garden", &[("app", &src)]).await;
    let mut conn = a.open(&b).await;
    let req = a.request(&b, p.id, Direction::Push).await;
    let preview = transfer::prepare(&mut conn, &a.shared, req).await.unwrap();
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let summary = transfer::execute(&mut conn, &a.shared, preview, tx, Default::default())
        .await
        .unwrap();
    let mut events = Vec::new();
    while let Ok(ev) = rx.try_recv() {
        events.push(ev);
    }
    use transfer::Progress as P;
    assert_eq!(
        events.first(),
        Some(&P::Started {
            total_files: 2,
            total_bytes: 3
        })
    );
    let files: Vec<&str> = events
        .iter()
        .filter_map(|e| match e {
            P::File { rel, .. } => Some(rel.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(files, ["one.txt", "two.txt"]);
    assert_eq!(events.last(), Some(&P::Done(summary)));
}
