//! Editing projects and their folders. Every edit keeps the primary folder a
//! member of the project and never touches files on disk.

use super::Core;
use crate::model::{Description, Folder, FolderId, Project, ProjectId};
use crate::transfer::home;
use crate::transfer::projects::{now_ms, update_projects};
use std::path::{Path, PathBuf};

pub(super) type Edit<T> = Result<T, String>;

impl Core {
    /// Applies `f`, saves, refreshes the UI and records `f`'s sentence.
    async fn edit(&self, f: impl FnOnce(&mut Vec<Project>) -> Edit<String>) -> anyhow::Result<()> {
        let text = update_projects(&self.shared, f).await?;
        self.sync_projects().await;
        self.ui.info(text);
        Ok(())
    }

    /// Shows the projects and the shared commands as they are now.
    pub(super) async fn sync_projects(&self) {
        let projects = self.shared.projects.read().await.clone();
        let library = self.shared.shared_commands.read().await.clone();
        self.ui.update(|s| {
            s.projects = projects;
            s.shared_commands = library;
        });
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
                description: Default::default(),
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

    pub(super) async fn set_description(&self, id: ProjectId, text: String) -> anyhow::Result<()> {
        // Trailing blank lines are a slip of the Enter key, not content.
        let text = text.trim_end().to_string();
        self.edit(|all| {
            let p = project(all, id)?;
            p.description = Description::edited(&text, &p.description, now_ms());
            Ok(format!("Saved the description of `{}`.", p.name))
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
}

const GONE_PROJECT: &str = "That project is no longer on this computer.";
const GONE_FOLDER: &str = "That folder is no longer part of the project.";

pub(super) fn project(all: &mut [Project], id: ProjectId) -> Edit<&mut Project> {
    all.iter_mut()
        .find(|p| p.id == id)
        .ok_or_else(|| GONE_PROJECT.to_string())
}

pub(super) fn required(text: &str, why: &str) -> Edit<String> {
    let t = text.trim();
    if t.is_empty() {
        return Err(why.to_string());
    }
    Ok(t.to_string())
}

/// A project's name is a label: anything but blank. Where another computer
/// needs a folder named after it, `naming::project_folder_name` makes one.
pub(crate) fn project_name(text: &str) -> Edit<String> {
    required(text, "Enter a name for the project.")
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
    if let Some(why) = home::too_broad(path, home::real_home().as_deref()) {
        return Err(why);
    }
    home::check_overlap(all, path, except)
}
