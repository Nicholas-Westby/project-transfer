//! Adding a pattern to the ignore list from an open preview.

use super::paired::*;
use super::*;
use crate::manifest::Change;
use crate::model::{Direction, Project, ProjectId};
use crate::transfer::{Preview, TransferRequest};

fn added(p: &Preview) -> Vec<String> {
    p.folders
        .iter()
        .flat_map(|f| &f.plan.changes)
        .filter_map(|c| match c {
            Change::Add(e) => Some(e.rel.clone()),
            _ => None,
        })
        .collect()
}

/// A and B paired with each other, each with its Projects folder in its
/// own temp dir, and a project on A with an `exports` folder in it.
fn setup() -> (Fixture, Fixture, Project) {
    let (a, b) = (Fixture::new(), Fixture::new());
    for f in [&a, &b] {
        f.act(Action::SetProjectsFolder(f.dir.path().join("Dev")));
    }
    pair(&a, &b, Some(at(&b)), None);
    pair(&b, &a, Some(at(&a)), None);
    let p = a.create("garden-planner", "app");
    let root = p.folders[0].local_path.clone().unwrap();
    std::fs::create_dir_all(root.join("exports/2026")).unwrap();
    std::fs::write(root.join("exports/2026/plot.svg"), "<svg/>").unwrap();
    std::fs::write(root.join("main.rs"), "fn main() {}").unwrap();
    (a, b, p)
}

fn push(a: &Fixture, b: &Fixture, project: ProjectId) {
    a.act(Action::Prepare(TransferRequest {
        peer: id(b),
        project,
        direction: Direction::Push,
        send_everything: false,
    }));
}

/// Whether the open preview adds anything under `exports`; None while no
/// preview is open.
fn shows_exports(s: &UiState) -> Option<bool> {
    match &s.transfer {
        TransferState::Ready(p) => Some(added(p).iter().any(|r| r.starts_with("exports"))),
        _ => None,
    }
}

#[test]
fn adding_a_pattern_saves_it_and_compares_the_open_preview_again() {
    let (a, b, p) = setup();
    push(&a, &b, p.id);
    wait_until(&a.core, "the first preview", |s| {
        shows_exports(s) == Some(true)
    });
    a.act(Action::AddIgnore(" /exports/ ".into()));
    wait_until(&a.core, "a preview without the exports", |s| {
        shows_exports(s) == Some(false)
    });
    let saved = a.store().load_or_create_instance().unwrap();
    assert_eq!(saved.extra_ignores, ["/exports/"]);
    let said = a.core.state().activity.clone();
    assert!(
        said.iter()
            .any(|l| l.text == "Added /exports/ to the ignore list."),
        "{said:#?}"
    );
}

#[test]
fn comparing_again_after_a_match_uses_the_project_as_asked_for() {
    let (a, b, p) = setup();
    // B made its own garden-planner, so the preview takes on B's ids.
    b.create("garden-planner", "app");
    push(&a, &b, p.id);
    wait_until(
        &a.core,
        "the first preview",
        |s| matches!(&s.transfer, TransferState::Ready(p) if p.link.is_some()),
    );
    a.act(Action::AddIgnore("/exports/".into()));
    wait_until(&a.core, "a second preview or a failure", |s| {
        shows_exports(s) == Some(false) || matches!(s.transfer, TransferState::Failed(_))
    });
    let s = a.core.state().transfer.clone();
    assert_eq!(shows_exports(&a.core.state()), Some(false), "{s:?}");
}

#[test]
fn a_removed_default_comes_back_and_nothing_is_added_twice() {
    let f = Fixture::new();
    f.act(Action::SetIgnores {
        extra: vec!["*.log".into()],
        always_include: vec![],
        removed_defaults: vec!["dist/".into()],
    });
    f.act(Action::AddIgnore("dist/".into()));
    assert_eq!(
        f.last_activity().text,
        "Turned dist/ back on in the ignore list."
    );
    f.act(Action::AddIgnore("*.log".into()));
    f.act(Action::AddIgnore("node_modules/".into()));
    let saved = f.store().load_or_create_instance().unwrap();
    assert!(saved.removed_default_ignores.is_empty());
    assert_eq!(saved.extra_ignores, ["*.log"]);
    assert_eq!(
        f.last_activity().text,
        "node_modules/ is already on the ignore list."
    );
    f.act(Action::AddIgnore("{a,b".into()));
    assert_eq!(f.last_activity().kind, ActivityKind::Error);
    let saved = f.store().load_or_create_instance().unwrap();
    assert_eq!(saved.extra_ignores, ["*.log"]);
}

#[test]
fn a_space_escaped_at_the_end_stays_part_of_the_pattern() {
    let f = Fixture::new();
    f.act(Action::AddIgnore(r" /notes/draft\  ".into()));
    let saved = f.store().load_or_create_instance().unwrap();
    assert_eq!(saved.extra_ignores, [r"/notes/draft\ "]);
}
