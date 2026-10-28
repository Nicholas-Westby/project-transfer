//! Macs and Windows PCs usually ignore case in names, and a name keeps the
//! case it was first written in: writing `README.md` over `readme.md`
//! changes the file but not its name. A name changed only in case still
//! reaches the other computer, so the next transfer finds nothing to do.

mod support;

use project_transfer::model::ProjectId;
use std::path::Path;
use support::*;

/// The names in `dir`, spelled as the disk holds them.
fn listed(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

/// Whether the disk holding `dir` takes two names differing only by case
/// as one. Where it doesn't, both names are kept and there is nothing to test.
fn ignores_case(dir: &Path) -> bool {
    write(dir, "probe", "");
    let one = dir.join("PROBE").exists();
    std::fs::remove_file(dir.join("probe")).unwrap();
    one
}

fn fill(root: &Path) {
    write(root, "readme.md", "r");
    write(root, "Docs/Guide.md", "g");
}

fn rename_by_case(root: &Path) {
    std::fs::rename(root.join("readme.md"), root.join("README.md")).unwrap();
    std::fs::rename(root.join("Docs"), root.join("docs")).unwrap();
}

fn check(root: &Path) {
    assert_eq!(listed(root), ["README.md", "docs"]);
    assert_eq!(listed(&root.join("docs")), ["Guide.md"]);
}

async fn matched(a: &Instance, b: &Instance, project: ProjectId) -> bool {
    let mut conn = a.open(b).await;
    let req = a
        .request(b, project, project_transfer::model::Direction::Push)
        .await;
    let preview = project_transfer::transfer::prepare(&mut conn, &a.shared, req)
        .await
        .unwrap();
    preview.files_match()
}

#[tokio::test]
async fn a_push_renames_a_name_changed_only_in_case() {
    let (a, b) = pair_full().await;
    let src = a.project_dir("app");
    if !ignores_case(&src) {
        return;
    }
    fill(&src);
    let p = a.add_project("Garden", &[("app", &src)]).await;
    push(&a, &b, p.id, false).await;
    rename_by_case(&src);
    let (_, summary) = push(&a, &b, p.id, false).await;
    assert!(summary.failures.is_empty(), "{:?}", summary.failures);
    check(&b.dev().join("app"));
    assert!(
        matched(&a, &b, p.id).await,
        "the next push has nothing to do"
    );
}

#[tokio::test]
async fn a_pull_renames_a_name_changed_only_in_case() {
    let (a, b) = pair_full().await;
    let theirs = b.project_dir("app");
    if !ignores_case(&theirs) {
        return;
    }
    fill(&theirs);
    let p = b.add_project("Garden", &[("app", &theirs)]).await;
    pull(&a, &b, p.id).await;
    rename_by_case(&theirs);
    let (_, summary) = pull(&a, &b, p.id).await;
    assert!(summary.failures.is_empty(), "{:?}", summary.failures);
    check(&a.dev().join("app"));
    assert!(
        matched(&a, &b, p.id).await,
        "the next push has nothing to do"
    );
}
