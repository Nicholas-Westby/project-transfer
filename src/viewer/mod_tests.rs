use super::*;

fn file(rel: &str) -> Entry {
    Entry {
        rel: rel.into(),
        kind: Kind::File {
            size: 1,
            mtime_ms: 0,
            exec: false,
        },
    }
}

#[test]
fn each_kind_of_change_reads_the_copies_it_is_about() {
    let push = Direction::Push;
    assert_eq!(
        Copies::of(&Change::Add(file("a")), push),
        Some(Copies::Here)
    );
    assert_eq!(
        Copies::of(&Change::Add(file("a")), Direction::Pull),
        Some(Copies::There)
    );
    let update = Change::Update {
        entry: file("a"),
        dest_newer: false,
    };
    assert_eq!(Copies::of(&update, push), Some(Copies::Both));
    assert_eq!(
        Copies::of(&Change::TimestampOnly(file("a")), push),
        Some(Copies::SameContent)
    );
    assert_eq!(
        Copies::of(&Change::RemoveFile("a".into()), push),
        Some(Copies::There)
    );
    assert_eq!(
        Copies::of(&Change::RemoveFile("a".into()), Direction::Pull),
        Some(Copies::Here)
    );
    let gone = Change::RemoveDir {
        rel: "d".into(),
        files: 1,
        ignored: 0,
    };
    assert_eq!(Copies::of(&gone, push), None);
    let dir = Entry {
        rel: "src".into(),
        kind: Kind::Dir,
    };
    assert_eq!(Copies::of(&Change::Add(dir), push), None);
    assert_eq!(
        Copies::of(&Change::Replace { entry: file("a") }, push),
        None
    );
    assert!(Copies::SameContent.here() && !Copies::SameContent.there());
    assert!(Copies::Both.here() && Copies::Both.there());
}

#[test]
fn a_pull_reads_this_computers_copy_in_the_destination() {
    let id = uuid::Uuid::from_u128;
    let p = Preview {
        request: crate::transfer::TransferRequest {
            peer: id(2),
            project: id(10),
            direction: Direction::Pull,
            send_everything: false,
        },
        folders: vec![crate::transfer::FolderPreview {
            folder: id(20),
            name: "app".into(),
            source_path: "/remote/app".into(),
            dest_path: "/work/app".into(),
            dest_will_be_created: false,
            plan: Default::default(),
            skipped: vec![],
            replaced: vec![],
        }],
        left_out: vec![],
        warnings: vec![],
        link: None,
        description: false,
        sync: None,
    };
    let ask = FileAsk::new(&p, 0, "a.txt", Copies::Here);
    assert_eq!(ask.local_root, PathBuf::from("/work/app"));
    assert_eq!((ask.peer, ask.project, ask.folder), (id(2), id(10), id(20)));
}

#[test]
fn both_texts_are_lined_up_and_a_binary_copy_is_shown_alone() {
    let fetched = |bytes: &[u8]| {
        Some(Ok(FilePart {
            size: bytes.len() as u64,
            mtime_ms: 5,
            bytes: bytes.to_vec(),
        }))
    };
    let s = Shown::build(fetched(b"a\nb\n"), fetched(b"a\r\nc\r\n"));
    assert_eq!(s.rows.len(), 2);
    assert!(!s.rows[0].differs && s.rows[1].differs);
    let s = Shown::build(fetched(b"a\n"), fetched(b"\x00\x01"));
    assert!(s.rows.is_empty());
    assert_eq!(s.there.unwrap().unwrap().body, Body::Binary);
    let s = Shown::build(fetched(b"a\n"), Some(Err("Desk is away.".into())));
    assert!(s.rows.is_empty());
    assert_eq!(s.there, Some(Err("Desk is away.".into())));
}

#[test]
fn matching_lines_are_marked_alike() {
    let part = |bytes: &[u8]| {
        Some(Ok(FilePart {
            size: bytes.len() as u64,
            mtime_ms: 5,
            bytes: bytes.to_vec(),
        }))
    };
    let alike = |a: &[u8], b: &[u8]| Shown::build(part(a), part(b)).alike;
    assert_eq!(alike(b"a\nb\n", b"a\nb"), Some(Alike::Lines));
    assert_eq!(alike(b"a\n", b"a\n"), Some(Alike::Bytes));
    assert_eq!(alike(b"a\n", b"b\n"), None);
    assert_eq!(alike(b"a\n", b"\x00\x01"), None);
}

#[test]
fn lines_past_where_a_cut_copy_stops_are_not_marked() {
    let lf: String = (0..80_000).map(|i| format!("line number {i}\n")).collect();
    let crlf = lf.replace('\n', "\r\n");
    let cut = |s: &str| {
        Some(Ok(FilePart {
            size: s.len() as u64,
            mtime_ms: 5,
            bytes: s.as_bytes()[..MAX_PART as usize].to_vec(),
        }))
    };
    let s = Shown::build(cut(&lf), cut(&crlf));
    assert!(s.rows.iter().all(|r| !r.differs));
    assert_eq!(s.alike, Some(Alike::Lines));
    // A copy read whole still shows the line only it has.
    let whole = b"line number 0\nline number 1\nextra\n";
    let read = Some(Ok(FilePart {
        size: whole.len() as u64,
        mtime_ms: 5,
        bytes: whole.to_vec(),
    }));
    let s = Shown::build(cut(&lf), read);
    assert!(s.rows.iter().any(|r| r.right == Some(2) && r.differs));
}

#[test]
fn a_cut_line_that_already_differs_stays_marked() {
    // One line longer than the part read, as minified code or a source map
    // often is.
    let long = "a".repeat(MAX_PART as usize + 10);
    let mut early = long.clone();
    early.replace_range(10..11, "b");
    let more = format!("abc\n{long}");
    let part = |s: &str| {
        let read = s.len().min(MAX_PART as usize);
        Some(Ok(FilePart {
            size: s.len() as u64,
            mtime_ms: 5,
            bytes: s.as_bytes()[..read].to_vec(),
        }))
    };
    // Against a cut copy, against a copy read whole that is the line's
    // start, and past the end of a copy read whole.
    let pairs = [
        ("cut", long.as_str(), early.as_str()),
        ("its start", long.as_str(), "aaa\n"),
        ("past the end", more.as_str(), "abc\n"),
    ];
    for (what, here, there) in pairs {
        let s = Shown::build(part(here), part(there));
        assert!(s.rows.last().is_some_and(|r| r.differs), "{what}");
        assert_eq!(s.alike, None, "{what}");
    }
}

#[test]
fn reading_here_stops_at_the_limit_and_never_leaves_the_folder() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("notes.txt"), "line one\nline two\n").unwrap();
    let f = read_local(dir.path(), "notes.txt").unwrap();
    assert_eq!((f.size, f.bytes.len()), (18, 18));
    let big = vec![b'x'; MAX_PART as usize + 10];
    std::fs::write(dir.path().join("big.txt"), &big).unwrap();
    let f = read_local(dir.path(), "big.txt").unwrap();
    assert_eq!(f.size, MAX_PART + 10);
    assert_eq!(f.bytes.len() as u64, MAX_PART);
    let why = read_local(dir.path(), "../outside.txt").unwrap_err();
    assert!(why.contains("not allowed"), "{why}");
    let why = read_local(dir.path(), "gone.txt").unwrap_err();
    assert!(why.contains("no longer a file here"), "{why}");
}

#[cfg(unix)]
#[test]
fn a_link_here_is_named_as_a_link_and_never_followed() {
    let dir = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("secret.txt"), "keep out").unwrap();
    let link = |to: &std::path::Path, name: &str| {
        std::os::unix::fs::symlink(to, dir.path().join(name)).unwrap();
    };
    link(&outside.path().join("secret.txt"), "notes.txt");
    link(outside.path(), "up");
    let why = read_local(dir.path(), "notes.txt").unwrap_err();
    assert_eq!(
        why,
        "\"notes.txt\" is a link here, not a file, so there is nothing to show."
    );
    let why = read_local(dir.path(), "up/secret.txt").unwrap_err();
    assert!(why.contains("passes through a symbolic link"), "{why}");
}
