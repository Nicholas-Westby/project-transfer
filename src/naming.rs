//! Default instance names and checks for names that break on Windows.

use rand::prelude::IndexedRandom;

const ADJECTIVES: [&str; 40] = [
    "Amber", "Brave", "Calm", "Clever", "Crimson", "Daring", "Eager", "Fuzzy", "Gentle", "Golden",
    "Happy", "Humble", "Jolly", "Keen", "Lively", "Lucky", "Mellow", "Mighty", "Nimble", "Noble",
    "Plucky", "Proud", "Quick", "Quiet", "Rapid", "Rustic", "Silent", "Silver", "Sleepy", "Snowy",
    "Solar", "Spry", "Steady", "Sunny", "Swift", "Tidy", "Velvet", "Vivid", "Witty", "Zesty",
];

const ANIMALS: [&str; 40] = [
    "Badger", "Beaver", "Bison", "Cheetah", "Condor", "Coyote", "Dolphin", "Falcon", "Ferret",
    "Fox", "Gecko", "Heron", "Ibis", "Jaguar", "Koala", "Lemur", "Lynx", "Marmot", "Narwhal",
    "Newt", "Otter", "Owl", "Panda", "Parrot", "Pelican", "Puffin", "Quokka", "Raven", "Salmon",
    "Seal", "Sparrow", "Tiger", "Toucan", "Turtle", "Walrus", "Weasel", "Wombat", "Yak", "Zebra",
    "Stoat",
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NameProblem {
    ReservedOnWindows,
    BadCharOnWindows(char),
    TrailingDotOrSpace,
    CaseCollision(String),
}

/// "<Host> <Adjective> <Animal>", so two machines are easy to tell apart.
pub fn default_name(rng: &mut impl rand::Rng) -> String {
    let adj = ADJECTIVES.choose(rng).copied().unwrap_or("Quiet");
    let animal = ANIMALS.choose(rng).copied().unwrap_or("Otter");
    format!("{} {adj} {animal}", host_label())
}

/// The name people know the computer by: on macOS the Computer Name from
/// Sharing settings ("Jane's MacBook Pro"), elsewhere the host name.
pub fn host_label() -> String {
    #[cfg(target_os = "macos")]
    if let Some(name) = computer_name() {
        return name;
    }
    let raw = gethostname::gethostname().to_string_lossy().into_owned();
    let trimmed = raw.strip_suffix(".local").unwrap_or(&raw);
    let label = trimmed.replace(['-', '_'], " ");
    let label = label.trim();
    if label.is_empty() {
        "Computer".to_string()
    } else {
        label.to_string()
    }
}

/// The macOS Computer Name, or None when it is unset.
#[cfg(target_os = "macos")]
pub fn computer_name() -> Option<String> {
    use core_foundation::base::TCFType;
    use core_foundation::string::CFString;
    use system_configuration_sys::dynamic_store_copy_specific::SCDynamicStoreCopyComputerName;
    // SAFETY: a null store asks for a temporary session, and a null encoding
    // pointer is allowed. The result is null or a string we own.
    let raw = unsafe { SCDynamicStoreCopyComputerName(std::ptr::null(), std::ptr::null_mut()) };
    if raw.is_null() {
        return None;
    }
    // SAFETY: a Copy function returns a +1 reference, which this takes over.
    let name = unsafe { CFString::wrap_under_create_rule(raw) }.to_string();
    let name = name.trim();
    (!name.is_empty()).then(|| name.to_string())
}

pub fn windows_problem(component: &str) -> Option<NameProblem> {
    // Windows reserves these even with an extension ("nul.txt").
    let stem = component.split('.').next().unwrap_or(component);
    let upper = stem.to_ascii_uppercase();
    let reserved = matches!(upper.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || ["COM", "LPT"].iter().any(|p| {
            upper
                .strip_prefix(p)
                .is_some_and(|n| matches!(n.as_bytes(), [b'1'..=b'9']))
        });
    if reserved {
        return Some(NameProblem::ReservedOnWindows);
    }
    if let Some(c) = component
        .chars()
        .find(|c| matches!(c, '<' | '>' | ':' | '"' | '|' | '?' | '*') || c.is_control())
    {
        return Some(NameProblem::BadCharOnWindows(c));
    }
    if component.ends_with('.') || component.ends_with(' ') {
        return Some(NameProblem::TrailingDotOrSpace);
    }
    None
}

/// Why `name` can't name a folder, which every paired computer uses as a
/// folder name too, or None when it can. `kind` names the thing in the
/// sentence; callers add what to do about it.
pub fn name_problem(kind: &str, name: &str) -> Option<String> {
    if name.trim().is_empty() {
        return Some(format!("{kind} names can't be empty."));
    }
    if name.contains(['/', '\\']) {
        return Some(format!(
            "{kind} names can't contain / or \\, because other computers use the name as a \
             folder name."
        ));
    }
    if name.starts_with(' ') {
        return Some(format!(
            "{kind} name `{name}` can't be used: it starts with a space."
        ));
    }
    if name == "." || name == ".." {
        return Some(format!("{kind} names can't be . or ..."));
    }
    let why = match windows_problem(name)? {
        NameProblem::ReservedOnWindows => format!("`{name}` is a reserved name on Windows"),
        NameProblem::BadCharOnWindows(c) if c.is_control() => {
            "it contains a control character, which Windows doesn't allow".to_string()
        }
        NameProblem::BadCharOnWindows(c) => {
            format!("it contains {c}, which Windows doesn't allow in folder names")
        }
        NameProblem::TrailingDotOrSpace => {
            "Windows doesn't allow a folder name that ends with a dot or a space".to_string()
        }
        NameProblem::CaseCollision(_) => return None,
    };
    Some(format!("{kind} name `{name}` can't be used: {why}."))
}

/// The folder that holds a project's folders on a computer that has several
/// of them. A project name is a label and may hold anything, so characters
/// some system can't use in a folder name become spaces.
pub fn project_folder_name(name: &str) -> String {
    const MAX_CHARS: usize = 100;
    let spaced: String = name
        .chars()
        .map(|c| {
            let bad = matches!(c, '/' | '\\' | '<' | '>' | ':' | '"' | '|' | '?' | '*');
            if bad || c.is_control() { ' ' } else { c }
        })
        .collect();
    let joined: String = spaced
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(MAX_CHARS)
        .collect();
    let out = joined.trim_end_matches(['.', ' ']);
    if out.is_empty() {
        return "Project".to_string();
    }
    if windows_problem(out) == Some(NameProblem::ReservedOnWindows) {
        // Windows reserves the part before the first dot, so mark that part.
        let stem = out.find('.').unwrap_or(out.len());
        return format!("{}_{}", &out[..stem], &out[stem..]);
    }
    out.to_string()
}

/// Windows and default macOS volumes are case-insensitive, so the first path
/// wins and later ones differing only by case are reported as (kept, skipped).
pub fn case_collisions<'a>(paths: impl IntoIterator<Item = &'a str>) -> Vec<(String, String)> {
    let mut seen: std::collections::HashMap<String, &str> = std::collections::HashMap::new();
    let mut out = Vec::new();
    for p in paths {
        match seen.get(&p.to_lowercase()) {
            Some(kept) if *kept != p => out.push((kept.to_string(), p.to_string())),
            Some(_) => {}
            None => {
                seen.insert(p.to_lowercase(), p);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::SeedableRng;
    use rand::rngs::StdRng;

    #[test]
    fn default_name_has_three_or_more_words() {
        let mut rng = StdRng::seed_from_u64(7);
        let name = default_name(&mut rng);
        assert!(name.split(' ').count() >= 3, "{name}");
    }

    #[test]
    fn default_name_is_deterministic_for_a_seed() {
        let a = default_name(&mut StdRng::seed_from_u64(1));
        let b = default_name(&mut StdRng::seed_from_u64(1));
        assert_eq!(a, b);
    }

    #[test]
    fn host_label_is_never_empty() {
        let l = host_label();
        assert!(!l.is_empty());
        assert!(!l.ends_with(".local"));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn mac_names_use_the_computer_name_as_written() {
        let name = computer_name().expect("every Mac has a Computer Name");
        assert_eq!(host_label(), name);
    }

    #[test]
    fn reserved_names_are_flagged() {
        for n in [
            "CON",
            "con",
            "Prn",
            "AUX",
            "NUL",
            "COM1",
            "com9",
            "LPT1",
            "lpt9",
            "nul.txt",
            "COM3.tar.gz",
        ] {
            assert_eq!(
                windows_problem(n),
                Some(NameProblem::ReservedOnWindows),
                "{n}"
            );
        }
        for n in ["COM0", "CONSOLE", "COM10", "aux1", "readme.md"] {
            assert_eq!(windows_problem(n), None, "{n}");
        }
    }

    #[test]
    fn bad_chars_are_flagged() {
        for c in ['<', '>', ':', '"', '|', '?', '*', '\u{1}', '\u{1f}'] {
            let n = format!("a{c}b");
            assert_eq!(windows_problem(&n), Some(NameProblem::BadCharOnWindows(c)));
        }
    }

    #[test]
    fn trailing_dot_or_space_is_flagged() {
        assert_eq!(
            windows_problem("name."),
            Some(NameProblem::TrailingDotOrSpace)
        );
        assert_eq!(
            windows_problem("name "),
            Some(NameProblem::TrailingDotOrSpace)
        );
        assert_eq!(windows_problem("name"), None);
    }

    #[test]
    fn folder_names_that_break_on_another_computer_are_refused() {
        for n in [
            "a/b", "a\\b", ".", "..", "CON", "nul.txt", "a:b", "a?", "end.",
        ] {
            assert!(name_problem("Folder", n).is_some(), "{n:?}");
        }
        // Callers check the name they will store, so spaces are not trimmed away.
        assert!(name_problem("Folder", "end ").is_some());
        assert!(name_problem("Folder", " lead").is_some());
        let slash = name_problem("Folder", "a/b").unwrap();
        assert!(
            slash.starts_with("Folder names can't contain / or \\"),
            "{slash}"
        );
        let colon = name_problem("Folder", "a:b").unwrap();
        assert!(colon.contains("contains :"), "{colon}");
        assert!(name_problem("Folder", "  ").is_some());
        for n in ["My project", "garden-2", "über", ".github"] {
            assert_eq!(name_problem("Folder", n), None, "{n}");
        }
    }

    #[test]
    fn any_project_name_gives_a_folder_name_every_system_can_hold() {
        for (name, folder) in [
            ("What Next?", "What Next"),
            ("web/app", "web app"),
            ("a: b|c", "a b c"),
            ("  spaced   out  ", "spaced out"),
            ("Ends with a dot.", "Ends with a dot"),
            ("CON", "CON_"),
            ("nul.txt", "nul_.txt"),
            ("???", "Project"),
            ("My project", "My project"),
        ] {
            assert_eq!(project_folder_name(name), folder, "{name:?}");
            assert_eq!(name_problem("Folder", folder), None, "{folder:?}");
        }
        let long = project_folder_name(&"x".repeat(300));
        assert_eq!(long.chars().count(), 100);
    }

    #[test]
    fn case_collisions_keep_first_and_skip_later() {
        let got = case_collisions(["a/Readme.md", "b.txt", "a/README.md", "a/readme.md"]);
        assert_eq!(
            got,
            vec![
                ("a/Readme.md".to_string(), "a/README.md".to_string()),
                ("a/Readme.md".to_string(), "a/readme.md".to_string()),
            ]
        );
        assert!(case_collisions(["a", "b"]).is_empty());
    }
}
