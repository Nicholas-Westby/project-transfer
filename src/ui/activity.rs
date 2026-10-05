//! The strip at the bottom: what the app has done this session.

use super::App;
use super::theme::Palette;
use super::widgets::{self, ago};
use crate::core::{ActivityKind, UiState};
use crate::transfer::projects::now_ms;
use egui::{Button, Frame, Margin, Panel, RichText, ScrollArea, Ui};

/// The strip shows the most recent lines; the log file keeps the rest.
const SHOWN: usize = 200;

impl App {
    pub(super) fn activity(&mut self, ui: &mut Ui, s: &UiState) {
        let pal = Palette::of(ui.ctx());
        let color = |k: ActivityKind| match k {
            ActivityKind::Info => pal.ink,
            ActivityKind::Warn => pal.changed,
            ActivityKind::Error => pal.removed,
        };
        Panel::bottom("activity")
            .resizable(false)
            .show_separator_line(false)
            .frame(
                Frame::new()
                    .fill(pal.ground)
                    .inner_margin(Margin::symmetric(16, 8)),
            )
            .show(ui, |ui| {
                let now = now_ms();
                ui.horizontal(|ui| {
                    let r = widgets::frameless(
                        ui,
                        Button::new(RichText::new("     Activity").color(pal.muted())),
                    );
                    let at = egui::pos2(r.rect.left() + 9.0, r.rect.center().y);
                    widgets::chevron(ui, at, self.view.activity_open, pal.muted());
                    if r.clicked() {
                        self.view.activity_open = !self.view.activity_open;
                    }
                    if !self.view.activity_open
                        && let Some(last) = s.activity.last()
                    {
                        ui.add(
                            egui::Label::new(RichText::new(&last.text).color(color(last.kind)))
                                .truncate(),
                        );
                    }
                });
                if !self.view.activity_open {
                    return;
                }
                ScrollArea::vertical()
                    .max_height(170.0)
                    .stick_to_bottom(true)
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        if s.activity.is_empty() {
                            widgets::small_muted(ui, "Nothing has happened yet this session.");
                        }
                        let start = s.activity.len().saturating_sub(SHOWN);
                        for line in &s.activity[start..] {
                            ui.horizontal(|ui| {
                                let (rect, _) = ui.allocate_exact_size(
                                    egui::vec2(80.0, 16.0),
                                    egui::Sense::hover(),
                                );
                                ui.painter().text(
                                    rect.left_center(),
                                    egui::Align2::LEFT_CENTER,
                                    ago(now, line.at_ms),
                                    egui::FontId::proportional(12.0),
                                    pal.muted(),
                                );
                                ui.label(RichText::new(&line.text).color(color(line.kind)));
                            });
                        }
                    });
            });
    }
}
