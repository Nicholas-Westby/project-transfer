//! Realistic states for the tests and screenshots: two peers, two
//! projects, a preview with every kind of change, a running command.

use project_transfer::core::{
    ActivityKind, ActivityLine, CommandRun, OutputLine, PairPromptView, PeerView, PromptState,
    RunStatus, UiState,
};
use project_transfer::manifest::{Change, Entry, Kind, Plan};
use project_transfer::model::{
    Command, Direction, Folder, InstanceSettings, Os, Peer, Permissions, Project, ThemeChoice,
    TransferRecord,
};
use project_transfer::protocol::{RemoteFolder, RemoteProject};
use project_transfer::transfer::{FolderPreview, Preview, TransferRequest};
use std::path::PathBuf;
use uuid::Uuid;

const ALL: Permissions = Permissions {
    may_push_to_me: true,
    may_pull_from_me: true,
};

fn id(n: u128) -> Uuid {
    Uuid::from_u128(n)
}

pub fn now_ms() -> i64 {
    project_transfer::transfer::projects::now_ms()
}

pub fn seeded_state() -> UiState {
    let me = InstanceSettings {
        id: id(1),
        name: "Studio Amber Otter".into(),
        projects_folder: PathBuf::from("/Users/jdoe/Dev"),
        extra_ignores: vec!["*.log".into()],
        always_include: vec![".env.example".into()],
        removed_default_ignores: vec!["packages/".into()],
        last_peer: Some(id(2)),
        theme: ThemeChoice::Dark,
        port: 47820,
    };
    let mut s = UiState::new(me, 47820);
    let peer = |n, name: &str, online, seen| PeerView {
        peer: Peer {
            id: id(n),
            name: name.into(),
            fingerprint: "ab".repeat(32),
            allows: ALL,
            granted: ALL,
            last_address: None,
            via: None,
        },
        online,
        address: None,
        last_seen_ms: Some(now_ms() - seen),
    };
    s.peers = vec![
        peer(2, "Desktop Swift Heron", true, 2_000),
        peer(3, "Laptop Quiet Finch", false, 3 * 60_000),
    ];
    s.peers[1].peer.granted.may_push_to_me = false;
    s.selected_peer = Some(id(2));
    s.discovered = vec![project_transfer::discovery::Discovered {
        id: id(4),
        name: "Mini Brisk Lynx".into(),
        addrs: vec!["192.168.1.40:47820".parse().unwrap()],
        version: 1,
        via: None,
    }];

    let hooks = Command {
        id: id(30),
        label: "Install git hooks".into(),
        line: "git config core.hooksPath .githooks".into(),
        created_on: Os::MacOs,
        updated_at_ms: 0,
        deleted: false,
        last_run_hash: None,
    };
    let hooks = Command {
        last_run_hash: Some(project_transfer::commands::command_hash(&hooks)),
        ..hooks
    };
    let restore = Command {
        id: id(31),
        label: "Fetch dependencies".into(),
        line: "make deps".into(),
        created_on: Os::Windows,
        updated_at_ms: 0,
        deleted: false,
        last_run_hash: None,
    };
    let folder = |n, name: &str, path: &str| Folder {
        id: id(n),
        name: name.into(),
        local_path: Some(PathBuf::from(path)),
    };
    s.projects = vec![
        Project {
            id: id(10),
            name: "garden-planner".into(),
            folders: vec![
                folder(20, "app", "/Users/jdoe/Dev/garden-planner/app"),
                folder(
                    21,
                    "plant-data",
                    "/Users/jdoe/Dev/garden-planner/plant-data",
                ),
                Folder {
                    id: id(22),
                    name: "docs".into(),
                    local_path: None,
                },
            ],
            primary: id(20),
            commands: vec![hooks, restore],
            last_transfer: Some(TransferRecord {
                at_ms: now_ms() - 12 * 60_000,
                peer: id(2),
                direction: Direction::Push,
                files: 42,
                by_peer: false,
            }),
        },
        Project {
            id: id(11),
            name: "tide-tables".into(),
            folders: vec![folder(23, "tide-tables", "/Users/jdoe/Dev/tide-tables")],
            primary: id(23),
            commands: vec![],
            last_transfer: None,
        },
    ];
    s.remote_projects.insert(
        id(10),
        RemoteProject {
            name: "garden-planner".into(),
            folders: vec![
                RemoteFolder {
                    id: id(20),
                    name: "app".into(),
                    path: Some("D:\\dev\\garden-planner\\app".into()),
                },
                RemoteFolder {
                    id: id(22),
                    name: "docs".into(),
                    path: Some("D:\\dev\\garden-planner\\docs".into()),
                },
            ],
            primary: id(20),
        },
    );
    // Only on the peer, so the project list offers to pull it.
    s.remote_projects.insert(
        id(12),
        RemoteProject {
            name: "seed-catalog".into(),
            folders: vec![RemoteFolder {
                id: id(24),
                name: "seed-catalog".into(),
                path: Some("D:\\dev\\seed-catalog".into()),
            }],
            primary: id(24),
        },
    );
    let out = |t: &str, stderr| OutputLine {
        stderr,
        text: t.into(),
    };
    s.command_runs.insert(
        id(30),
        CommandRun {
            lines: vec![
                out("Setting hooks path to .githooks", false),
                out("pre-commit: installed", false),
                out("warning: commit-msg hook is not executable", true),
            ],
            status: RunStatus::Running,
        },
    );
    let line = |ago: i64, text: &str, kind| ActivityLine {
        at_ms: now_ms() - ago,
        text: text.into(),
        kind,
    };
    s.activity = vec![
        line(
            3_600_000,
            "Studio Amber Otter is listening on port 47820.",
            ActivityKind::Info,
        ),
        line(
            720_000,
            "Pushed 42 files to Desktop Swift Heron in 3.1 s.",
            ActivityKind::Info,
        ),
        line(
            180_000,
            "Laptop Quiet Finch went offline.",
            ActivityKind::Warn,
        ),
    ];
    s
}

fn file(rel: &str) -> Entry {
    Entry {
        rel: rel.into(),
        kind: Kind::File {
            size: 120,
            mtime_ms: 0,
            exec: false,
        },
    }
}

pub fn sample_preview(s: &UiState) -> Preview {
    let app = FolderPreview {
        folder: id(20),
        name: "app".into(),
        source_path: "/Users/jdoe/Dev/garden-planner/app".into(),
        dest_path: "D:\\dev\\garden-planner\\app".into(),
        dest_will_be_created: false,
        plan: Plan {
            changes: vec![
                Change::Add(file("src/new.rs")),
                Change::Add(file(".git/HEAD")),
                Change::Add(file(".git/objects/ab/cdef")),
                Change::Update {
                    entry: file("README.md"),
                    dest_newer: true,
                },
                Change::Update {
                    entry: file("plants.json"),
                    dest_newer: false,
                },
                Change::TimestampOnly(file("beds.toml")),
                Change::RemoveFile("todo.txt".into()),
                Change::RemoveDir {
                    rel: "old-sketches".into(),
                    files: 32,
                    ignored: 280,
                },
            ],
            needs_hash: vec![],
        },
        skipped: vec![(
            "aux.txt".into(),
            "`aux.txt` is a reserved name on Windows.".into(),
        )],
        replaced: vec![],
    };
    let data = FolderPreview {
        folder: id(21),
        name: "plant-data".into(),
        source_path: "/Users/jdoe/Dev/garden-planner/plant-data".into(),
        dest_path: "D:\\dev\\garden-planner\\plant-data".into(),
        dest_will_be_created: true,
        plan: Plan {
            changes: vec![Change::Add(file("tomato.csv"))],
            needs_hash: vec![],
        },
        skipped: vec![],
        replaced: vec![],
    };
    Preview {
        request: TransferRequest {
            peer: s.selected_peer.unwrap(),
            project: id(10),
            direction: Direction::Push,
            send_everything: false,
        },
        folders: vec![app, data],
        warnings: vec![
            "Folder `docs` was left out: it is not on this computer, and pushing it would empty \
             the copy on Desktop Swift Heron."
                .into(),
        ],
    }
}

pub fn with_pair_prompt(s: &mut UiState) {
    s.pair_prompt = Some(PairPromptView {
        from_id: id(4),
        from_name: "Mini Brisk Lynx".into(),
        code: "481 205".into(),
        requested: ALL,
        offered: Permissions {
            may_push_to_me: false,
            may_pull_from_me: true,
        },
        state: PromptState::Asking,
    });
}
