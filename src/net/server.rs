//! Accepts connections, checks who is calling, and dispatches requests.

use super::gate::{self, Refusal};
use super::handlers::{self, Ctx, SessionState};
use super::pair_server::{self, After, Caller};
use super::{NetEvent, Shared, tls};
use crate::address::is_local;
use crate::model::{InstanceId, Peer, Permissions};
use crate::protocol::{
    PROTOCOL_VERSION, ProjectSummary, RemoteFolder, RemoteProject, Request, Response, read_msg,
    read_msg_limited, write_msg,
};
use anyhow::Context;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::time::timeout;
use tokio_rustls::TlsAcceptor;
use tracing::{debug, info, warn};

const HANDSHAKE_WAIT: Duration = Duration::from_secs(10);
const HELLO_WAIT: Duration = Duration::from_secs(10);
/// Strangers may not hold connections open indefinitely.
const UNPAIRED_IDLE: Duration = Duration::from_secs(180);
/// Hello and pairing messages are tiny; strangers get no room to make us allocate.
pub(super) const UNPAIRED_MAX: u32 = 64 * 1024;

pub async fn serve(shared: Shared, listener: tokio::net::TcpListener) -> anyhow::Result<()> {
    let acceptor = TlsAcceptor::from(Arc::new(tls::server_config(&shared.identity)?));
    let pairing = PairSlot::new();
    loop {
        let (tcp, remote) = match listener.accept().await {
            Ok(x) => x,
            Err(e) => {
                // Usually out of file descriptors; back off rather than spin.
                warn!("could not accept a connection: {e}");
                tokio::time::sleep(Duration::from_millis(200)).await;
                continue;
            }
        };
        if !accept_source(remote) {
            warn!("dropped a connection from {remote}: not a private network address");
            continue;
        }
        let (shared, acceptor, pairing) = (shared.clone(), acceptor.clone(), pairing.clone());
        tokio::spawn(async move {
            match connection(&shared, &pairing, acceptor, tcp, remote).await {
                Ok(()) => debug!("connection from {remote} closed"),
                Err(e) => info!("connection from {remote} ended: {e:#}"),
            }
        });
    }
}

/// At most one pairing prompt waits for the user across all connections.
#[derive(Clone)]
pub(super) struct PairSlot(pub(super) Arc<tokio::sync::Semaphore>);

impl PairSlot {
    pub(super) fn new() -> PairSlot {
        PairSlot(Arc::new(tokio::sync::Semaphore::new(1)))
    }
}

fn accept_source(remote: SocketAddr) -> bool {
    is_local(remote.ip())
}

async fn connection(
    shared: &Shared,
    pairing: &PairSlot,
    acceptor: TlsAcceptor,
    tcp: tokio::net::TcpStream,
    remote: SocketAddr,
) -> anyhow::Result<()> {
    let _ = tcp.set_nodelay(true);
    let mut stream = timeout(HANDSHAKE_WAIT, acceptor.accept(tcp))
        .await
        .context("the TLS handshake took longer than 10 seconds")?
        .context("the TLS handshake failed")?;
    let fingerprint = tls::peer_fingerprint(stream.get_ref().1.peer_certificates())?;
    debug!("secure connection from {remote}");
    session(shared, pairing, &mut stream, fingerprint, remote).await
}

/// Everything after the handshake; generic so it can be tested without TLS.
pub(super) async fn session<S: AsyncRead + AsyncWrite + Unpin + Send>(
    shared: &Shared,
    pairing: &PairSlot,
    stream: &mut S,
    fingerprint: String,
    remote: SocketAddr,
) -> anyhow::Result<()> {
    let first: Request = timeout(HELLO_WAIT, read_msg_limited(stream, UNPAIRED_MAX))
        .await
        .context("no hello within 10 seconds")??;
    let Request::Hello {
        id,
        name,
        version,
        port,
    } = first
    else {
        let reason = "Say hello first. Update Project Transfer on the other computer.";
        return refuse(shared, stream, &remote.to_string(), &Refusal::same(reason)).await;
    };
    if version != PROTOCOL_VERSION {
        let reason = format!(
            "These computers run different versions of Project Transfer (protocol {version} \
             and {PROTOCOL_VERSION}). Update both to the same version."
        );
        return refuse(shared, stream, &name, &Refusal::same(reason)).await;
    }
    // A known id with another certificate is someone else using that id.
    let changed = shared
        .peers
        .read()
        .await
        .iter()
        .any(|p| p.id == id && p.fingerprint != fingerprint);
    if changed {
        return refuse(shared, stream, &name, &Refusal::identity_changed()).await;
    }
    refresh_address(shared, id, &fingerprint, remote, port).await;
    let (my_id, my_name) = {
        let s = shared.settings.read().await;
        (s.id, s.name.clone())
    };
    let hello = Response::Hello {
        id: my_id,
        name: my_name,
        version: PROTOCOL_VERSION,
    };
    write_msg(stream, &hello).await?;
    debug!("{name} ({id}) at {remote} said hello");

    let mut state = SessionState::default();
    loop {
        let peer = stored_peer(shared, id, &fingerprint).await;
        let req = if peer.is_some() {
            read_msg::<_, Request>(stream).await
        } else {
            match timeout(UNPAIRED_IDLE, read_msg_limited(stream, UNPAIRED_MAX)).await {
                Ok(r) => r,
                Err(_) => anyhow::bail!("unpaired connection idle too long"),
            }
        };
        let req = match req {
            Ok(r) => r,
            Err(e) if is_eof(&e) => return Ok(()),
            Err(e) => return Err(e),
        };
        // Re-read per request so unpairing or a permission change applies at once.
        let peer = stored_peer(shared, id, &fingerprint).await;
        let peer_name = peer.as_ref().map_or(name.clone(), |p| p.name.clone());
        let my_name = shared.settings.read().await.name.clone();
        if let Err(refusal) = gate::check(&req, peer.as_ref(), &my_name) {
            info!(
                "{peer_name} sent {}, which needs {:?}",
                req.kind(),
                gate::need(&req)
            );
            refuse(shared, stream, &peer_name, &refusal).await?;
            if matches!(req, Request::PutFile { .. }) {
                // Its raw bytes follow and would be misread as messages.
                return Ok(());
            }
            continue;
        }
        match req {
            Request::Hello { .. } => {
                let refusal = Refusal::same("Already said hello.");
                refuse(shared, stream, &peer_name, &refusal).await?;
            }
            Request::PairCommit {
                commitment,
                requested,
                offered,
            } => {
                let who = Caller {
                    id,
                    name: &name,
                    fingerprint: &fingerprint,
                    remote,
                    port,
                };
                let after = pair_server::respond(
                    shared, pairing, stream, who, commitment, requested, offered,
                )
                .await?;
                if let After::Close = after {
                    return Ok(());
                }
            }
            Request::PairReveal { .. } | Request::PairFinal { .. } => {
                let reason = "Start pairing again; this step came out of order.";
                refuse(shared, stream, &peer_name, &Refusal::same(reason)).await?;
            }
            Request::Status => {
                let allows = peer.map(|p| p.allows).unwrap_or_default();
                write_msg(stream, &status(shared, allows).await).await?;
            }
            Request::ProjectInfo { project } => {
                write_msg(stream, &project_info(shared, project).await).await?;
            }
            other => {
                let peer = peer.context("the gate let an unpaired peer through")?;
                let ctx = Ctx {
                    shared,
                    peer: &peer,
                    remote,
                };
                handlers::handle(&ctx, &mut state, stream, other).await?;
            }
        }
    }
}

/// A paired peer calling us shows where it is now; keeping that makes the
/// stored address follow it when its IP or port changes.
async fn refresh_address(
    shared: &Shared,
    id: InstanceId,
    fingerprint: &str,
    remote: SocketAddr,
    port: u16,
) {
    if port == 0 {
        return;
    }
    let addr = SocketAddr::new(remote.ip(), port);
    let known = stored_peer(shared, id, fingerprint)
        .await
        .is_some_and(|p| p.last_address != Some(addr));
    if known && let Err(e) = super::set_last_address(shared, id, fingerprint, addr).await {
        warn!("could not save the new address of {id}: {e:#}");
    }
}

fn is_eof(e: &anyhow::Error) -> bool {
    e.downcast_ref::<std::io::Error>()
        .is_some_and(|io| io.kind() == std::io::ErrorKind::UnexpectedEof)
}

async fn stored_peer(shared: &Shared, id: InstanceId, fingerprint: &str) -> Option<Peer> {
    shared
        .peers
        .read()
        .await
        .iter()
        .find(|p| p.id == id && p.fingerprint == fingerprint)
        .cloned()
}

/// Answers the asker with the reason worded for it, and tells this
/// computer's user in words for this side.
pub(super) async fn refuse<S: AsyncWrite + Unpin>(
    shared: &Shared,
    stream: &mut S,
    peer_name: &str,
    refusal: &Refusal,
) -> anyhow::Result<()> {
    // The user's activity line records the refusal at warn level already.
    debug!("refused {peer_name}: {}", refusal.theirs);
    let _ = shared.events.send(NetEvent::Refused {
        peer_name: peer_name.to_string(),
        reason: refusal.ours.clone(),
    });
    let reply = Response::Refused {
        reason: refusal.theirs.clone(),
    };
    write_msg(stream, &reply).await
}

async fn status(shared: &Shared, allows: Permissions) -> Response {
    let projects = shared
        .projects
        .read()
        .await
        .iter()
        .map(|p| ProjectSummary {
            id: p.id,
            name: p.name.clone(),
        })
        .collect();
    Response::Status { allows, projects }
}

async fn project_info(shared: &Shared, project: crate::model::ProjectId) -> Response {
    let projects = shared.projects.read().await;
    let info = projects
        .iter()
        .find(|p| p.id == project)
        .map(|p| RemoteProject {
            name: p.name.clone(),
            primary: p.primary,
            folders: p
                .folders
                .iter()
                .map(|f| RemoteFolder {
                    id: f.id,
                    name: f.name.clone(),
                    path: f.local_path.as_ref().map(|l| l.display().to_string()),
                })
                .collect(),
        });
    Response::ProjectInfo(info)
}

#[cfg(test)]
#[path = "server_tests.rs"]
mod tests;
