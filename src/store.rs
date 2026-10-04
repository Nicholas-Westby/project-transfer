//! Reads and writes the files in the data home.

use crate::model::{InstanceSettings, Peer, Project, ThemeChoice};
use anyhow::Context;
use serde::{Serialize, de::DeserializeOwned};
use std::io::Write;
use std::path::{Path, PathBuf};

pub struct Store {
    root: PathBuf,
}

/// Where the data home is unless `PROJECT_TRANSFER_HOME` says otherwise.
pub fn default_root() -> Option<PathBuf> {
    // The local folder on Windows, not the roaming one: the identity in here
    // belongs to this computer and must never follow the user to another.
    directories::BaseDirs::new().map(|b| b.data_local_dir().join("Project Transfer"))
}

impl Store {
    pub fn open_default() -> anyhow::Result<Store> {
        let root = match std::env::var_os("PROJECT_TRANSFER_HOME") {
            Some(p) if !p.is_empty() => PathBuf::from(p),
            _ => default_root()
                .context("could not work out the data folder; set PROJECT_TRANSFER_HOME")?,
        };
        Store::open_at(root)
    }

    pub fn open_at(root: PathBuf) -> anyhow::Result<Store> {
        let store = Store { root };
        for dir in [store.root.clone(), store.identity_dir(), store.logs_dir()] {
            std::fs::create_dir_all(&dir)
                .with_context(|| format!("could not create {}", dir.display()))?;
        }
        Ok(store)
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn load_or_create_instance(&self) -> anyhow::Result<InstanceSettings> {
        if let Some(existing) = self.read_json("instance.json")? {
            return Ok(existing);
        }
        let home = directories::UserDirs::new()
            .map(|d| d.home_dir().to_path_buf())
            .context("could not find the home folder")?;
        let settings = InstanceSettings {
            id: uuid::Uuid::new_v4(),
            name: crate::naming::default_name(&mut rand::rng()),
            projects_folder: home.join("Dev"),
            extra_ignores: Vec::new(),
            always_include: Vec::new(),
            removed_default_ignores: Vec::new(),
            last_peer: None,
            theme: ThemeChoice::Dark,
            port: 0,
        };
        self.save_instance(&settings)?;
        Ok(settings)
    }

    pub fn save_instance(&self, s: &InstanceSettings) -> anyhow::Result<()> {
        self.write_json("instance.json", s)
    }

    pub fn load_peers(&self) -> anyhow::Result<Vec<Peer>> {
        Ok(self.read_json("peers.json")?.unwrap_or_default())
    }

    pub fn save_peers(&self, p: &[Peer]) -> anyhow::Result<()> {
        self.write_json("peers.json", p)
    }

    pub fn load_projects(&self) -> anyhow::Result<Vec<Project>> {
        Ok(self.read_json("projects.json")?.unwrap_or_default())
    }

    pub fn save_projects(&self, p: &[Project]) -> anyhow::Result<()> {
        self.write_json("projects.json", p)
    }

    pub fn identity_dir(&self) -> PathBuf {
        self.root.join("identity")
    }

    pub fn logs_dir(&self) -> PathBuf {
        self.root.join("logs")
    }

    /// A missing file is None; an unreadable or corrupt one is an error so the
    /// caller never overwrites the user's data with an empty list.
    fn read_json<T: DeserializeOwned>(&self, name: &str) -> anyhow::Result<Option<T>> {
        let path = self.root.join(name);
        let bytes = match std::fs::read(&path) {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e).with_context(|| format!("could not read {}", path.display())),
        };
        serde_json::from_slice(&bytes)
            .map(Some)
            .with_context(|| format!("{} is not valid; fix or delete it", path.display()))
    }

    fn write_json<T: Serialize + ?Sized>(&self, name: &str, value: &T) -> anyhow::Result<()> {
        let path = self.root.join(name);
        let bytes = serde_json::to_vec_pretty(value)?;
        write_atomic(&path, &bytes).with_context(|| format!("could not write {}", path.display()))
    }
}

/// Writes to a temp file in the same directory, syncs it, then renames over the
/// target so a crash never leaves a half-written file.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    write_atomic_with(path, bytes, false)
}

/// Like `write_atomic`, but the file is owner-only (0600 on unix) from the
/// moment it is created, so it is never visible with wider permissions.
pub fn write_atomic_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    write_atomic_with(path, bytes, true)
}

fn write_atomic_with(path: &Path, bytes: &[u8], private: bool) -> std::io::Result<()> {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let tmp = path.with_file_name(format!(".{name}.{}.tmp", uuid::Uuid::new_v4().simple()));
    let result = (|| {
        let mut opts = std::fs::OpenOptions::new();
        opts.write(true).create_new(true);
        #[cfg(unix)]
        if private {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        #[cfg(not(unix))]
        let _ = private;
        let mut f = opts.open(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        std::fs::rename(&tmp, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

#[cfg(test)]
mod tests {
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
        };
        s.save_peers(std::slice::from_ref(&p)).unwrap();
        assert_eq!(s.load_peers().unwrap(), vec![p]);
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
}
