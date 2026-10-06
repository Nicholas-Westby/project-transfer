use super::*;
use crate::model::Folder;

#[test]
fn a_hint_on_the_wire_turns_into_this_systems_parts_under_home() {
    let home = Path::new("home-root");
    let p = resolve(Some(home), ".microsoft/usersecrets/abc").unwrap();
    let parts: Vec<_> = p.components().map(|c| c.as_os_str().to_owned()).collect();
    assert_eq!(parts, ["home-root", ".microsoft", "usersecrets", "abc"]);
    assert_eq!(resolve(None, "a/b"), None);
}

#[test]
fn a_hint_that_could_leave_home_or_name_a_drive_is_ignored() {
    let home = Path::new("h");
    for bad in [
        "", "/etc", "\\x", "a//b", "a/./b", "../x", "a/..", "C:/x", "a\\b", "a/c:d", "a/",
    ] {
        assert_eq!(resolve(Some(home), bad), None, "{bad:?}");
    }
}

#[cfg(unix)]
#[test]
fn a_mac_folder_under_home_but_outside_projects_gets_a_hint() {
    let home = Path::new("/Users/me");
    let pf = Path::new("/Users/me/Dev");
    let hint = |p: &str| home_hint(Path::new(p), pf, home);
    assert_eq!(
        hint("/Users/me/.microsoft/usersecrets/abc").as_deref(),
        Some(".microsoft/usersecrets/abc")
    );
    assert_eq!(hint("/Users/me/Dev/app"), None, "inside Projects");
    assert_eq!(hint("/Users/me"), None, "the home itself");
    assert_eq!(hint("/Volumes/USB/app"), None, "outside home");
    assert_eq!(hint("/Users/meg/app"), None, "a sibling, not inside");
    assert_eq!(hint("/Users/me/odd:name"), None, "Windows can't hold it");
}

#[cfg(windows)]
#[test]
fn a_windows_folder_under_home_but_outside_projects_gets_a_hint() {
    let home = Path::new(r"C:\Users\me");
    let pf = Path::new(r"C:\Users\me\Dev");
    let hint = |p: &str| home_hint(Path::new(p), pf, home);
    assert_eq!(
        hint(r"C:\Users\me\.microsoft\usersecrets\abc").as_deref(),
        Some(".microsoft/usersecrets/abc")
    );
    assert_eq!(hint(r"C:\Users\me\Dev\app"), None);
    assert_eq!(hint(r"C:\Users\me"), None);
    assert_eq!(hint(r"D:\work\app"), None);
    assert_eq!(
        resolve(Some(home), ".microsoft/usersecrets/abc").unwrap(),
        PathBuf::from(r"C:\Users\me\.microsoft\usersecrets\abc")
    );
}

#[test]
fn hints_cover_only_folders_with_a_path_under_home() {
    let home = std::env::temp_dir().join("pt-home");
    let pf = home.join("Dev");
    let folder = |n: u128, path: Option<PathBuf>| Folder {
        id: uuid::Uuid::from_u128(n),
        name: format!("f{n}"),
        local_path: path,
    };
    let p = Project {
        id: uuid::Uuid::from_u128(9),
        name: "P".into(),
        folders: vec![
            folder(1, Some(home.join(".config").join("tool"))),
            folder(2, Some(pf.join("app"))),
            folder(3, None),
        ],
        primary: uuid::Uuid::from_u128(1),
        commands: vec![],
        last_transfer: None,
        description: Default::default(),
    };
    let hints = hints_for(&p, &pf, Some(&home));
    assert_eq!(hints.len(), 1);
    assert_eq!(hints[&uuid::Uuid::from_u128(1)], ".config/tool");
    assert!(hints_for(&p, &pf, None).is_empty());
}

#[test]
fn a_tilde_stands_for_home_when_typed_and_when_shown() {
    let home = std::env::temp_dir().join("pt-home");
    let sep = std::path::MAIN_SEPARATOR;
    assert_eq!(expand("~", Some(&home)), home);
    assert_eq!(expand("~/a/b", Some(&home)), home.join("a/b"));
    assert_eq!(expand("~a", Some(&home)), PathBuf::from("~a"));
    assert_eq!(expand("~/a", None), PathBuf::from("~/a"));
    assert_eq!(shown(&home.join("a"), Some(&home)), format!("~{sep}a"));
    assert_eq!(shown(&home, Some(&home)), "~");
    let elsewhere = std::env::temp_dir().join("other");
    assert_eq!(
        shown(&elsewhere, Some(&home)),
        elsewhere.display().to_string()
    );
}
