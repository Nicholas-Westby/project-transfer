//! Passing a connection along between two computers this one is paired with.
//! The caller runs its own TLS session with the target inside its session
//! with this computer, so what is copied here can't be read here.

use super::{Connection, Shared};
use crate::model::{InstanceId, Peer};
use crate::protocol::{Response, write_msg};
use std::net::SocketAddr;
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpStream;
use tokio::time::timeout;
use tracing::debug;

/// The caller stops waiting after 15 seconds. Giving up sooner lets a refusal
/// reach it in time, and a long list of announced addresses can't keep this
/// computer busy.
const SEARCH_WAIT: Duration = Duration::from_secs(12);
/// Checking who answers takes well under a second on a local network. An
/// address that stays silent must not use up the time the next one needs.
const ADDRESS_WAIT: Duration = Duration::from_secs(4);

/// Connects the paired `caller` to the paired computer `to` and copies bytes
/// both ways until either side closes; the session is over afterwards, also
/// when refused. Logs only at debug: polls pass through every few seconds.
pub(super) async fn relay<S: AsyncRead + AsyncWrite + Unpin + Send>(
    shared: &Shared,
    stream: &mut S,
    caller: &Peer,
    to: InstanceId,
) -> anyhow::Result<()> {
    let (my_id, me) = {
        let s = shared.settings.read().await;
        (s.id, s.name.clone())
    };
    let target = shared
        .peers
        .read()
        .await
        .iter()
        .find(|p| p.id == to && p.id != caller.id && p.id != my_id)
        .cloned();
    let Some(target) = target else {
        debug!(
            "{} asked to be passed along to {to}, which isn't paired here",
            caller.name
        );
        let reason =
            format!("{me} isn't paired with that computer, so it can't pass the connection along.");
        return write_msg(stream, &Response::Refused { reason }).await;
    };
    let mut addrs = shared
        .found
        .read()
        .await
        .get(&to)
        .cloned()
        .unwrap_or_default();
    addrs.extend(target.last_address.filter(|a| !addrs.contains(a)));
    let reached = search(&addrs, |addr| answer_at(shared, &target, addr)).await;
    let Some(mut tcp) = reached else {
        debug!(
            "could not pass {} along to {}: it answered at none of {addrs:?}",
            caller.name, target.name
        );
        let reason = format!("{} isn't reachable from {me} right now.", target.name);
        return write_msg(stream, &Response::Refused { reason }).await;
    };
    write_msg(stream, &Response::Ok).await?;
    debug!("passing {} along to {}", caller.name, target.name);
    match tokio::io::copy_bidirectional(stream, &mut tcp).await {
        Ok((up, down)) => debug!(
            "passed {} along to {}: {up} bytes there, {down} back",
            caller.name, target.name
        ),
        // Either side going away ends it; that is how every relayed session ends.
        Err(e) => debug!(
            "passing {} along to {} ended: {e}",
            caller.name, target.name
        ),
    }
    Ok(())
}

/// The first of `addrs`, in order, where `try_one` gets an answer. Each
/// address gets ADDRESS_WAIT, and the whole list SEARCH_WAIT.
async fn search<T, F, Fut>(addrs: &[SocketAddr], try_one: F) -> Option<T>
where
    F: Fn(SocketAddr) -> Fut,
    Fut: Future<Output = Option<T>>,
{
    let each = async {
        for &addr in addrs {
            match timeout(ADDRESS_WAIT, try_one(addr)).await {
                Ok(Some(found)) => return Some(found),
                Ok(None) => {}
                Err(_) => debug!("nothing answered at {addr} within 4 seconds"),
            }
        }
        None
    };
    timeout(SEARCH_WAIT, each).await.ok().flatten()
}

/// A fresh TCP connection to `addr` if the target itself answers there. The
/// caller gets the raw connection, and anyone on the network can announce
/// the target's id with any address, even one of this computer's own
/// services; so this computer first checks, as the target's paired
/// computer, who answers there.
async fn answer_at(shared: &Shared, target: &Peer, addr: SocketAddr) -> Option<TcpStream> {
    match Connection::open_peer(addr, shared, target).await {
        Ok(mut check) => check.close().await,
        Err(e) => {
            debug!("{} isn't at {addr}: {e:#}", target.name);
            return None;
        }
    }
    match TcpStream::connect(addr).await {
        Ok(tcp) => {
            let _ = tcp.set_nodelay(true);
            Some(tcp)
        }
        Err(e) => {
            debug!("could not connect to {addr} again: {e}");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::future::pending;
    use tokio::time::Instant;

    fn at(n: u8) -> SocketAddr {
        SocketAddr::from(([192, 168, 64, n], 47820))
    }

    /// Real connections can't run on a paused clock, so these stand in for
    /// them: an address that never answers, and one where the target does.
    #[tokio::test(start_paused = true)]
    async fn addresses_that_never_answer_leave_time_for_the_right_one() {
        let right = at(4);
        let found = search(&[at(7), at(8), right], |addr| async move {
            if addr == right {
                Some(addr)
            } else {
                pending().await
            }
        })
        .await;
        assert_eq!(found, Some(right));
    }

    #[tokio::test(start_paused = true)]
    async fn the_search_gives_up_after_twelve_seconds() {
        let started = Instant::now();
        let addrs: Vec<SocketAddr> = (1..=5).map(at).collect();
        let found = search(&addrs, |_| pending::<Option<()>>()).await;
        assert_eq!(found, None);
        assert_eq!(started.elapsed().as_secs(), 12);
    }
}
