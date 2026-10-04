//! Settings: this computer's name, the Projects folder, the theme, the
//! ignore list and the log folder.

use super::theme::Palette;
use super::widgets::{self, display_path};
use super::{App, Sheet, dialogs};
use crate::core::{Action, UiState};
use crate::ignore_rules::DEFAULT_IGNORES;
use crate::model::{InstanceSettings, ThemeChoice};
use egui::{Grid, ScrollArea, TextEdit, Ui};
use std::path::PathBuf;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Draft {
    pub name: String,
    pub projects_folder: PathBuf,
    pub extra: Vec<String>,
    pub always_include: Vec<String>,
    pub removed_defaults: Vec<String>,
    pub new_pattern: String,
    pub new_include: String,
}

impl From<&InstanceSettings> for Draft {
    fn from(s: &InstanceSettings) -> Draft {
        Draft {
            name: s.name.clone(),
            projects_folder: s.projects_folder.clone(),
            extra: s.extra_ignores.clone(),
            always_include: s.always_include.clone(),
            removed_defaults: s.removed_default_ignores.clone(),
            new_pattern: String::new(),
            new_include: String::new(),
        }
    }
}

impl Draft {
    /// The actions that turn `me` into this draft; nothing for unchanged parts.
    pub fn actions(&self, me: &InstanceSettings) -> Vec<Action> {
        let mut out = Vec::new();
        let name = self.name.trim();
        if !name.is_empty() && name != me.name {
            out.push(Action::Rename(name.to_string()));
        }
        if self.projects_folder != me.projects_folder {
            out.push(Action::SetProjectsFolder(self.projects_folder.clone()));
        }
        if self.extra != me.extra_ignores
            || self.always_include != me.always_include
            || self.removed_defaults != me.removed_default_ignores
        {
            out.push(Action::SetIgnores {
                extra: self.extra.clone(),
                always_include: self.always_include.clone(),
                removed_defaults: self.removed_defaults.clone(),
            });
        }
        out
    }
}

impl App {
    pub(super) fn settings_sheet(&mut self, ui: &mut Ui, s: &UiState, mut d: Draft) {
        let r = dialogs::sheet(ui, "settings", 640.0, true, |ui| {
            dialogs::sheet_title(ui, "Settings");
            ScrollArea::vertical()
                .max_height((ui.ctx().content_rect().height() - 230.0).max(240.0))
                .auto_shrink([false, true])
                .show(ui, |ui| self.settings_body(ui, s, &mut d));
            dialogs::button_row(ui, "Save settings", false, !d.name.trim().is_empty())
        });
        match (r.dismissed, r.inner) {
            (true, _) | (_, Some(false)) => self.view.sheet = Sheet::None,
            (_, Some(true)) => {
                for a in d.actions(&s.me) {
                    self.act(a);
                }
                self.view.sheet = Sheet::None;
            }
            _ => {
                if matches!(self.view.sheet, Sheet::Settings(_)) {
                    self.view.sheet = Sheet::Settings(d);
                }
            }
        }
    }

    fn settings_body(&mut self, ui: &mut Ui, s: &UiState, d: &mut Draft) {
        let pal = Palette::of(ui.ctx());
        widgets::section(ui, "This computer");
        widgets::muted(ui, "Name");
        ui.add(TextEdit::singleline(&mut d.name).desired_width(320.0));
        widgets::small_muted(ui, "Other computers see this name.");
        ui.add_space(6.0);
        widgets::muted(ui, "Address");
        let here = super::reach_text(&self.view.my_addresses, s.port);
        ui.label(super::preview_view::ticks(&here, pal.ink));
        ui.add_space(10.0);
        widgets::muted(ui, "Projects folder");
        ui.horizontal(|ui| {
            ui.label(widgets::mono(display_path(&d.projects_folder)).color(pal.ink));
            if widgets::quiet(ui, "Choose folder", Some(pal.route)).clicked()
                && let Some(p) = self.pick_folder("Choose the Projects folder")
            {
                d.projects_folder = p;
            }
        });
        widgets::small_muted(
            ui,
            "Projects that arrive from another computer go here unless you choose a folder.",
        );

        ui.add_space(16.0);
        widgets::section(ui, "Appearance");
        ui.horizontal(|ui| {
            for (choice, label) in [
                (ThemeChoice::Dark, "Dark"),
                (ThemeChoice::Light, "Light"),
                (ThemeChoice::System, "Match system"),
            ] {
                if ui.selectable_label(s.me.theme == choice, label).clicked()
                    && s.me.theme != choice
                {
                    self.act(Action::SetTheme(choice));
                }
            }
        });

        ui.add_space(16.0);
        widgets::section(ui, "Ignore list");
        widgets::small_muted(
            ui,
            "Ticked patterns are left out of transfers. Changes apply to the next transfer.",
        );
        ui.add_space(4.0);
        Grid::new("defaults")
            .num_columns(4)
            .spacing([24.0, 4.0])
            .show(ui, |ui| {
                // The transfer's own temp files are always skipped; not a choice.
                let shown = DEFAULT_IGNORES.iter().filter(|p| !p.ends_with(".pt-tmp"));
                for (i, pat) in shown.enumerate() {
                    let mut on = !d.removed_defaults.iter().any(|r| r == pat);
                    if ui.checkbox(&mut on, widgets::mono(*pat)).changed() {
                        if on {
                            d.removed_defaults.retain(|r| r != pat);
                        } else {
                            d.removed_defaults.push(pat.to_string());
                        }
                    }
                    if i % 4 == 3 {
                        ui.end_row();
                    }
                }
            });
        ui.add_space(8.0);
        pattern_list(
            ui,
            "Your patterns",
            &mut d.extra,
            &mut d.new_pattern,
            "*.log",
            "Add pattern",
        );
        ui.add_space(8.0);
        pattern_list(
            ui,
            "Always include",
            &mut d.always_include,
            &mut d.new_include,
            ".env.example",
            "Add to always include",
        );
        widgets::small_muted(ui, "Always include wins over every ignore pattern.");

        ui.add_space(16.0);
        widgets::section(ui, "Logs");
        ui.horizontal(|ui| {
            if ui.button("Open log folder").clicked() {
                self.act(Action::OpenLogFolder);
            }
            widgets::small_muted(
                ui,
                "Every connection, transfer and command run is logged there.",
            );
        });
    }
}

fn pattern_list(
    ui: &mut Ui,
    title: &str,
    list: &mut Vec<String>,
    draft: &mut String,
    hint: &str,
    add_label: &str,
) {
    let pal = Palette::of(ui.ctx());
    widgets::muted(ui, title);
    let mut remove = None;
    for (i, pat) in list.iter().enumerate() {
        ui.horizontal(|ui| {
            ui.label(widgets::mono(pat).color(pal.ink));
            if widgets::quiet(ui, "Remove", Some(pal.removed)).clicked() {
                remove = Some(i);
            }
        });
    }
    if let Some(i) = remove {
        list.remove(i);
    }
    ui.horizontal(|ui| {
        let r = ui.add(
            TextEdit::singleline(draft)
                .hint_text(widgets::hint(hint).monospace())
                .font(egui::TextStyle::Monospace)
                .desired_width(220.0),
        );
        let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        let ok = !draft.trim().is_empty() && !list.iter().any(|p| p == draft.trim());
        if (ui.add_enabled(ok, egui::Button::new(add_label)).clicked() || enter && ok) && ok {
            list.push(draft.trim().to_string());
            draft.clear();
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn me() -> InstanceSettings {
        InstanceSettings {
            id: uuid::Uuid::nil(),
            name: "Studio".into(),
            projects_folder: "/dev".into(),
            extra_ignores: vec![],
            always_include: vec![],
            removed_default_ignores: vec![],
            last_peer: None,
            theme: ThemeChoice::Dark,
            port: 0,
        }
    }

    #[test]
    fn an_unchanged_draft_sends_nothing() {
        assert!(Draft::from(&me()).actions(&me()).is_empty());
    }

    #[test]
    fn only_changed_parts_are_sent() {
        let mut d = Draft::from(&me());
        d.name = "  Desk  ".into();
        d.extra.push("*.log".into());
        let a = d.actions(&me());
        assert_eq!(a[0], Action::Rename("Desk".into()));
        assert!(matches!(a[1], Action::SetIgnores { .. }));
        assert_eq!(a.len(), 2);
    }
}
