//! Everything the UI can ask for, and where each request goes.

use super::Core;
use crate::discovery::Discovered;
use crate::model::{CommandId, FolderId, InstanceId, Permissions, ProjectId, ThemeChoice};
use crate::transfer::TransferRequest;
use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    Rename(String),
    SetProjectsFolder(PathBuf),
    SetTheme(ThemeChoice),
    SetIgnores {
        extra: Vec<String>,
        always_include: Vec<String>,
        removed_defaults: Vec<String>,
    },
    OpenLogFolder,

    SelectPeer(InstanceId),
    /// `requested` is what we ask the target to allow us; `offered` is what
    /// we allow it.
    Pair {
        target: Discovered,
        requested: Permissions,
        offered: Permissions,
    },
    /// Answers "Does <name> show this code?" on the computer that started
    /// pairing.
    ConfirmPairCode(bool),
    /// Closes the outgoing pairing view, stopping the pairing if it is still
    /// running.
    DismissPairing,
    /// `ip` or `ip:port`, as typed.
    AddByAddress(String),
    /// Answers `UiState::pair_prompt`: what to allow the other computer, or
    /// None to decline.
    AnswerPair(Option<Permissions>),
    /// Closes the incoming pairing view, withdrawing an acceptance the other
    /// computer has not confirmed yet.
    DismissPairPrompt,
    Unpair(InstanceId),
    SetAllows(InstanceId, Permissions),

    CreateProject {
        name: String,
        folder: PathBuf,
    },
    RenameProject(ProjectId, String),
    /// Saves the project's description, as typed.
    SetDescription(ProjectId, String),
    /// Forgets the project here; its files stay on disk.
    DeleteProject(ProjectId),
    AddFolder(ProjectId, PathBuf),
    /// Forgets the folder here; its files stay on disk.
    RemoveFolder(ProjectId, FolderId),
    SetFolderPath(ProjectId, FolderId, PathBuf),
    SetPrimary(ProjectId, FolderId),
    /// Sends the project's folders, description and commands to the
    /// selected peer and takes in what it has; copies no files.
    SyncDetails(ProjectId),
    /// Where the selected peer keeps a folder, as typed; `~` is its home.
    SetPeerFolderPath(ProjectId, FolderId, String),

    Prepare(TransferRequest),
    /// Runs the preview in `TransferState::Ready`.
    Execute,
    CancelTransfer,
    /// Returns to `TransferState::Idle` after a preview, result or failure.
    DismissTransfer,

    /// Project, label, command line.
    AddCommand(ProjectId, String, String),
    EditCommand(ProjectId, CommandId, String, String),
    DeleteCommand(ProjectId, CommandId),
    /// The last field is the hash of the command text the user confirmed, or
    /// None when `commands::must_confirm` said no confirmation was needed.
    /// The core refuses if the stored text no longer matches either way.
    RunCommand(ProjectId, CommandId, Option<String>),
    StopCommand(CommandId),
}

impl Core {
    pub(super) async fn handle(&self, a: Action) -> anyhow::Result<()> {
        match a {
            Action::Rename(name) => self.rename(name).await,
            Action::SetProjectsFolder(p) => self.set_projects_folder(p).await,
            Action::SetTheme(t) => self.set_theme(t).await,
            Action::SetIgnores {
                extra,
                always_include,
                removed_defaults,
            } => {
                self.set_ignores(extra, always_include, removed_defaults)
                    .await
            }
            Action::OpenLogFolder => self.open_log_folder(),

            Action::SelectPeer(id) => self.select_peer(id).await,
            Action::Pair {
                target,
                requested,
                offered,
            } => {
                self.start_pair(target, requested, offered);
                Ok(())
            }
            Action::ConfirmPairCode(matches) => {
                self.confirm_code(matches);
                Ok(())
            }
            Action::DismissPairing => {
                self.dismiss_pairing();
                Ok(())
            }
            Action::AddByAddress(text) => {
                tokio::spawn(self.clone().add_by_address(text));
                Ok(())
            }
            Action::AnswerPair(answer) => {
                self.answer_pair(answer);
                Ok(())
            }
            Action::DismissPairPrompt => {
                self.dismiss_prompt();
                Ok(())
            }
            Action::Unpair(id) => self.unpair(id).await,
            Action::SetAllows(id, p) => self.set_allows(id, p).await,

            Action::CreateProject { name, folder } => self.create_project(name, folder).await,
            Action::RenameProject(id, name) => self.rename_project(id, name).await,
            Action::SetDescription(id, text) => self.set_description(id, text).await,
            Action::DeleteProject(id) => self.delete_project(id).await,
            Action::AddFolder(id, path) => self.add_folder(id, path).await,
            Action::RemoveFolder(id, f) => self.remove_folder(id, f).await,
            Action::SetFolderPath(id, f, path) => self.set_folder_path(id, f, path).await,
            Action::SetPrimary(id, f) => self.set_primary(id, f).await,
            Action::SyncDetails(id) => self.sync_details(id),
            Action::SetPeerFolderPath(id, f, path) => self.set_peer_folder(id, f, path),

            Action::Prepare(req) => self.prepare(req),
            Action::Execute => self.execute(),
            Action::CancelTransfer => {
                self.cancel_transfer();
                Ok(())
            }
            Action::DismissTransfer => {
                self.dismiss_transfer();
                Ok(())
            }

            Action::AddCommand(p, label, line) => self.add_command(p, label, line).await,
            Action::EditCommand(p, c, label, line) => self.edit_command(p, c, label, line).await,
            Action::DeleteCommand(p, c) => self.delete_command(p, c).await,
            Action::RunCommand(p, c, confirmed) => self.run_command(p, c, confirmed).await,
            Action::StopCommand(c) => {
                self.stop_command(c);
                Ok(())
            }
        }
    }
}
