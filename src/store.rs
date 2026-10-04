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
#[path = "store_tests.rs"]
mod tests;
