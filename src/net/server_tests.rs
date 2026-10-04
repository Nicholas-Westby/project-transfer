use super::*;
use crate::model::{Peer, Permissions};
use crate::net::test_support::{addr, shared};

#[test]
fn only_private_sources_are_accepted() {
    assert!(accept_source(addr("192.168.1.4:5000")));
    assert!(accept_source(addr("[fe80::1]:5000")));
    assert!(!accept_source(addr("8.8.8.8:5000")));
    assert!(!accept_source(addr("[2001:4860::8888]:5000")));
}

#[tokio::test]
async fn first_message_must_be_hello() {
    let dir = tempfile::tempdir().unwrap();
    let (s, mut rx) = shared(dir.path());
    let (mut client, mut server) = tokio::io::duplex(64 * 1024);
    let task = tokio::spawn(async move {
        session(
            &s,
            &PairSlot::new(),
            &mut server,
            "ff".into(),
            addr("127.0.0.1:1"),
        )
        .await
    });
    write_msg(&mut client, &Request::Status).await.unwrap();
    let resp: Response = read_msg(&mut client).await.unwrap();
    assert!(
        matches!(resp, Response::Refused { ref reason } if reason.contains("hello")),
        "{resp:?}"
    );
    task.await.unwrap().unwrap();
    assert!(
        read_msg::<_, Response>(&mut client).await.is_err(),
        "the connection should be closed after the refusal"
    );
    assert!(matches!(rx.recv().await, Some(NetEvent::Refused { .. })));
}

#[tokio::test]
async fn other_protocol_version_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let (s, _rx) = shared(dir.path());
    let (mut client, mut server) = tokio::io::duplex(64 * 1024);
    let task = tokio::spawn(async move {
        session(
            &s,
            &PairSlot::new(),
            &mut server,
            "ff".into(),
            addr("127.0.0.1:1"),
        )
        .await
    });
    let hello = Request::Hello {
        id: uuid::Uuid::new_v4(),
        name: "Old".into(),
        version: PROTOCOL_VERSION + 1,
        port: 0,
        addrs: vec![],
        via: None,
    };
    write_msg(&mut client, &hello).await.unwrap();
    let resp: Response = read_msg(&mut client).await.unwrap();
    assert!(
        matches!(resp, Response::Refused { ref reason } if reason.contains("versions")),
        "{resp:?}"
    );
    task.await.unwrap().unwrap();
}

#[tokio::test]
async fn refused_put_file_closes_the_connection() {
    let dir = tempfile::tempdir().unwrap();
    let (s, _rx) = shared(dir.path());
    let (mut client, mut server) = tokio::io::duplex(64 * 1024);
    let task = tokio::spawn(async move {
        session(
            &s,
            &PairSlot::new(),
            &mut server,
            "ff".into(),
            addr("127.0.0.1:1"),
        )
        .await
    });
    let hello = Request::Hello {
        id: uuid::Uuid::new_v4(),
        name: "X".into(),
        version: PROTOCOL_VERSION,
        port: 0,
        addrs: vec![],
        via: None,
    };
    write_msg(&mut client, &hello).await.unwrap();
    let _: Response = read_msg(&mut client).await.unwrap();
    let put = Request::PutFile {
        rel: "a".into(),
        size: 4,
        mtime_ms: 0,
        exec: false,
    };
    write_msg(&mut client, &put).await.unwrap();
    let resp: Response = read_msg(&mut client).await.unwrap();
    assert!(matches!(resp, Response::Refused { .. }));
    task.await.unwrap().unwrap();
}

/// Sends just a length prefix; the server must give up before reading a body.
async fn oversize_prefix(client: &mut tokio::io::DuplexStream, len: u32) {
    use tokio::io::AsyncWriteExt;
    client.write_all(&len.to_be_bytes()).await.unwrap();
}

#[tokio::test]
async fn unpaired_hello_over_64_kib_closes_the_connection() {
    let dir = tempfile::tempdir().unwrap();
    let (s, _rx) = shared(dir.path());
    let (mut client, mut server) = tokio::io::duplex(64 * 1024);
    let task = tokio::spawn(async move {
        session(
            &s,
            &PairSlot::new(),
            &mut server,
            "ff".into(),
            addr("127.0.0.1:1"),
        )
        .await
    });
    oversize_prefix(&mut client, 64 * 1024 + 1).await;
    let err = task.await.unwrap().unwrap_err();
    assert!(err.to_string().contains("64 KiB"), "{err:#}");
}

#[tokio::test]
async fn unpaired_request_over_64_kib_closes_the_connection() {
    let dir = tempfile::tempdir().unwrap();
    let (s, _rx) = shared(dir.path());
    let (mut client, mut server) = tokio::io::duplex(64 * 1024);
    let task = tokio::spawn(async move {
        session(
            &s,
            &PairSlot::new(),
            &mut server,
            "ff".into(),
            addr("127.0.0.1:1"),
        )
        .await
    });
    let hello = Request::Hello {
        id: uuid::Uuid::new_v4(),
        name: "X".into(),
        version: PROTOCOL_VERSION,
        port: 0,
        addrs: vec![],
        via: None,
    };
    write_msg(&mut client, &hello).await.unwrap();
    let _: Response = read_msg(&mut client).await.unwrap();
    oversize_prefix(&mut client, 1024 * 1024).await;
    let err = task.await.unwrap().unwrap_err();
    assert!(err.to_string().contains("64 KiB"), "{err:#}");
}

#[tokio::test]
async fn paired_peer_may_send_large_messages() {
    let dir = tempfile::tempdir().unwrap();
    let (s, _rx) = shared(dir.path());
    let id = uuid::Uuid::new_v4();
    s.peers.write().await.push(Peer {
        id,
        name: "Laptop".into(),
        fingerprint: "ff".into(),
        allows: Permissions {
            may_push_to_me: true,
            may_pull_from_me: true,
        },
        granted: Permissions::default(),
        last_address: None,
        via: None,
    });
    let (mut client, mut server) = tokio::io::duplex(1024 * 1024);
    tokio::spawn(async move {
        session(
            &s,
            &PairSlot::new(),
            &mut server,
            "ff".into(),
            addr("127.0.0.1:1"),
        )
        .await
    });
    let hello = Request::Hello {
        id,
        name: "Laptop".into(),
        version: PROTOCOL_VERSION,
        port: 0,
        addrs: vec![],
        via: None,
    };
    write_msg(&mut client, &hello).await.unwrap();
    let _: Response = read_msg(&mut client).await.unwrap();
    let big = Request::Hashes {
        project: uuid::Uuid::new_v4(),
        folder: uuid::Uuid::new_v4(),
        paths: vec!["p".repeat(1000); 100],
    };
    write_msg(&mut client, &big).await.unwrap();
    let resp: Response = read_msg(&mut client).await.unwrap();
    // Reached the handler: an unknown folder has no hashes.
    assert_eq!(resp, Response::Hashes(Default::default()));
}

#[tokio::test]
async fn refreshing_an_address_leaves_every_other_field_as_it_is() {
    let dir = tempfile::tempdir().unwrap();
    let (s, _rx) = shared(dir.path());
    let id = uuid::Uuid::new_v4();
    let peer = Peer {
        id,
        name: "Laptop".into(),
        fingerprint: "ff".into(),
        allows: Permissions::default(),
        granted: Permissions::default(),
        last_address: None,
        via: None,
    };
    s.peers.write().await.push(peer);
    // A permission change lands after the caller last looked at the peer.
    s.peers.write().await[0].allows.may_push_to_me = true;
    refresh_address(&s, id, "ff", Some(addr("127.0.0.1:4242"))).await;
    let live = s.peers.read().await[0].clone();
    assert_eq!(live.last_address, Some(addr("127.0.0.1:4242")));
    assert!(live.allows.may_push_to_me);
    assert_eq!(
        s.store.load_peers().unwrap()[0].last_address,
        live.last_address
    );
}

#[tokio::test]
async fn a_refusal_is_worded_for_each_side() {
    let dir = tempfile::tempdir().unwrap();
    let (s, mut rx) = shared(dir.path());
    let id = uuid::Uuid::new_v4();
    s.peers.write().await.push(Peer {
        id,
        name: "Laptop".into(),
        fingerprint: "ff".into(),
        allows: Permissions::default(),
        granted: Permissions::default(),
        last_address: None,
        via: None,
    });
    let (mut client, mut server) = tokio::io::duplex(64 * 1024);
    tokio::spawn(async move {
        session(
            &s,
            &PairSlot::new(),
            &mut server,
            "ff".into(),
            addr("127.0.0.1:1"),
        )
        .await
    });
    let hello = Request::Hello {
        id,
        name: "Laptop".into(),
        version: PROTOCOL_VERSION,
        port: 0,
        addrs: vec![],
        via: None,
    };
    write_msg(&mut client, &hello).await.unwrap();
    let _: Response = read_msg(&mut client).await.unwrap();
    let ask = Request::ProjectInfo {
        project: uuid::Uuid::new_v4(),
    };
    write_msg(&mut client, &ask).await.unwrap();
    let Response::Refused { reason } = read_msg(&mut client).await.unwrap() else {
        panic!("expected a refusal");
    };
    assert!(
        reason.starts_with("Desk doesn't let this computer"),
        "{reason}"
    );
    let Some(NetEvent::Refused { peer_name, reason }) = rx.recv().await else {
        panic!("expected a refusal event");
    };
    assert_eq!(peer_name, "Laptop");
    assert!(reason.starts_with("it asked about a project"), "{reason}");
}
