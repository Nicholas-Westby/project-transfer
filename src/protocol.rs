//! Messages exchanged between instances and their length-prefixed framing.

use crate::ignore_rules::IgnoreSpec;
use crate::model::{
    Command, FolderId, InstanceId, Os, Permissions, Project, ProjectId, SharedCommand,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::IpAddr;

#[path = "protocol_framing.rs"]
mod framing;
pub use framing::{read_msg, read_msg_limited, write_msg};

#[path = "protocol_types.rs"]
mod types;
pub use types::{FolderScan, ProjectSummary, Reachable, RemoteFolder, RemoteProject};

pub const PROTOCOL_VERSION: u32 = 3;
const MAX_MSG: u32 = 64 * 1024 * 1024;
/// The most of a file one `ReadFile` answer carries, and so the most the
/// file viewer shows of each copy.
pub const MAX_PART: u64 = 1024 * 1024;

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
        /// The folder's place under the asker's home (see `transfer::home`),
        /// which a computer without a path for it uses when it is free.
        #[serde(default)]
        home_hint: Option<String>,
        /// Ignored folders come back named in `ignored_dirs` rather than
        /// counted, since counting walks all of them; the asker counts the
        /// few a removal needs with `CountFiles`. Older versions don't ask,
        /// and get every one counted.
        #[serde(default)]
        name_ignored_dirs: bool,
    },
    /// Hashes of files in the folder the same connection last scanned, or the
    /// responder's own folder.
    Hashes {
        project: ProjectId,
        folder: FolderId,
        paths: Vec<String>,
    },
    /// The files at any depth in each of `dirs`, folders inside the folder
    /// `Hashes` would read. Answered with `FileCounts`. Sent only to a
    /// computer whose scan named its ignored folders, which knows it.
    CountFiles {
        project: ProjectId,
        folder: FolderId,
        dirs: Vec<String>,
    },
    /// Pull: the reply is `FileHeader` followed by raw bytes.
    GetFile {
        project: ProjectId,
        folder: FolderId,
        rel: String,
    },
    /// The start of a file, to show it: answered with `FilePart` and then
    /// `len` raw bytes, or `Refused`. A version that doesn't know this
    /// request closes the connection instead.
    ReadFile {
        project: ProjectId,
        folder: FolderId,
        rel: String,
        /// The most bytes to send; never more than `MAX_PART` are.
        max: u64,
    },
    /// The sender's project with `local_path` cleared. `expected_path` is
    /// the destination the confirmed preview showed for this folder; the
    /// receiver refuses if its folder is somewhere else now.
    BeginPush {
        project: Project,
        folder: FolderId,
        expected_path: String,
        /// As `Manifest::home_hint`, for each folder that has one.
        #[serde(default)]
        home_hints: HashMap<FolderId, String>,
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
    /// Commands without `last_run_hash`, and every shared command the asker
    /// knows. Older versions send no shared commands, and get `Commands`.
    ExchangeCommands {
        project: ProjectId,
        commands: Vec<Command>,
        #[serde(default)]
        shared: Option<Vec<SharedCommand>>,
    },
    /// The project's details without any files, as in `BeginPush`: the
    /// receiver takes in its folders, description and commands and answers
    /// `ProjectInfo` with what it has now.
    SyncProject {
        project: Project,
        #[serde(default)]
        home_hints: HashMap<FolderId, String>,
    },
    /// Where the receiver keeps one folder of a project, as typed; a
    /// leading `~` is the receiver's home. Nothing on disk changes.
    SetFolderPath {
        project: ProjectId,
        folder: FolderId,
        path: String,
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
            Request::CountFiles { .. } => "CountFiles",
            Request::GetFile { .. } => "GetFile",
            Request::ReadFile { .. } => "ReadFile",
            Request::BeginPush { .. } => "BeginPush",
            Request::PutFile { .. } => "PutFile",
            Request::MakeDir { .. } => "MakeDir",
            Request::SetMtime { .. } => "SetMtime",
            Request::MakeSymlink { .. } => "MakeSymlink",
            Request::Remove { .. } => "Remove",
            Request::EndPush => "EndPush",
            Request::EndPull { .. } => "EndPull",
            Request::ExchangeCommands { .. } => "ExchangeCommands",
            Request::SyncProject { .. } => "SyncProject",
            Request::SetFolderPath { .. } => "SetFolderPath",
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
    /// How many files each folder `CountFiles` asked about holds, by path.
    FileCounts(HashMap<String, u64>),
    FileHeader {
        size: u64,
        mtime_ms: i64,
        exec: bool,
    },
    /// The start of a file: `size` is the whole file's, and `len` bytes
    /// from its start follow.
    FilePart {
        size: u64,
        mtime_ms: i64,
        len: u64,
    },
    /// The merged list, without `last_run_hash`.
    Commands(Vec<Command>),
    /// `Commands` with every shared command the answering computer has
    /// after the merge, for an asker that sent its own.
    CommandsWithShared {
        commands: Vec<Command>,
        shared: Vec<SharedCommand>,
    },
    Ok,
    Refused {
        reason: String,
    },
}

#[cfg(test)]
#[path = "protocol_tests.rs"]
mod tests;
