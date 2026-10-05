//! The left column: every project, and the way to add one.

use super::theme::{CONTROL_RADIUS, Palette, semibold};
use super::widgets::{self, display_path};
use super::{App, Sheet, dialogs};
use crate::core::{Action, UiState};
use egui::{Button, CornerRadius, FontId, Frame, Margin, Panel, RichText, ScrollArea, Ui, Vec2};
use std::path::PathBuf;

impl App {
    pub(super) fn projects_list(&mut self, ui: &mut Ui, s: &UiState) {
        let pal = Palette::of(ui.ctx());
        Panel::left("projects")
            .resizable(false)
            .exact_size(230.0)
            .show_separator_line(false)
            .frame(
                Frame::new()
                    .fill(pal.ground)
                    .inner_margin(Margin::symmetric(12, 18)),
            )
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.add_space(8.0);
                    ui.label(
                        RichText::new("Projects")
                            .font(FontId::new(15.0, semibold()))
                            .color(pal.muted()),
                    );
                });
                ui.add_space(6.0);
                ScrollArea::vertical()
                    .auto_shrink([false, true])
                    .max_height(ui.available_height() - 48.0)
                    .show(ui, |ui| {
                        for p in &s.projects {
                            let selected = self.view.selected == Some(p.id);
                            if project_row(ui, &p.name, selected).clicked() {
                                self.view.selected = Some(p.id);
                                self.view.rename_project = None;
                            }
                        }
                        self.remote_projects(ui, s);
                    });
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.add_space(4.0);
                    if ui.button("New project").clicked() {
                        self.start_new_project();
                    }
                });
            });
    }

    pub(super) fn start_new_project(&mut self) {
        if let Some(folder) = self.pick_folder("Choose the project's folder") {
            let name = folder_name(&folder);
            self.view.sheet = Sheet::NewProject { name, folder };
        }
    }

    pub(super) fn new_project_sheet(&mut self, ui: &mut Ui, mut name: String, mut folder: PathBuf) {
        let r = dialogs::sheet(ui, "new_project", 460.0, true, |ui| {
            dialogs::sheet_title(ui, "New project");
            widgets::muted(ui, "Folder on this computer");
            ui.horizontal(|ui| {
                ui.label(widgets::mono(display_path(&folder)));
            });
            if widgets::quiet(ui, "Choose another folder", None).clicked()
                && let Some(f) = self.pick_folder("Choose the project's folder")
            {
                if name == folder_name(&folder) {
                    name = folder_name(&f);
                }
                folder = f;
            }
            ui.add_space(10.0);
            widgets::muted(ui, "Name");
            let edit = ui.add(egui::TextEdit::singleline(&mut name).desired_width(f32::INFINITY));
            if edit.gained_focus() || !edit.has_focus() && ui.memory(|m| m.focused().is_none()) {
                edit.request_focus();
            }
            let enter = edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            widgets::small_muted(
                ui,
                "Other computers get this name too. You can add more folders later.",
            );
            let problem = name_problem(&name, &folder);
            if let Some(why) = &problem {
                widgets::error_text(ui, why);
            }
            let ok = problem.is_none() && !name.trim().is_empty();
            match dialogs::button_row(ui, "Create project", false, ok) {
                Some(answer) => Some(answer),
                None if enter && ok => Some(true),
                None => None,
            }
        });
        match (r.dismissed, r.inner) {
            (true, _) | (_, Some(false)) => self.view.sheet = Sheet::None,
            (_, Some(true)) => {
                self.act(Action::CreateProject {
                    name: name.trim().to_string(),
                    folder,
                });
                self.view.sheet = Sheet::None;
                self.view.select_when_added = Some(name.trim().to_string());
            }
            _ => self.view.sheet = Sheet::NewProject { name, folder },
        }
    }
}

/// Why the project can't be created as entered, for an inline error. Any
/// name will do; a blank one only disables the button, so only the folder's
/// name, which other computers use as a folder name, is checked.
fn name_problem(name: &str, folder: &std::path::Path) -> Option<String> {
    if name.trim().is_empty() {
        return None;
    }
    crate::naming::name_problem("Folder", &folder_name(folder))
        .map(|why| format!("{why} Rename the folder on disk or choose another one."))
}

fn folder_name(p: &std::path::Path) -> String {
    p.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn project_row(ui: &mut Ui, name: &str, selected: bool) -> egui::Response {
    let pal = Palette::of(ui.ctx());
    let fill = if selected {
        pal.route_tint()
    } else {
        egui::Color32::TRANSPARENT
    };
    let color = if selected { pal.ink } else { pal.muted() };
    let width = ui.available_width();
    widgets::with_fill(ui, fill, |ui| {
        ui.add(
            // The trailing grow atom keeps the name on the left of a wide row.
            Button::new((RichText::new(name).color(color), egui::Atom::grow()))
                .corner_radius(CornerRadius::same(CONTROL_RADIUS))
                .min_size(Vec2::new(width, 30.0))
                .wrap_mode(egui::TextWrapMode::Truncate),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::name_problem;
    use std::path::Path;

    #[test]
    fn a_trailing_space_while_typing_a_project_name_is_no_problem() {
        let folder = Path::new("/work/app");
        assert_eq!(name_problem("My ", folder), None);
        assert_eq!(name_problem("What Next?", folder), None);
        // The folder name is used as it is on disk.
        assert!(name_problem("My", Path::new("/work/app ")).is_some());
    }
}
