use super::*;
use uuid::Uuid;

fn folder(name: &str, path: Option<&str>) -> Folder {
    Folder {
        id: Uuid::new_v4(),
        name: name.into(),
        local_path: path.map(PathBuf::from),
    }
}

fn project(folders: Vec<Folder>) -> Project {
    Project {
        id: Uuid::new_v4(),
        name: "Garden".into(),
        primary: folders[0].id,
        folders,
        commands: vec![],
        last_transfer: None,
    }
}

#[test]
fn default_path_single_and_multi() {
    let pf = Path::new("/dev");
    let (pid, id) = (Uuid::new_v4(), Uuid::new_v4());
    let d = |name: &str, f: &str, multi| default_path(pf, &[], pid, name, id, f, multi);
    assert_eq!(d("Garden", "app", false).unwrap(), Path::new("/dev/app"));
    assert_eq!(
        d("Garden", "app", true).unwrap(),
        Path::new("/dev/Garden/app")
    );
    assert!(d("Garden", "..", false).is_err());
    // A project name is a label; the folder made from it stays put.
    assert_eq!(d("../x", "app", true).unwrap(), Path::new("/dev/.. x/app"));
}

#[test]
fn default_path_avoids_other_projects_folders() {
    let pf = Path::new("/dev");
    let mut other = project(vec![folder("app", Some("/dev/app"))]);
    let mut nested = project(vec![folder("x", Some("/dev/Garden"))]);
    let (pid, id) = (Uuid::new_v4(), Uuid::new_v4());
    let all = vec![other.clone(), nested.clone()];
    let d = |multi| default_path(pf, &all, pid, "Garden", id, "app", multi).unwrap();
    assert_eq!(d(false), Path::new("/dev/app 2"));
    // `/dev/Garden` is another project's folder, so this project's moves.
    assert_eq!(d(true), Path::new("/dev/Garden 2/app"));

    // An existing folder of another project that contains the default.
    other.folders[0].local_path = Some("/dev".into());
    let err = default_path(pf, &[other.clone()], pid, "Garden", id, "app", false).unwrap_err();
    assert!(err.contains("/dev"), "{err}");

    // Nor inside the project's own folder: pushing that one would carry it.
    nested.id = pid;
    let own = default_path(pf, &[nested.clone()], pid, "Garden", id, "app", true).unwrap();
    assert_eq!(own, Path::new("/dev/Garden 2/app"));
    // Its own folders beside each other are fine, and a folder's own path
    // never blocks itself.
    nested.folders[0].local_path = Some("/dev/Garden/x".into());
    let beside = default_path(pf, &[nested.clone()], pid, "Garden", id, "app", true).unwrap();
    assert_eq!(beside, Path::new("/dev/Garden/app"));
    let x = nested.folders[0].id;
    let itself = default_path(pf, &[nested], pid, "Garden", x, "x", true).unwrap();
    assert_eq!(itself, Path::new("/dev/Garden/x"));
}

#[test]
fn adopt_creates_and_never_moves_a_path() {
    let incoming = project(vec![folder("app", None), folder("docs", None)]);
    let mut all = vec![project(vec![folder("app", Some("/dev/app"))])];
    adopt(&mut all, &incoming, Path::new("/dev"), &HashMap::new()).unwrap();
    assert_eq!(all[1].folders[0].local_path, Some("/dev/Garden/app".into()));
    assert_eq!(all[1].primary, incoming.primary);

    all[1].folders[0].local_path = Some("/mine".into());
    all[1].folders[1].local_path = None;
    all[1].name = "Local name".into();
    let chosen = HashMap::from([(incoming.folders[1].id, PathBuf::from("/picked"))]);
    adopt(&mut all, &incoming, Path::new("/dev"), &chosen).unwrap();
    assert_eq!(all.len(), 2);
    assert_eq!(all[1].name, "Local name");
    assert_eq!(all[1].folders[0].local_path, Some("/mine".into()));
    assert_eq!(all[1].folders[1].local_path, Some("/picked".into()));

    // A single-folder project whose default is taken gets a free name.
    let lone = project(vec![folder("app", None)]);
    adopt(&mut all, &lone, Path::new("/dev"), &HashMap::new()).unwrap();
    assert_eq!(all[2].folders[0].local_path, Some("/dev/app 2".into()));
}

#[test]
fn wire_forms_drop_local_state() {
    let mut p = project(vec![folder("app", Some("/x"))]);
    p.commands = vec![Command {
        id: Uuid::new_v4(),
        label: "b".into(),
        line: "b".into(),
        created_on: crate::model::Os::MacOs,
        updated_at_ms: 1,
        deleted: false,
        last_run_hash: Some("h".into()),
    }];
    let w = wire_project(&p);
    assert_eq!(w.folders[0].local_path, None);
    assert_eq!(w.commands[0].last_run_hash, None);
    assert_eq!(wire_commands(&p.commands)[0].last_run_hash, None);
    assert_eq!(p.commands[0].last_run_hash.as_deref(), Some("h"));
}

#[test]
fn names_are_checked_for_the_receiving_system() {
    use crate::model::Os;
    assert!(check_names_for(&["app"], Os::Windows, "Desk").is_ok());
    let err = check_names_for(&["CON"], Os::Windows, "Desk").unwrap_err();
    let err = err.to_string();
    assert!(
        err.contains("reserved name on Windows") && err.contains("Desk can't hold it"),
        "{err}"
    );
    // A Mac holds "CON" and "notes:old" fine, but no system holds a slash.
    assert!(check_names_for(&["CON", "notes:old"], Os::MacOs, "Desk").is_ok());
    assert!(check_names_for(&["a/b"], Os::MacOs, "Desk").is_err());
    // The name is checked as stored; a trailing space is not trimmed away.
    assert!(check_names_for(&["app "], Os::Windows, "Desk").is_err());
    let err = check_names_for(&["a?"], Os::Windows, "Desk").unwrap_err();
    assert!(err.to_string().contains("Remove the folder"), "{err}");
}

#[test]
fn a_project_with_several_folders_lands_in_a_folder_named_after_it() {
    let (pid, id) = (uuid::Uuid::new_v4(), uuid::Uuid::new_v4());
    let d = default_path(Path::new("/dev"), &[], pid, "What Next?", id, "app", true).unwrap();
    assert_eq!(d, Path::new("/dev/What Next/app"));
}
