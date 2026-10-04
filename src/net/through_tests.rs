//! The calling side of a relay, against a pretend relay over in-memory pipes:
//! what a well-behaved relay never lets happen still has to be caught here.

use super::client::Connection;
use super::server::{PairSlot, session};
use super::test_support::{addr, laptop, listen_as, shared};
use super::{NotThePairedComputer, Shared, tls};
use crate::model::{InstanceId, Peer};
use crate::protocol::{Request, Response, read_msg, write_msg};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncWrite, DuplexStream, duplex};
use tokio_rustls::TlsAcceptor;

/// A connection to a paired computer called Studio, over `stream`.
fn relay_over(stream: DuplexStream) -> Connection {
    Connection {
        stream: Box::new(stream),
        addr: addr("192.168.64.1:47820"),
        via: None,
        peer_id: uuid::Uuid::new_v4(),
        peer_name: "Studio".into(),
        peer_fingerprint: "ab".repeat(32),
    }
}

/// Plays the relay at the other end of `near`: agrees to pass the connection
/// along, then joins the caller to `far`, whatever that is.
fn pass_along(
    mut near: DuplexStream,
    mut far: impl AsyncRead + AsyncWrite + Unpin + Send + 'static,
) {
    tokio::spawn(async move {
        let _: Request = read_msg(&mut near).await.unwrap();
        write_msg(&mut near, &Response::Ok).await.unwrap();
        let _ = tokio::io::copy_bidirectional(&mut near, &mut far).await;
    });
}

/// Answers as `who` would, over TLS at the far end of `stream`.
fn answer_as(who: Shared, stream: DuplexStream) {
    tokio::spawn(async move {
        let acceptor = TlsAcceptor::from(Arc::new(tls::server_config(&who.identity).unwrap()));
        let mut tls = acceptor.accept(stream).await.unwrap();
        let fingerprint = tls::peer_fingerprint(tls.get_ref().1.peer_certificates()).unwrap();
        let from = addr("192.168.64.1:50000");
        let _ = session(&who, &PairSlot::new(), &mut tls, fingerprint, from).await;
    });
}

#[tokio::test]
async fn a_relayed_handshake_that_fails_names_the_relay_not_its_address() {
    let dir = tempfile::tempdir().unwrap();
    let (s, _rx) = shared(dir.path());
    let (ours, theirs) = duplex(64 * 1024);
    let (far, hung_up) = duplex(64 * 1024);
    drop(hung_up);
    pass_along(theirs, far);
    let err = match Connection::through(relay_over(ours), uuid::Uuid::new_v4(), &s).await {
        Ok(_) => panic!("connected to nothing"),
        Err(e) => format!("{e:#}"),
    };
    assert!(err.contains("the computer reached through Studio"), "{err}");
    assert!(!err.contains("192.168.64.1"), "{err}");
}

/// The stranger even holds Laptop's certificate here, so only its id, known
/// after the hello, gives it away.
#[tokio::test]
async fn a_relay_handing_the_caller_to_another_computer_is_caught() {
    let (d1, d2) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let (s, _rx) = shared(d1.path());
    let (stranger, _rx2) = shared(d2.path());
    let expected = Peer {
        fingerprint: stranger.identity.fingerprint(),
        ..laptop()
    };
    let (to_stranger, at_stranger) = duplex(64 * 1024);
    answer_as(stranger, at_stranger);
    let (ours, theirs) = duplex(64 * 1024);
    pass_along(theirs, to_stranger);
    let err = match Connection::open_peer_through(relay_over(ours), &s, &expected).await {
        Ok(_) => panic!("took another computer for Laptop"),
        Err(e) => e,
    };
    assert!(err.is::<NotThePairedComputer>(), "{err:#}");
    assert!(
        err.to_string()
            .starts_with("Laptop isn't reachable through Studio"),
        "{err}"
    );
}

/// As with a direct connection: the certificate is checked before the hello.
#[tokio::test]
async fn a_computer_passed_off_as_a_paired_one_hears_nothing_from_this_one() {
    let (d1, d2) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let (s, _rx) = shared(d1.path());
    let (other, _rx2) = shared(d2.path());
    let (to_other, at_other) = duplex(64 * 1024);
    let heard = listen_as(other, at_other);
    let (ours, theirs) = duplex(64 * 1024);
    pass_along(theirs, to_other);
    let err = match Connection::open_peer_through(relay_over(ours), &s, &laptop()).await {
        Ok(_) => panic!("took another computer for Laptop"),
        Err(e) => e,
    };
    assert!(err.is::<NotThePairedComputer>(), "{err:#}");
    let want = "Laptop isn't reachable through Studio any more: another computer answers there.";
    assert!(err.to_string().starts_with(want), "{err}");
    let heard = heard.await.unwrap();
    assert!(heard.is_none(), "the other computer heard {heard:?}");
}

/// What the target hears first from a caller Studio passes along, and
/// Studio's id.
async fn relayed_hello() -> (Request, InstanceId) {
    let (d1, d2) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let (s, _rx) = shared(d1.path());
    let (target, _rx2) = shared(d2.path());
    let (to_target, at_target) = duplex(64 * 1024);
    let heard = tokio::spawn(async move {
        let acceptor = TlsAcceptor::from(Arc::new(tls::server_config(&target.identity).unwrap()));
        let mut tls = acceptor.accept(at_target).await.unwrap();
        read_msg::<_, Request>(&mut tls).await.unwrap()
    });
    let (ours, theirs) = duplex(64 * 1024);
    pass_along(theirs, to_target);
    let relay = relay_over(ours);
    let studio = relay.peer_id;
    // The target hangs up after the hello, which ends this attempt.
    let _ = Connection::through(relay, uuid::Uuid::new_v4(), &s).await;
    (heard.await.unwrap(), studio)
}

/// The call reaches the target from the relay's address, which can be one of
/// the caller's own too: every computer running virtual machines may have it.
#[tokio::test]
async fn a_hello_passed_along_lists_none_of_the_callers_addresses() {
    let (Request::Hello { addrs, .. }, _) = relayed_hello().await else {
        panic!("the first message was not a hello");
    };
    assert!(addrs.is_empty(), "{addrs:?}");
}

/// So the target knows a way back to the caller.
#[tokio::test]
async fn a_hello_passed_along_names_the_relay() {
    let (Request::Hello { via, .. }, studio) = relayed_hello().await else {
        panic!("the first message was not a hello");
    };
    assert_eq!(via, Some(studio));
}

#[tokio::test(start_paused = true)]
async fn a_relay_that_never_answers_is_given_up_on() {
    let dir = tempfile::tempdir().unwrap();
    let (s, _rx) = shared(dir.path());
    let (ours, _silent) = duplex(64 * 1024);
    let asked = Connection::through(relay_over(ours), uuid::Uuid::new_v4(), &s);
    let err = match tokio::time::timeout(Duration::from_secs(60), asked).await {
        Err(_) => panic!("still waiting for the relay after a minute"),
        Ok(Ok(_)) => panic!("passed along by a relay that never answered"),
        Ok(Err(e)) => format!("{e:#}"),
    };
    assert!(err.starts_with("Studio did not answer"), "{err}");
}
