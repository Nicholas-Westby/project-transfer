//! A project's description travels with it, and the newer edit wins.

mod support;

use project_transfer::model::Description;
use support::*;

fn described(text: &str, at_ms: i64) -> Description {
    Description {
        text: text.into(),
        at_ms,
    }
}

#[tokio::test]
async fn a_push_and_a_pull_carry_the_description() {
    let (a, b) = pair_full().await;
    let src = a.project_dir("garden-planner");
    write(&src, "beds.txt", "b");
    let mut p = a
        .add_project("Garden Planner", &[("garden-planner", &src)])
        .await;
    p.description = described("Plans the beds.", 10);
    a.put_project(p.clone()).await;

    push(&a, &b, p.id, false).await;
    assert_eq!(b.project(p.id).await.unwrap().description, p.description);

    let mut there = b.project(p.id).await.unwrap();
    there.description = described("Plans the beds.\nWaters on Mondays.", 20);
    b.put_project(there.clone()).await;
    pull(&a, &b, p.id).await;
    assert_eq!(
        a.project(p.id).await.unwrap().description,
        there.description
    );
}

#[tokio::test]
async fn an_older_description_never_replaces_a_newer_one() {
    let (a, b) = pair_full().await;
    let src = a.project_dir("tide-tables");
    write(&src, "march.csv", "m");
    let mut p = a.add_project("Tide Tables", &[("tide-tables", &src)]).await;
    p.description = described("Old notes", 10);
    a.put_project(p.clone()).await;
    push(&a, &b, p.id, false).await;

    let mut there = b.project(p.id).await.unwrap();
    there.description = described("Edited there later", 30);
    b.put_project(there).await;
    write(&src, "april.csv", "a");
    push(&a, &b, p.id, false).await;
    assert_eq!(
        b.project(p.id).await.unwrap().description.text,
        "Edited there later"
    );
}

/// Files that already match don't make the preview empty when only the
/// description differs, and running it carries the description.
#[tokio::test]
async fn a_description_alone_is_a_change() {
    let (a, b) = pair_full().await;
    let src = a.project_dir("seed-catalog");
    write(&src, "list.txt", "l");
    let mut p = a
        .add_project("Seed Catalog", &[("seed-catalog", &src)])
        .await;
    push(&a, &b, p.id, false).await;

    p.description = described("Heirloom seeds only.", 50);
    a.put_project(p.clone()).await;
    let (preview, summary) = push(&a, &b, p.id, false).await;
    assert!(preview.files_match() && preview.description && !preview.is_empty());
    assert_eq!(summary.files, 0);
    assert_eq!(b.project(p.id).await.unwrap().description, p.description);
    let (again, _) = push(&a, &b, p.id, false).await;
    assert!(again.is_empty());
}
