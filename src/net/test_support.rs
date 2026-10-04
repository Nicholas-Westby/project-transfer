//! What the tests that talk to `session` over an in-memory pipe share.

use super::{NetEvent, Shared, tls};
use crate::identity::Identity;
use crate::model::{InstanceSettings, Peer, Permissions};
use crate::protocol::{PROTOCOL_VERSION, Request, Response, read_msg, write_msg};
use crate::store::Store;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::RwLock;
use tokio::task::JoinHandle;
use tokio_rustls::TlsAcceptor;

/// A computer called Desk with nothing paired and no projects.
pub(super) fn shared(
    dir: &std::path::Path,
) -> (Shared, tokio::sync::mpsc::UnboundedReceiver<NetEvent>) {
    let store = Store::open_at(dir.to_path_buf()).unwrap();
    let identity = Identity::load_or_create(&store.identity_dir()).unwrap();
    let settings = InstanceSettings {
        id: uuid::Uuid::new_v4(),
        name: "Desk".into(),
        projects_folder: dir.join("Dev"),
        extra_ignores: vec![],
        always_include: vec![],
        removed_default_ignores: vec![],
        last_peer: None,
        theme: Default::default(),
        port: 0,
    };
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    let s = Shared {
        settings: Arc::new(RwLock::new(settings)),
        peers: Arc::new(RwLock::new(vec![])),
        projects: Arc::new(RwLock::new(vec![])),
        store: Arc::new(store),
        identity: Arc::new(identity),
        events: tx,
        found: Default::default(),
    };
    (s, rx)
}

pub(super) fn addr(s: &str) -> SocketAddr {
    s.parse().unwrap()
}

/// A paired computer called Laptop, as this computer stored it, whose
/// certificate nobody in these tests has.
pub(super) fn laptop() -> Peer {
    Peer {
        id: uuid::Uuid::new_v4(),
        name: "Laptop".into(),
        fingerprint: "cd".repeat(32),
        allows: Permissions::default(),
        granted: Permissions::default(),
        last_address: None,
        via: None,
    }
}

/// Completes TLS as `who` at the far end of `stream` and returns the first
/// message it hears, if any. It answers a hello with its own, as any
/// Project Transfer would.
pub(super) fn listen_as(
    who: Shared,
    stream: impl AsyncRead + AsyncWrite + Unpin + Send + 'static,
) -> JoinHandle<Option<Request>> {
    tokio::spawn(async move {
        let acceptor = TlsAcceptor::from(Arc::new(tls::server_config(&who.identity).unwrap()));
        let mut tls = acceptor.accept(stream).await.ok()?;
        let first: Request = read_msg(&mut tls).await.ok()?;
        let (id, name) = {
            let s = who.settings.read().await;
            (s.id, s.name.clone())
        };
        let hello = Response::Hello {
            id,
            name,
            version: PROTOCOL_VERSION,
        };
        let _ = write_msg(&mut tls, &hello).await;
        Some(first)
    })
}
