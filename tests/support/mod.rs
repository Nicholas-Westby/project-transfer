//! Instances, folders and small file helpers for the transfer tests.
#![allow(dead_code)]

use project_transfer::identity::Identity;
use project_transfer::model::{
    Command, Direction, Folder, InstanceSettings, Os, Permissions, Project, ProjectId,
};
use project_transfer::net::{Connection, NetEvent, Shared, pair_finish, pair_start, serve};
use project_transfer::store::Store;
use project_transfer::transfer::{self, Preview, Summary, TransferRequest};
use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tokio::sync::{RwLock, mpsc};

pub struct Instance {
    pub shared: Shared,
    pub addr: SocketAddr,
    /// (project, peer, files) from each `NetEvent::Received`.
    pub received: Arc<Mutex<Vec<(ProjectId, uuid::Uuid, u64)>>>,
    dir: tempfile::TempDir,
}

pub fn perms(push: bool, pull: bool) -> Permissions {
    Permissions {
        may_push_to_me: push,
        may_pull_from_me: pull,
    }
}

impl Instance {
    /// Answers every pairing prompt with `allows`.
    pub async fn start(allows: Permissions) -> Instance {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open_at(dir.path().join("home")).unwrap();
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
        let received = Arc::new(Mutex::new(Vec::new()));
        let r = received.clone();
        tokio::spawn(async move {
            while let Some(ev) = rx.recv().await {
                match ev {
                    NetEvent::PairPrompt { reply, .. } => {
                        let _ = reply.send(Some(allows));
                    }
                    NetEvent::Received {
                        project,
                        peer,
                        files,
                    } => r.lock().unwrap().push((project, peer, files)),
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
            received,
            dir,
        }
    }

    pub fn root(&self) -> &Path {
        self.dir.path()
    }

    /// The Projects folder, where folders arriving here land by default.
    pub fn dev(&self) -> PathBuf {
        self.dir.path().join("Dev")
    }

    /// A source folder outside the Projects folder.
    pub fn project_dir(&self, name: &str) -> PathBuf {
        let p = self.dir.path().join("src").join(name);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    pub async fn id(&self) -> uuid::Uuid {
        self.shared.settings.read().await.id
    }

    pub async fn add_project(&self, name: &str, folders: &[(&str, &Path)]) -> Project {
        let folders: Vec<Folder> = folders
            .iter()
            .map(|(n, p)| Folder {
                id: uuid::Uuid::new_v4(),
                name: n.to_string(),
                local_path: Some(p.to_path_buf()),
            })
            .collect();
        let p = Project {
            id: uuid::Uuid::new_v4(),
            name: name.into(),
            primary: folders[0].id,
            folders,
            commands: vec![],
            last_transfer: None,
            description: Default::default(),
        };
        self.put_project(p.clone()).await;
        p
    }

    /// Adds or replaces by id, in memory and on disk.
    pub async fn put_project(&self, p: Project) {
        let mut all = self.shared.projects.write().await;
        all.retain(|x| x.id != p.id);
        all.push(p);
        self.shared.store.save_projects(&all).unwrap();
    }

    pub async fn project(&self, id: ProjectId) -> Option<Project> {
        self.shared
            .projects
            .read()
            .await
            .iter()
            .find(|p| p.id == id)
            .cloned()
    }

    pub async fn open(&self, other: &Instance) -> Connection {
        let peer = self
            .shared
            .peers
            .read()
            .await
            .iter()
            .find(|p| p.last_address == Some(other.addr))
            .cloned()
            .expect("paired");
        Connection::open_peer(other.addr, &self.shared, &peer)
            .await
            .unwrap()
    }

    pub async fn request(
        &self,
        other: &Instance,
        project: ProjectId,
        d: Direction,
    ) -> TransferRequest {
        TransferRequest {
            peer: other.id().await,
            project,
            direction: d,
            send_everything: false,
        }
    }
}

/// A pairs with B; B allows A `allows`, A allows B everything.
pub async fn pair_with(allows: Permissions) -> (Instance, Instance) {
    let a = Instance::start(perms(true, true)).await;
    let b = Instance::start(allows).await;
    let mut conn = Connection::open(b.addr, &a.shared).await.unwrap();
    let started = pair_start(&mut conn, &a.shared, perms(true, true), perms(true, true))
        .await
        .unwrap();
    // A's user confirms the code at once.
    pair_finish(&mut conn, &a.shared, started, async { true }, || {})
        .await
        .unwrap();
    (a, b)
}

pub async fn pair_full() -> (Instance, Instance) {
    pair_with(perms(true, true)).await
}

async fn run(
    a: &Instance,
    b: &Instance,
    project: ProjectId,
    direction: Direction,
    send_everything: bool,
) -> (Preview, Summary) {
    let mut conn = a.open(b).await;
    let mut req = a.request(b, project, direction).await;
    req.send_everything = send_everything;
    let preview = transfer::prepare(&mut conn, &a.shared, req).await.unwrap();
    let (tx, _rx) = mpsc::unbounded_channel();
    let summary = transfer::execute(
        &mut conn,
        &a.shared,
        preview.clone(),
        tx,
        Default::default(),
    )
    .await
    .unwrap();
    (preview, summary)
}

pub async fn push(
    a: &Instance,
    b: &Instance,
    project: ProjectId,
    everything: bool,
) -> (Preview, Summary) {
    run(a, b, project, Direction::Push, everything).await
}

pub async fn pull(a: &Instance, b: &Instance, project: ProjectId) -> (Preview, Summary) {
    run(a, b, project, Direction::Pull, false).await
}

pub fn write(root: &Path, rel: &str, body: &str) {
    let p = root.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, body).unwrap();
}

pub fn read(root: &Path, rel: &str) -> String {
    std::fs::read_to_string(root.join(rel)).unwrap()
}

pub fn mtime(p: &Path) -> i64 {
    let t = filetime::FileTime::from_last_modification_time(&std::fs::metadata(p).unwrap());
    t.unix_seconds() * 1000 + (t.nanoseconds() / 1_000_000) as i64
}

pub fn set_mtime(p: &Path, ms: i64) {
    let t = filetime::FileTime::from_unix_time(ms / 1000, ((ms % 1000) * 1_000_000) as u32);
    filetime::set_file_mtime(p, t).unwrap();
}

/// Every entry under `root` with its content ("<dir>" for folders) and mtime.
pub fn tree(root: &Path) -> BTreeMap<String, (String, i64)> {
    fn walk(root: &Path, dir: &Path, out: &mut BTreeMap<String, (String, i64)>) {
        for e in std::fs::read_dir(dir).unwrap() {
            let p = e.unwrap().path();
            let rel = p
                .strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            if p.is_dir() {
                out.insert(rel, ("<dir>".into(), 0));
                walk(root, &p, out);
            } else {
                out.insert(rel, (std::fs::read_to_string(&p).unwrap(), mtime(&p)));
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(root, root, &mut out);
    out
}

pub fn temps(root: &Path) -> Vec<String> {
    tree(root)
        .into_keys()
        .filter(|k| k.ends_with(".pt-tmp"))
        .collect()
}

pub async fn wait_for(mut cond: impl FnMut() -> bool) {
    for _ in 0..200 {
        if cond() {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
    panic!("condition not met within 5 seconds");
}

pub fn command(label: &str, at: i64, hash: Option<&str>) -> Command {
    Command {
        id: uuid::Uuid::new_v4(),
        label: label.into(),
        line: format!("run {label}"),
        created_on: Os::current(),
        updated_at_ms: at,
        deleted: false,
        last_run_hash: hash.map(str::to_string),
    }
}
