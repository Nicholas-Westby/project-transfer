use super::*;
use crate::ignore_rules::IgnoreSpec;
use crate::model::Permissions;

fn peer(push: bool, pull: bool) -> Peer {
    Peer {
        id: uuid::Uuid::new_v4(),
        name: "Laptop".into(),
        fingerprint: "ab".into(),
        allows: Permissions {
            may_push_to_me: push,
            may_pull_from_me: pull,
        },
        granted: Permissions::default(),
        last_address: None,
    }
}

fn u() -> uuid::Uuid {
    uuid::Uuid::new_v4()
}

fn project() -> crate::model::Project {
    crate::model::Project {
        id: u(),
        name: "p".into(),
        folders: vec![],
        primary: u(),
        commands: vec![],
        last_transfer: None,
    }
}

fn all_requests() -> Vec<(Request, Need)> {
    use Request as R;
    vec![
        (
            R::Hello {
                id: u(),
                name: "x".into(),
                version: 1,
                port: 0,
            },
            Need::Nothing,
        ),
        (
            R::PairCommit {
                commitment: "00".into(),
                requested: Permissions::default(),
                offered: Permissions::default(),
            },
            Need::Nothing,
        ),
        (R::PairReveal { nonce: "00".into() }, Need::Nothing),
        (R::PairFinal { confirmed: true }, Need::Nothing),
        (R::Status, Need::Paired),
        (R::ProjectInfo { project: u() }, Need::PullOrPush),
        (
            R::Manifest {
                project: u(),
                folder: u(),
                ignore: IgnoreSpec::default(),
                folder_name: "f".into(),
                project_name: "p".into(),
                multi_folder: false,
            },
            Need::PullOrPush,
        ),
        (
            R::ExchangeCommands {
                project: u(),
                commands: vec![],
            },
            Need::PullOrPush,
        ),
        (
            R::Hashes {
                project: u(),
                folder: u(),
                paths: vec![],
            },
            Need::PullOrPush,
        ),
        (
            R::EndPull {
                project: u(),
                files: 0,
            },
            Need::Pull,
        ),
        (
            R::SetMtime {
                rel: "a".into(),
                mtime_ms: 0,
            },
            Need::Push,
        ),
        (
            R::GetFile {
                project: u(),
                folder: u(),
                rel: "a".into(),
            },
            Need::Pull,
        ),
        (
            R::PutFile {
                rel: "a".into(),
                size: 1,
                mtime_ms: 0,
                exec: false,
            },
            Need::Push,
        ),
        (R::MakeDir { rel: "a".into() }, Need::Push),
        (
            R::MakeSymlink {
                rel: "a".into(),
                target: "b".into(),
            },
            Need::Push,
        ),
        (
            R::Remove {
                rel: "a".into(),
                is_dir: false,
            },
            Need::Push,
        ),
        (
            R::BeginPush {
                project: project(),
                folder: u(),
                expected_path: "/x".into(),
            },
            Need::Push,
        ),
        (R::EndPush, Need::Push),
    ]
}

#[test]
fn every_request_has_the_expected_need() {
    for (req, expected) in all_requests() {
        assert_eq!(need(&req), expected, "{req:?}");
    }
}

#[test]
fn unpaired_may_only_hello_and_pair() {
    for (req, n) in all_requests() {
        let r = check(&req, None, "Desk");
        if n == Need::Nothing {
            assert!(r.is_ok(), "{req:?}");
        } else {
            let r = r.unwrap_err();
            assert_eq!(r.theirs, UNPAIRED, "{req:?}");
            assert_eq!(r.ours, "it isn't paired with this computer.", "{req:?}");
        }
    }
}

#[test]
fn permissions_gate_each_need() {
    let cases = [
        (peer(false, false), [true, false, false, false]),
        (peer(true, false), [true, true, false, true]),
        (peer(false, true), [true, true, true, false]),
        (peer(true, true), [true, true, true, true]),
    ];
    for (p, [paired, either, pull, push]) in cases {
        for (req, n) in all_requests() {
            let want = match n {
                Need::Nothing | Need::Paired => paired,
                Need::PullOrPush => either,
                Need::Pull => pull,
                Need::Push => push,
            };
            assert_eq!(
                check(&req, Some(&p), "Desk").is_ok(),
                want,
                "{req:?} {:?}",
                p.allows
            );
        }
    }
}

#[test]
fn refusals_name_the_missing_permission() {
    let get = Request::GetFile {
        project: u(),
        folder: u(),
        rel: "a".into(),
    };
    let r = check(&get, Some(&peer(true, false)), "Desk").unwrap_err();
    assert!(
        r.theirs.contains("pull") && r.ours.contains("pull"),
        "{r:?}"
    );
    let r = check(&Request::EndPush, Some(&peer(false, true)), "Desk").unwrap_err();
    assert!(
        r.theirs.contains("push") && r.ours.contains("push"),
        "{r:?}"
    );
}

/// "This computer" is the asker in the text sent back, and the refusing
/// computer in the line it shows itself.
#[test]
fn each_side_reads_a_refusal_from_where_it_sits() {
    let info = Request::ProjectInfo { project: u() };
    let r = check(&info, Some(&peer(false, false)), "Desk").unwrap_err();
    assert!(
        r.theirs.starts_with("Desk doesn't let this computer"),
        "{}",
        r.theirs
    );
    assert_eq!(
        r.ours,
        "it asked about a project, but you haven't allowed it to push to or pull from this \
         computer. Allow it under Manage paired computers if you meant to."
    );
    for (req, _) in all_requests() {
        for p in [peer(false, false), peer(true, false), peer(false, true)] {
            if let Err(r) = check(&req, Some(&p), "Desk") {
                assert!(!r.ours.contains("Desk"), "{r:?}");
            }
        }
    }
}

#[test]
fn a_caller_knows_in_advance_what_the_gate_lets_through() {
    for push in [false, true] {
        for pull in [false, true] {
            let p = peer(push, pull);
            for (req, _) in all_requests() {
                assert_eq!(
                    may_ask(&req, p.allows),
                    check(&req, Some(&p), "Desk").is_ok(),
                    "{req:?} {:?}",
                    p.allows
                );
            }
        }
    }
}

#[test]
fn every_request_has_its_own_name_for_the_log() {
    let mut seen = std::collections::HashSet::new();
    for (req, _) in all_requests() {
        assert!(format!("{req:?}").starts_with(req.kind()), "{req:?}");
        assert!(seen.insert(req.kind()), "{} named twice", req.kind());
    }
}
