use super::*;
use crate::model::{Command, Os};

#[test]
fn create_project_makes_the_first_folder_primary_and_saves() {
    let f = Fixture::new();
    let p = f.create("  Garden  ", "app");
    assert_eq!(p.name, "Garden");
    assert_eq!(p.folders.len(), 1);
    assert_eq!(p.folders[0].name, "app");
    assert_eq!(
        p.folders[0].local_path.as_deref(),
        Some(f.folder("app").as_path())
    );
    assert_eq!(p.primary, p.folders[0].id);
    assert_eq!(f.store().load_projects().unwrap(), vec![p]);
    assert_eq!(f.last_activity().kind, ActivityKind::Info);
}

#[test]
fn create_project_refuses_a_blank_name_or_missing_folder() {
    let f = Fixture::new();
    f.act(Action::CreateProject {
        name: " ".into(),
        folder: f.folder("app"),
    });
    assert_eq!(f.last_activity().text, "Enter a name for the project.");
    f.act(Action::CreateProject {
        name: "X".into(),
        folder: f.dir.path().join("nope"),
    });
    assert_eq!(f.last_activity().kind, ActivityKind::Error);
    assert!(f.core.state().projects.is_empty());
}

#[test]
fn add_folder_keeps_the_primary_and_suffixes_a_clashing_name() {
    let f = Fixture::new();
    let p = f.create("Garden", "app");
    let other = f.dir.path().join("elsewhere/app");
    std::fs::create_dir_all(&other).unwrap();
    f.act(Action::AddFolder(p.id, f.folder("docs")));
    f.act(Action::AddFolder(p.id, other));
    let q = f.project();
    let names: Vec<_> = q.folders.iter().map(|x| x.name.as_str()).collect();
    assert_eq!(names, ["app", "docs", "app 2"]);
    assert_eq!(q.primary, p.primary);
    assert_eq!(f.store().load_projects().unwrap()[0], q);
}

#[test]
fn a_folder_inside_another_projects_folder_is_refused() {
    let f = Fixture::new();
    let p = f.create("Garden", "app");
    let inner = f.folder("app/sub");
    f.act(Action::AddFolder(p.id, inner));
    assert_eq!(f.project().folders.len(), 1);
    let line = f.last_activity();
    assert_eq!(line.kind, ActivityKind::Error);
    assert!(line.text.contains("overlaps"), "{}", line.text);
}

#[test]
fn removing_the_primary_moves_it_to_the_next_folder() {
    let f = Fixture::new();
    let p = f.create("Garden", "app");
    f.act(Action::AddFolder(p.id, f.folder("docs")));
    f.act(Action::AddFolder(p.id, f.folder("tools")));
    let docs = f.project().folders[1].id;
    f.act(Action::RemoveFolder(p.id, p.primary));
    let q = f.project();
    assert_eq!(q.folders.len(), 2);
    assert_eq!(q.primary, docs);
    assert!(f.folder("app").is_dir(), "files stay on disk");
    // The last folder in the list hands primary back to the one before it.
    let tools = q.folders[1].id;
    f.act(Action::SetPrimary(p.id, tools));
    f.act(Action::RemoveFolder(p.id, tools));
    assert_eq!(f.project().primary, docs);
    assert_eq!(f.store().load_projects().unwrap()[0].primary, docs);
}

#[test]
fn the_last_folder_cannot_be_removed() {
    let f = Fixture::new();
    let p = f.create("Garden", "app");
    f.act(Action::RemoveFolder(p.id, p.primary));
    assert_eq!(f.project(), p);
    let line = f.last_activity();
    assert_eq!(line.kind, ActivityKind::Error);
    assert!(line.text.contains("at least one folder"), "{}", line.text);
}

#[test]
fn set_primary_only_accepts_a_folder_of_the_project() {
    let f = Fixture::new();
    let p = f.create("Garden", "app");
    f.act(Action::SetPrimary(p.id, uuid::Uuid::new_v4()));
    assert_eq!(f.project().primary, p.primary);
    assert_eq!(f.last_activity().kind, ActivityKind::Error);
}

#[test]
fn delete_project_forgets_it_but_keeps_the_files() {
    let f = Fixture::new();
    let p = f.create("Garden", "app");
    std::fs::write(f.folder("app").join("keep.txt"), "x").unwrap();
    f.act(Action::DeleteProject(p.id));
    assert!(f.core.state().projects.is_empty());
    assert!(f.store().load_projects().unwrap().is_empty());
    assert_eq!(
        std::fs::read_to_string(f.folder("app").join("keep.txt")).unwrap(),
        "x"
    );
}

#[test]
fn rename_and_folder_path_changes_persist() {
    let f = Fixture::new();
    let p = f.create("Garden", "app");
    f.act(Action::RenameProject(p.id, "Web".into()));
    let moved = f.folder("moved");
    f.act(Action::SetFolderPath(p.id, p.primary, moved.clone()));
    let q = f.store().load_projects().unwrap().remove(0);
    assert_eq!(q.name, "Web");
    assert_eq!(q.folders[0].local_path, Some(moved));
}

fn live(p: &crate::model::Project) -> Vec<Command> {
    p.commands.iter().filter(|c| !c.deleted).cloned().collect()
}

#[test]
fn commands_are_added_edited_and_deleted_with_fresh_times() {
    let f = Fixture::new();
    let p = f.create("Garden", "app");
    let before = crate::transfer::projects::now_ms();
    f.act(Action::AddCommand(
        p.id,
        "Build".into(),
        "cargo build".into(),
    ));
    let c = f.project().commands[0].clone();
    assert_eq!(
        (c.label.as_str(), c.line.as_str()),
        ("Build", "cargo build")
    );
    assert_eq!(c.created_on, Os::current());
    assert!(c.updated_at_ms >= before && !c.deleted && c.last_run_hash.is_none());

    std::thread::sleep(Duration::from_millis(5));
    f.act(Action::EditCommand(
        p.id,
        c.id,
        "Test".into(),
        "cargo test".into(),
    ));
    let e = f.project().commands[0].clone();
    assert_eq!((e.label.as_str(), e.line.as_str()), ("Test", "cargo test"));
    assert!(e.updated_at_ms > c.updated_at_ms);

    std::thread::sleep(Duration::from_millis(5));
    f.act(Action::DeleteCommand(p.id, c.id));
    let d = f.project().commands[0].clone();
    assert!(d.deleted, "a deleted marker stays so the deletion spreads");
    assert!(d.updated_at_ms > e.updated_at_ms);
    assert!(live(&f.project()).is_empty());
    assert_eq!(f.store().load_projects().unwrap()[0].commands[0], d);
}

#[test]
fn names_other_computers_could_not_use_are_refused() {
    let f = Fixture::new();
    f.act(Action::CreateProject {
        name: "a/b".into(),
        folder: f.folder("app"),
    });
    assert!(f.core.state().projects.is_empty());
    let line = f.last_activity();
    assert_eq!(line.kind, ActivityKind::Error);
    assert!(
        line.text.starts_with("Project names can't contain / or \\"),
        "{}",
        line.text
    );

    let p = f.create("Garden", "app");
    f.act(Action::RenameProject(p.id, "CON".into()));
    assert_eq!(f.project().name, "Garden");
    assert!(f.last_activity().text.contains("reserved name on Windows"));

    // A folder's name travels too, so one Windows can't hold is refused.
    f.act(Action::AddFolder(p.id, f.folder("notes:old")));
    assert_eq!(f.project().folders.len(), 1);
    let line = f.last_activity().text;
    assert!(
        line.contains("Folder name `notes:old`") && line.contains("Rename the folder"),
        "{line}"
    );
}

#[test]
fn the_home_folder_and_disk_roots_are_not_project_folders() {
    let f = Fixture::new();
    let home = directories::BaseDirs::new()
        .unwrap()
        .home_dir()
        .to_path_buf();
    let root = home.ancestors().last().unwrap().to_path_buf();
    for (path, says) in [(home, "home folder"), (root, "top of a disk")] {
        f.act(Action::CreateProject {
            name: "Everything".into(),
            folder: path.clone(),
        });
        let line = f.last_activity();
        assert_eq!(line.kind, ActivityKind::Error, "{path:?}");
        assert!(line.text.contains(says), "{}", line.text);
    }
    assert!(f.core.state().projects.is_empty());
}

#[test]
fn a_folder_name_with_a_trailing_space_is_refused() {
    let f = Fixture::new();
    let path = f.folder("app ");
    f.act(Action::CreateProject {
        name: "Garden".into(),
        folder: path,
    });
    let line = f.last_activity();
    assert_eq!(line.kind, ActivityKind::Error);
    assert!(line.text.contains("Windows"), "{}", line.text);
    assert!(f.core.state().projects.is_empty());
}
