//! When a computer won't pass a connection along, or something other than
//! the paired computer answers where it looks.

mod relay_support;
mod support;

use project_transfer::model::{InstanceId, Peer};
use project_transfer::net::{Connection, NotThePairedComputer};
use project_transfer::protocol::{Request, Response};
use relay_support::*;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use support::Instance;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

/// Stands in for the computer M would pass the connection to.
async fn watched() -> TcpListener {
    TcpListener::bind("127.0.0.1:0").await.unwrap()
}

/// Nothing connected to `target`: M refused before dialling it.
async fn assert_untouched(target: &TcpListener) {
    let knock = tokio::time::timeout(Duration::from_millis(200), target.accept()).await;
    assert!(knock.is_err(), "the relay connected to the target");
}

/// Why `relay` would not pass a connection from `from` along to `to`.
async fn refusal(from: &Instance, relay: Connection, to: InstanceId) -> String {
    match Connection::through(relay, to, &from.shared).await {
        Ok(_) => panic!("the connection was passed along"),
        Err(e) => format!("{e:#}"),
    }
}

#[tokio::test]
async fn a_caller_the_relay_is_not_paired_with_is_turned_away() {
    let t = trio().await;
    let b_id = t.b.id().await;
    let target = watched().await;
    // Where M would dial first, were it to pass the connection along.
    let at = vec![target.local_addr().unwrap()];
    t.m.shared.found.write().await.insert(b_id, at);
    let stranger = start().await;
    let relay = Connection::open(t.m.addr, &stranger.shared).await.unwrap();
    let why = refusal(&stranger, relay, b_id).await;
    assert!(why.contains("aren't paired"), "{why}");
    assert_untouched(&target).await;
}

#[tokio::test]
async fn a_target_the_relay_is_not_paired_with_is_turned_away() {
    let t = trio().await;
    let target = watched().await;
    // M sees it on the network, but isn't paired with it.
    let unpaired = InstanceId::new_v4();
    let at = vec![target.local_addr().unwrap()];
    t.m.shared.found.write().await.insert(unpaired, at);
    let m = name(&t.m).await;
    let want = format!(
        "{m}, which passes the connection along, says: {m} isn't paired with that computer, so \
         it can't pass the connection along."
    );
    // Nor does M pass a connection back to the caller, or to itself.
    for to in [unpaired, t.a.id().await, t.m.id().await] {
        let why = refusal(&t.a, open(&t.a, &t.m).await, to).await;
        assert_eq!(why, want);
    }
    assert_untouched(&target).await;
}

#[tokio::test]
async fn a_paired_target_with_no_working_address_is_not_reachable() {
    let t = trio().await;
    let b_id = t.b.id().await;
    t.m.shared.found.write().await.remove(&b_id);
    set_stored_address(&t.m, b_id, None).await;
    let m = name(&t.m).await;
    let want = format!(
        "{m}, which passes the connection along, says: {} isn't reachable from {m} right now.",
        stored(&t.m, b_id).await.name
    );
    let why = refusal(&t.a, open(&t.a, &t.m).await, b_id).await;
    assert_eq!(why, want);

    // An address that doesn't answer any more counts the same.
    let gone = vec![closed_port().await];
    t.m.shared.found.write().await.insert(b_id, gone);
    let why = refusal(&t.a, open(&t.a, &t.m).await, b_id).await;
    assert_eq!(why, want);
}

async fn open_through(t: &Trio, expected: &Peer) -> anyhow::Error {
    let relay = open(&t.a, &t.m).await;
    match Connection::open_peer_through(relay, &t.a.shared, expected).await {
        Ok(_) => panic!("took another computer for {}", expected.name),
        Err(e) => e,
    }
}

/// M checks B's certificate as M stored it; A checks it against its own.
#[tokio::test]
async fn a_certificate_other_than_the_paired_one_through_the_relay_is_refused() {
    let t = trio().await;
    let b = pair_through(&t).await;
    let mut other_certificate = b.clone();
    other_certificate.fingerprint = "00".repeat(32);
    let err = open_through(&t, &other_certificate).await;
    assert!(err.is::<NotThePairedComputer>(), "{err:#}");
    assert!(err.to_string().contains(&b.name), "{err}");
}

/// Something on the network that isn't Project Transfer: it greets every
/// connection, as many services do, and keeps whatever it is sent.
struct Service {
    addr: SocketAddr,
    heard: Arc<Mutex<Vec<u8>>>,
    calls: Arc<AtomicUsize>,
}

async fn service() -> Service {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (heard, calls) = (
        Arc::new(Mutex::new(Vec::new())),
        Arc::new(AtomicUsize::new(0)),
    );
    let (h, c) = (heard.clone(), calls.clone());
    tokio::spawn(async move {
        while let Ok((mut s, _)) = listener.accept().await {
            c.fetch_add(1, Ordering::SeqCst);
            let h = h.clone();
            tokio::spawn(async move {
                let _ = s.write_all(b"SSH-2.0-Garden\r\n").await;
                let mut buf = [0u8; 4096];
                loop {
                    match s.read(&mut buf).await {
                        Ok(0) | Err(_) => break,
                        Ok(n) => h.lock().unwrap().extend_from_slice(&buf[..n]),
                    }
                }
            });
        }
    });
    Service { addr, heard, calls }
}

#[tokio::test]
async fn a_relay_checks_who_answers_before_passing_anything_along() {
    let t = trio().await;
    let b_id = t.b.id().await;
    // Anyone on the network can announce B's id at an address of their choosing.
    let service = service().await;
    t.m.shared
        .found
        .write()
        .await
        .insert(b_id, vec![service.addr]);

    // M finds it isn't B there, so it uses where it last reached B.
    let mut relay = open(&t.a, &t.m).await;
    let reply = relay.request(&Request::Relay { to: b_id }).await.unwrap();
    assert_eq!(reply, Response::Ok);
    relay.send_raw(b"PRIVATE").await.unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;
    let heard = service.heard.lock().unwrap().clone();
    assert!(
        !heard.windows(7).any(|w| w == b"PRIVATE"),
        "the caller reached the service"
    );
    assert_eq!(
        service.calls.load(Ordering::SeqCst),
        1,
        "M looked where it saw B first"
    );

    // With nowhere else to look, M refuses.
    set_stored_address(&t.m, b_id, None).await;
    let mut relay = open(&t.a, &t.m).await;
    let reply = relay.request(&Request::Relay { to: b_id }).await.unwrap();
    let reason = format!(
        "{} isn't reachable from {} right now.",
        stored(&t.m, b_id).await.name,
        name(&t.m).await
    );
    assert_eq!(reply, Response::Refused { reason });
}

#[tokio::test]
async fn a_relay_passes_over_another_computer_where_it_sees_the_target() {
    let t = trio().await;
    let b_id = t.b.id().await;
    let stranger = start().await;
    t.m.shared
        .found
        .write()
        .await
        .insert(b_id, vec![stranger.addr]);
    let relay = open(&t.a, &t.m).await;
    let conn = Connection::through(relay, b_id, &t.a.shared).await;
    assert_eq!(conn.map(|c| c.peer_id()).ok(), Some(b_id));
}
