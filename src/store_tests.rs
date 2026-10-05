use super::*;
use crate::model::*;
use uuid::Uuid;

#[test]
fn the_default_home_is_named_after_the_app_alone() {
    let root = default_root().unwrap();
    assert_eq!(root.file_name().unwrap(), "Project Transfer");
    #[cfg(target_os = "macos")]
    assert!(root.ends_with("Library/Application Support/Project Transfer"));
}

fn store() -> (tempfile::TempDir, Store) {
    let dir = tempfile::tempdir().unwrap();
    let s = Store::open_at(dir.path().join("home")).unwrap();
    (dir, s)
}

fn sample_project() -> Project {
    let f = Folder {
        id: Uuid::new_v4(),
        name: "app".into(),
        local_path: Some("/x/app".into()),
    };
    Project {
        id: Uuid::new_v4(),
        name: "App".into(),
        primary: f.id,
        folders: vec![
            f,
            Folder {
                id: Uuid::new_v4(),
                name: "docs".into(),
                local_path: None,
            },
        ],
        commands: vec![Command {
            id: Uuid::new_v4(),
            label: "Build".into(),
            line: "cargo build".into(),
            created_on: Os::current(),
            updated_at_ms: 5,
            deleted: false,
            last_run_hash: Some("abc".into()),
        }],
        last_transfer: Some(TransferRecord {
            at_ms: 9,
            peer: Uuid::new_v4(),
            direction: Direction::Pull,
            files: 3,
            by_peer: false,
        }),
        description: Description {
            text: "Plans the beds.\nWaters on Mondays.".into(),
            at_ms: 7,
        },
    }
}

#[test]
fn open_at_creates_dirs() {
    let (_d, s) = store();
    assert!(s.root().is_dir());
    assert!(s.identity_dir().is_dir());
    assert!(s.logs_dir().is_dir());
}

#[test]
fn instance_is_stable_across_loads() {
    let (_d, s) = store();
    let a = s.load_or_create_instance().unwrap();
    let b = s.load_or_create_instance().unwrap();
    assert_eq!(a, b);
    assert!(a.name.split(' ').count() >= 3);
    assert_eq!(a.theme, ThemeChoice::Dark);
    assert_eq!(a.port, 0);
}

#[test]
fn default_projects_folder_is_home_dev() {
    let (_d, s) = store();
    let a = s.load_or_create_instance().unwrap();
    let home = directories::UserDirs::new()
        .unwrap()
        .home_dir()
        .to_path_buf();
    assert_eq!(a.projects_folder, home.join("Dev"));
}

#[test]
fn instance_round_trips() {
    let (_d, s) = store();
    let mut a = s.load_or_create_instance().unwrap();
    a.name = "Renamed".into();
    a.theme = ThemeChoice::System;
    a.port = 4000;
    a.extra_ignores = vec!["*.tmp".into()];
    a.last_peer = Some(Uuid::new_v4());
    s.save_instance(&a).unwrap();
    assert_eq!(s.load_or_create_instance().unwrap(), a);
}

#[test]
fn peers_round_trip_and_missing_is_empty() {
    let (_d, s) = store();
    assert!(s.load_peers().unwrap().is_empty());
    let p = Peer {
        id: Uuid::new_v4(),
        name: "Other".into(),
        fingerprint: "ff".into(),
        allows: Permissions {
            may_push_to_me: true,
            may_pull_from_me: false,
        },
        granted: Permissions {
            may_push_to_me: false,
            may_pull_from_me: true,
        },
        last_address: Some("192.168.1.5:4000".parse().unwrap()),
        via: None,
    };
    s.save_peers(std::slice::from_ref(&p)).unwrap();
    assert_eq!(s.load_peers().unwrap(), vec![p]);
}

#[test]
fn peers_saved_before_relays_existed_load_as_reached_directly() {
    let (_d, s) = store();
    let old = r#"[{"id":"7d8b6c1e-2f4a-4b5c-9d6e-0f1a2b3c4d5e","name":"Desk",
        "fingerprint":"ff","allows":{"may_push_to_me":true,"may_pull_from_me":false},
        "granted":{"may_push_to_me":false,"may_pull_from_me":true},
        "last_address":"192.168.1.5:4000"}]"#;
    std::fs::write(s.root().join("peers.json"), old).unwrap();
    let peers = s.load_peers().unwrap();
    assert_eq!(peers.len(), 1);
    assert_eq!(peers[0].via, None);
    assert_eq!(
        peers[0].last_address,
        Some("192.168.1.5:4000".parse().unwrap())
    );
}

#[test]
fn projects_round_trip_and_missing_is_empty() {
    let (_d, s) = store();
    assert!(s.load_projects().unwrap().is_empty());
    let p = sample_project();
    s.save_projects(std::slice::from_ref(&p)).unwrap();
    assert_eq!(s.load_projects().unwrap(), vec![p]);
}

#[test]
fn corrupt_projects_error_names_file_and_keeps_it() {
    let (_d, s) = store();
    let path = s.root().join("projects.json");
    std::fs::write(&path, "{ not json").unwrap();
    let err = format!("{:#}", s.load_projects().unwrap_err());
    assert!(err.contains("projects.json"), "{err}");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "{ not json");
}

#[test]
fn write_atomic_replaces_and_leaves_no_temp() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("f.json");
    write_atomic(&path, b"one").unwrap();
    write_atomic(&path, b"two").unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), b"two");
    let names: Vec<_> = std::fs::read_dir(dir.path())
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(names, vec![std::ffi::OsString::from("f.json")]);
}
