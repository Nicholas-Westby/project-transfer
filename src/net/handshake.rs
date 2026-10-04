//! TLS and the hello exchange on the calling side, the same for a direct
//! connection and for one a paired computer passes along.

use super::client::{Connection, Io, NotThePairedComputer};
use super::gate::IDENTITY_CHANGED_THERE;
use super::{Shared, tls};
use crate::address::own_ips;
use crate::model::{InstanceId, Peer};
use crate::protocol::{PROTOCOL_VERSION, Request, Response, read_msg, write_msg};
use anyhow::{Context, anyhow, bail};
use rustls::pki_types::ServerName;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::{Instant, timeout_at};
use tokio_rustls::TlsConnector;
use tracing::debug;

const HANDSHAKE_WAIT: Duration = Duration::from_secs(10);

/// The paired computer passing a connection along.
pub(super) struct Via<'a> {
    pub id: InstanceId,
    pub name: &'a str,
}

/// TLS, then the hello exchange, over a TCP stream or over a relay's
/// connection, so connections passed along are checked like direct ones.
/// With `expected`, whoever answers must hold that paired computer's
/// certificate before it hears anything about this one.
pub(super) async fn handshake<IO: Io + 'static>(
    io: IO,
    addr: SocketAddr,
    shared: &Shared,
    via: Option<Via<'_>>,
    expected: Option<&Peer>,
) -> anyhow::Result<Connection> {
    // The relay's address would point at the relay, not at the computer that
    // failed to answer.
    let place = match &via {
        None => addr.to_string(),
        Some(relay) => format!("the computer reached through {}", relay.name),
    };
    let lead = upper_first(&place);
    let late = || {
        anyhow!(
            "{lead} did not finish the secure handshake within 10 seconds. Try again, and \
             check that it is running Project Transfer."
        )
    };
    let failed = || format!("Could not set up a secure connection to {place}");
    let connector = TlsConnector::from(Arc::new(tls::client_config(&shared.identity)?));
    let name = ServerName::try_from(tls::SERVER_NAME)?;
    let deadline = Instant::now() + HANDSHAKE_WAIT;
    let mut stream = timeout_at(deadline, connector.connect(name, io))
        .await
        .map_err(|_| late())?
        .with_context(failed)?;
    // Boxing the stream hides the TLS session, so read the certificate first.
    let peer_fingerprint = tls::peer_fingerprint(stream.get_ref().1.peer_certificates())?;
    if let Some(p) = expected
        && p.fingerprint != peer_fingerprint
    {
        debug!("another computer than {} answered at {place}", p.name);
        return Err(NotThePairedComputer::instead_of(p, addr, via.map(|r| r.name)).into());
    }
    let hello = hello(shared, via.as_ref().map(|r| r.id)).await;
    let reply = timeout_at(deadline, async {
        write_msg(&mut stream, &hello).await?;
        read_msg::<_, Response>(&mut stream).await
    })
    .await
    .map_err(|_| late())?
    .with_context(failed)?;
    let (peer_id, peer_name) = match reply {
        Response::Hello { id, name, version } if version == PROTOCOL_VERSION => (id, name),
        Response::Hello { version, .. } => bail!(
            "{lead} runs a different version of Project Transfer (protocol {version}). Update \
             both computers to the same version."
        ),
        Response::Refused { reason } => bail!("{reason}"),
        other => bail!("{lead} answered hello with {other:?}"),
    };
    let changed = shared
        .peers
        .read()
        .await
        .iter()
        .any(|p| p.id == peer_id && p.fingerprint != peer_fingerprint);
    if changed {
        debug!("{peer_name} at {place} has another certificate than the paired one");
        let why = format!("{peer_name}: {IDENTITY_CHANGED_THERE}");
        return Err(NotThePairedComputer(why).into());
    }
    debug!("connected to {peer_name} ({peer_id}) at {place}");
    Ok(Connection {
        stream: Box::new(stream),
        addr,
        via: via.map(|relay| relay.id),
        peer_id,
        peer_name,
        peer_fingerprint,
    })
}

async fn hello(shared: &Shared, via: Option<InstanceId>) -> Request {
    // Passed along, the call arrives from the relay's address, which can be
    // one of this computer's too; listed, it would be remembered as ours.
    let addrs = if via.is_some() { Vec::new() } else { own_ips() };
    let s = shared.settings.read().await;
    Request::Hello {
        id: s.id,
        name: s.name.clone(),
        version: PROTOCOL_VERSION,
        port: s.port,
        addrs,
        via,
    }
}

/// For a place named at the start of a sentence.
fn upper_first(s: &str) -> String {
    let mut chars = s.chars();
    chars
        .next()
        .map_or_else(String::new, |c| c.to_uppercase().chain(chars).collect())
}
