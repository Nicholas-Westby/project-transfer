use super::*;
use crate::merge::merge_commands;
use crate::model::{Command, Os, Project};
use uuid::Uuid;

fn shared(id: Uuid, line: &str, at: i64) -> SharedCommand {
    SharedCommand {
        id,
        label: "Release".into(),
        line: line.into(),
        created_on: Os::MacOs,
        updated_at_ms: at,
        deleted: false,
    }
}

fn linked(id: Uuid, to: Uuid, line: &str, at: i64) -> Command {
    Command {
        id,
        label: "Release".into(),
        line: line.into(),
        created_on: Os::MacOs,
        updated_at_ms: at,
        deleted: false,
        last_run_hash: None,
        linked_to: Some(to),
    }
}

fn project(commands: Vec<Command>) -> Project {
    Project {
        id: Uuid::new_v4(),
        name: "garden-planner".into(),
        folders: vec![],
        primary: Uuid::nil(),
        commands,
        last_transfer: None,
        description: Default::default(),
    }
}

#[test]
fn merge_adds_what_only_the_other_computer_has() {
    let (a, b) = (
        shared(Uuid::new_v4(), "a", 1),
        shared(Uuid::new_v4(), "b", 1),
    );
    let m = merge_shared(std::slice::from_ref(&a), std::slice::from_ref(&b));
    assert_eq!(m.len(), 2);
    assert!(m.contains(&a) && m.contains(&b));
}

#[test]
fn merge_takes_the_newer_edit_either_way_and_keeps_local_on_a_tie() {
    let id = Uuid::new_v4();
    let (old, new) = (
        shared(id, "./release.sh", 1),
        shared(id, "./release.sh --notes", 2),
    );
    let lines = |m: Vec<SharedCommand>| m[0].line.clone();
    assert_eq!(
        lines(merge_shared(
            std::slice::from_ref(&old),
            std::slice::from_ref(&new)
        )),
        "./release.sh --notes"
    );
    assert_eq!(lines(merge_shared(&[new], &[old])), "./release.sh --notes");
    let theirs = shared(id, "theirs", 5);
    assert_eq!(
        lines(merge_shared(&[shared(id, "mine", 5)], &[theirs])),
        "mine"
    );
}

#[test]
fn merge_spreads_a_deletion() {
    let id = Uuid::new_v4();
    let gone = SharedCommand {
        deleted: true,
        ..shared(id, "x", 9)
    };
    let m = merge_shared(&[shared(id, "x", 3)], &[gone]);
    assert!(m[0].deleted);
}

#[test]
fn an_edit_is_stamped_after_the_version_it_replaces() {
    // That version came from a computer whose clock runs ahead.
    assert_eq!(stamp_after(1_000, 400), 1_001);
    assert_eq!(stamp_after(1_000, 5_000), 5_000);
}

#[test]
fn a_shared_edit_reaches_every_linked_command() {
    let s = Uuid::new_v4();
    let on_windows = SharedCommand {
        created_on: Os::Windows,
        ..shared(s, "./release.sh v2", 10)
    };
    let mut lib = vec![on_windows.clone()];
    let mut projects = vec![
        project(vec![linked(Uuid::new_v4(), s, "./release.sh", 5)]),
        project(vec![linked(Uuid::new_v4(), s, "./release.sh", 5)]),
    ];
    assert!(reconcile(&mut projects, &mut lib));
    for p in &projects {
        assert_eq!(p.commands[0].line, "./release.sh v2");
        assert_eq!(p.commands[0].created_on, Os::Windows);
        assert_eq!(p.commands[0].updated_at_ms, 10);
        assert_eq!(p.commands[0].linked_to, Some(s));
    }
    assert_eq!(lib, vec![on_windows]);
}

#[test]
fn a_newer_linked_command_carries_its_edit_to_the_shared_one_and_the_rest() {
    let s = Uuid::new_v4();
    let mut lib = vec![shared(s, "old", 5)];
    let mut projects = vec![
        project(vec![linked(Uuid::new_v4(), s, "old", 5)]),
        project(vec![linked(Uuid::new_v4(), s, "new", 9)]),
    ];
    assert!(reconcile(&mut projects, &mut lib));
    assert_eq!(lib, vec![shared(s, "new", 9)]);
    assert_eq!(projects[0].commands[0].line, "new");
    assert_eq!(projects[0].commands[0].updated_at_ms, 9);
}

#[test]
fn a_link_to_an_unknown_shared_command_adds_it() {
    let s = Uuid::new_v4();
    let mut lib = vec![];
    let mut projects = vec![project(vec![linked(Uuid::new_v4(), s, "make", 4)])];
    assert!(reconcile(&mut projects, &mut lib));
    assert_eq!(lib, vec![shared(s, "make", 4)]);
    assert_eq!(projects[0].commands[0].linked_to, Some(s));
}

#[test]
fn a_deleted_shared_command_unlinks_its_commands_and_they_keep_their_text() {
    let s = Uuid::new_v4();
    let mut lib = vec![SharedCommand {
        deleted: true,
        ..shared(s, "./release.sh", 20)
    }];
    let mut projects = vec![project(vec![linked(Uuid::new_v4(), s, "./release.sh", 10)])];
    assert!(reconcile(&mut projects, &mut lib));
    let c = &projects[0].commands[0];
    assert_eq!(c.linked_to, None);
    assert_eq!((c.line.as_str(), c.updated_at_ms), ("./release.sh", 20));
}

#[test]
fn a_deleted_command_is_left_alone() {
    let s = Uuid::new_v4();
    let mut lib = vec![shared(s, "current", 5)];
    let gone = Command {
        deleted: true,
        ..linked(Uuid::new_v4(), s, "older edit", 9)
    };
    let mut projects = vec![project(vec![gone.clone()])];
    assert!(!reconcile(&mut projects, &mut lib));
    assert_eq!(lib, vec![shared(s, "current", 5)]);
    assert_eq!(projects[0].commands[0], gone);
}

#[test]
fn nothing_changes_when_everything_agrees() {
    let s = Uuid::new_v4();
    let mut lib = vec![shared(s, "make", 4)];
    let plain = Command {
        linked_to: None,
        ..linked(Uuid::new_v4(), s, "ls", 1)
    };
    let mut projects = vec![project(vec![linked(Uuid::new_v4(), s, "make", 4), plain])];
    let before = (projects.clone(), lib.clone());
    assert!(!reconcile(&mut projects, &mut lib));
    assert_eq!((projects, lib), before);
}

/// One computer's side of an exchange: its project and its shared commands
/// after taking in the other computer's.
fn take_in(
    mine: &(Project, Vec<SharedCommand>),
    theirs: &(Project, Vec<SharedCommand>),
) -> (Project, Vec<SharedCommand>) {
    let mut p = mine.0.clone();
    p.commands = merge_commands(&mine.0.commands, &theirs.0.commands);
    let mut lib = merge_shared(&mine.1, &theirs.1);
    let mut all = vec![p];
    reconcile(&mut all, &mut lib);
    (all.remove(0), lib)
}

#[test]
fn a_deletion_and_an_edit_on_two_computers_agree_on_the_newer_one() {
    let (s, c) = (Uuid::new_v4(), Uuid::new_v4());
    let base = project(vec![]);
    let with = |cmd: Command| Project {
        commands: vec![cmd],
        ..base.clone()
    };
    let deleted_at = |at| {
        let gone = SharedCommand {
            deleted: true,
            ..shared(s, "./release.sh", at)
        };
        let unlinked = Command {
            linked_to: None,
            ..linked(c, s, "./release.sh", at)
        };
        (with(unlinked), vec![gone])
    };
    let edited_at = |at| {
        (
            with(linked(c, s, "./release.sh v2", at)),
            vec![shared(s, "./release.sh v2", at)],
        )
    };
    // The edit came after the deletion, then before it.
    for (deleted, edited, edit_wins) in [(20, 30, true), (20, 15, false)] {
        let (a, b) = (deleted_at(deleted), edited_at(edited));
        let (here, there) = (take_in(&a, &b), take_in(&b, &a));
        assert_eq!(here, there, "both computers end up the same");
        let (p, lib) = here;
        assert_eq!(lib[0].deleted, !edit_wins);
        assert_eq!(p.commands[0].linked_to.is_some(), edit_wins);
    }
}

#[test]
fn a_cut_link_takes_the_last_text_of_the_deleted_command() {
    // Edited and then deleted on the other computer; this one only had the
    // first text.
    let s = Uuid::new_v4();
    let mut lib = vec![SharedCommand {
        deleted: true,
        ..shared(s, "./release.sh --notes", 20)
    }];
    let mut projects = vec![project(vec![linked(Uuid::new_v4(), s, "./release.sh", 10)])];
    assert!(reconcile(&mut projects, &mut lib));
    let c = &projects[0].commands[0];
    assert_eq!(c.linked_to, None);
    assert_eq!(
        (c.line.as_str(), c.updated_at_ms),
        ("./release.sh --notes", 20)
    );
}

#[test]
fn a_deletion_learned_through_another_project_still_agrees_later() {
    let (s, c) = (Uuid::new_v4(), Uuid::new_v4());
    let with = |cmd: Command| project(vec![cmd]);
    // There, Release was edited and then deleted, which cut the link.
    let gone = SharedCommand {
        deleted: true,
        ..shared(s, "./release.sh --notes", 20)
    };
    let cut = Command {
        linked_to: None,
        ..linked(c, s, "./release.sh --notes", 20)
    };
    let there = with(cut);
    // Here, the deletion arrives with another project's exchange, before
    // this project's own.
    let mut here = vec![with(linked(c, s, "./release.sh", 10))];
    let mut lib = merge_shared(&[shared(s, "./release.sh", 10)], &[gone]);
    reconcile(&mut here, &mut lib);
    assert_eq!(
        merge_commands(&here[0].commands, &there.commands),
        merge_commands(&there.commands, &here[0].commands),
        "both computers end up with the same command"
    );
}
