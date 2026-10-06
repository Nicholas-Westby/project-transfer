//! Folders the two computers don't agree on: what a transfer says about them,
//! sending the project's details on their own, and where a folder lands.

mod support;

use project_transfer::model::{Description, Folder, Project};
use std::path::Path;
use support::*;

#[tokio::test]
async fn pull_names_a_folder_only_this_computer_has() {
    let (a, b) = pair_full().await;
    let theirs = b.project_dir("beds");
    write(&theirs, "plan.txt", "x");
    let p = b.add_project("Orchard", &[("beds", &theirs)]).await;
    pull(&a, &b, p.id).await;
    let secrets = a.project_dir("secrets");
    write(&secrets, "key.json", "{}");
    let mut local = a.project(p.id).await.unwrap();
    local.folders.push(Folder {
        id: uuid::Uuid::new_v4(),
        name: "secrets".into(),
        local_path: Some(secrets),
    });
    a.put_project(local).await;

    let (preview, _) = pull(&a, &b, p.id).await;
    assert_eq!(preview.folders.len(), 1);
    assert_eq!(preview.left_out.len(), 1);
    assert_eq!(preview.left_out[0].name, "secrets");
    assert!(
        preview.left_out[0]
            .reason
            .contains("doesn't have this folder yet"),
        "{}",
        preview.left_out[0].reason
    );
}

/// A project of one folder on A, pushed to B so both know it.
async fn shared_project(a: &Instance, b: &Instance) -> Project {
    let beds = a.project_dir("beds");
    write(&beds, "plan.txt", "x");
    let p = a.add_project("Orchard", &[("beds", &beds)]).await;
    push(a, b, p.id, false).await;
    p
}

/// Adds a folder at `path` to `on`'s copy of `project` and returns its id.
async fn add_folder(on: &Instance, project: uuid::Uuid, name: &str, path: &Path) -> uuid::Uuid {
    let id = uuid::Uuid::new_v4();
    let mut p = on.project(project).await.unwrap();
    p.folders.push(Folder {
        id,
        name: name.into(),
        local_path: Some(path.to_path_buf()),
    });
    on.put_project(p).await;
    id
}

/// Where a folder at `~/.microsoft/usersecrets/abc` lands on `on`.
fn secrets_place(on: &Instance) -> std::path::PathBuf {
    on.home().join(".microsoft").join("usersecrets").join("abc")
}

#[tokio::test]
async fn sync_adds_a_new_folder_there_at_the_default_place_and_copies_nothing() {
    let (a, b) = pair_full().await;
    let p = shared_project(&a, &b).await;
    let notes = a.project_dir("notes");
    write(&notes, "n.txt", "hello");
    let id = add_folder(&a, p.id, "notes", &notes).await;

    sync(&a, &b, p.id).await.unwrap();

    let there = path_of(&b, p.id, id).await.expect("set up on B");
    assert_eq!(there, b.dev().join("Orchard").join("notes"));
    assert!(!there.exists(), "a sync never creates folders");
    let beds = path_of(&b, p.id, p.folders[0].id).await;
    assert_eq!(beds, Some(b.dev().join("beds")));
    assert_eq!(b.received.lock().unwrap().len(), 1, "only the first push");
}

#[tokio::test]
async fn sync_brings_back_what_only_the_other_computer_has() {
    let (a, b) = pair_full().await;
    let p = shared_project(&a, &b).await;
    let extra = b.project_dir("extra");
    let id = add_folder(&b, p.id, "extra", &extra).await;
    let mut there = b.project(p.id).await.unwrap();
    there.description = Description {
        text: "From B".into(),
        at_ms: 5,
    };
    there.commands.push(command("build", 3, None));
    b.put_project(there).await;

    sync(&a, &b, p.id).await.unwrap();

    let here = a.project(p.id).await.unwrap();
    let want = a.dev().join("Orchard").join("extra");
    assert_eq!(path_of(&a, p.id, id).await, Some(want.clone()));
    assert!(!want.exists());
    assert_eq!(here.description.text, "From B");
    assert_eq!(here.commands.len(), 1);
}

#[tokio::test]
async fn a_folder_under_home_lands_at_the_same_place_under_home_there() {
    let (a, b) = pair_full().await;
    let p = shared_project(&a, &b).await;
    let secrets = a.home_dir(".microsoft/usersecrets/abc");
    write(&secrets, "secrets.json", "{}");
    let id = add_folder(&a, p.id, "abc", &secrets).await;

    sync(&a, &b, p.id).await.unwrap();

    assert_eq!(path_of(&b, p.id, id).await, Some(secrets_place(&b)));
    assert!(!secrets_place(&b).exists());
}

#[tokio::test]
async fn a_home_place_taken_by_another_project_falls_back_to_the_projects_folder() {
    let (a, b) = pair_full().await;
    let secrets = a.home_dir(".microsoft/usersecrets/abc");
    let p = a.add_project("Secrets", &[("abc", &secrets)]).await;
    // B keeps the folder around that place in a project of its own.
    let theirs = b.home_dir(".microsoft/usersecrets");
    b.add_project("Tools", &[("usersecrets", &theirs)]).await;

    sync(&a, &b, p.id).await.unwrap();

    let there = path_of(&b, p.id, p.folders[0].id).await;
    assert_eq!(there, Some(b.dev().join("abc")));
}

#[tokio::test]
async fn a_push_lands_a_home_folder_where_its_preview_said() {
    let (a, b) = pair_full().await;
    let secrets = a.home_dir(".microsoft/usersecrets/abc");
    write(&secrets, "secrets.json", "{}");
    let p = a.add_project("Secrets", &[("abc", &secrets)]).await;

    let (preview, _) = push(&a, &b, p.id, false).await;

    let want = secrets_place(&b);
    assert_eq!(preview.folders[0].dest_path, want.display().to_string());
    assert_eq!(path_of(&b, p.id, p.folders[0].id).await, Some(want.clone()));
    assert_eq!(read(&want, "secrets.json"), "{}");
}

#[tokio::test]
async fn a_pull_lands_a_home_folder_at_the_same_place_here() {
    let (a, b) = pair_full().await;
    let secrets = b.home_dir(".microsoft/usersecrets/abc");
    write(&secrets, "secrets.json", "{}");
    let p = b.add_project("Secrets", &[("abc", &secrets)]).await;

    let (preview, _) = pull(&a, &b, p.id).await;

    let want = secrets_place(&a);
    assert_eq!(preview.folders[0].dest_path, want.display().to_string());
    assert_eq!(read(&want, "secrets.json"), "{}");
}

#[tokio::test]
async fn sync_needs_permission_to_push() {
    let (a, b) = pair_with(perms(false, true)).await;
    let beds = a.project_dir("beds");
    let p = a.add_project("Orchard", &[("beds", &beds)]).await;

    let err = sync(&a, &b, p.id).await.unwrap_err();

    let text = format!("{err:#}");
    assert!(text.contains("doesn't let this computer push"), "{text}");
    assert!(b.project(p.id).await.is_none());
}

#[tokio::test]
async fn after_a_sync_a_pull_says_the_folder_is_missing_there_not_unknown() {
    let (a, b) = pair_full().await;
    let p = shared_project(&a, &b).await;
    let secrets = a.project_dir("secrets");
    write(&secrets, "key.json", "{}");
    add_folder(&a, p.id, "secrets", &secrets).await;

    sync(&a, &b, p.id).await.unwrap();
    let (preview, _) = pull(&a, &b, p.id).await;

    let out = &preview.left_out;
    assert_eq!(out.len(), 1, "{out:?}");
    assert!(!out[0].reason.contains("doesn't have this folder yet"));
    assert!(out[0].reason.contains("It is not on"), "{}", out[0].reason);
}

#[tokio::test]
async fn the_other_computer_keeps_a_folder_where_this_one_says() {
    let (a, b) = pair_full().await;
    let p = shared_project(&a, &b).await;
    let f = p.folders[0].id;

    let elsewhere = b.root().join("elsewhere").join("beds");
    let text = elsewhere.display().to_string();
    set_peer_path(&a, &b, p.id, f, &text).await.unwrap();
    assert_eq!(path_of(&b, p.id, f).await, Some(elsewhere));

    // `~` is the other computer's home folder.
    set_peer_path(&a, &b, p.id, f, "~/beds").await.unwrap();
    assert_eq!(path_of(&b, p.id, f).await, Some(b.home().join("beds")));
}

#[tokio::test]
async fn the_other_computer_refuses_a_folder_it_cannot_keep() {
    let (a, b) = pair_full().await;
    let p = shared_project(&a, &b).await;
    let f = p.folders[0].id;
    let tools = b.project_dir("tools");
    b.add_project("Tools", &[("tools", &tools)]).await;
    let before = path_of(&b, p.id, f).await;

    let cases = [
        ("relative/beds".to_string(), "full path"),
        (b.home().display().to_string(), "home folder"),
        (tools.join("beds").display().to_string(), "overlaps"),
        (
            b.root()
                .join("x")
                .join("..")
                .join("beds")
                .display()
                .to_string(),
            "..",
        ),
    ];
    for (path, says) in cases {
        let err = set_peer_path(&a, &b, p.id, f, &path).await.unwrap_err();
        assert!(format!("{err:#}").contains(says), "{path}: {err:#}");
    }
    assert_eq!(path_of(&b, p.id, f).await, before);
}

#[tokio::test]
async fn changing_a_folder_there_needs_permission_to_push() {
    let (a, b) = pair_with(perms(false, true)).await;
    let beds = b.project_dir("beds");
    let p = b.add_project("Orchard", &[("beds", &beds)]).await;
    let f = p.folders[0].id;

    let err = set_peer_path(&a, &b, p.id, f, "~/beds").await.unwrap_err();

    assert!(format!("{err:#}").contains("push"), "{err:#}");
    assert_eq!(path_of(&b, p.id, f).await, Some(beds));
}
