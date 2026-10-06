//! Folders the two computers don't agree on: what a transfer says about them.

mod support;

use project_transfer::model::Folder;
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
