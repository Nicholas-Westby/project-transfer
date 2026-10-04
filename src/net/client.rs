//! The calling side: opening a connection and talking over it, directly or
//! passed along by a paired computer.

use super::Shared;
use super::handshake::{Via, handshake};
use crate::address::is_local;
use crate::model::{InstanceId, Peer};
use crate::protocol::{Request, Response, read_msg, write_msg};
use anyhow::{Context, anyhow, bail};
use std::net::SocketAddr;
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time::timeout;
use tracing::debug;

const CONNECT_WAIT: Duration = Duration::from_secs(10);
/// How long a relay may take to agree or refuse. It stops looking for the
/// target sooner, so its own refusal arrives first.
const RELAY_WAIT: Duration = Duration::from_secs(15);

/// Another computer answers where a paired one was expected: the address now
/// belongs to someone else, or the paired computer was set up again. Not a
/// refusal of anyone; whoever polls says so once.
#[derive(Debug)]
pub struct NotThePairedComputer(pub String);

impl std::fmt::Display for NotThePairedComputer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for NotThePairedComputer {}

impl NotThePairedComputer {
    /// Another computer answers where `expected` was reached: at `addr`, or
    /// through the paired computer named `relay`.
    pub(super) fn instead_of(
        expected: &Peer,
        addr: SocketAddr,
        relay: Option<&str>,
    ) -> NotThePairedComputer {
        let name = &expected.name;
        let gone = match relay {
            None => format!("{name} isn't at {addr} any more"),
            Some(relay) => format!("{name} isn't reachable through {relay} any more"),
        };
        NotThePairedComputer(format!(
            "{gone}: another computer answers there. If Project Transfer was set up again on \
             {name}, unpair it and pair again."
        ))
    }
}

/// What a connection runs over: a TCP stream, or a relay's encrypted
/// connection with the session to the computer behind it running inside.
/// Sync because transfers hold a shared reference to the connection across
/// awaits in spawned tasks.
pub trait Io: AsyncRead + AsyncWrite + Unpin + Send + Sync {}

impl<T: AsyncRead + AsyncWrite + Unpin + Send + Sync> Io for T {}

pub struct Connection {
    pub(super) stream: Box<dyn Io>,
    /// The address dialled, which is the relay's when `via` is set.
    pub(super) addr: SocketAddr,
    /// The paired computer that passed this connection along.
    pub(super) via: Option<InstanceId>,
    pub(super) peer_id: InstanceId,
    pub(super) peer_name: String,
    pub(super) peer_fingerprint: String,
}

impl Connection {
    /// Connects, completes TLS and the hello exchange. Fails if the address is
    /// not private or the peer's id is stored with a different certificate.
    /// Whoever answers may be a stranger, so this is only for pairing; use
    /// `open_peer` to talk to a computer that is already paired.
    pub async fn open(addr: SocketAddr, shared: &Shared) -> anyhow::Result<Connection> {
        Connection::dial(addr, shared, None).await
    }

    async fn dial(
        addr: SocketAddr,
        shared: &Shared,
        expected: Option<&Peer>,
    ) -> anyhow::Result<Connection> {
        if !is_local(addr.ip()) {
            bail!(
                "{addr} is not a private network address. Project Transfer only connects to \
                 computers on your local network."
            );
        }
        let tcp = timeout(CONNECT_WAIT, TcpStream::connect(addr))
            .await
            .map_err(|_| {
                anyhow!(
                    "Could not reach {addr} within 10 seconds. Check that the other computer \
                     is on and running Project Transfer."
                )
            })?
            .with_context(|| format!("Could not connect to {addr}"))?;
        let _ = tcp.set_nodelay(true);
        handshake(tcp, addr, shared, None, expected).await
    }

    /// Asks `relay`, a paired computer, to pass a connection along to `to`,
    /// then completes TLS and the hello exchange with `to` inside the relay's
    /// connection, so the relay only copies bytes it can't read. Like `open`,
    /// this is for pairing; use `open_peer_through` to talk to a paired
    /// computer.
    pub async fn through(
        relay: Connection,
        to: InstanceId,
        shared: &Shared,
    ) -> anyhow::Result<Connection> {
        Connection::pass_along(relay, to, shared, None).await
    }

    async fn pass_along(
        mut relay: Connection,
        to: InstanceId,
        shared: &Shared,
        expected: Option<&Peer>,
    ) -> anyhow::Result<Connection> {
        let relay_name = relay.peer_name.clone();
        let reply = timeout(RELAY_WAIT, relay.request(&Request::Relay { to }))
            .await
            .map_err(|_| {
                anyhow!(
                    "{relay_name} did not answer within 15 seconds when asked to pass the \
                     connection along. Try again in a moment."
                )
            })?
            .with_context(|| format!("{relay_name} stopped answering"))?;
        match reply {
            Response::Ok => {}
            // Unnamed, a refusal like "These computers aren't paired" would
            // seem to come from the computer behind the relay.
            Response::Refused { reason } => {
                bail!("{relay_name}, which passes the connection along, says: {reason}")
            }
            other => {
                bail!("{relay_name} answered the request to pass a connection along with {other:?}")
            }
        }
        let via = Via {
            id: relay.peer_id,
            name: &relay_name,
        };
        handshake(relay.stream, relay.addr, shared, Some(via), expected).await
    }

    /// Opens a connection to a paired computer, failing unless whoever answers
    /// has both its id and its certificate. Use this for every transfer.
    pub async fn open_peer(
        addr: SocketAddr,
        shared: &Shared,
        expected: &Peer,
    ) -> anyhow::Result<Connection> {
        let conn = Connection::dial(addr, shared, Some(expected)).await?;
        if !conn.is(expected) {
            debug!(
                "{} answered at {addr} instead of {}",
                conn.peer_name, expected.name
            );
            return Err(NotThePairedComputer::instead_of(expected, addr, None).into());
        }
        Ok(conn)
    }

    /// `through` to a paired computer, failing unless whoever answers has both
    /// its id and its certificate. Use this for every transfer through a relay.
    pub async fn open_peer_through(
        relay: Connection,
        shared: &Shared,
        expected: &Peer,
    ) -> anyhow::Result<Connection> {
        let relay_name = relay.peer_name.clone();
        let conn = Connection::pass_along(relay, expected.id, shared, Some(expected)).await?;
        if !conn.is(expected) {
            debug!(
                "{} answered through {relay_name} instead of {}",
                conn.peer_name, expected.name
            );
            let wrong = NotThePairedComputer::instead_of(expected, conn.addr, Some(&relay_name));
            return Err(wrong.into());
        }
        Ok(conn)
    }

    fn is(&self, expected: &Peer) -> bool {
        self.peer_id == expected.id && self.peer_fingerprint == expected.fingerprint
    }

    /// The paired computer that passed this connection along, if any.
    pub fn via(&self) -> Option<InstanceId> {
        self.via
    }

    pub fn peer_fingerprint(&self) -> &str {
        &self.peer_fingerprint
    }

    pub fn peer_id(&self) -> InstanceId {
        self.peer_id
    }

    pub fn peer_name(&self) -> &str {
        &self.peer_name
    }

    pub async fn request(&mut self, req: &Request) -> anyhow::Result<Response> {
        write_msg(&mut self.stream, req).await?;
        read_msg(&mut self.stream).await
    }

    /// Sends without waiting, for a request followed by raw bytes.
    pub async fn send(&mut self, req: &Request) -> anyhow::Result<()> {
        write_msg(&mut self.stream, req).await
    }

    pub async fn recv(&mut self) -> anyhow::Result<Response> {
        read_msg(&mut self.stream).await
    }

    /// Reads up to `buf.len()` raw bytes; 0 means the peer closed.
    pub async fn read_raw(&mut self, buf: &mut [u8]) -> anyhow::Result<usize> {
        Ok(self.stream.read(buf).await?)
    }

    /// Ends the connection, so a peer waiting for the rest of a file stops
    /// waiting and discards it.
    pub async fn close(&mut self) {
        let _ = self.stream.shutdown().await;
    }

    pub async fn send_raw(&mut self, bytes: &[u8]) -> anyhow::Result<()> {
        self.stream.write_all(bytes).await?;
        self.stream.flush().await?;
        Ok(())
    }

    /// Copies exactly `len` raw bytes from the peer into `sink`.
    pub async fn recv_raw(
        &mut self,
        len: u64,
        sink: &mut (impl AsyncWrite + Unpin),
    ) -> anyhow::Result<()> {
        let copied = tokio::io::copy(&mut (&mut self.stream).take(len), sink).await?;
        if copied != len {
            bail!(
                "{} closed the connection after {copied} of {len} bytes",
                self.peer_name
            );
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "client_tests.rs"]
mod tests;
