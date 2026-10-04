//! Pairing: choosing what to grant, waiting with the code, and answering
//! another computer's request.

use super::theme::{Palette, semibold};
use super::widgets;
use super::{App, Sheet, dialogs};
use crate::core::{Action, OutgoingPairView, PairPromptView, PairState, PromptState, UiState};
use crate::discovery::Discovered;
use crate::model::Permissions;
use egui::{FontId, RichText, Ui};

pub const SAME_CODE: &str = "Check that both screens show the same code.";

/// The code, large, so two screens can be compared from a step away.
fn code(ui: &mut Ui, code: &str) {
    let pal = Palette::of(ui.ctx());
    ui.add_space(6.0);
    ui.vertical_centered(|ui| {
        ui.label(
            RichText::new(code)
                .font(FontId::new(44.0, semibold()))
                .color(pal.ink),
        );
        widgets::muted(ui, SAME_CODE);
    });
    ui.add_space(10.0);
}

fn allow_boxes(ui: &mut Ui, name: &str, p: &mut Permissions) {
    ui.checkbox(
        &mut p.may_push_to_me,
        format!("Let {name} push to this computer (overwrite this computer's files)"),
    );
    ui.checkbox(
        &mut p.may_pull_from_me,
        format!("Let {name} pull from this computer (read this computer's files)"),
    );
}

impl App {
    pub(super) fn pair_setup_sheet(
        &mut self,
        ui: &mut Ui,
        _s: &UiState,
        target: Discovered,
        mut requested: Permissions,
        mut offered: Permissions,
    ) {
        let name = target.name.clone();
        let r = dialogs::sheet(ui, "pair_setup", 520.0, true, |ui| {
            dialogs::sheet_title(ui, &format!("Pair with {name}"));
            widgets::muted(
                ui,
                format!(
                    "Both computers will show the same 6-digit code. Check it here and on \
                     {name}. Nothing is saved until both of you confirm."
                ),
            );
            ui.add_space(12.0);
            widgets::section(ui, "What this computer allows");
            allow_boxes(ui, &name, &mut offered);
            ui.add_space(10.0);
            widgets::section(ui, &format!("What you ask {name} to allow"));
            ui.checkbox(
                &mut requested.may_push_to_me,
                format!("Let this computer push to {name}"),
            );
            ui.checkbox(
                &mut requested.may_pull_from_me,
                format!("Let this computer pull from {name}"),
            );
            widgets::small_muted(
                ui,
                format!("{name} can change what it allows before accepting."),
            );
            dialogs::button_row(ui, "Pair", false, true)
        });
        match (r.dismissed, r.inner) {
            (true, _) | (_, Some(false)) => self.view.sheet = Sheet::Computers,
            (_, Some(true)) => {
                self.act(Action::Pair {
                    target,
                    requested,
                    offered,
                });
                self.view.sheet = Sheet::None;
            }
            _ => {
                self.view.sheet = Sheet::PairSetup {
                    target,
                    requested,
                    offered,
                }
            }
        }
    }

    pub(super) fn pairing(&mut self, ui: &mut Ui, s: &UiState) {
        if let Some(prompt) = &s.pair_prompt {
            self.pair_prompt(ui, prompt);
        } else {
            self.view.prompt_allows = None;
            if let Some(out) = &s.pairing {
                self.pair_wait(ui, out);
            }
        }
    }

    fn pair_wait(&mut self, ui: &mut Ui, out: &OutgoingPairView) {
        let name = &out.target_name;
        let finished = matches!(out.state, PairState::Done | PairState::Failed(_));
        let r = dialogs::sheet(ui, "pair_wait", 460.0, finished, |ui| {
            let pal = Palette::of(ui.ctx());
            match &out.state {
                PairState::Connecting => {
                    dialogs::sheet_title(ui, &format!("Pairing with {name}"));
                    ui.horizontal(|ui| {
                        ui.spinner();
                        widgets::muted(ui, "Connecting…");
                    });
                }
                PairState::Confirm => {
                    dialogs::sheet_title(ui, &format!("Does {name} show this code?"));
                    if let Some(c) = &out.code {
                        code(ui, c);
                    }
                    widgets::muted(ui, accepted_text(name, out.other_accepted));
                    ui.add_space(12.0);
                    return dialogs::right_row(ui, |ui| {
                        if widgets::primary(ui, "Codes match").clicked() {
                            return Some(WaitAnswer::Confirm(true));
                        }
                        if ui.button("Cancel").clicked() {
                            return Some(WaitAnswer::Confirm(false));
                        }
                        None
                    })
                    .inner;
                }
                PairState::Waiting => {
                    dialogs::sheet_title(ui, &format!("Pairing with {name}"));
                    if let Some(c) = &out.code {
                        code(ui, c);
                    }
                    ui.horizontal(|ui| {
                        ui.spinner();
                        widgets::muted(ui, format!("Waiting for someone to accept on {name}."));
                    });
                }
                PairState::Done => {
                    dialogs::sheet_title(ui, &format!("Paired with {name}"));
                    ui.label(
                        RichText::new("You can push and pull as soon as it shows online.")
                            .color(pal.ink),
                    );
                }
                PairState::Failed(why) => {
                    dialogs::sheet_title(ui, "Pairing didn't finish");
                    ui.add(egui::Label::new(RichText::new(why).color(pal.removed)).wrap());
                }
            }
            ui.add_space(12.0);
            dialogs::right_row(ui, |ui| {
                let clicked = if finished {
                    widgets::primary(ui, "Done").clicked()
                } else {
                    ui.button("Stop waiting").clicked()
                };
                clicked.then_some(WaitAnswer::Close)
            })
            .inner
        });
        match r.inner {
            Some(WaitAnswer::Confirm(yes)) => self.act(Action::ConfirmPairCode(yes)),
            Some(WaitAnswer::Close) => self.act(Action::DismissPairing),
            None if r.dismissed => self.act(Action::DismissPairing),
            None => {}
        }
    }

    fn pair_prompt(&mut self, ui: &mut Ui, p: &PairPromptView) {
        if p.state != PromptState::Asking {
            return self.pair_prompt_after(ui, p);
        }
        let mut allows = match self.view.prompt_allows {
            Some((id, a)) if id == p.from_id => a,
            // Overwriting this computer's files is opted into here, whatever was asked.
            _ => Permissions {
                may_push_to_me: false,
                may_pull_from_me: p.requested.may_pull_from_me,
            },
        };
        let name = &p.from_name;
        let r = dialogs::sheet(ui, "pair_prompt", 560.0, false, |ui| {
            dialogs::sheet_title(ui, &format!("{name} wants to pair"));
            code(ui, &p.code);
            widgets::section(ui, &format!("What this computer allows {name}"));
            widgets::muted(ui, request_text(name, p.requested));
            ui.add_space(4.0);
            allow_boxes(ui, name, &mut allows);
            widgets::small_muted(ui, offer_text(name, p.offered));
            ui.add_space(12.0);
            dialogs::right_row(ui, |ui| {
                if widgets::primary(ui, "Pair").clicked() {
                    return Some(true);
                }
                if ui.button("Decline").clicked() {
                    return Some(false);
                }
                None
            })
            .inner
        });
        self.view.prompt_allows = Some((p.from_id, allows));
        match r.inner {
            Some(true) => self.act(Action::AnswerPair(Some(allows))),
            Some(false) => self.act(Action::AnswerPair(None)),
            None => {}
        }
    }

    /// The incoming pairing once this computer's user has accepted.
    fn pair_prompt_after(&mut self, ui: &mut Ui, p: &PairPromptView) {
        let name = &p.from_name;
        let waiting = p.state == PromptState::Waiting;
        let r = dialogs::sheet(ui, "pair_prompt", 460.0, !waiting, |ui| {
            let pal = Palette::of(ui.ctx());
            match &p.state {
                PromptState::Done => {
                    dialogs::sheet_title(ui, &format!("Paired with {name}"));
                    ui.label(RichText::new("Both computers confirmed the code.").color(pal.ink));
                }
                PromptState::Failed(why) => {
                    dialogs::sheet_title(ui, "Pairing didn't finish");
                    ui.add(egui::Label::new(RichText::new(why).color(pal.removed)).wrap());
                }
                _ => {
                    dialogs::sheet_title(ui, &format!("Pairing with {name}"));
                    code(ui, &p.code);
                    ui.horizontal(|ui| {
                        ui.spinner();
                        widgets::muted(ui, format!("Waiting for {name} to confirm the code."));
                    });
                }
            }
            ui.add_space(12.0);
            dialogs::right_row(ui, |ui| {
                if waiting {
                    ui.button("Cancel pairing").clicked()
                } else {
                    widgets::primary(ui, "Done").clicked()
                }
            })
            .inner
        });
        if r.inner || r.dismissed {
            self.act(Action::DismissPairPrompt);
        }
    }
}

enum WaitAnswer {
    Confirm(bool),
    Close,
}

fn accepted_text(name: &str, accepted: bool) -> String {
    if accepted {
        format!("Someone on {name} accepted. Confirm the code to finish pairing.")
    } else {
        format!(
            "Compare it with the code on {name}. Nothing is saved until both computers confirm."
        )
    }
}

/// What the asking computer would like this one to allow it, in one sentence.
pub fn request_text(name: &str, requested: Permissions) -> String {
    match (requested.may_push_to_me, requested.may_pull_from_me) {
        (true, true) => format!("{name} asks to be allowed to push and pull."),
        (true, false) => format!("{name} asks to be allowed to push."),
        (false, true) => format!("{name} asks to be allowed to pull."),
        (false, false) => format!("{name} does not ask to push or pull."),
    }
}

/// What the asking computer allows this one, in one sentence.
pub fn offer_text(name: &str, offered: Permissions) -> String {
    match (offered.may_push_to_me, offered.may_pull_from_me) {
        (true, true) => {
            format!("In return, {name} lets this computer push to it and pull from it.")
        }
        (true, false) => {
            format!("In return, {name} lets this computer push to it, but not pull from it.")
        }
        (false, true) => {
            format!("In return, {name} lets this computer pull from it, but not push to it.")
        }
        (false, false) => format!("{name} does not let this computer push to it or pull from it."),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_offer_reads_as_one_sentence() {
        let pull_only = Permissions {
            may_push_to_me: false,
            may_pull_from_me: true,
        };
        assert_eq!(
            offer_text("Mini", pull_only),
            "In return, Mini lets this computer pull from it, but not push to it."
        );
    }
}
