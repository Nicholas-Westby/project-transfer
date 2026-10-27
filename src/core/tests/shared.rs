use super::*;
use crate::model::{Command, Os, Project, ProjectId, SharedCommand};
use crate::transfer::projects::now_ms;

/// Two projects, Garden and Tides, and their ids in that order.
fn two_projects(f: &Fixture) -> (ProjectId, ProjectId) {
    for (name, folder) in [("Garden", "garden"), ("Tides", "tides")] {
        f.act(Action::CreateProject {
            name: name.into(),
            folder: f.folder(folder),
        });
    }
    let s = f.core.state();
    (s.projects[0].id, s.projects[1].id)
}

fn commands(f: &Fixture, p: ProjectId) -> Vec<Command> {
    let s = f.core.state();
    let p = s.project(p).unwrap();
    p.commands.iter().filter(|c| !c.deleted).cloned().collect()
}

fn library(f: &Fixture) -> Vec<SharedCommand> {
    f.core.state().shared_commands.clone()
}

/// A shared command made in Garden and linked into Tides, with its id.
fn release_in_both(f: &Fixture) -> (ProjectId, ProjectId, uuid::Uuid) {
    let (garden, tides) = two_projects(f);
    f.act(Action::AddSharedCommand(
        garden,
        "Release".into(),
        "./release.sh".into(),
    ));
    let id = library(f)[0].id;
    f.act(Action::LinkSharedCommand(tides, id));
    (garden, tides, id)
}

#[test]
fn a_new_shared_command_is_listed_and_linked_in_its_project() {
    let f = Fixture::new();
    let (garden, _) = two_projects(&f);
    f.act(Action::AddSharedCommand(
        garden,
        " Release ".into(),
        "./release.sh".into(),
    ));
    let lib = library(&f);
    assert_eq!(lib.len(), 1);
    assert_eq!(
        (lib[0].label.as_str(), lib[0].line.as_str(), lib[0].deleted),
        ("Release", "./release.sh", false)
    );
    assert_eq!(lib[0].created_on, Os::current());
    let c = &commands(&f, garden)[0];
    assert_eq!(c.linked_to, Some(lib[0].id));
    assert_eq!(c.line, "./release.sh");
    assert_eq!(f.store().load_shared_commands().unwrap(), lib);
    assert_eq!(f.last_activity().kind, ActivityKind::Info);
}

#[test]
fn editing_a_linked_command_changes_every_project_that_uses_it() {
    let f = Fixture::new();
    let (garden, tides, id) = release_in_both(&f);
    let there = commands(&f, tides)[0].clone();
    assert_eq!(there.linked_to, Some(id));
    f.act(Action::EditCommand(
        tides,
        there.id,
        "Release".into(),
        "./release.sh --notes".into(),
    ));
    assert_eq!(library(&f)[0].line, "./release.sh --notes");
    for p in [garden, tides] {
        assert_eq!(commands(&f, p)[0].line, "./release.sh --notes");
    }
    let saved = f.store().load_projects().unwrap();
    assert!(
        saved
            .iter()
            .all(|p| p.commands[0].line == "./release.sh --notes")
    );
}

#[test]
fn editing_a_shared_command_reaches_its_linked_commands() {
    let f = Fixture::new();
    let (garden, tides, id) = release_in_both(&f);
    f.act(Action::EditSharedCommand(
        id,
        "Ship".into(),
        "./ship.sh".into(),
    ));
    for p in [garden, tides] {
        let c = &commands(&f, p)[0];
        assert_eq!((c.label.as_str(), c.line.as_str()), ("Ship", "./ship.sh"));
    }
    assert_eq!(f.store().load_shared_commands().unwrap()[0].label, "Ship");
}

#[test]
fn a_separate_copy_does_not_follow_later_edits_nor_lead_them() {
    let f = Fixture::new();
    let (garden, tides) = two_projects(&f);
    f.act(Action::AddSharedCommand(
        garden,
        "Release".into(),
        "./release.sh".into(),
    ));
    let id = library(&f)[0].id;
    f.act(Action::CopySharedCommand(tides, id));
    let copy = commands(&f, tides)[0].clone();
    assert_eq!((copy.line.as_str(), copy.linked_to), ("./release.sh", None));
    f.act(Action::EditSharedCommand(
        id,
        "Release".into(),
        "./release.sh v2".into(),
    ));
    assert_eq!(commands(&f, tides)[0].line, "./release.sh");
    f.act(Action::EditCommand(
        tides,
        copy.id,
        "Release".into(),
        "./mine.sh".into(),
    ));
    assert_eq!(library(&f)[0].line, "./release.sh v2");
    assert_eq!(commands(&f, garden)[0].line, "./release.sh v2");
}

#[test]
fn an_unlinked_command_keeps_its_text_and_stops_following() {
    let f = Fixture::new();
    let (garden, tides, id) = release_in_both(&f);
    let there = commands(&f, tides)[0].clone();
    f.act(Action::UnlinkCommand(tides, there.id));
    let unlinked = commands(&f, tides)[0].clone();
    assert_eq!(unlinked.linked_to, None);
    assert_eq!(unlinked.line, "./release.sh");
    assert!(unlinked.updated_at_ms > there.updated_at_ms);
    f.act(Action::EditSharedCommand(
        id,
        "Release".into(),
        "./release.sh v2".into(),
    ));
    assert_eq!(commands(&f, tides)[0].line, "./release.sh");
    assert_eq!(commands(&f, garden)[0].line, "./release.sh v2");
}

#[test]
fn deleting_a_shared_command_unlinks_its_commands_and_they_keep_their_text() {
    let f = Fixture::new();
    let (garden, tides, id) = release_in_both(&f);
    f.act(Action::DeleteSharedCommand(id));
    let lib = library(&f);
    assert!(
        lib[0].deleted,
        "a deleted marker stays so the deletion spreads"
    );
    for p in [garden, tides] {
        let c = &commands(&f, p)[0];
        assert_eq!((c.line.as_str(), c.linked_to), ("./release.sh", None));
    }
    assert!(f.last_activity().text.contains("2 commands"));
    assert_eq!(f.store().load_shared_commands().unwrap(), lib);
}

#[test]
fn sharing_a_command_adds_it_linked() {
    let f = Fixture::new();
    let (garden, _) = two_projects(&f);
    f.act(Action::AddCommand(garden, "Build".into(), "make".into()));
    let c = commands(&f, garden)[0].clone();
    f.act(Action::ShareCommand(garden, c.id));
    let lib = library(&f);
    assert_eq!(
        (lib[0].label.as_str(), lib[0].line.as_str()),
        ("Build", "make")
    );
    assert_eq!(commands(&f, garden)[0].linked_to, Some(lib[0].id));
    f.act(Action::ShareCommand(garden, c.id));
    assert_eq!(library(&f).len(), 1, "a linked command is shared already");
    assert_eq!(f.last_activity().kind, ActivityKind::Error);
}

#[test]
fn a_project_links_a_shared_command_once() {
    let f = Fixture::new();
    let (_, tides, id) = release_in_both(&f);
    f.act(Action::LinkSharedCommand(tides, id));
    assert_eq!(commands(&f, tides).len(), 1);
    assert_eq!(f.last_activity().kind, ActivityKind::Error);
}

#[test]
fn deleting_a_linked_command_keeps_the_shared_command() {
    let f = Fixture::new();
    let (garden, _, id) = release_in_both(&f);
    let c = commands(&f, garden)[0].clone();
    f.act(Action::DeleteCommand(garden, c.id));
    assert!(commands(&f, garden).is_empty());
    let lib = library(&f);
    assert_eq!((lib[0].id, lib[0].deleted), (id, false));
}

#[test]
fn an_edit_wins_over_a_version_stamped_ahead_of_this_clock() {
    let dir = tempfile::tempdir().unwrap();
    let ahead = now_ms() + 3_600_000;
    let c = Command {
        id: uuid::Uuid::new_v4(),
        label: "Build".into(),
        line: "make".into(),
        created_on: Os::current(),
        updated_at_ms: ahead,
        deleted: false,
        last_run_hash: None,
        linked_to: None,
    };
    let p = Project {
        id: uuid::Uuid::new_v4(),
        name: "Garden".into(),
        folders: vec![],
        primary: uuid::Uuid::nil(),
        commands: vec![c.clone()],
        last_transfer: None,
        description: Default::default(),
    };
    store_in(dir.path())
        .save_projects(std::slice::from_ref(&p))
        .unwrap();
    let core = AppCore::start_with(store_in(dir.path()), None, opts()).unwrap();
    core.act(Action::EditCommand(
        p.id,
        c.id,
        "Build".into(),
        "make all".into(),
    ));
    core.settle();
    let edited = core.state().projects[0].commands[0].clone();
    assert_eq!(edited.line, "make all");
    assert!(edited.updated_at_ms > ahead);
}

/// A core started on a store holding Garden with a Release command for
/// each link in `links`, and no shared commands.
fn garden_with_releases(dir: &Path, links: &[uuid::Uuid]) -> AppCore {
    let release = |link: &uuid::Uuid| Command {
        id: uuid::Uuid::new_v4(),
        label: "Release".into(),
        line: "./release.sh".into(),
        created_on: Os::MacOs,
        updated_at_ms: 7,
        deleted: false,
        last_run_hash: None,
        linked_to: Some(*link),
    };
    let p = Project {
        id: uuid::Uuid::new_v4(),
        name: "Garden".into(),
        folders: vec![],
        primary: uuid::Uuid::nil(),
        commands: links.iter().map(release).collect(),
        last_transfer: None,
        description: Default::default(),
    };
    store_in(dir).save_projects(&[p]).unwrap();
    AppCore::start_with(store_in(dir), None, opts()).unwrap()
}

#[test]
fn start_up_repairs_a_link_to_a_shared_command_it_lost() {
    let dir = tempfile::tempdir().unwrap();
    let lost = uuid::Uuid::new_v4();
    let core = garden_with_releases(dir.path(), &[lost]);
    let lib = core.state().shared_commands.clone();
    assert_eq!(lib.len(), 1);
    assert_eq!((lib[0].id, lib[0].line.as_str()), (lost, "./release.sh"));
    assert_eq!(store_in(dir.path()).load_shared_commands().unwrap(), lib);
}

#[test]
fn deleting_a_shared_command_counts_the_commands_it_unlinks() {
    // Both computers linked Release into Garden before they first synced.
    let dir = tempfile::tempdir().unwrap();
    let s = uuid::Uuid::new_v4();
    let core = garden_with_releases(dir.path(), &[s, s]);
    core.act(Action::DeleteSharedCommand(s));
    core.settle();
    let line = core.state().activity.last().cloned().unwrap();
    assert!(
        line.text
            .contains("The 2 commands linked to it keep their text"),
        "{}",
        line.text
    );
}
