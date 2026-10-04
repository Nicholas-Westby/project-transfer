//! The starting side of pairing. Nothing is stored unless this computer's
//! user confirmed the code and the other computer's user accepted.

use super::client::Connection;
use super::pair_server::PAIR_WAIT;
use super::{NetEvent, Shared, remember};
use crate::identity::{NONCE_LEN, commitment, hex, nonce, pairing_code, unhex};
use crate::model::{Peer, Permissions};
use crate::protocol::{Request, Response, read_msg, write_msg};
use anyhow::{Context, anyhow, bail};
use std::future::Future;
use std::time::Duration;
use tokio::io::AsyncWriteExt;
use tokio::time::timeout;
use tracing::info;

/// Steps that need no user wait at most this long for the other computer.
const STEP_WAIT: Duration = Duration::from_secs(30);

/// The code to show once the nonces are exchanged.
#[derive(Debug)]
pub struct PairStarted {
    pub code: String,
    offered: Permissions,
}

/// Exchanges nonces and returns the code both screens show. `requested` is
/// what we ask the peer to allow us; `offered` is what we allow it.
pub async fn pair_start(
    conn: &mut Connection,
    shared: &Shared,
    requested: Permissions,
    offered: Permissions,
) -> anyhow::Result<PairStarted> {
    info!("asking {} to pair", conn.peer_name);
    let mine = nonce();
    let commit = Request::PairCommit {
        commitment: commitment(&mine),
        requested,
        offered,
    };
    let reply = timeout(STEP_WAIT, conn.request(&commit))
        .await
        .map_err(|_| anyhow!("{} did not answer the pairing request.", conn.peer_name))??;
    let theirs = match reply {
        Response::PairNonce { nonce } => unhex(&nonce, NONCE_LEN)
            .ok_or_else(|| anyhow!("{} sent a malformed pairing nonce.", conn.peer_name))?,
        Response::Refused { reason } => bail!("{reason}"),
        other => bail!(
            "{} answered the pairing request with {other:?}",
            conn.peer_name
        ),
    };
    conn.send(&Request::PairReveal { nonce: hex(&mine) })
        .await?;
    let code = pairing_code(
        &shared.identity.fingerprint(),
        &conn.peer_fingerprint,
        &mine,
        &theirs,
    );
    Ok(PairStarted { code, offered })
}

/// Waits for both answers: the other computer's user (calls `on_accepted`
/// when they accept) and this computer's (`confirm`), in either order.
/// Stores the peer only when both said yes.
pub async fn pair_finish(
    conn: &mut Connection,
    shared: &Shared,
    started: PairStarted,
    confirm: impl Future<Output = bool>,
    on_accepted: impl FnOnce(),
) -> anyhow::Result<Peer> {
    let name = conn.peer_name.clone();
    let (mut rd, mut wr) = tokio::io::split(&mut conn.stream);
    let confirm = async { timeout(PAIR_WAIT, confirm).await.unwrap_or(false) };
    tokio::pin!(confirm);
    let mut answer = Box::pin(timeout(
        PAIR_WAIT + Duration::from_secs(10),
        read_msg::<_, Response>(&mut rd),
    ));
    let first = tokio::select! {
        r = &mut answer => Err(r),
        u = &mut confirm => Ok(u),
    };
    let granted = match first {
        Ok(false) => {
            let _ = write_msg(&mut wr, &Request::PairFinal { confirmed: false }).await;
            let _ = wr.shutdown().await;
            bail!(CANCELLED);
        }
        Ok(true) => {
            let granted = accepted(&name, answer.await)?;
            on_accepted();
            granted
        }
        Err(r) => {
            drop(answer);
            let granted = accepted(&name, r)?;
            on_accepted();
            // Whatever the other side sends now means it gave up.
            let gave_up = read_msg::<_, Response>(&mut rd);
            let user = tokio::select! {
                u = &mut confirm => u,
                m = gave_up => match m {
                    Ok(Response::Refused { reason }) => bail!("{reason}"),
                    _ => bail!("{name} stopped pairing. Nothing was saved."),
                },
            };
            if !user {
                let _ = write_msg(&mut wr, &Request::PairFinal { confirmed: false }).await;
                let _ = wr.shutdown().await;
                bail!(CANCELLED);
            }
            granted
        }
    };
    write_msg(&mut wr, &Request::PairFinal { confirmed: true }).await?;
    let done = timeout(STEP_WAIT, read_msg::<_, Response>(&mut rd))
        .await
        .map_err(|_| anyhow!("{name} did not finish pairing. Pair again."))?
        .with_context(|| format!("{name} did not finish pairing. Pair again"))?;
    match done {
        Response::Ok => {}
        Response::Refused { reason } => bail!("{reason}"),
        other => bail!("{name} finished pairing with {other:?}"),
    }
    let peer = Peer {
        id: conn.peer_id,
        name,
        fingerprint: conn.peer_fingerprint.clone(),
        allows: started.offered,
        granted,
        // A relay's address would reach the relay, never this peer.
        last_address: conn.via.is_none().then_some(conn.addr),
        via: conn.via,
    };
    remember(shared, peer.clone())
        .await
        .context("Paired, but could not save it on this computer. Pair again.")?;
    info!("paired with {} ({})", peer.name, peer.id);
    let _ = shared.events.send(NetEvent::Paired(peer.clone()));
    Ok(peer)
}

pub const CANCELLED: &str = "You cancelled pairing. Nothing was saved on either computer.";

/// What the other user granted, or why pairing stops here.
fn accepted(
    name: &str,
    r: Result<anyhow::Result<Response>, tokio::time::error::Elapsed>,
) -> anyhow::Result<Permissions> {
    match r {
        Ok(Ok(Response::PairResult {
            accepted: true,
            granted,
        })) => Ok(granted),
        Ok(Ok(Response::PairResult {
            accepted: false, ..
        })) => bail!("{name} declined pairing, or nobody answered within 2 minutes."),
        Ok(Ok(Response::Refused { reason })) => bail!("{reason}"),
        Ok(Ok(other)) => bail!("{name} answered the pairing request with {other:?}"),
        Ok(Err(_)) => bail!("{name} stopped pairing. Nothing was saved."),
        Err(_) => bail!("{name} did not answer the pairing request."),
    }
}
