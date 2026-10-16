//! What some answers carry: a folder as the answering computer sees it, and
//! the projects and computers it tells about.

use crate::manifest::Manifest;
use crate::model::{Description, FolderId, InstanceId, Os, ProjectId};
use serde::{Deserialize, Serialize};

/// A folder as the responder sees it.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct FolderScan {
    pub manifest: Manifest,
    /// For display only; never used to resolve a path.
    pub path: String,
    pub exists: bool,
    /// False when the responder has no path stored for the folder and
    /// `path` is where a push would create it.
    pub set_up: bool,
    pub os: Os,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct ProjectSummary {
    pub id: ProjectId,
    pub name: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Reachable {
    pub id: InstanceId,
    pub name: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct RemoteProject {
    pub name: String,
    pub folders: Vec<RemoteFolder>,
    pub primary: FolderId,
    /// Missing from computers that don't have descriptions yet.
    #[serde(default)]
    pub description: Description,
    /// Whether the answering computer's latest transfer of this project
    /// was with the asking one. Missing from older versions.
    #[serde(default)]
    pub last_with_you: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct RemoteFolder {
    pub id: FolderId,
    pub name: String,
    /// For display only; never used to resolve a path.
    pub path: Option<String>,
    /// As `Request::Manifest::home_hint`, from the answering computer.
    #[serde(default)]
    pub home_hint: Option<String>,
    /// `path` with `~` for the answering computer's home, for display.
    #[serde(default)]
    pub shown: Option<String>,
}
