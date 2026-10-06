//! The window: a session bar, the project list, the selected project, and
//! the sheets and confirmations over them. Every screen reads one snapshot
//! of `UiState` per frame and sends `Action`s back.

mod activity;
mod commands_view;
mod computers;
mod confirmations;
pub mod description;
pub mod dialogs;
mod folders;
mod pairing;
mod path_cut;
mod peer_folder;
mod picker;
mod preview;
mod preview_view;
mod project_view;
mod projects_list;
pub mod remote_projects;
mod session_bar;
mod settings;
pub mod theme;
mod transfer_bar;
mod transfer_running;
mod transfer_sheet;
pub mod widgets;

pub use confirmations::Confirm;
pub use picker::{FolderPicker, RfdPicker};

use crate::core::{Action, AppCore, TransferState, UiState};
use crate::discovery::Discovered;
use crate::model::{CommandId, Permissions, ProjectId, ThemeChoice};
use crate::transfer::TransferRequest;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;

/// Where the window gets its state and sends its requests. The app uses
/// `AppCore`; tests use a hand-built state.
pub trait Backend: Send + Sync {
    fn snapshot(&self) -> UiState;
    fn act(&self, a: Action);
}

impl Backend for AppCore {
    fn snapshot(&self) -> UiState {
        // A clone, so pickers and dialogs never hold the core's lock.
        self.state().clone()
    }

    fn act(&self, a: Action) {
        AppCore::act(self, a);
    }
}

/// A sheet opened from the window, at most one at a time.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum Sheet {
    #[default]
    None,
    NewProject {
        name: String,
        folder: PathBuf,
    },
    Computers,
    PairSetup {
        target: Discovered,
        requested: Permissions,
        offered: Permissions,
    },
    Settings(settings::Draft),
    CommandEditor {
        project: ProjectId,
        id: Option<CommandId>,
        label: String,
        line: String,
    },
    PeerFolder(peer_folder::PeerFolderDraft),
}

/// What only the window remembers: selection, open sheets, drafts.
#[derive(Default)]
pub struct View {
    pub selected: Option<ProjectId>,
    pub sheet: Sheet,
    pub confirm: Option<Confirm>,
    pub rename_me: Option<String>,
    pub rename_project: Option<(ProjectId, String)>,
    /// The description being typed, until its box loses focus.
    pub description: Option<description::Draft>,
    pub send_everything: bool,
    pub activity_open: bool,
    pub address: String,
    pub hidden_output: HashSet<CommandId>,
    /// The request behind the current transfer, for the running sheet's
    /// wording after the preview is gone.
    pub last_request: Option<TransferRequest>,
    /// A project just asked for, selected once the core adds it.
    pub select_when_added: Option<String>,
    /// A project that takes the other computer's id when its transfer runs:
    /// (old, new), so the selection stays on it.
    pub follow: Option<(ProjectId, ProjectId)>,
    /// What this computer will allow a computer asking to pair, as edited.
    pub prompt_allows: Option<(crate::model::InstanceId, Permissions)>,
    /// This computer's addresses, read when a sheet opens.
    pub my_addresses: Option<Vec<String>>,
    applied_theme: Option<ThemeChoice>,
}

pub struct App {
    backend: Arc<dyn Backend>,
    picker: Box<dyn FolderPicker>,
    styled: bool,
    pub view: View,
}

impl App {
    pub fn new(backend: Arc<dyn Backend>, picker: Box<dyn FolderPicker>) -> App {
        App {
            backend,
            picker,
            styled: false,
            view: View::default(),
        }
    }

    fn act(&self, a: Action) {
        self.backend.act(a);
    }

    /// Draws one frame.
    pub fn show(&mut self, ui: &mut egui::Ui) {
        if !self.styled {
            // New fonts take effect from the next frame, and the styles
            // name a font family, so this frame draws nothing.
            theme::install(ui.ctx());
            self.styled = true;
            ui.ctx().request_repaint();
            return;
        }
        let s = self.backend.snapshot();
        if self.view.applied_theme != Some(s.me.theme) {
            ui.ctx().set_theme(theme::preference(s.me.theme));
            self.view.applied_theme = Some(s.me.theme);
        }
        if matches!(s.transfer, TransferState::Idle) {
            // Cancelled: the project kept its id.
            self.view.follow = None;
        }
        if let TransferState::Ready(p) = &s.transfer {
            self.view.last_request = Some(p.request.clone());
            if let Some(l) = &p.link {
                self.view.follow = Some((l.from, l.to));
            }
        }
        self.keep_selection(&s);

        self.session_bar(ui, &s);
        self.activity(ui, &s);
        self.projects_list(ui, &s);
        self.project_view(ui, &s);

        // Later modals draw on top: sheets, then the transfer, then
        // confirmations, then anything another computer started.
        self.sheets(ui, &s);
        self.transfer_sheet(ui, &s);
        self.confirmations(ui, &s);
        self.pairing(ui, &s);
    }

    /// Keeps a valid project selected, so a fresh list opens its first one.
    fn keep_selection(&mut self, s: &UiState) {
        if let Some((from, to)) = self.view.follow
            && s.project(from).is_none()
            && s.project(to).is_some()
        {
            if self.view.selected == Some(from) {
                self.view.selected = Some(to);
            }
            self.view.follow = None;
        }
        if let Some(name) = &self.view.select_when_added
            && let Some(p) = s.projects.iter().rev().find(|p| &p.name == name)
        {
            self.view.selected = Some(p.id);
            self.view.select_when_added = None;
        }
        let valid = self.view.selected.is_some_and(|id| s.project(id).is_some());
        if !valid {
            self.view.selected = s.projects.first().map(|p| p.id);
        }
    }

    fn sheets(&mut self, ui: &mut egui::Ui, s: &UiState) {
        // Interfaces change when the network does; read them fresh per sheet.
        if self.view.sheet == Sheet::None {
            self.view.my_addresses = None;
        } else if self.view.my_addresses.is_none() {
            self.view.my_addresses = Some(crate::address::this_computer(s.port));
        }
        match self.view.sheet.clone() {
            Sheet::None => {}
            Sheet::NewProject { name, folder } => self.new_project_sheet(ui, name, folder),
            Sheet::Computers => self.computers_sheet(ui, s),
            Sheet::PairSetup {
                target,
                requested,
                offered,
            } => self.pair_setup_sheet(ui, s, target, requested, offered),
            Sheet::Settings(draft) => self.settings_sheet(ui, s, draft),
            Sheet::CommandEditor {
                project,
                id,
                label,
                line,
            } => self.command_editor(ui, project, id, label, line),
            Sheet::PeerFolder(draft) => self.peer_folder_sheet(ui, s, draft),
        }
    }

    fn pick_folder(&self, title: &str) -> Option<PathBuf> {
        self.picker.pick_folder(title)
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.show(ui);
    }
}

/// "Other computers can add this one at `192.168.1.20:47820`." for Settings
/// and the Computers sheet; draw it with `preview_view::ticks`.
pub(crate) fn reach_text(addresses: &Option<Vec<String>>, port: u16) -> String {
    match addresses.as_deref() {
        Some([]) | None => format!(
            "This computer listens on port {port}, but no local network address was found. \
             Check that it is connected to a network."
        ),
        Some(list) => {
            let list: Vec<String> = list.iter().map(|a| format!("`{a}`")).collect();
            format!("Other computers can add this one at {}.", list.join(" or "))
        }
    }
}

/// The selected peer's name, or a stand-in for sentences when none is.
pub(crate) fn peer_name(s: &UiState) -> Option<String> {
    s.selected_peer
        .and_then(|id| s.peer(id))
        .map(|p| p.peer.name.clone())
}
