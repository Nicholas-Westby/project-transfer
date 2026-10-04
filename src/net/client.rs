//! The calling side: opening a connection and talking over it.

use super::gate::IDENTITY_CHANGED_THERE;
use super::{NetEvent, Shared, tls};
use crate::address::is_local;
use crate::model::{InstanceId, Peer};
use crate::protocol::{PROTOCOL_VERSION, Request, Response, read_msg, write_msg};
use anyhow::{Context, anyhow, bail};
use rustls::pki_types::ServerName;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time::timeout;
use tokio_rustls::TlsConnector;
use tokio_rustls::client::TlsStream;
use tracing::{debug, warn};

const CONNECT_WAIT: Duration = Duration::from_secs(10);
const HANDSHAKE_WAIT: Duration = Duration::from_secs(10);

pub struct Connection {
    pub(super) stream: TlsStream<TcpStream>,
    pub(super) addr: SocketAddr,
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
        let connector = TlsConnector::from(Arc::new(tls::client_config(&shared.identity)?));
        let name = ServerName::try_from(tls::SERVER_NAME)?;
        let (stream, reply) = timeout(HANDSHAKE_WAIT, async {
            let mut stream = connector.connect(name, tcp).await?;
            let (id, my_name, port) = {
                let s = shared.settings.read().await;
                (s.id, s.name.clone(), s.port)
            };
            let hello = Request::Hello {
                id,
                name: my_name,
                version: PROTOCOL_VERSION,
                port,
            };
            write_msg(&mut stream, &hello).await?;
            let reply: Response = read_msg(&mut stream).await?;
            anyhow::Ok((stream, reply))
        })
        .await
        .map_err(|_| {
            anyhow!(
                "{addr} did not finish the secure handshake within 10 seconds. Try again, \
                 and check that it is running Project Transfer."
            )
        })?
        .with_context(|| format!("Could not set up a secure connection to {addr}"))?;
        let (peer_id, peer_name) = match reply {
            Response::Hello { id, name, version } if version == PROTOCOL_VERSION => (id, name),
            Response::Hello { version, .. } => bail!(
                "{addr} runs a different version of Project Transfer (protocol {version}). \
                 Update both computers to the same version."
            ),
            Response::Refused { reason } => bail!("{reason}"),
            other => bail!("{addr} answered hello with {other:?}"),
        };
        let peer_fingerprint = tls::peer_fingerprint(stream.get_ref().1.peer_certificates())?;
        let changed = shared
            .peers
            .read()
            .await
            .iter()
            .any(|p| p.id == peer_id && p.fingerprint != peer_fingerprint);
        if changed {
            warn!("refused {peer_name} at {addr}: {IDENTITY_CHANGED_THERE}");
            let _ = shared.events.send(NetEvent::Refused {
                peer_name: peer_name.clone(),
                reason: IDENTITY_CHANGED_THERE.to_string(),
            });
            bail!("{peer_name}: {IDENTITY_CHANGED_THERE}");
        }
        debug!("connected to {peer_name} ({peer_id}) at {addr}");
        Ok(Connection {
            stream,
            addr,
            peer_id,
            peer_name,
            peer_fingerprint,
        })
    }

    /// Opens a connection to a paired computer, failing unless whoever answers
    /// has both its id and its certificate. Use this for every transfer.
    pub async fn open_peer(
        addr: SocketAddr,
        shared: &Shared,
        expected: &Peer,
    ) -> anyhow::Result<Connection> {
        let conn = Connection::open(addr, shared).await?;
        if conn.peer_id != expected.id || conn.peer_fingerprint != expected.fingerprint {
            let reason = format!(
                "The computer at {addr} is not {}. Its address may have changed; wait for it \
                 to appear on the network again, or pair again.",
                expected.name
            );
            warn!("refused {} at {addr}: {reason}", conn.peer_name);
            let _ = shared.events.send(NetEvent::Refused {
                peer_name: expected.name.clone(),
                reason: reason.clone(),
            });
            bail!(reason);
        }
        Ok(conn)
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
