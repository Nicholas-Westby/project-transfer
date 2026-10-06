//! The large sheet over the project view for a transfer: comparing, the
//! preview, progress, and the result.

use super::theme::Palette;
use super::widgets::{self, plural};
use super::{App, dialogs, preview, preview_view, transfer_running};
use crate::core::{Action, TransferState, UiState};
use crate::model::Direction;
use crate::transfer::{Preview, Summary};
use crate::units::{size, speed, took};
use egui::{RichText, Ui};
use std::time::Duration;

const WIDTH: f32 = 680.0;

impl App {
    pub(super) fn transfer_sheet(&mut self, ui: &mut Ui, s: &UiState) {
        let req = self.view.last_request.clone();
        let peer = req
            .as_ref()
            .and_then(|r| s.peer(r.peer))
            .map(|p| p.peer.name.clone())
            .unwrap_or_else(|| "the other computer".into());
        // A pull of a project that is not here yet is named by the peer's copy.
        let project = req
            .as_ref()
            .and_then(|r| {
                // A matched project is still under its old id here until
                // the transfer runs.
                let old = self.view.follow.filter(|(_, to)| *to == r.project);
                s.project(r.project)
                    .or_else(|| old.and_then(|(from, _)| s.project(from)))
                    .map(|p| p.name.clone())
                    .or_else(|| s.remote_projects.get(&r.project).map(|p| p.name.clone()))
            })
            .unwrap_or_else(|| "the project".into());
        let dir = req.as_ref().map_or(Direction::Push, |r| r.direction);
        let wide = ui.ctx().content_rect().width().min(WIDTH + 48.0) - 48.0;
        // Only the preview needs the room; progress and results stay compact.
        let width = wide.min(520.0);

        let action = match &s.transfer {
            TransferState::Idle => return,
            TransferState::Preparing => {
                let r = dialogs::sheet(ui, "transfer_preparing", width, true, |ui| {
                    dialogs::sheet_title(ui, "Comparing folders");
                    ui.horizontal(|ui| {
                        ui.spinner();
                        widgets::muted(
                            ui,
                            format!(
                                "Scanning {project} here and on {peer}. Nothing is written yet."
                            ),
                        );
                    });
                    cancel_row(ui, "Cancel")
                });
                (r.inner || r.dismissed).then_some(Action::CancelTransfer)
            }
            TransferState::Ready(p) => {
                let title = match dir {
                    Direction::Push => format!("Push {project} to {peer}"),
                    Direction::Pull => format!("Pull {project} from {peer}"),
                };
                let r = dialogs::sheet(ui, "transfer_ready", wide, true, |ui| {
                    dialogs::sheet_title(ui, &title);
                    ready(ui, p, &peer)
                });
                match (r.dismissed, r.inner) {
                    (true, _) | (_, Some(false)) => Some(Action::CancelTransfer),
                    (_, Some(true)) if p.is_empty() => Some(Action::DismissTransfer),
                    (_, Some(true)) => Some(Action::Execute),
                    _ => None,
                }
            }
            TransferState::Running {
                done,
                total,
                files,
                current,
                started,
                left,
            } => {
                let title = match dir {
                    Direction::Push => format!("Pushing {project} to {peer}"),
                    Direction::Pull => format!("Pulling {project} from {peer}"),
                };
                let r = dialogs::sheet(ui, "transfer_running", width, false, |ui| {
                    dialogs::sheet_title(ui, &title);
                    transfer_running::body(ui, *done, *total, *files, current, *started, *left)
                });
                r.inner.then_some(Action::CancelTransfer)
            }
            TransferState::Finished(sum) => {
                let r = dialogs::sheet(ui, "transfer_finished", width, true, |ui| {
                    dialogs::sheet_title(ui, "Transfer finished");
                    finished(ui, sum, dir, &peer);
                    done_row(ui)
                });
                (r.inner || r.dismissed).then_some(Action::DismissTransfer)
            }
            TransferState::Failed(why) => {
                let r = dialogs::sheet(ui, "transfer_failed", width, true, |ui| {
                    dialogs::sheet_title(ui, "The transfer stopped");
                    let pal = Palette::of(ui.ctx());
                    ui.add(egui::Label::new(RichText::new(why).color(pal.removed)).wrap());
                    widgets::small_muted(
                        ui,
                        "Files already copied stay in place. Try again when the other computer \
                         is reachable.",
                    );
                    done_row(ui)
                });
                (r.inner || r.dismissed).then_some(Action::DismissTransfer)
            }
        };
        if let Some(a) = action {
            self.act(a);
        }
    }
}

fn ready(ui: &mut Ui, p: &Preview, peer: &str) -> Option<bool> {
    let pal = Palette::of(ui.ctx());
    if p.is_empty() {
        let what = match p.request.direction {
            Direction::Push => format!("Nothing to push. {peer} already matches this computer."),
            Direction::Pull => format!("Nothing to pull. This computer already matches {peer}."),
        };
        widgets::muted(ui, what);
        preview_view::folder_lines(ui, p);
        for w in preview::warnings(p, peer) {
            ui.label(preview_view::ticks(&w, pal.changed));
        }
        return done_row(ui).then_some(true);
    }
    let description = match p.request.direction {
        Direction::Push => format!("The description on {peer} is replaced with this one."),
        Direction::Pull => format!("The description here is replaced with the one on {peer}."),
    };
    if p.files_match() {
        // Only the description differs; the transfer still carries it.
        widgets::muted(ui, "The files already match.");
        widgets::muted(ui, description);
        preview_view::folder_lines(ui, p);
        for w in preview::warnings(p, peer) {
            ui.label(preview_view::ticks(&w, pal.changed));
        }
        let label = match p.request.direction {
            Direction::Push => format!("Push the description to {peer}"),
            Direction::Pull => format!("Pull the description from {peer}"),
        };
        return dialogs::button_row(ui, &label, false, true);
    }
    if p.description {
        widgets::muted(ui, description);
    }
    preview_view::show(ui, p, peer);
    let removes = p.counts().removed_files > 0;
    dialogs::button_row(ui, &preview::confirm_label(p, peer), removes, true)
}

fn finished(ui: &mut Ui, sum: &Summary, dir: Direction, peer: &str) {
    let pal = Palette::of(ui.ctx());
    let verb = match dir {
        Direction::Push => format!("Pushed {} to {peer}", plural(sum.files, "file", "files")),
        Direction::Pull => format!("Pulled {} from {peer}", plural(sum.files, "file", "files")),
    };
    let elapsed = Duration::from_millis(sum.took_ms);
    ui.label(RichText::new(format!("{verb} in {}.", took(elapsed))).color(pal.ink));
    if sum.removed > 0 {
        widgets::muted(
            ui,
            format!(
                "Removed {}.",
                plural(sum.removed, "file or folder", "files or folders")
            ),
        );
    }
    let sent = match speed(sum.bytes, elapsed) {
        Some(rate) => format!("{} sent at {rate}.", size(sum.bytes)),
        None => format!("{} sent.", size(sum.bytes)),
    };
    widgets::small_muted(ui, sent);
    if !sum.failures.is_empty() {
        ui.add_space(8.0);
        ui.label(
            RichText::new(format!(
                "{} could not be written:",
                plural(sum.failures.len() as u64, "file", "files")
            ))
            .color(pal.removed),
        );
        egui::ScrollArea::vertical()
            .max_height(160.0)
            .show(ui, |ui| {
                for (rel, why) in &sum.failures {
                    ui.label(widgets::mono(rel).color(pal.ink));
                    widgets::small_muted(ui, why);
                }
            });
    }
}

fn cancel_row(ui: &mut Ui, label: &str) -> bool {
    ui.add_space(12.0);
    dialogs::right_row(ui, |ui| ui.button(label).clicked()).inner
}

fn done_row(ui: &mut Ui) -> bool {
    ui.add_space(12.0);
    dialogs::right_row(ui, |ui| widgets::primary(ui, "Done").clicked()).inner
}
