//! A local link that holds every chunk for a while in each direction, like
//! a network with a long round trip.

use super::Instance;
use project_transfer::net::Connection;
use std::net::SocketAddr;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tokio::time::Instant;

pub struct Link {
    pub addr: SocketAddr,
}

impl Link {
    /// Passes connections on to `to`, each chunk arriving `one_way` late.
    pub async fn start(to: SocketAddr, one_way: Duration) -> Link {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            while let Ok((inbound, _)) = listener.accept().await {
                let Ok(outbound) = TcpStream::connect(to).await else {
                    continue;
                };
                let _ = inbound.set_nodelay(true);
                let _ = outbound.set_nodelay(true);
                let (ir, iw) = inbound.into_split();
                let (or, ow) = outbound.into_split();
                tokio::spawn(pump(ir, ow, one_way));
                tokio::spawn(pump(or, iw, one_way));
            }
        });
        Link { addr }
    }
}

/// Copies `from` to `to` in order, each chunk leaving `delay` after it came.
async fn pump(mut from: OwnedReadHalf, mut to: OwnedWriteHalf, delay: Duration) {
    let (tx, mut rx) = mpsc::unbounded_channel::<(Instant, Vec<u8>)>();
    tokio::spawn(async move {
        while let Some((due, chunk)) = rx.recv().await {
            tokio::time::sleep_until(due).await;
            if to.write_all(&chunk).await.is_err() {
                return;
            }
        }
        let _ = to.shutdown().await;
    });
    let mut buf = vec![0u8; 64 * 1024];
    while let Ok(n) = from.read(&mut buf).await {
        if n == 0
            || tx
                .send((Instant::now() + delay, buf[..n].to_vec()))
                .is_err()
        {
            return;
        }
    }
}

/// Like `Instance::open`, but through `link`.
pub async fn open_through(a: &Instance, b: &Instance, link: &Link) -> Connection {
    let peer = a
        .shared
        .peers
        .read()
        .await
        .iter()
        .find(|p| p.last_address == Some(b.addr))
        .cloned()
        .expect("paired");
    Connection::open_peer(link.addr, &a.shared, &peer)
        .await
        .unwrap()
}
