//! Listening instances whose users answer pairing prompts as each test needs.
#![allow(dead_code)]

use project_transfer::identity::Identity;
use project_transfer::ignore_rules::IgnoreSpec;
use project_transfer::model::{Folder, InstanceSettings, Peer, Permissions, Project};
use project_transfer::net::{Connection, NetEvent, Shared, pair_finish, pair_start, serve};
use project_transfer::protocol::{Request, Response};
use project_transfer::store::Store;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::{RwLock, mpsc, oneshot};

pub const NOT_IMPLEMENTED: &str = "Not implemented yet";

pub struct Instance {
    pub shared: Shared,
    pub addr: SocketAddr,
    pub codes: Arc<Mutex<Vec<String>>>,
    pub refusals: Arc<Mutex<Vec<String>>>,
    /// Why each pairing another computer started ended without pairing.
    pub ended: Arc<Mutex<Vec<String>>>,
    /// Prompts left unanswered by an instance started with `holding()`.
    pub held: Arc<Mutex<Vec<oneshot::Sender<Option<Permissions>>>>>,
    _dir: tempfile::TempDir,
}

/// Starts a listening instance whose user answers every pairing prompt with
/// `answer` (None declines).
pub async fn instance(answer: Option<Permissions>) -> Instance {
    start(Some(answer)).await
}

/// An instance whose user leaves every pairing prompt waiting.
pub async fn holding() -> Instance {
    start(None).await
}

async fn start(answer: Option<Option<Permissions>>) -> Instance {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_at(dir.path().to_path_buf()).unwrap();
    let identity = Identity::load_or_create(&store.identity_dir()).unwrap();
    let settings = InstanceSettings {
        id: uuid::Uuid::new_v4(),
        name: format!("Test {}", &uuid::Uuid::new_v4().simple().to_string()[..4]),
        projects_folder: dir.path().join("Dev"),
        extra_ignores: vec![],
        always_include: vec![],
        removed_default_ignores: vec![],
        last_peer: None,
        theme: Default::default(),
        port: 0,
    };
    let (tx, mut rx) = mpsc::unbounded_channel();
    let shared = Shared {
        settings: Arc::new(RwLock::new(settings)),
        peers: Arc::new(RwLock::new(Vec::new())),
        projects: Arc::new(RwLock::new(Vec::new())),
        store: Arc::new(store),
        identity: Arc::new(identity),
        events: tx,
        found: Default::default(),
    };
    let codes = Arc::new(Mutex::new(Vec::new()));
    let refusals = Arc::new(Mutex::new(Vec::new()));
    let ended = Arc::new(Mutex::new(Vec::new()));
    let held = Arc::new(Mutex::new(Vec::new()));
    let (c, r, h, e) = (codes.clone(), refusals.clone(), held.clone(), ended.clone());
    tokio::spawn(async move {
        while let Some(ev) = rx.recv().await {
            match ev {
                NetEvent::PairPrompt { code, reply, .. } => {
                    c.lock().unwrap().push(code);
                    match answer {
                        Some(a) => {
                            let _ = reply.send(a);
                        }
                        None => h.lock().unwrap().push(reply),
                    }
                }
                NetEvent::Refused { reason, .. } => r.lock().unwrap().push(reason),
                NetEvent::PairEnded { reason, .. } => e.lock().unwrap().push(reason),
                _ => {}
            }
        }
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    shared.settings.write().await.port = addr.port();
    tokio::spawn(serve(shared.clone(), listener));
    Instance {
        shared,
        addr,
        codes,
        refusals,
        ended,
        held,
        _dir: dir,
    }
}

pub async fn id_of(i: &Instance) -> uuid::Uuid {
    i.shared.settings.read().await.id
}

pub fn perms(push: bool, pull: bool) -> Permissions {
    Permissions {
        may_push_to_me: push,
        may_pull_from_me: pull,
    }
}

pub async fn paired(allows: Permissions) -> (Instance, Instance) {
    let a = instance(None).await;
    let b = instance(Some(allows)).await;
    let mut conn = Connection::open(b.addr, &a.shared).await.unwrap();
    pair(&mut conn, &a.shared, perms(true, true), perms(true, true))
        .await
        .unwrap();
    (a, b)
}

pub fn refusal(resp: Response) -> String {
    match resp {
        Response::Refused { reason } => reason,
        other => panic!("expected a refusal, got {other:?}"),
    }
}

pub fn manifest_req() -> Request {
    Request::Manifest {
        project: uuid::Uuid::new_v4(),
        folder: uuid::Uuid::new_v4(),
        ignore: IgnoreSpec::default(),
        folder_name: "app".into(),
        project_name: "Garden".into(),
        multi_folder: false,
        from_os: None,
    }
}

pub fn project() -> Project {
    let folder = Folder {
        id: uuid::Uuid::new_v4(),
        name: "app".into(),
        local_path: Some(PathBuf::from("/tmp/app")),
    };
    Project {
        id: uuid::Uuid::new_v4(),
        name: "Garden".into(),
        primary: folder.id,
        folders: vec![folder],
        commands: vec![],
        last_transfer: None,
        description: Default::default(),
    }
}

/// Pairs with a user on this side who confirms the code at once.
pub async fn pair(
    conn: &mut Connection,
    shared: &Shared,
    requested: Permissions,
    offered: Permissions,
) -> anyhow::Result<Peer> {
    let started = pair_start(conn, shared, requested, offered).await?;
    pair_finish(conn, shared, started, async { true }, || {}).await
}

/// Waits until `f` holds, for things another task does.
pub async fn until(what: &str, f: impl Fn() -> bool) {
    let end = tokio::time::Instant::now() + Duration::from_secs(10);
    while !f() {
        assert!(
            tokio::time::Instant::now() < end,
            "timed out waiting for {what}"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}
