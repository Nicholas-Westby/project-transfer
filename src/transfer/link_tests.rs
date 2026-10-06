use super::*;
use crate::model::Folder;
use crate::protocol::RemoteFolder;
use uuid::Uuid;

fn project(name: &str, folders: &[&str]) -> Project {
    let folders: Vec<Folder> = folders
        .iter()
        .map(|n| Folder {
            id: Uuid::new_v4(),
            name: n.to_string(),
            local_path: Some(format!("/Users/jdoe/Dev/{n}").into()),
        })
        .collect();
    Project {
        id: Uuid::new_v4(),
        name: name.into(),
        primary: folders[0].id,
        folders,
        commands: vec![],
        last_transfer: None,
        description: Default::default(),
    }
}

fn summary(p: &Project) -> ProjectSummary {
    ProjectSummary {
        id: p.id,
        name: p.name.clone(),
    }
}

fn info(p: &Project) -> RemoteProject {
    RemoteProject {
        name: p.name.clone(),
        primary: p.primary,
        description: Default::default(),
        folders: p
            .folders
            .iter()
            .map(|f| RemoteFolder {
                id: f.id,
                name: f.name.clone(),
                path: Some(format!("C:\\dev\\{}", f.name)),
                home_hint: None,
                shown: None,
            })
            .collect(),
    }
}

#[test]
fn a_project_made_on_each_computer_is_matched_by_name() {
    let mine = project("Garden Planner", &["garden-planner"]);
    let theirs = project("garden planner ", &["garden-planner"]);
    let local = [mine.clone(), project("Tide Tables", &["tide-tables"])];
    assert_eq!(
        counterpart(&local, &mine, &[summary(&theirs)]),
        Some(theirs.id)
    );
    // Already the same project: nothing to match.
    assert_eq!(counterpart(&local, &mine, &[summary(&mine)]), None);
    assert_eq!(counterpart(&local, &local[1], &[summary(&theirs)]), None);
}

#[test]
fn an_ambiguous_name_stays_two_projects() {
    let mine = project("Garden", &["garden"]);
    let (one, two) = (project("Garden", &["a"]), project("Garden", &["b"]));
    let local = [mine.clone()];
    assert_eq!(
        counterpart(&local, &mine, &[summary(&one), summary(&two)]),
        None
    );
    let twice = [mine.clone(), project("garden", &["c"])];
    assert_eq!(counterpart(&twice, &mine, &[summary(&one)]), None);
    // Their id is another project's here.
    let clash = [mine.clone(), one.clone()];
    assert_eq!(counterpart(&clash, &mine, &[summary(&one)]), None);
}

#[test]
fn folders_match_by_name_only_and_each_of_theirs_once() {
    let mine = project("Garden", &["app", "docs", "extra"]);
    let theirs = project("Garden", &["App", "docs", "tools"]);
    let map = folder_map(&mine, &info(&theirs));
    assert_eq!(map.len(), 2);
    assert_eq!(map[&mine.folders[0].id], theirs.folders[0].id);
    assert_eq!(map[&mine.folders[1].id], theirs.folders[1].id);

    // Unrelated folders are never mirrored onto each other.
    let one = project("Garden", &["garden-planner"]);
    let renamed = project("Garden", &["planner"]);
    assert!(folder_map(&one, &info(&renamed)).is_empty());

    let twice = project("Garden", &["app", "App "]);
    let map = folder_map(&twice, &info(&project("Garden", &["app"])));
    assert!(map.is_empty(), "two of ours read alike: {map:?}");
}

#[test]
fn rekey_takes_their_ids_and_keeps_paths_and_primary() {
    let mut mine = project("Garden", &["app", "docs"]);
    mine.primary = mine.folders[1].id;
    let theirs = project("Garden", &["app", "docs"]);
    let map = folder_map(&mine, &info(&theirs));
    let mut all = vec![mine.clone()];
    rekey(&mut all, mine.id, theirs.id, &map).unwrap();
    let p = &all[0];
    assert_eq!(p.id, theirs.id);
    assert_eq!(p.primary, theirs.folders[1].id);
    assert_eq!(p.folders[0].id, theirs.folders[0].id);
    assert_eq!(p.folders[0].local_path, mine.folders[0].local_path);
    assert!(rekey(&mut all, theirs.id, theirs.id, &map).is_err());
}

#[test]
fn their_copy_shows_under_our_ids() {
    let mine = project("Garden", &["app"]);
    let theirs = project("Garden", &["app"]);
    let only_there = project("Seed Catalog", &["seed-catalog"]);
    let list = [summary(&theirs), summary(&only_there)];
    let infos = HashMap::from([
        (theirs.id, info(&theirs)),
        (only_there.id, info(&only_there)),
    ]);
    let seen = as_seen_here(std::slice::from_ref(&mine), &list, infos);
    assert_eq!(seen.len(), 2);
    let shown = &seen[&mine.id];
    assert_eq!(shown.folders[0].id, mine.folders[0].id);
    assert_eq!(shown.primary, mine.primary);
    assert_eq!(shown.folders[0].path.as_deref(), Some("C:\\dev\\app"));
    assert!(seen.contains_key(&only_there.id));
}
