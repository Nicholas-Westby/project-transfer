//! Messages exchanged between instances and their length-prefixed framing.

use crate::ignore_rules::IgnoreSpec;
use crate::manifest::Manifest;
use crate::model::{
    Command, Description, FolderId, InstanceId, Os, Permissions, Project, ProjectId,
};
use anyhow::{Context, bail};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::collections::HashMap;
use std::net::IpAddr;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

pub const PROTOCOL_VERSION: u32 = 3;
const MAX_MSG: u32 = 64 * 1024 * 1024;

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub enum Request {
    Hello {
        id: InstanceId,
        name: String,
        version: u32,
        /// The port this instance listens on; the connection's own source
        /// port is ephemeral, so the other side cannot call back on it.
        port: u16,
        /// This instance's own addresses, so the other side remembers where
        /// to call back only when the call came straight from one of them.
        /// Defaults to none so an older version still parses and is told to
        /// update.
        #[serde(default)]
        addrs: Vec<IpAddr>,
        /// The paired computer passing this call along, if any: a hint at
        /// how to reach the caller again.
        #[serde(default)]
        via: Option<InstanceId>,
    },
    /// Starts pairing. `commitment` is the hex SHA-256 of the initiator's
    /// nonce, sent before it sees the responder's. `requested` is what the
    /// initiator asks the responder to allow it; `offered` what it allows.
    PairCommit {
        commitment: String,
        requested: Permissions,
        offered: Permissions,
    },
    /// The nonce behind the commitment, in hex. Answered with `PairResult`
    /// once the responder's user has decided.
    PairReveal {
        nonce: String,
    },
    /// Whether the initiator's user confirmed the code. The responder stores
    /// the pairing only after `confirmed: true`.
    PairFinal {
        confirmed: bool,
    },
    Status,
    /// Asks the answering computer to pass this connection along to `to`,
    /// another computer it is paired with. After `Ok`, the bytes both ways
    /// are the caller's own session with `to`, copied untouched.
    Relay {
        to: InstanceId,
    },
    ProjectInfo {
        project: ProjectId,
    },
    Manifest {
        project: ProjectId,
        folder: FolderId,
        ignore: IgnoreSpec,
        folder_name: String,
        project_name: String,
        /// Whether the initiator's project has more than one folder, which
        /// decides the default path on a computer that has not set it up.
        multi_folder: bool,
        /// The asking computer's system, so a Mac answers with the spelling that
        /// system expects. Missing from older versions, which get names as stored.
        #[serde(default)]
        from_os: Option<Os>,
    },
    /// Hashes of files in the folder the same connection last scanned, or the
    /// responder's own folder.
    Hashes {
        project: ProjectId,
        folder: FolderId,
        paths: Vec<String>,
    },
    /// Pull: the reply is `FileHeader` followed by raw bytes.
    GetFile {
        project: ProjectId,
        folder: FolderId,
        rel: String,
    },
    /// The sender's project with `local_path` cleared. `expected_path` is
    /// the destination the confirmed preview showed for this folder; the
    /// receiver refuses if its folder is somewhere else now.
    BeginPush {
        project: Project,
        folder: FolderId,
        expected_path: String,
    },
    /// Followed by `size` raw bytes.
    PutFile {
        rel: String,
        size: u64,
        mtime_ms: i64,
        exec: bool,
    },
    MakeDir {
        rel: String,
    },
    /// Fixes a file whose content already matches, without sending it.
    SetMtime {
        rel: String,
        mtime_ms: i64,
    },
    MakeSymlink {
        rel: String,
        target: String,
    },
    Remove {
        rel: String,
        is_dir: bool,
    },
    EndPush,
    /// Tells the source of a pull that it finished, so it records it.
    EndPull {
        project: ProjectId,
        files: u64,
    },
    /// Commands without `last_run_hash`.
    ExchangeCommands {
        project: ProjectId,
        commands: Vec<Command>,
    },
}

impl Request {
    /// Its name alone, for logs: a payload can be large or hold paths.
    pub fn kind(&self) -> &'static str {
        match self {
            Request::Hello { .. } => "Hello",
            Request::PairCommit { .. } => "PairCommit",
            Request::PairReveal { .. } => "PairReveal",
            Request::PairFinal { .. } => "PairFinal",
            Request::Status => "Status",
            Request::Relay { .. } => "Relay",
            Request::ProjectInfo { .. } => "ProjectInfo",
            Request::Manifest { .. } => "Manifest",
            Request::Hashes { .. } => "Hashes",
            Request::GetFile { .. } => "GetFile",
            Request::BeginPush { .. } => "BeginPush",
            Request::PutFile { .. } => "PutFile",
            Request::MakeDir { .. } => "MakeDir",
            Request::SetMtime { .. } => "SetMtime",
            Request::MakeSymlink { .. } => "MakeSymlink",
            Request::Remove { .. } => "Remove",
            Request::EndPush => "EndPush",
            Request::EndPull { .. } => "EndPull",
            Request::ExchangeCommands { .. } => "ExchangeCommands",
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub enum Response {
    Hello {
        id: InstanceId,
        name: String,
        version: u32,
    },
    /// The responder's nonce, in hex, sent as soon as the commitment arrives.
    PairNonce {
        nonce: String,
    },
    /// `granted` is what the responder allows the requester.
    PairResult {
        accepted: bool,
        granted: Permissions,
    },
    Status {
        allows: Permissions,
        projects: Vec<ProjectSummary>,
        /// The answering computer's other paired computers that it sees on
        /// the network right now, and so can pass a connection along to.
        #[serde(default)]
        reachable: Vec<Reachable>,
    },
    ProjectInfo(Option<RemoteProject>),
    Manifest(FolderScan),
    Hashes(HashMap<String, String>),
    FileHeader {
        size: u64,
        mtime_ms: i64,
        exec: bool,
    },
    /// The merged list, without `last_run_hash`.
    Commands(Vec<Command>),
    Ok,
    Refused {
        reason: String,
    },
}

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
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct RemoteFolder {
    pub id: FolderId,
    pub name: String,
    /// For display only; never used to resolve a path.
    pub path: Option<String>,
}

/// u32 big-endian length, then JSON.
pub async fn write_msg<W: AsyncWrite + Unpin, T: Serialize>(
    w: &mut W,
    msg: &T,
) -> anyhow::Result<()> {
    let mut framed = vec![0u8; 4];
    serde_json::to_writer(&mut framed, msg)?;
    let len = framed.len() - 4;
    if len as u64 > MAX_MSG as u64 {
        bail!("message of {len} bytes is over the 64 MiB limit");
    }
    framed[..4].copy_from_slice(&(len as u32).to_be_bytes());
    // One write: over TLS a write is a record, and without Nagle a packet.
    w.write_all(&framed).await?;
    w.flush().await?;
    Ok(())
}

pub async fn read_msg<R: AsyncRead + Unpin, T: DeserializeOwned>(r: &mut R) -> anyhow::Result<T> {
    read_msg_limited(r, MAX_MSG).await
}

/// Like `read_msg`, with a lower cap for callers that expect only small messages.
pub async fn read_msg_limited<R: AsyncRead + Unpin, T: DeserializeOwned>(
    r: &mut R,
    max: u32,
) -> anyhow::Result<T> {
    let mut len = [0u8; 4];
    r.read_exact(&mut len).await?;
    let len = u32::from_be_bytes(len);
    // Checked before allocating so a bad peer cannot make us reserve gigabytes.
    if len > max.min(MAX_MSG) {
        bail!(
            "incoming message of {len} bytes is over the {} limit",
            size(max.min(MAX_MSG))
        );
    }
    let mut body = vec![0u8; len as usize];
    r.read_exact(&mut body).await?;
    serde_json::from_slice(&body).context("could not parse message")
}

fn size(bytes: u32) -> String {
    match bytes {
        b if b % (1024 * 1024) == 0 => format!("{} MiB", b / (1024 * 1024)),
        b if b % 1024 == 0 => format!("{} KiB", b / 1024),
        b => format!("{b} bytes"),
    }
}

#[cfg(test)]
#[path = "protocol_tests.rs"]
mod tests;
