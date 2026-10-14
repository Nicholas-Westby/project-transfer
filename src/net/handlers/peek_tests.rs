use crate::model::{Folder, FolderId, Peer, Permissions, Project, ProjectId};
use crate::net::server::{PairSlot, session};
use crate::net::test_support::{addr, shared};
use crate::protocol::{MAX_PART, PROTOCOL_VERSION, Request, Response, read_msg, write_msg};
use tokio::io::{AsyncReadExt, DuplexStream};

/// A session with a computer called Desk, which keeps the folder `app` of
/// a project in a temp folder and lets the caller, Laptop, push and maybe
/// pull. The temp folder holds `notes.txt`.
struct Desk {
    dir: tempfile::TempDir,
    to: DuplexStream,
    project: ProjectId,
    folder: FolderId,
}

async fn desk(may_pull: bool) -> Desk {
    let dir = tempfile::tempdir().unwrap();
    let (s, _events) = shared(dir.path());
    let root = dir.path().join("app");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("notes.txt"), "line one\nline two\n").unwrap();
    let (project, folder) = (uuid::Uuid::from_u128(10), uuid::Uuid::from_u128(20));
    s.projects.write().await.push(Project {
        id: project,
        name: "garden-planner".into(),
        folders: vec![Folder {
            id: folder,
            name: "app".into(),
            local_path: Some(root),
        }],
        primary: folder,
        commands: vec![],
        last_transfer: None,
        description: Default::default(),
    });
    let laptop = uuid::Uuid::new_v4();
    s.peers.write().await.push(Peer {
        id: laptop,
        name: "Laptop".into(),
        fingerprint: "ff".into(),
        allows: Permissions {
            may_push_to_me: true,
            may_pull_from_me: may_pull,
        },
        granted: Permissions::default(),
        last_address: None,
        via: None,
    });
    let (mut to, mut from) = tokio::io::duplex(4 * 1024 * 1024);
    tokio::spawn(async move {
        let caller = addr("127.0.0.1:1");
        session(&s, &PairSlot::new(), &mut from, "ff".into(), caller).await
    });
    let hello = Request::Hello {
        id: laptop,
        name: "Laptop".into(),
        version: PROTOCOL_VERSION,
        port: 0,
        addrs: vec![],
        via: None,
    };
    write_msg(&mut to, &hello).await.unwrap();
    let _: Response = read_msg(&mut to).await.unwrap();
    Desk {
        dir,
        to,
        project,
        folder,
    }
}

impl Desk {
    async fn ask(&mut self, rel: &str, max: u64) -> Response {
        let ask = Request::ReadFile {
            project: self.project,
            folder: self.folder,
            rel: rel.into(),
            max,
        };
        write_msg(&mut self.to, &ask).await.unwrap();
        read_msg(&mut self.to).await.unwrap()
    }

    async fn bytes(&mut self, len: u64) -> Vec<u8> {
        let mut buf = vec![0; len as usize];
        self.to.read_exact(&mut buf).await.unwrap();
        buf
    }
}

#[tokio::test]
async fn the_start_of_a_file_comes_with_its_whole_size() {
    let mut d = desk(true).await;
    let Response::FilePart { size, len, .. } = d.ask("notes.txt", 4).await else {
        panic!("expected a part of the file");
    };
    assert_eq!((size, len), (18, 4));
    assert_eq!(d.bytes(len).await, b"line");
}

#[tokio::test]
async fn no_answer_holds_more_than_the_limit() {
    let mut d = desk(true).await;
    let big = vec![b'x'; MAX_PART as usize + 10];
    std::fs::write(d.dir.path().join("app/big.txt"), big).unwrap();
    let Response::FilePart { size, len, .. } = d.ask("big.txt", u64::MAX).await else {
        panic!("expected a part of the file");
    };
    assert_eq!((size, len), (MAX_PART + 10, MAX_PART));
    assert_eq!(d.bytes(len).await.len() as u64, MAX_PART);
}

#[tokio::test]
async fn a_computer_that_may_not_pull_hears_why() {
    let mut d = desk(false).await;
    let Response::Refused { reason } = d.ask("notes.txt", 4).await else {
        panic!("expected a refusal");
    };
    assert!(reason.contains("pull"), "{reason}");
}

#[tokio::test]
async fn a_path_outside_the_folder_is_refused_and_the_session_goes_on() {
    let mut d = desk(true).await;
    let Response::Refused { reason } = d.ask("../secret.txt", 4).await else {
        panic!("expected a refusal");
    };
    assert!(reason.contains("not allowed"), "{reason}");
    let Response::Refused { reason } = d.ask("missing.txt", 4).await else {
        panic!("expected a refusal");
    };
    assert_eq!(reason, "\"missing.txt\" is not a file on Desk.");
    assert!(matches!(
        d.ask("notes.txt", 2).await,
        Response::FilePart { len: 2, .. }
    ));
}

#[cfg(unix)]
#[tokio::test]
async fn a_link_is_never_followed() {
    let mut d = desk(true).await;
    let app = d.dir.path().join("app");
    std::fs::write(d.dir.path().join("secret.txt"), "keep out").unwrap();
    let secret = d.dir.path().join("secret.txt");
    std::os::unix::fs::symlink(secret, app.join("notes-link.txt")).unwrap();
    std::os::unix::fs::symlink(d.dir.path(), app.join("up")).unwrap();
    let Response::Refused { reason } = d.ask("notes-link.txt", 4).await else {
        panic!("expected a refusal");
    };
    assert!(reason.contains("is a link on Desk"), "{reason}");
    let Response::Refused { reason } = d.ask("up/secret.txt", 4).await else {
        panic!("expected a refusal");
    };
    assert!(
        reason.contains("passes through a symbolic link"),
        "{reason}"
    );
    assert!(matches!(
        d.ask("notes.txt", 2).await,
        Response::FilePart { len: 2, .. }
    ));
}

#[tokio::test]
async fn a_folder_not_set_up_here_or_an_absolute_path_is_refused() {
    let mut d = desk(true).await;
    let set_up = d.folder;
    d.folder = uuid::Uuid::from_u128(99);
    let Response::Refused { reason } = d.ask("notes.txt", 4).await else {
        panic!("expected a refusal");
    };
    assert_eq!(reason, "Desk has no folder set up for this project.");
    d.folder = set_up;
    let Response::Refused { reason } = d.ask("/etc/hosts", 4).await else {
        panic!("expected a refusal");
    };
    assert!(reason.contains("not allowed"), "{reason}");
}

/// What a version without `ReadFile` does with it: the request does not
/// parse, so the session ends and the caller sees the connection close.
#[tokio::test]
async fn a_request_this_version_does_not_know_ends_the_session() {
    let mut d = desk(true).await;
    let unknown = serde_json::json!({"ReadSomethingNew": {"rel": "notes.txt"}});
    write_msg(&mut d.to, &unknown).await.unwrap();
    let err = read_msg::<_, Response>(&mut d.to).await.unwrap_err();
    let io = err.downcast_ref::<std::io::Error>().expect("an I/O error");
    assert_eq!(io.kind(), std::io::ErrorKind::UnexpectedEof);
}
