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
    #[serde(default)]
    pub description: Description,
}

/// Free text about a project. It travels with the project on a transfer,
/// and the newer edit wins, so pushing never undoes a later edit there.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Description {
    pub text: String,
    /// When it was last edited; 0 for never.
    pub at_ms: i64,
}

/// Long enough for notes, short enough to stay a description. Held to on
/// every way in, the other computer included.
pub const MAX_DESCRIPTION_CHARS: usize = 4000;

impl Description {
    /// `other` when it was edited later than this one, cut to length.
    pub fn newer(&self, other: &Description) -> Description {
        if other.at_ms > self.at_ms {
            Description {
                text: other.text.chars().take(MAX_DESCRIPTION_CHARS).collect(),
                at_ms: other.at_ms,
            }
        } else {
            self.clone()
        }
    }

    /// Whether this one, sent over, would change `dest`'s text.
    pub fn replaces(&self, dest: &Description) -> bool {
        self.at_ms > dest.at_ms && self.text != dest.text
    }

    /// An edit made now. Stamped after the one it replaces even when this
    /// computer's clock is behind the one that made that, so an edit made
    /// after seeing the other computer's text always wins over it.
    pub fn edited(text: &str, previous: &Description, now_ms: i64) -> Description {
        Description {
            text: text.chars().take(MAX_DESCRIPTION_CHARS).collect(),
            at_ms: now_ms.max(previous.at_ms + 1),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_edit_is_stamped_after_the_text_it_replaces() {
        // That text came from a computer whose clock runs ahead.
        let ahead = Description {
            text: "From the other computer".into(),
            at_ms: 1_000,
        };
        let mine = Description::edited("Mine", &ahead, 400);
        assert_eq!(mine.at_ms, 1_001);
        assert_eq!(ahead.newer(&mine), mine);
        assert_eq!(Description::edited("Later", &mine, 5_000).at_ms, 5_000);
    }

    #[test]
    fn a_description_from_elsewhere_is_cut_to_length() {
        let long = Description {
            text: "x".repeat(MAX_DESCRIPTION_CHARS + 50),
            at_ms: 2,
        };
        let kept = Description::default().newer(&long);
        assert_eq!(kept.text.chars().count(), MAX_DESCRIPTION_CHARS);
        assert!(long.replaces(&Description::default()));
        assert!(!Description::default().replaces(&long));
    }
}
