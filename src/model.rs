//! Persisted data types shared by the store, the sync code and the UI.

use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::path::PathBuf;

pub type InstanceId = uuid::Uuid;
pub type ProjectId = uuid::Uuid;
pub type FolderId = uuid::Uuid;
pub type CommandId = uuid::Uuid;

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub enum ThemeChoice {
    #[default]
    Dark,
    Light,
    System,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct InstanceSettings {
    pub id: InstanceId,
    pub name: String,
    pub projects_folder: PathBuf,
    #[serde(default)]
    pub extra_ignores: Vec<String>,
    #[serde(default)]
    pub always_include: Vec<String>,
    #[serde(default)]
    pub removed_default_ignores: Vec<String>,
    #[serde(default)]
    pub last_peer: Option<InstanceId>,
    #[serde(default)]
    pub theme: ThemeChoice,
    /// 0 means pick a free port at start-up.
    #[serde(default)]
    pub port: u16,
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Permissions {
    pub may_push_to_me: bool,
    pub may_pull_from_me: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Peer {
    pub id: InstanceId,
    pub name: String,
    /// Hex sha256 of the peer's certificate DER.
    pub fingerprint: String,
    /// What this instance allows the peer to do.
    pub allows: Permissions,
    /// What the peer said it allows us, as last reported.
    pub granted: Permissions,
    pub last_address: Option<SocketAddr>,
    /// The paired computer that passes connections to this peer along, for
    /// a peer this computer can't reach directly.
    #[serde(default)]
    pub via: Option<InstanceId>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Folder {
    pub id: FolderId,
    pub name: String,
    pub local_path: Option<PathBuf>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum Os {
    MacOs,
    Windows,
}

impl Os {
    pub fn current() -> Os {
        if cfg!(windows) {
            Os::Windows
        } else {
            Os::MacOs
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Command {
    pub id: CommandId,
    pub label: String,
    pub line: String,
    pub created_on: Os,
    pub updated_at_ms: i64,
    pub deleted: bool,
    /// Local only: the network layer strips this before sending a command.
    #[serde(default)]
    pub last_run_hash: Option<String>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum Direction {
    Push,
    Pull,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct TransferRecord {
    pub at_ms: i64,
    pub peer: InstanceId,
    pub direction: Direction,
    pub files: u64,
    /// The peer started it and this computer answered, so `direction` is
    /// the peer's: a push by the peer landed here.
    #[serde(default)]
    pub by_peer: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Project {
    pub id: ProjectId,
    pub name: String,
    pub folders: Vec<Folder>,
    /// The folder shown first; commands run in it.
    pub primary: FolderId,
    pub commands: Vec<Command>,
    pub last_transfer: Option<TransferRecord>,
}
