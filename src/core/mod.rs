//! The bridge between the window and everything that touches the disk or the
//! network. The UI reads a snapshot (`state`) and sends requests (`act`);
//! the work itself runs on a tokio runtime this type owns, so the window
//! never waits for it.

mod actions;
mod found;
mod pairing;
mod peers;
mod poll;
mod progress;
mod projects;
mod reach;
mod runs;
mod settings;
mod state;
mod transfers;

#[cfg(test)]
mod tests;

pub use actions::Action;
pub use state::{
    ACTIVITY_CAP, ActivityKind, ActivityLine, CommandRun, OUTPUT_CAP, OutgoingPairView, OutputLine,
    PairPromptView, PairState, PeerView, PromptState, RunStatus, TransferState, UiState,
};

use crate::address::DEFAULT_PORT;
use crate::commands::Running;
use crate::discovery::Discovery;
use crate::identity::Identity;
use crate::model::{CommandId, InstanceId};
use crate::net::{NetEvent, Shared};
use crate::store::Store;
use anyhow::Context;
use state::StateHandle;
use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};
use tokio::sync::{Notify, RwLock, mpsc};
use tracing::{error, info, warn};

/// How the core listens; the defaults are what the app uses.
#[derive(Clone, Debug)]
pub struct StartOptions {
    /// Announce and browse with mDNS. Tests turn it off and add by address.
    pub discovery: bool,
    pub bind_ip: IpAddr,
    /// Tried first when the saved port is 0; 0 here means any free port.
    pub preferred_port: u16,
}

impl Default for StartOptions {
    fn default() -> StartOptions {
        StartOptions {
            discovery: true,
            bind_ip: IpAddr::V4(Ipv4Addr::UNSPECIFIED),
            preferred_port: DEFAULT_PORT,
        }
    }
}

pub struct AppCore {
    runtime: Option<tokio::runtime::Runtime>,
    tx: mpsc::UnboundedSender<Msg>,
    core: Core,
    port: u16,
}

enum Msg {
    Act(Action),
    Settle(std::sync::mpsc::Sender<()>),
}

/// Everything the background tasks share. Cheap to clone.
#[derive(Clone)]
struct Core {
    shared: Shared,
    ui: StateHandle,
    discovery: Arc<Mutex<Option<Discovery>>>,
    incoming: Arc<Mutex<Option<pairing::Incoming>>>,
    outgoing: Arc<Mutex<Option<pairing::Outgoing>>>,
    transfer: Arc<Mutex<transfers::Slot>>,
    runs: Arc<Mutex<HashMap<CommandId, Running>>>,
    poll_now: Arc<Notify>,
    /// The last poll failure shown per peer, so it is said once.
    poll_problems: Arc<Mutex<HashMap<InstanceId, String>>>,
}

/// A panic in one task must not wedge every other user of the lock.
fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

impl AppCore {
    pub fn start(store: Store, ctx: Option<egui::Context>) -> anyhow::Result<AppCore> {
        AppCore::start_with(store, ctx, StartOptions::default())
    }

    pub fn start_with(
        store: Store,
        ctx: Option<egui::Context>,
        opts: StartOptions,
    ) -> anyhow::Result<AppCore> {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let mut settings = store.load_or_create_instance()?;
        let identity = Identity::load_or_create(&store.identity_dir())?;
        let peers = store.load_peers()?;
        let projects = store.load_projects()?;
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .thread_name("project-transfer")
            .enable_all()
            .build()
            .context("could not start the background workers")?;
        let (listener, moved) = {
            let _rt = runtime.enter();
            listen(opts.bind_ip, settings.port, opts.preferred_port)?
        };
        let port = listener.local_addr()?.port();
        let old_port = settings.port;
        if settings.port != port {
            settings.port = port;
            store.save_instance(&settings)?;
        }

        let (events, net_rx) = mpsc::unbounded_channel();
        let mut state = UiState::new(settings.clone(), port);
        state.projects = projects.clone();
        state.selected_peer = settings
            .last_peer
            .filter(|id| peers.iter().any(|p| p.id == *id));
        state.peers = peers.iter().map(peers::offline_view).collect();
        let ui = StateHandle::new(state, ctx);
        let shared = Shared {
            settings: Arc::new(RwLock::new(settings.clone())),
            peers: Arc::new(RwLock::new(peers)),
            projects: Arc::new(RwLock::new(projects)),
            store: Arc::new(store),
            identity: Arc::new(identity),
            events,
            found: Default::default(),
        };
        let core = Core {
            shared: shared.clone(),
            ui: ui.clone(),
            discovery: Arc::new(Mutex::new(None)),
            incoming: Arc::new(Mutex::new(None)),
            outgoing: Arc::new(Mutex::new(None)),
            transfer: Arc::new(Mutex::new(transfers::Slot::default())),
            runs: Arc::new(Mutex::new(HashMap::new())),
            poll_now: Arc::new(Notify::new()),
            poll_problems: Arc::new(Mutex::new(HashMap::new())),
        };

        let server_ui = ui.clone();
        runtime.spawn(async move {
            if let Err(e) = crate::net::serve(shared, listener).await {
                error!("the server stopped: {e:#}");
                server_ui.error(format!(
                    "This computer stopped accepting connections ({e:#}). Restart Project \
                     Transfer."
                ));
            }
        });
        runtime.spawn(core.clone().net_events(net_rx));
        runtime.spawn(core.clone().poll_loop());
        if ui.lock().selected_peer.is_some() {
            // The restored peer's dot should not wait for the first tick.
            core.poll_now.notify_one();
        }
        let (tx, rx) = mpsc::unbounded_channel();
        runtime.spawn(core.clone().run_actions(rx));

        if moved && old_port != 0 {
            ui.warn(format!(
                "Port {old_port} was in use, so this computer now listens on port {port}. \
                 Paired computers find it again once discovery sees it."
            ));
        }
        info!("{} listening on port {port}", settings.name);
        if opts.discovery {
            core.start_discovery(&settings.name, settings.id, port);
        }
        Ok(AppCore {
            runtime: Some(runtime),
            tx,
            core,
            port,
        })
    }

    /// The snapshot to draw. Hold the guard only while drawing.
    pub fn state(&self) -> MutexGuard<'_, UiState> {
        self.core.ui.lock()
    }

    /// Queues a request; never waits for it.
    pub fn act(&self, a: Action) {
        let _ = self.tx.send(Msg::Act(a));
    }

    /// Blocks until every action sent so far has been handled. Work an action
    /// starts in the background (a transfer, a pairing) may still be running.
    /// For tests and shutdown, not for the UI thread.
    pub fn settle(&self) {
        let (tx, rx) = std::sync::mpsc::channel();
        if self.tx.send(Msg::Settle(tx)).is_ok() {
            let _ = rx.recv();
        }
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    pub fn logs_dir(&self) -> PathBuf {
        self.core.shared.store.logs_dir()
    }
}

impl Drop for AppCore {
    fn drop(&mut self) {
        if let Some(d) = lock(&self.core.discovery).take() {
            d.stop();
        }
        for (_, r) in lock(&self.core.runs).drain() {
            r.stop();
        }
        if let Some(rt) = self.runtime.take() {
            rt.shutdown_background();
        }
    }
}

impl Core {
    async fn run_actions(self, mut rx: mpsc::UnboundedReceiver<Msg>) {
        while let Some(msg) = rx.recv().await {
            match msg {
                Msg::Act(a) => {
                    if let Err(e) = self.handle(a).await {
                        self.ui.error(format!("{e:#}"));
                    }
                }
                Msg::Settle(done) => {
                    let _ = done.send(());
                }
            }
        }
    }

    async fn net_events(self, mut rx: mpsc::UnboundedReceiver<NetEvent>) {
        while let Some(ev) = rx.recv().await {
            self.net_event(ev).await;
        }
    }
}

/// Binds the saved port, else the preferred one, else any free port. True in
/// the second value when the saved port could not be used.
fn listen(
    ip: IpAddr,
    saved: u16,
    preferred: u16,
) -> anyhow::Result<(tokio::net::TcpListener, bool)> {
    let first = if saved != 0 { saved } else { preferred };
    if first != 0 {
        match bind(SocketAddr::new(ip, first)) {
            Ok(l) => return Ok((l, false)),
            Err(e) => warn!("could not listen on port {first}: {e}"),
        }
    }
    let l = bind(SocketAddr::new(ip, 0)).context("could not listen for connections")?;
    Ok((l, saved != 0))
}

/// Reusing the address lets a restart take its port back while old
/// connections are still closing.
fn bind(addr: SocketAddr) -> std::io::Result<tokio::net::TcpListener> {
    let socket = match addr {
        SocketAddr::V4(_) => tokio::net::TcpSocket::new_v4()?,
        SocketAddr::V6(_) => tokio::net::TcpSocket::new_v6()?,
    };
    // Windows lets SO_REUSEADDR bind a port another socket is listening on,
    // which would hide a busy port; its restarts do not need it anyway.
    #[cfg(not(windows))]
    socket.set_reuseaddr(true)?;
    socket.bind(addr)?;
    socket.listen(128)
}
