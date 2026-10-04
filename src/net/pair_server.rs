//! The answering side of pairing: commit-reveal nonces, the user's prompt, and
//! storing the pairing only once both users have confirmed.

use super::gate::Refusal;
use super::server::{PairSlot, UNPAIRED_MAX, refuse};
use super::{NetEvent, Shared, remember};
use crate::identity::{NONCE_LEN, commitment, hex, nonce, pairing_code, unhex};
use crate::model::{InstanceId, Peer, Permissions};
use crate::protocol::{Request, Response, read_msg_limited, write_msg};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt};
use tokio::sync::{Notify, oneshot};
use tokio::time::timeout;
use tracing::info;

/// How long each user has to answer before the pairing counts as abandoned.
pub(super) const PAIR_WAIT: Duration = Duration::from_secs(120);
/// The reveal follows the nonce without any user involved.
const REVEAL_WAIT: Duration = Duration::from_secs(30);
const PAIR_BUSY: &str = "Another pairing request is waiting for an answer.";
pub(super) const BAD_REVEAL: &str =
    "The pairing check failed, so nothing was saved. Start pairing again.";

pub(super) struct Caller<'a> {
    pub id: InstanceId,
    pub name: &'a str,
    pub fingerprint: &'a str,
    pub remote: SocketAddr,
    /// The port the caller listens on, 0 if it did not say.
    pub port: u16,
}

/// Whether the connection stays open afterwards.
pub(super) enum After {
    KeepOpen,
    Close,
}

pub(super) async fn respond<S: AsyncRead + AsyncWrite + Unpin + Send>(
    shared: &Shared,
    slot: &PairSlot,
    stream: &mut S,
    who: Caller<'_>,
    committed: String,
    requested: Permissions,
    offered: Permissions,
) -> anyhow::Result<After> {
    // Held until this function returns, so the slot frees on every path.
    let Ok(_slot) = slot.0.clone().try_acquire_owned() else {
        refuse(shared, stream, who.name, &Refusal::same(PAIR_BUSY)).await?;
        return Ok(After::KeepOpen);
    };
    info!("{} ({}) asked to pair", who.name, who.id);
    let mine = nonce();
    let reply = Response::PairNonce { nonce: hex(&mine) };
    write_msg(stream, &reply).await?;
    let reveal = timeout(REVEAL_WAIT, read_msg_limited(stream, UNPAIRED_MAX)).await;
    let theirs = match reveal {
        Ok(Ok(Request::PairReveal { nonce })) => unhex(&nonce, NONCE_LEN),
        _ => None,
    };
    let Some(theirs) = theirs.filter(|n| commitment(n) == committed.to_ascii_lowercase()) else {
        refuse(shared, stream, who.name, &Refusal::same(BAD_REVEAL)).await?;
        return Ok(After::Close);
    };
    let code = pairing_code(
        &shared.identity.fingerprint(),
        who.fingerprint,
        &theirs,
        &mine,
    );

    let (reply, answer) = oneshot::channel();
    let cancel = Arc::new(Notify::new());
    let prompt = NetEvent::PairPrompt {
        from_id: who.id,
        from_name: who.name.to_string(),
        code,
        requested,
        offered,
        reply,
        cancel: cancel.clone(),
    };
    if shared.events.send(prompt).is_err() {
        return Ok(After::Close);
    }
    let (mut rd, mut wr) = tokio::io::split(stream);
    // One read runs across both waits, so nothing the initiator sends is lost
    // when the user answers in the middle of it.
    let next = read_msg_limited::<_, Request>(&mut rd, UNPAIRED_MAX);
    tokio::pin!(next);
    let answered = tokio::select! {
        a = timeout(PAIR_WAIT, answer) => a,
        m = &mut next => {
            let why = match m {
                Ok(Request::PairFinal { confirmed: false }) => format!(
                    "{} cancelled pairing before you answered. Nothing was saved.",
                    who.name
                ),
                _ => format!("{} stopped pairing before you answered. Nothing was saved.", who.name),
            };
            ended(shared, &who, why);
            return Ok(After::Close);
        }
    };
    let allows = match answered {
        Ok(Ok(Some(allows))) => allows,
        Ok(Ok(None)) => {
            info!("pairing with {} declined", who.name);
            write_msg(&mut wr, &declined()).await?;
            return Ok(After::Close);
        }
        _ => {
            let why = format!(
                "The pairing request from {} expired without an answer. Nothing was saved.",
                who.name
            );
            ended(shared, &who, why);
            write_msg(&mut wr, &declined()).await?;
            return Ok(After::Close);
        }
    };
    let accepted = Response::PairResult {
        accepted: true,
        granted: allows,
    };
    write_msg(&mut wr, &accepted).await?;

    let last = tokio::select! {
        m = timeout(PAIR_WAIT, &mut next) => m,
        _ = cancel.notified() => {
            let me = shared.settings.read().await.name.clone();
            let refused = Response::Refused {
                reason: format!("Someone on {me} cancelled pairing. Nothing was saved."),
            };
            let _ = write_msg(&mut wr, &refused).await;
            let _ = wr.shutdown().await;
            info!("pairing with {} cancelled here", who.name);
            return Ok(After::Close);
        }
    };
    match last {
        Ok(Ok(Request::PairFinal { confirmed: true })) => {}
        other => {
            let why = match other {
                Ok(Ok(Request::PairFinal { confirmed: false })) => format!(
                    "{} did not confirm the code. Nothing was saved. If the codes matched, \
                     pair again.",
                    who.name
                ),
                Err(_) => format!(
                    "{} did not confirm the code within 2 minutes. Nothing was saved.",
                    who.name
                ),
                _ => format!("{} stopped pairing. Nothing was saved.", who.name),
            };
            ended(shared, &who, why);
            return Ok(After::Close);
        }
    }
    let peer = Peer {
        id: who.id,
        name: who.name.to_string(),
        fingerprint: who.fingerprint.to_string(),
        allows,
        granted: offered,
        // The source port is ephemeral; only the advertised one is dialable.
        last_address: (who.port != 0).then(|| SocketAddr::new(who.remote.ip(), who.port)),
    };
    if let Err(e) = remember(shared, peer.clone()).await {
        let me = shared.settings.read().await.name.clone();
        let reason = format!("{me} could not save the pairing: {e:#}. Try again.");
        ended(shared, &who, reason.clone());
        write_msg(&mut wr, &Response::Refused { reason }).await?;
        return Ok(After::Close);
    }
    info!("paired with {} ({})", who.name, who.id);
    let _ = shared.events.send(NetEvent::Paired(peer));
    write_msg(&mut wr, &Response::Ok).await?;
    Ok(After::Close)
}

fn declined() -> Response {
    Response::PairResult {
        accepted: false,
        granted: Permissions::default(),
    }
}

fn ended(shared: &Shared, who: &Caller<'_>, reason: String) {
    info!("pairing with {} ended: {reason}", who.name);
    let _ = shared.events.send(NetEvent::PairEnded {
        from_id: who.id,
        reason,
    });
}
