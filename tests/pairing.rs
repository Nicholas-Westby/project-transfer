//! Two (or three) instances on 127.0.0.1: permissions and refusals after pairing.

mod pairing_support;

use pairing_support::*;
use project_transfer::net::Connection;
use project_transfer::protocol::{Request, Response};

#[tokio::test]
async fn repairing_updates_permissions_without_duplicating() {
    let (a, b) = paired(perms(true, false)).await;
    let mut conn = Connection::open(b.addr, &a.shared).await.unwrap();
    let peer = pair(&mut conn, &a.shared, perms(true, true), perms(false, false))
        .await
        .unwrap();
    // B's auto-answer is fixed, so check the side whose offer changed.
    assert_eq!(peer.allows, perms(false, false));
    assert_eq!(a.shared.store.load_peers().unwrap().len(), 1);
    let on_b = b.shared.store.load_peers().unwrap();
    assert_eq!(on_b.len(), 1);
    assert_eq!(on_b[0].granted, perms(false, false));
}

#[tokio::test]
async fn unpaired_manifest_is_refused_by_permission() {
    let a = instance(None).await;
    let b = instance(None).await;
    let mut conn = Connection::open(b.addr, &a.shared).await.unwrap();
    let reason = refusal(conn.request(&manifest_req()).await.unwrap());
    assert_ne!(reason, NOT_IMPLEMENTED);
    assert!(reason.contains("aren't paired"), "{reason}");
    let reason = refusal(conn.request(&Request::Status).await.unwrap());
    assert!(reason.contains("aren't paired"), "{reason}");
    assert_eq!(b.refusals.lock().unwrap().len(), 2);
}

#[tokio::test]
async fn paired_without_pull_is_refused_for_get_file() {
    let (a, b) = paired(perms(true, false)).await;
    let mut conn = Connection::open(b.addr, &a.shared).await.unwrap();
    let get = Request::GetFile {
        project: uuid::Uuid::new_v4(),
        folder: uuid::Uuid::new_v4(),
        rel: "a.txt".into(),
    };
    let reason = refusal(conn.request(&get).await.unwrap());
    assert!(reason.contains("pull"), "{reason}");
    assert_ne!(reason, NOT_IMPLEMENTED);
    // A push preview needs the manifest, so push permission alone passes the gate.
    let reply = conn.request(&manifest_req()).await.unwrap();
    assert!(matches!(reply, Response::Manifest(_)), "{reply:?}");
}

#[tokio::test]
async fn paired_without_push_is_refused_for_begin_push() {
    let (a, b) = paired(perms(false, true)).await;
    let mut conn = Connection::open(b.addr, &a.shared).await.unwrap();
    let p = project();
    let begin = Request::BeginPush {
        folder: p.primary,
        project: p,
        expected_path: "/tmp/app".into(),
    };
    let reason = refusal(conn.request(&begin).await.unwrap());
    assert!(reason.contains("push"), "{reason}");
    assert_ne!(reason, NOT_IMPLEMENTED);
    let reason = refusal(conn.request(&Request::EndPush).await.unwrap());
    assert!(reason.contains("push"), "{reason}");
}

#[tokio::test]
async fn paired_peer_gets_status_and_project_info() {
    let (a, b) = paired(perms(false, true)).await;
    let p = project();
    b.shared.projects.write().await.push(p.clone());
    let mut conn = Connection::open(b.addr, &a.shared).await.unwrap();
    match conn.request(&Request::Status).await.unwrap() {
        Response::Status { allows, projects } => {
            assert_eq!(allows, perms(false, true));
            assert_eq!(projects.len(), 1);
            assert_eq!(projects[0].id, p.id);
            assert_eq!(projects[0].name, "Garden");
        }
        other => panic!("{other:?}"),
    }
    match conn
        .request(&Request::ProjectInfo { project: p.id })
        .await
        .unwrap()
    {
        Response::ProjectInfo(Some(info)) => {
            assert_eq!(info.name, "Garden");
            assert_eq!(info.folders.len(), 1);
            assert_eq!(info.folders[0].id, p.primary);
            assert_eq!(info.folders[0].path.as_deref(), Some("/tmp/app"));
        }
        other => panic!("{other:?}"),
    }
    let missing = Request::ProjectInfo {
        project: uuid::Uuid::new_v4(),
    };
    assert_eq!(
        conn.request(&missing).await.unwrap(),
        Response::ProjectInfo(None)
    );
}

/// Sent to the computer whose identity changed.
const CHANGED: &str = "This computer's identity changed. Unpair it and pair again.";
/// Shown on the computer that noticed.
const CHANGED_THERE: &str = "its identity changed since you paired";

#[tokio::test]
async fn impostor_with_paired_id_and_other_cert_is_refused() {
    let (a, b) = paired(perms(true, true)).await;
    let c = instance(None).await;
    c.shared.settings.write().await.id = id_of(&a).await;
    let err = match Connection::open(b.addr, &c.shared).await {
        Ok(mut conn) => conn.request(&Request::Status).await.map(refusal).unwrap(),
        Err(e) => e.to_string(),
    };
    assert!(err.contains(CHANGED), "{err}");
    let seen = b.refusals.lock().unwrap().clone();
    assert!(
        seen.iter().any(|r| r.starts_with(CHANGED_THERE)),
        "{seen:?}"
    );
}

#[tokio::test]
async fn client_refuses_server_whose_cert_changed() {
    let (a, b) = paired(perms(true, true)).await;
    let impostor = instance(None).await;
    impostor.shared.settings.write().await.id = id_of(&b).await;
    let err = Connection::open(impostor.addr, &a.shared)
        .await
        .err()
        .expect("must refuse");
    assert!(err.to_string().contains(CHANGED_THERE), "{err}");
}

#[tokio::test]
async fn open_refuses_public_address() {
    let a = instance(None).await;
    let err = Connection::open("8.8.8.8:4000".parse().unwrap(), &a.shared)
        .await
        .err()
        .expect("must refuse");
    assert!(err.to_string().contains("private"), "{err}");
}

#[tokio::test]
async fn open_peer_accepts_the_paired_computer() {
    let (a, b) = paired(perms(true, true)).await;
    let stored = a.shared.peers.read().await[0].clone();
    let conn = Connection::open_peer(b.addr, &a.shared, &stored)
        .await
        .unwrap();
    assert_eq!(conn.peer_id(), stored.id);
}

#[tokio::test]
async fn open_peer_refuses_a_stranger_at_the_peers_address() {
    let (a, _b) = paired(perms(true, true)).await;
    let stored = a.shared.peers.read().await[0].clone();
    // A fresh instance with its own id and cert answering where the peer was.
    let stranger = instance(Some(perms(true, true))).await;
    let err = Connection::open_peer(stranger.addr, &a.shared, &stored)
        .await
        .err()
        .expect("must refuse");
    let msg = err.to_string();
    assert!(msg.contains(&format!("is not {}", stored.name)), "{msg}");
    // The id matches but the cert does not: still not the paired computer.
    let mut same_id = stored.clone();
    same_id.fingerprint = "00".repeat(32);
    let err = Connection::open_peer(stranger.addr, &a.shared, &same_id).await;
    assert!(err.is_err());
}

#[tokio::test]
async fn only_one_pairing_prompt_waits_at_a_time() {
    let b = holding().await;
    let (a1, a2) = (instance(None).await, instance(None).await);
    let first = {
        let shared = a1.shared.clone();
        let addr = b.addr;
        tokio::spawn(async move {
            let mut conn = Connection::open(addr, &shared).await.unwrap();
            pair(&mut conn, &shared, perms(true, true), perms(true, true)).await
        })
    };
    while b.held.lock().unwrap().is_empty() {
        tokio::task::yield_now().await;
    }
    let mut conn = Connection::open(b.addr, &a2.shared).await.unwrap();
    let err = pair(&mut conn, &a2.shared, perms(true, true), perms(true, true))
        .await
        .unwrap_err();
    let busy = "Another pairing request is waiting for an answer.";
    assert!(err.to_string().contains(busy), "{err}");
    assert_eq!(b.codes.lock().unwrap().len(), 1);

    // Once the first prompt is answered the slot is free again.
    let reply = b.held.lock().unwrap().pop().unwrap();
    reply.send(Some(perms(false, true))).unwrap();
    first.await.unwrap().unwrap();
    let shared = a2.shared.clone();
    let again = tokio::spawn(async move {
        pair(&mut conn, &shared, perms(true, true), perms(true, true)).await
    });
    while b.held.lock().unwrap().is_empty() {
        tokio::task::yield_now().await;
    }
    let reply = b.held.lock().unwrap().pop().unwrap();
    reply.send(Some(perms(true, false))).unwrap();
    again.await.unwrap().unwrap();
    assert_eq!(b.shared.peers.read().await.len(), 2);
}
