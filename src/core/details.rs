//! "Sync details" and changing where a folder lives on the selected peer,
//! from the UI's side. Both run in the background and report in the
//! activity strip, then ask the peer for its projects again so the folders
//! table shows the result at once.

use super::Core;
use crate::model::{FolderId, InstanceId, ProjectId};
use crate::transfer;
use anyhow::{Context, bail};

impl Core {
    /// The selected peer and its name, for actions aimed at it.
    fn target(&self) -> anyhow::Result<(InstanceId, String)> {
        let s = self.ui.lock();
        let id = s
            .selected_peer
            .context("Choose a computer in the bar above.")?;
        let name = s
            .peer(id)
            .map_or("the other computer".into(), |v| v.peer.name.clone());
        Ok((id, name))
    }

    fn project_name(&self, id: ProjectId) -> String {
        self.ui
            .lock()
            .project(id)
            .map_or("this project".into(), |p| format!("`{}`", p.name))
    }

    pub(super) fn sync_details(&self, project: ProjectId) -> anyhow::Result<()> {
        if self.busy() {
            bail!(super::transfers::BUSY);
        }
        let (peer, name) = self.target()?;
        let what = self.project_name(project);
        let core = self.clone();
        tokio::spawn(async move {
            let result = async {
                let mut conn = core.connect(peer).await?;
                transfer::sync_details(&mut conn, &core.shared, project).await
            }
            .await;
            core.sync_projects().await;
            match result {
                Ok(synced) => {
                    if let Some(l) = &synced.link {
                        core.ui.info(format!(
                            "Matched {what} with the project `{}` {name} already has, so both \
                             computers now use the same folders for it.",
                            l.their_name
                        ));
                    }
                    core.ui
                        .info(format!("Synced details of {what} with {name}."));
                }
                Err(e) => core.ui.error(format!(
                    "Could not sync details of {what} with {name}: {e:#}"
                )),
            }
            core.poll_now.notify_one();
        });
        Ok(())
    }

    pub(super) fn set_peer_folder(
        &self,
        project: ProjectId,
        folder: FolderId,
        path: String,
    ) -> anyhow::Result<()> {
        let (peer, name) = self.target()?;
        let folder_name = self
            .ui
            .lock()
            .project(project)
            .and_then(|p| p.folders.iter().find(|f| f.id == folder))
            .map_or("the folder".into(), |f| format!("`{}`", f.name));
        let core = self.clone();
        tokio::spawn(async move {
            let result = async {
                let mut conn = core.connect(peer).await?;
                transfer::set_peer_folder_path(&mut conn, project, folder, &path).await
            }
            .await;
            match result {
                Ok(()) => core
                    .ui
                    .info(format!("{name} now keeps {folder_name} at {path}.")),
                Err(e) => core.ui.error(format!(
                    "Could not change where {folder_name} lives on {name}: {e:#}"
                )),
            }
            core.poll_now.notify_one();
        });
        Ok(())
    }
}
