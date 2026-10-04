//! Opening a connection to a paired computer.

use super::{Connection, NotThePairedComputer};
use crate::model::Peer;
use crate::net::test_support::{laptop, listen_as, shared};
use tokio::net::TcpListener;

/// The certificate is known before the hello, so a computer that isn't the
/// paired one never learns this one's id, name or addresses.
#[tokio::test]
async fn a_computer_answering_in_place_of_a_paired_one_hears_nothing_from_this_one() {
    let (d1, d2) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let (s, _rx) = shared(d1.path());
    let (other, _rx2) = shared(d2.path());
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let at = listener.local_addr().unwrap();
    let heard = tokio::spawn(async move {
        let (tcp, _) = listener.accept().await.unwrap();
        listen_as(other, tcp).await.unwrap()
    });
    let err = match Connection::open_peer(at, &s, &laptop()).await {
        Ok(_) => panic!("took another computer for Laptop"),
        Err(e) => e,
    };
    assert!(err.is::<NotThePairedComputer>(), "{err:#}");
    let want = format!("Laptop isn't at {at} any more: another computer answers there.");
    assert!(err.to_string().starts_with(&want), "{err}");
    let heard = heard.await.unwrap();
    assert!(heard.is_none(), "the other computer heard {heard:?}");
}

/// Holding the paired computer's certificate isn't enough: its id, known
/// after the hello, has to match too.
#[tokio::test]
async fn a_computer_with_the_paired_certificate_but_another_id_is_not_the_paired_one() {
    let (d1, d2) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let (s, _rx) = shared(d1.path());
    let (other, _rx2) = shared(d2.path());
    let expected = Peer {
        fingerprint: other.identity.fingerprint(),
        ..laptop()
    };
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let at = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let (tcp, _) = listener.accept().await.unwrap();
        listen_as(other, tcp).await.unwrap()
    });
    let err = match Connection::open_peer(at, &s, &expected).await {
        Ok(_) => panic!("took another computer for Laptop"),
        Err(e) => e,
    };
    assert!(err.is::<NotThePairedComputer>(), "{err:#}");
    let want = format!("Laptop isn't at {at} any more: another computer answers there.");
    assert!(err.to_string().starts_with(&want), "{err}");
}
