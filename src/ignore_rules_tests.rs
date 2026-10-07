use super::*;

fn defaults() -> IgnoreSpec {
    IgnoreSpec {
        patterns: DEFAULT_IGNORES.iter().map(|s| s.to_string()).collect(),
        ..Default::default()
    }
}

#[test]
fn nested_default_is_ignored() {
    let m = Matcher::new(&defaults()).unwrap();
    assert!(m.is_ignored("a/node_modules/x", false));
    assert!(m.is_ignored("a/node_modules", true));
    assert!(m.is_ignored("sub/.DS_Store", false));
    assert!(!m.is_ignored("src/main.rs", false));
}

#[test]
fn always_include_beats_default() {
    let mut spec = defaults();
    spec.always_include = vec!["dist/".into()];
    let m = Matcher::new(&spec).unwrap();
    assert!(!m.is_ignored("dist/app.js", false));
    assert!(m.is_ignored("node_modules/x", false));
}

#[test]
fn send_everything_only_ignores_temp_files() {
    let mut spec = defaults();
    spec.send_everything = true;
    let m = Matcher::new(&spec).unwrap();
    assert!(!m.is_ignored("node_modules/x", false));
    assert!(m.is_ignored("a/file.bin.pt-tmp", false));
}

#[test]
fn invalid_pattern_names_itself() {
    let spec = IgnoreSpec {
        patterns: vec!["{a,b".into()],
        ..Default::default()
    };
    let err = Matcher::new(&spec).err().unwrap();
    assert!(format!("{err:#}").contains("{a,b"));
}

fn with(patterns: &[&str], always: &[&str]) -> Matcher {
    Matcher::new(&IgnoreSpec {
        patterns: patterns.iter().map(|s| s.to_string()).collect(),
        always_include: always.iter().map(|s| s.to_string()).collect(),
        send_everything: false,
    })
    .unwrap()
}

#[test]
fn a_folder_left_out_takes_everything_inside_with_it() {
    let m = with(&["/exports/"], &[]);
    assert!(m.leaves_out("exports/2026/plot.svg", false));
    assert!(m.leaves_out("exports", true));
    assert!(!m.leaves_out("src/exports/plot.svg", false));
    assert!(!m.leaves_out("exports.txt", false));
}

#[test]
fn always_include_cannot_reach_inside_a_folder_left_out() {
    let m = with(&["tmp/"], &["tmp/keep.txt"]);
    assert!(!m.is_ignored("tmp/keep.txt", false));
    assert!(m.leaves_out("tmp/keep.txt", false));
    let m = with(&["tmp/"], &["tmp/"]);
    assert!(!m.leaves_out("tmp/keep.txt", false));
}

#[test]
fn leaves_out_agrees_with_what_a_scan_lists() {
    let t = tempfile::tempdir().unwrap();
    let files = [
        "keep.txt",
        "notes.log",
        "logs/keep.log",
        "logs/old.log",
        "tmp/keep.txt",
        "tmp/x.bin",
        "src/thumbs/a.png",
        "src/main.rs",
        "deep/a/b/thumbs/c.png",
    ];
    for f in files {
        let p = t.path().join(f);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, "x").unwrap();
    }
    let m = with(
        &["*.log", "!logs/keep.log", "tmp/", "thumbs/"],
        &["tmp/keep.txt"],
    );
    let listed = crate::manifest::scan(t.path(), &m, false).unwrap();
    for f in files {
        let is_listed = listed.entries.iter().any(|e| e.rel == f);
        assert_eq!(is_listed, !m.leaves_out(f, false), "{f}");
    }
}

#[test]
fn many_paths_share_what_is_known_about_their_folders() {
    let m = with(&["thumbs/"], &[]);
    let mut seen = HashMap::new();
    assert!(m.leaves_out_with("a/thumbs/1.png", false, &mut seen));
    assert!(m.leaves_out_with("a/thumbs/2.png", false, &mut seen));
    assert_eq!(seen.get("a/thumbs"), Some(&true));
    assert_eq!(seen.get("a"), Some(&false));
}

#[test]
fn an_escaped_name_matches_only_itself() {
    assert_eq!(escape("a*b?[c]{d}"), r"a\*b\?\[c\]\{d\}");
    // Each neighbour matches the name used as a pattern, not once escaped.
    for (name, neighbour) in [
        ("[old] plan.md", "o plan.md"),
        ("a{b,c}", "ab"),
        ("50%*", "50%x"),
        ("draft?", "drafts"),
    ] {
        assert!(with(&[name], &[]).leaves_out(neighbour, false), "{name}");
        let m = with(&[&escape(name)], &[]);
        assert!(m.leaves_out(name, false), "{name}");
        assert!(!m.leaves_out(neighbour, false), "{name}");
    }
    // Unescaped, these would be an exception and a comment.
    for name in ["!draft", "#1"] {
        assert!(!with(&[name], &[]).leaves_out(name, false), "{name}");
        assert!(
            with(&[&escape(name)], &[]).leaves_out(name, false),
            "{name}"
        );
    }
    // Spaces around a name are part of it.
    for name in [" lead", "trail ", " "] {
        let m = with(&[trim(&escape(name))], &[]);
        assert!(m.leaves_out(name, false), "{name:?}");
    }
}

#[test]
fn trim_keeps_a_trailing_space_that_belongs_to_a_name() {
    assert_eq!(trim("  *.log \t"), "*.log");
    assert_eq!(trim(r" /notes/draft\  "), r"/notes/draft\ ");
    // An escaped backslash escapes nothing after it.
    assert_eq!(trim(r"a\\ "), r"a\\");
    assert_eq!(trim(r"\ lead"), r"\ lead");
}

#[test]
fn check_gives_the_reason_a_pattern_cannot_be_used() {
    assert_eq!(check("/exports/"), Ok(()));
    let why = check("{a,b").unwrap_err();
    assert!(why.contains("unclosed"), "{why}");
    assert!(!why.contains("{a,b"), "{why}");
}

#[test]
fn spec_from_settings_applies_removals_and_extras() {
    let s = InstanceSettings {
        id: uuid::Uuid::new_v4(),
        name: "x".into(),
        projects_folder: "/p".into(),
        extra_ignores: vec!["*.log".into()],
        always_include: vec!["keep/".into()],
        removed_default_ignores: vec!["bin/".into()],
        last_peer: None,
        theme: Default::default(),
        port: 0,
    };
    let spec = IgnoreSpec::from_settings(&s);
    assert!(!spec.patterns.contains(&"bin/".to_string()));
    assert!(spec.patterns.contains(&"obj/".to_string()));
    assert_eq!(spec.patterns.last().unwrap(), "*.log");
    assert_eq!(spec.always_include, vec!["keep/".to_string()]);
    assert!(!spec.send_everything);
}
