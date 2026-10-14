//! The transfer the Dock menu offers stays on its project when a match
//! with another computer's project gives the project that computer's id.

use super::paired::*;
use super::*;
use crate::model::{Direction, LastStarted, TransferKind};
use crate::transfer::TransferRequest;

#[test]
fn syncing_details_that_match_a_project_keeps_its_dock_items() {
    let (a, b, c) = (Fixture::new(), Fixture::new(), Fixture::new());
    for f in [&a, &b, &c] {
        f.act(Action::SetProjectsFolder(f.dir.path().join("Dev")));
    }
    for (x, y) in [(&a, &b), (&b, &a), (&a, &c), (&c, &a)] {
        pair(x, y, Some(at(y)), None);
    }
    let p = a.create("garden-planner", "app");
    let root = p.folders[0].local_path.clone().unwrap();
    std::fs::write(root.join("main.rs"), "fn main() {}").unwrap();
    a.act(Action::Prepare(TransferRequest {
        peer: id(&b),
        project: p.id,
        direction: Direction::Push,
        send_everything: false,
    }));
    wait_until(&a.core, "the preview", |s| {
        matches!(s.transfer, TransferState::Ready(_))
    });
    a.act(Action::Execute);
    wait_until(&a.core, "the push", |s| {
        matches!(s.transfer, TransferState::Finished(_))
    });
    // C made its own garden-planner; syncing details with it matches them.
    let theirs = c.create("garden-planner", "app").id;
    a.act(Action::SelectPeer(id(&c)));
    a.act(Action::SyncDetails(p.id));
    let want = Some(LastStarted {
        kind: TransferKind::Push,
        project: theirs,
        peer: id(&b),
    });
    wait_until(&a.core, "the Dock menu to follow", |s| {
        s.me.last_started == want
    });
    assert_eq!(a.project().id, theirs);
    assert_eq!(
        a.store().load_or_create_instance().unwrap().last_started,
        want
    );
}
