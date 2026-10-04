//! Editing projects, their folders and their commands. Every edit keeps the
//! primary folder a member of the project and never touches files on disk.

use super::Core;
use crate::model::{Command, CommandId, Folder, FolderId, Os, Project, ProjectId};
use crate::transfer::projects::{now_ms, update_projects};
use std::path::{Path, PathBuf};

type Edit<T> = Result<T, String>;

impl Core {
    /// Applies `f`, saves, refreshes the UI and records `f`'s sentence.
    async fn edit(&self, f: impl FnOnce(&mut Vec<Project>) -> Edit<String>) -> anyhow::Result<()> {
        let text = update_projects(&self.shared, f).await?;
        self.sync_projects().await;
        self.ui.info(text);
        Ok(())
    }

    pub(super) async fn sync_projects(&self) {
        let projects = self.shared.projects.read().await.clone();
        self.ui.update(|s| s.projects = projects);
    }

    pub(super) async fn create_project(&self, name: String, folder: PathBuf) -> anyhow::Result<()> {
        self.edit(|all| {
            let name = project_name(&name)?;
            check_folder(all, &folder, None)?;
            let f = Folder {
                id: FolderId::new_v4(),
                name: usable_folder_name(&folder)?,
                local_path: Some(folder.clone()),
            };
            all.push(Project {
                id: ProjectId::new_v4(),
                name: name.clone(),
                primary: f.id,
                folders: vec![f],
                commands: Vec::new(),
                last_transfer: None,
            });
            Ok(format!(
                "Created project `{name}` with folder {}.",
                folder.display()
            ))
        })
        .await
    }

    pub(super) async fn rename_project(&self, id: ProjectId, name: String) -> anyhow::Result<()> {
        self.edit(|all| {
            let name = project_name(&name)?;
            let p = project(all, id)?;
            let old = std::mem::replace(&mut p.name, name.clone());
            Ok(format!("Renamed project `{old}` to `{name}`."))
        })
        .await
    }

    pub(super) async fn delete_project(&self, id: ProjectId) -> anyhow::Result<()> {
        self.edit(|all| {
            let name = project(all, id)?.name.clone();
            all.retain(|p| p.id != id);
            Ok(format!(
                "Removed project `{name}` from this computer. Its files are still on disk."
            ))
        })
        .await
    }

    pub(super) async fn add_folder(&self, id: ProjectId, path: PathBuf) -> anyhow::Result<()> {
        self.edit(|all| {
            check_folder(all, &path, None)?;
            let base = usable_folder_name(&path)?;
            let p = project(all, id)?;
            let mut name = base.clone();
            for n in 2.. {
                if !p.folders.iter().any(|f| f.name.eq_ignore_ascii_case(&name)) {
                    break;
                }
                name = format!("{base} {n}");
            }
            p.folders.push(Folder {
                id: FolderId::new_v4(),
                name: name.clone(),
                local_path: Some(path.clone()),
            });
            Ok(format!(
                "Added folder `{name}` ({}) to `{}`.",
                path.display(),
                p.name
            ))
        })
        .await
    }

    pub(super) async fn remove_folder(
        &self,
        id: ProjectId,
        folder: FolderId,
    ) -> anyhow::Result<()> {
        self.edit(|all| {
            let p = project(all, id)?;
            let at = p
                .folders
                .iter()
                .position(|f| f.id == folder)
                .ok_or(GONE_FOLDER)?;
            if p.folders.len() == 1 {
                return Err(format!(
                    "`{}` needs at least one folder. Delete the project instead.",
                    p.name
                ));
            }
            let removed = p.folders.remove(at);
            if p.primary == folder {
                p.primary = p.folders[at.min(p.folders.len() - 1)].id;
            }
            Ok(format!(
                "Removed folder `{}` from `{}`. Its files are still on disk.",
                removed.name, p.name
            ))
        })
        .await
    }

    pub(super) async fn set_folder_path(
        &self,
        id: ProjectId,
        folder: FolderId,
        path: PathBuf,
    ) -> anyhow::Result<()> {
        self.edit(|all| {
            check_folder(all, &path, Some(folder))?;
            let p = project(all, id)?;
            let f = p
                .folders
                .iter_mut()
                .find(|f| f.id == folder)
                .ok_or(GONE_FOLDER)?;
            f.local_path = Some(path.clone());
            Ok(format!(
                "Folder `{}` of `{}` is now {} on this computer.",
                f.name,
                p.name,
                path.display()
            ))
        })
        .await
    }

    pub(super) async fn set_primary(&self, id: ProjectId, folder: FolderId) -> anyhow::Result<()> {
        self.edit(|all| {
            let p = project(all, id)?;
            let f = p
                .folders
                .iter()
                .find(|f| f.id == folder)
                .ok_or(GONE_FOLDER)?;
            let text = format!(
                "`{}` is now the primary folder of `{}`. Commands run there.",
                f.name, p.name
            );
            p.primary = folder;
            Ok(text)
        })
        .await
    }

    pub(super) async fn add_command(
        &self,
        id: ProjectId,
        label: String,
        line: String,
    ) -> anyhow::Result<()> {
        self.edit(|all| {
            let (label, line) = command_text(&label, &line)?;
            let p = project(all, id)?;
            p.commands.push(Command {
                id: CommandId::new_v4(),
                label: label.clone(),
                line,
                created_on: Os::current(),
                updated_at_ms: now_ms(),
                deleted: false,
                last_run_hash: None,
            });
            Ok(format!("Added command `{label}` to `{}`.", p.name))
        })
        .await
    }

    pub(super) async fn edit_command(
        &self,
        id: ProjectId,
        cmd: CommandId,
        label: String,
        line: String,
    ) -> anyhow::Result<()> {
        self.edit(|all| {
            let (label, line) = command_text(&label, &line)?;
            let c = live_command(project(all, id)?, cmd)?;
            c.label = label.clone();
            c.line = line;
            c.updated_at_ms = now_ms();
            Ok(format!("Saved command `{label}`."))
        })
        .await
    }

    pub(super) async fn delete_command(&self, id: ProjectId, cmd: CommandId) -> anyhow::Result<()> {
        self.edit(|all| {
            let c = live_command(project(all, id)?, cmd)?;
            c.deleted = true;
            c.updated_at_ms = now_ms();
            Ok(format!(
                "Deleted command `{}`. Paired computers drop it after the next transfer.",
                c.label
            ))
        })
        .await
    }
}

const GONE_PROJECT: &str = "That project is no longer on this computer.";
const GONE_FOLDER: &str = "That folder is no longer part of the project.";
const GONE_COMMAND: &str = "That command no longer exists.";

fn project(all: &mut [Project], id: ProjectId) -> Edit<&mut Project> {
    all.iter_mut()
        .find(|p| p.id == id)
        .ok_or_else(|| GONE_PROJECT.to_string())
}

pub(super) fn live_command(p: &mut Project, id: CommandId) -> Edit<&mut Command> {
    p.commands
        .iter_mut()
        .find(|c| c.id == id && !c.deleted)
        .ok_or_else(|| GONE_COMMAND.to_string())
}

fn required(text: &str, why: &str) -> Edit<String> {
    let t = text.trim();
    if t.is_empty() {
        return Err(why.to_string());
    }
    Ok(t.to_string())
}

/// Every paired computer uses the project name as a folder name, whatever
/// system it runs, so names that break anywhere are refused here.
pub(crate) fn project_name(text: &str) -> Edit<String> {
    let name = required(text, "Enter a name for the project.")?;
    match crate::naming::name_problem("Project", &name) {
        Some(why) => Err(format!("{why} Choose another name.")),
        None => Ok(name),
    }
}

fn usable_folder_name(path: &Path) -> Edit<String> {
    let name = folder_name(path);
    match crate::naming::name_problem("Folder", &name) {
        Some(why) => Err(format!(
            "{why} Rename the folder on disk or choose another one."
        )),
        None => Ok(name),
    }
}

fn command_text(label: &str, line: &str) -> Edit<(String, String)> {
    let label = required(label, "Enter a label for the command.")?;
    let line = required(line, "Enter the command to run.")?;
    Ok((label, line))
}

fn folder_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| "folder".to_string())
}

/// A folder must exist, and must not be, contain or sit inside a folder that
/// is already in a project: a transfer to one would rewrite the other.
fn check_folder(all: &[Project], path: &Path, except: Option<FolderId>) -> Edit<()> {
    if !path.is_dir() {
        return Err(format!(
            "{} is not a folder on this computer. Choose another folder.",
            path.display()
        ));
    }
    if let Some(why) = too_broad(path) {
        return Err(why);
    }
    for p in all {
        for f in p.folders.iter().filter(|f| Some(f.id) != except) {
            let Some(other) = &f.local_path else { continue };
            if path.starts_with(other) || other.starts_with(path) {
                return Err(format!(
                    "{} overlaps {}, folder `{}` of project `{}`. Choose a folder that is not \
                     part of another project.",
                    path.display(),
                    other.display(),
                    f.name,
                    p.name
                ));
            }
        }
    }
    Ok(())
}

/// A project folder is mirrored, deletions included, so the home folder or a
/// whole disk is refused: one wrong push would rewrite everything on it.
fn too_broad(path: &Path) -> Option<String> {
    let real = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let home = directories::BaseDirs::new().map(|d| d.home_dir().to_path_buf());
    let home = home.map(|h| h.canonicalize().unwrap_or(h));
    if home.as_deref() == Some(real.as_path()) {
        return Some(format!(
            "{} is your home folder. Choose the project's own folder inside it.",
            path.display()
        ));
    }
    if real.parent().is_none() || is_mount_point(&real) {
        return Some(format!(
            "{} is the top of a disk. Choose the project's own folder on it.",
            path.display()
        ));
    }
    None
}

/// A folder on another device than its parent is where a disk is mounted,
/// such as `/Volumes/USB` on a Mac.
#[cfg(unix)]
fn is_mount_point(path: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    let (Some(parent), Ok(me)) = (path.parent(), std::fs::metadata(path)) else {
        return false;
    };
    std::fs::metadata(parent).is_ok_and(|p| p.dev() != me.dev())
}

/// Drive roots and shares have no parent, which `too_broad` checks already.
#[cfg(not(unix))]
fn is_mount_point(_path: &Path) -> bool {
    false
}
