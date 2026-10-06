//! The snapshot the UI draws each frame, and the handle that changes it.

use crate::discovery::Discovered;
use crate::model::{
    CommandId, InstanceId, InstanceSettings, Peer, Permissions, Project, ProjectId,
};
use crate::protocol::RemoteProject;
use crate::transfer::{Preview, Summary};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

/// The activity strip shows recent lines; older ones live in the log file.
pub const ACTIVITY_CAP: usize = 500;
/// Output lines kept per command; the oldest go first.
pub const OUTPUT_CAP: usize = 2000;

#[derive(Clone, Debug)]
pub struct UiState {
    pub me: InstanceSettings,
    /// The port this instance listens on, for "Add by address" on another computer.
    pub port: u16,
    pub peers: Vec<PeerView>,
    pub discovered: Vec<Discovered>,
    pub projects: Vec<Project>,
    pub selected_peer: Option<InstanceId>,
    pub activity: Vec<ActivityLine>,
    /// Another computer asks to pair; answer with `Action::AnswerPair`.
    pub pair_prompt: Option<PairPromptView>,
    /// A pairing this computer started.
    pub pairing: Option<OutgoingPairView>,
    /// Why the last "Add by address" failed, for an inline error.
    pub address_error: Option<String>,
    pub transfer: TransferState,
    /// The selected peer's projects, as of its last status answer.
    pub remote_projects: HashMap<ProjectId, RemoteProject>,
    pub command_runs: HashMap<CommandId, CommandRun>,
}

impl UiState {
    pub fn new(me: InstanceSettings, port: u16) -> UiState {
        UiState {
            me,
            port,
            peers: Vec::new(),
            discovered: Vec::new(),
            projects: Vec::new(),
            selected_peer: None,
            activity: Vec::new(),
            pair_prompt: None,
            pairing: None,
            address_error: None,
            transfer: TransferState::Idle,
            remote_projects: HashMap::new(),
            command_runs: HashMap::new(),
        }
    }

    pub fn peer(&self, id: InstanceId) -> Option<&PeerView> {
        self.peers.iter().find(|p| p.peer.id == id)
    }

    pub fn project(&self, id: ProjectId) -> Option<&Project> {
        self.projects.iter().find(|p| p.id == id)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct PeerView {
    pub peer: Peer,
    /// Answered the last status request.
    pub online: bool,
    /// Where it answered, or where it was last reached.
    pub address: Option<SocketAddr>,
    pub last_seen_ms: Option<i64>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub enum TransferState {
    #[default]
    Idle,
    Preparing,
    Ready(Preview),
    /// `done` and `total` are bytes of file content; `files` is how many
    /// files and links the transfer writes.
    Running {
        done: u64,
        total: u64,
        files: u64,
        current: String,
        /// When it began, for the elapsed time.
        started: Instant,
        /// The estimate of the time still needed, once there is one.
        left: Option<Duration>,
    },
    Finished(Summary),
    Failed(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActivityKind {
    Info,
    Warn,
    Error,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ActivityLine {
    pub at_ms: i64,
    pub text: String,
    pub kind: ActivityKind,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PairPromptView {
    pub from_id: InstanceId,
    pub from_name: String,
    pub code: String,
    /// What the other computer asks this one to allow it.
    pub requested: Permissions,
    /// What the other computer allows this one.
    pub offered: Permissions,
    pub state: PromptState,
}

#[derive(Clone, Debug, PartialEq)]
pub enum PromptState {
    /// Waiting for this computer's user to accept or decline.
    Asking,
    /// Accepted here; waiting for the other computer's user to confirm the code.
    Waiting,
    Done,
    Failed(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct OutgoingPairView {
    pub target_name: String,
    /// Shown once the connection is up, so both screens can be compared.
    pub code: Option<String>,
    pub state: PairState,
    /// Someone on the other computer accepted.
    pub other_accepted: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum PairState {
    Connecting,
    /// The code is shown; this computer's user says whether it matches.
    Confirm,
    /// Confirmed here; waiting for someone to accept on the other computer.
    Waiting,
    Done,
    Failed(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct CommandRun {
    pub lines: Vec<OutputLine>,
    pub status: RunStatus,
}

#[derive(Clone, Debug, PartialEq)]
pub struct OutputLine {
    pub stderr: bool,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq)]
pub enum RunStatus {
    Running,
    /// None when the process was stopped by a signal.
    Exited(Option<i32>),
    Failed(String),
}

impl CommandRun {
    pub fn push(&mut self, line: OutputLine) {
        self.lines.push(line);
        if self.lines.len() > OUTPUT_CAP {
            let extra = self.lines.len() - OUTPUT_CAP;
            self.lines.drain(..extra);
        }
    }
}

/// Shared by the UI thread and the background tasks. Every change goes
/// through `update`, which asks egui to draw again.
#[derive(Clone)]
pub(crate) struct StateHandle {
    inner: Arc<Mutex<UiState>>,
    ctx: Option<egui::Context>,
}

impl StateHandle {
    pub(crate) fn new(state: UiState, ctx: Option<egui::Context>) -> StateHandle {
        StateHandle {
            inner: Arc::new(Mutex::new(state)),
            ctx,
        }
    }

    /// A panic elsewhere must not take the window down with it.
    pub(crate) fn lock(&self) -> MutexGuard<'_, UiState> {
        self.inner.lock().unwrap_or_else(|p| p.into_inner())
    }

    pub(crate) fn update<T>(&self, f: impl FnOnce(&mut UiState) -> T) -> T {
        let out = f(&mut self.lock());
        if let Some(ctx) = &self.ctx {
            ctx.request_repaint();
        }
        out
    }

    pub(crate) fn info(&self, text: impl Into<String>) {
        self.activity(ActivityKind::Info, text.into());
    }

    pub(crate) fn warn(&self, text: impl Into<String>) {
        self.activity(ActivityKind::Warn, text.into());
    }

    pub(crate) fn error(&self, text: impl Into<String>) {
        self.activity(ActivityKind::Error, text.into());
    }

    fn activity(&self, kind: ActivityKind, text: String) {
        let line = ActivityLine {
            at_ms: crate::transfer::projects::now_ms(),
            text,
            kind,
        };
        let repeat = self.update(|s| {
            // A peer that keeps failing would otherwise fill the strip.
            if let Some(last) = s.activity.last_mut()
                && last.text == line.text
                && last.kind == line.kind
            {
                last.at_ms = line.at_ms;
                return true;
            }
            s.activity.push(line.clone());
            if s.activity.len() > ACTIVITY_CAP {
                let extra = s.activity.len() - ACTIVITY_CAP;
                s.activity.drain(..extra);
            }
            false
        });
        // ...and the log, which has no line to fold a repeat into.
        if !repeat {
            match kind {
                ActivityKind::Info => tracing::info!("{}", line.text),
                ActivityKind::Warn => tracing::warn!("{}", line.text),
                ActivityKind::Error => tracing::error!("{}", line.text),
            }
        }
    }
}
