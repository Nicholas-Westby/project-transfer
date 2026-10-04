//! Computers on this network and paired ones: pair, unpair, permissions,
//! and adding one by address when discovery can't see it.

use super::session_bar::peer_status;
use super::theme::Palette;
use super::widgets::{self, dot};
use super::{App, Confirm, Sheet, dialogs};
use crate::core::{Action, PeerView, UiState};
use crate::model::Permissions;
use crate::transfer::projects::now_ms;
use egui::{Align, Key, Layout, RichText, ScrollArea, TextEdit, Ui};

pub const NO_PEERS: &str =
    "Open Project Transfer on another computer on this network. It will show up here.";

/// Pull only, both ways: overwriting files is something each side opts into.
const PULL_ONLY: Permissions = Permissions {
    may_push_to_me: false,
    may_pull_from_me: true,
};

impl App {
    pub(super) fn computers_sheet(&mut self, ui: &mut Ui, s: &UiState) {
        let r = dialogs::sheet(ui, "computers", 560.0, true, |ui| {
            dialogs::sheet_title(ui, "Computers");
            ScrollArea::vertical()
                .max_height((ui.ctx().content_rect().height() - 230.0).max(240.0))
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    // Room for the scroll bar, so right-aligned buttons stay clear of it.
                    egui::Frame::new()
                        .inner_margin(egui::Margin {
                            right: 16,
                            ..Default::default()
                        })
                        .show(ui, |ui| {
                            self.discovered_list(ui, s);
                            ui.add_space(14.0);
                            self.add_by_address(ui, s);
                            ui.add_space(18.0);
                            self.paired_list(ui, s);
                        });
                });
            ui.add_space(12.0);
            dialogs::right_row(ui, |ui| ui.button("Close").clicked()).inner
        });
        if (r.inner || r.dismissed) && self.view.sheet == Sheet::Computers {
            self.view.sheet = Sheet::None;
        }
    }

    fn discovered_list(&mut self, ui: &mut Ui, s: &UiState) {
        widgets::section(ui, "On this network");
        let new: Vec<_> = s
            .discovered
            .iter()
            .filter(|d| d.id != s.me.id && s.peer(d.id).is_none())
            .collect();
        if new.is_empty() {
            ui.horizontal(|ui| {
                ui.spinner();
                widgets::muted(ui, NO_PEERS);
            });
            return;
        }
        for d in new {
            ui.horizontal(|ui| {
                ui.label(RichText::new(&d.name).color(Palette::of(ui.ctx()).ink));
                if let Some(a) = d.addrs.first() {
                    ui.label(
                        widgets::mono(a.to_string())
                            .small()
                            .color(Palette::of(ui.ctx()).muted()),
                    );
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if widgets::primary(ui, "Pair").clicked() {
                        self.view.sheet = Sheet::PairSetup {
                            target: d.clone(),
                            requested: PULL_ONLY,
                            offered: PULL_ONLY,
                        };
                    }
                });
            });
        }
    }

    fn paired_list(&mut self, ui: &mut Ui, s: &UiState) {
        widgets::section(ui, "Paired");
        if s.peers.is_empty() {
            widgets::muted(ui, "No paired computers yet. Pair one from the list above.");
            return;
        }
        widgets::small_muted(
            ui,
            "Each computer decides what it allows. Change these on the computer whose files are at stake.",
        );
        for p in &s.peers {
            ui.add_space(8.0);
            self.paired_row(ui, p);
        }
    }

    fn paired_row(&mut self, ui: &mut Ui, p: &PeerView) {
        let pal = Palette::of(ui.ctx());
        let name = &p.peer.name;
        ui.horizontal(|ui| {
            dot(ui, if p.online { pal.added } else { pal.muted() });
            ui.label(RichText::new(name).strong().color(pal.ink));
            widgets::small_muted(ui, peer_status(p, now_ms()));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if widgets::quiet(ui, "Unpair", Some(pal.removed)).clicked() {
                    self.view.confirm = Some(Confirm::Unpair(p.peer.id));
                }
            });
        });
        let old = p.peer.allows;
        let mut new = old;
        ui.indent(("allows", p.peer.id), |ui| {
            ui.checkbox(
                &mut new.may_push_to_me,
                "Can push to this computer (overwrites files here)",
            );
            ui.checkbox(
                &mut new.may_pull_from_me,
                "Can pull from this computer (reads files here)",
            );
            let offer = super::pairing::offer_text(name, p.peer.granted);
            widgets::small_muted(ui, offer.trim_start_matches("In return, "));
        });
        if new != old {
            let revokes = (old.may_push_to_me && !new.may_push_to_me)
                || (old.may_pull_from_me && !new.may_pull_from_me);
            if revokes {
                self.view.confirm = Some(Confirm::Revoke(p.peer.id, new));
            } else {
                self.act(Action::SetAllows(p.peer.id, new));
            }
        }
    }

    fn add_by_address(&mut self, ui: &mut Ui, s: &UiState) {
        let pal = Palette::of(ui.ctx());
        widgets::muted(ui, "Not listed? Add it by address");
        let here = super::reach_text(&self.view.my_addresses, s.port);
        let text = format!("Type the address the other computer shows in its Settings. {here}");
        ui.add(egui::Label::new(super::preview_view::ticks(&text, pal.muted())).wrap());
        let mut send = false;
        ui.horizontal(|ui| {
            let r = ui.add(
                TextEdit::singleline(&mut self.view.address)
                    .hint_text(widgets::hint("192.168.1.20"))
                    .font(egui::TextStyle::Monospace)
                    .desired_width(260.0),
            );
            send |= r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
            let ok = !self.view.address.trim().is_empty();
            send |= ui
                .add_enabled(ok, egui::Button::new("Add computer"))
                .clicked();
        });
        if send && !self.view.address.trim().is_empty() {
            self.act(Action::AddByAddress(self.view.address.trim().to_string()));
        }
        if let Some(e) = &s.address_error {
            ui.add(egui::Label::new(RichText::new(e).color(pal.removed)).wrap());
        }
    }
}
