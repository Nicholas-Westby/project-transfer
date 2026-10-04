//! The strip across the top: this computer, the selected peer, and the way
//! to Settings. It is the one place the window uses Route as a fill.

use super::theme::{Palette, semibold};
use super::widgets::{self, ago, dot, this_computer};
use super::{App, Sheet, settings};
use crate::core::{Action, PeerView, UiState};
use crate::transfer::projects::now_ms;
use egui::{
    Align, Button, FontId, Frame, Key, Layout, Margin, Panel, Popup, RichText, Sense, TextEdit, Ui,
    WidgetInfo, WidgetType,
};

/// "Online", "Offline, last seen 3 min ago" and so on.
pub fn peer_status(p: &PeerView, now: i64) -> String {
    if p.online {
        "Online".into()
    } else if let Some(seen) = p.last_seen_ms {
        format!("Offline, last seen {}", ago(now, seen))
    } else {
        "Offline".into()
    }
}

const VERSION: &str = concat!("v", env!("CARGO_PKG_VERSION"));

impl App {
    pub(super) fn session_bar(&mut self, ui: &mut Ui, s: &UiState) {
        let pal = Palette::of(ui.ctx());
        let frame = Frame::new()
            .fill(pal.bar())
            .inner_margin(Margin::symmetric(16, 10));
        Panel::top("session_bar")
            .frame(frame)
            .resizable(false)
            .show_separator_line(false)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.set_min_height(30.0);
                    self.my_name(ui, s);
                    ui.add_space(14.0);
                    widgets::swap_glyph(ui, pal.route);
                    ui.add_space(14.0);
                    self.peer_picker(ui, s);
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if widgets::icon_button(ui, "⚙", "Settings").clicked() {
                            self.view.sheet = Sheet::Settings(settings::Draft::from(&s.me));
                        }
                        // Counts up with every commit, so a screenshot says
                        // which build is running.
                        widgets::small_muted(ui, VERSION)
                            .on_hover_text("The version of Project Transfer on this computer.");
                    });
                });
            });
    }

    fn my_name(&mut self, ui: &mut Ui, s: &UiState) {
        let pal = Palette::of(ui.ctx());
        let name_font = FontId::new(15.0, semibold());
        if let Some(mut draft) = self.view.rename_me.take() {
            let r = ui.add(
                TextEdit::singleline(&mut draft)
                    .font(name_font)
                    .desired_width(220.0),
            );
            r.request_focus();
            let enter = ui.input(|i| i.key_pressed(Key::Enter));
            let escape = ui.input(|i| i.key_pressed(Key::Escape));
            if escape {
                return;
            }
            if enter || r.lost_focus() {
                if draft.trim() != s.me.name && !draft.trim().is_empty() {
                    self.act(Action::Rename(draft.trim().to_string()));
                }
                return;
            }
            self.view.rename_me = Some(draft);
            return;
        }
        let r = ui
            .add(
                egui::Label::new(RichText::new(&s.me.name).font(name_font).color(pal.ink))
                    .sense(Sense::click()),
            )
            .on_hover_cursor(egui::CursorIcon::Text)
            .on_hover_text("Click to rename this computer");
        if r.clicked() {
            self.view.rename_me = Some(s.me.name.clone());
        }
        widgets::muted(ui, format!("({})", this_computer()));
    }

    fn peer_picker(&mut self, ui: &mut Ui, s: &UiState) {
        let pal = Palette::of(ui.ctx());
        let selected = s.selected_peer.and_then(|id| s.peer(id));
        let (dot_color, label) = match selected {
            Some(p) if p.online => (pal.added, p.peer.name.clone()),
            Some(p) => (pal.muted(), p.peer.name.clone()),
            None if s.peers.is_empty() => (pal.muted(), "Pair a computer".to_string()),
            None => (pal.muted(), "Choose a computer".to_string()),
        };
        dot(ui, dot_color);
        // Trailing room for the drawn chevron.
        let text = RichText::new(format!("{label}     "))
            .font(FontId::new(15.0, semibold()))
            .color(pal.ink);
        let button = ui
            .add(Button::new(text).frame(false))
            .on_hover_cursor(egui::CursorIcon::PointingHand);
        let at = egui::pos2(button.rect.right() - 9.0, button.rect.center().y);
        widgets::chevron(ui, at, true, pal.muted());
        button.widget_info(|| WidgetInfo::labeled(WidgetType::ComboBox, true, "Choose a computer"));
        let now = now_ms();
        let status = match selected {
            Some(p) => peer_status(p, now),
            None if s.peers.is_empty() => "Not paired yet".into(),
            None => "No computer selected".into(),
        };
        ui.add_space(6.0);
        widgets::muted(ui, status);

        Popup::menu(&button).width(300.0).show(|ui| {
            ui.set_min_width(280.0);
            if s.peers.is_empty() {
                widgets::small_muted(ui, "No paired computers yet.");
            }
            for p in &s.peers {
                let is_selected = Some(p.peer.id) == s.selected_peer;
                let r = ui
                    .horizontal(|ui| {
                        dot(ui, if p.online { pal.added } else { pal.muted() });
                        let text = RichText::new(&p.peer.name).color(pal.ink);
                        let r = ui.add(Button::selectable(is_selected, text));
                        widgets::small_muted(ui, peer_status(p, now));
                        r
                    })
                    .inner;
                if r.clicked() && !is_selected {
                    self.act(Action::SelectPeer(p.peer.id));
                }
            }
            ui.separator();
            if ui.button("Pair another computer…").clicked() {
                self.view.sheet = Sheet::Computers;
            }
            if !s.peers.is_empty() && ui.button("Manage paired computers…").clicked() {
                self.view.sheet = Sheet::Computers;
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Peer, Permissions};

    fn view(online: bool, seen: Option<i64>) -> PeerView {
        PeerView {
            peer: Peer {
                id: uuid::Uuid::nil(),
                name: "Desktop".into(),
                fingerprint: String::new(),
                allows: Permissions::default(),
                granted: Permissions::default(),
                last_address: None,
                via: None,
            },
            online,
            address: None,
            last_seen_ms: seen,
        }
    }

    #[test]
    fn status_says_when_the_peer_was_last_seen() {
        let now = 1_000_000_000;
        assert_eq!(peer_status(&view(true, Some(now)), now), "Online");
        assert_eq!(
            peer_status(&view(false, Some(now - 180_000)), now),
            "Offline, last seen 3 min ago"
        );
        assert_eq!(peer_status(&view(false, None), now), "Offline");
    }
}
