//! Modal sheets and the one confirmation dialog every risky action uses.

use super::theme::{Palette, SHEET_RADIUS};
use super::widgets;
use egui::{Align, Frame, Id, Layout, Margin, Modal, RichText, Ui};
use std::hash::Hash;

pub const CONFIRM_WIDTH: f32 = 440.0;

pub fn sheet_frame(ui: &Ui) -> Frame {
    let pal = Palette::of(ui.ctx());
    Frame::new()
        .fill(pal.surface)
        .stroke(egui::Stroke::new(1.0, pal.border()))
        .corner_radius(SHEET_RADIUS)
        .inner_margin(Margin::same(24))
        .shadow(ui.visuals().window_shadow)
}

/// What a sheet's content asked for this frame.
pub struct SheetResponse<T> {
    pub inner: T,
    /// Escape or a click on the backdrop.
    pub dismissed: bool,
}

/// A modal sheet `width` wide. `dismissable` lets Escape and the backdrop
/// close it; a sheet in the middle of work turns that off.
pub fn sheet<T>(
    ui: &Ui,
    id: impl Hash + std::fmt::Debug,
    width: f32,
    dismissable: bool,
    content: impl FnOnce(&mut Ui) -> T,
) -> SheetResponse<T> {
    let pal = Palette::of(ui.ctx());
    let backdrop = egui::Color32::from_black_alpha(if pal.dark { 150 } else { 90 });
    let id = Id::new(id);
    // Pinned near the top rather than centred, so a sheet whose content
    // grows or shrinks never moves under the pointer.
    let top = (ui.ctx().content_rect().height() * 0.08).clamp(24.0, 72.0);
    // An area's content may only use the size it had last frame, so the
    // first frame offers the whole height; the sheet then shrinks to fit.
    let tall = ui.ctx().content_rect().height() - top - 16.0;
    let area = Modal::default_area(id)
        .anchor(egui::Align2::CENTER_TOP, egui::vec2(0.0, top))
        .default_size(egui::vec2(width + 48.0, tall));
    let r = Modal::new(id)
        .area(area)
        .frame(sheet_frame(ui))
        .backdrop_color(backdrop)
        .show(ui.ctx(), |ui| {
            ui.set_width(width);
            content(ui)
        });
    let dismissed = dismissable && r.should_close();
    SheetResponse {
        inner: r.inner,
        dismissed,
    }
}

/// A title line for a sheet, with space under it.
pub fn sheet_title(ui: &mut Ui, title: &str) {
    widgets::heading(ui, title);
    ui.add_space(8.0);
}

/// A one-line row of buttons on the right edge. Its height is fixed, so a
/// sheet that remembers a larger size never pushes the buttons down.
pub fn right_row<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> egui::InnerResponse<R> {
    let size = egui::vec2(ui.available_width(), ui.spacing().interact_size.y + 4.0);
    ui.allocate_ui_with_layout(size, Layout::right_to_left(Align::Center), add)
}

/// Cancel on the left of the confirm button, both on the right edge.
/// Returns Some(true) for confirm, Some(false) for cancel.
pub fn button_row(
    ui: &mut Ui,
    confirm_label: &str,
    destructive: bool,
    enabled: bool,
) -> Option<bool> {
    let pal = Palette::of(ui.ctx());
    let fill = if destructive { pal.removed } else { pal.route };
    ui.add_space(12.0);
    right_row(ui, |ui| {
        if widgets::filled(ui, enabled, confirm_label, fill, false).clicked() {
            return Some(true);
        }
        if ui.button("Cancel").clicked() {
            return Some(false);
        }
        None
    })
    .inner
}

/// Asks before a risky action. Some(true) once confirmed, Some(false) on
/// Cancel, Escape or a click outside, None while it is still open.
pub fn confirm(
    ui: &mut Ui,
    id: impl Hash + std::fmt::Debug,
    title: &str,
    body: &str,
    confirm_label: &str,
    destructive: bool,
) -> Option<bool> {
    confirm_with(ui, id, title, body, confirm_label, destructive, |_| {})
}

/// `confirm` with extra content under the body, such as a command's text.
pub fn confirm_with(
    ui: &mut Ui,
    id: impl Hash + std::fmt::Debug,
    title: &str,
    body: &str,
    confirm_label: &str,
    destructive: bool,
    extra: impl FnOnce(&mut Ui),
) -> Option<bool> {
    let r = sheet(ui, id, CONFIRM_WIDTH, true, |ui| {
        sheet_title(ui, title);
        let ink = Palette::of(ui.ctx()).ink;
        ui.label(RichText::new(body).color(ink));
        extra(ui);
        button_row(ui, confirm_label, destructive, true)
    });
    if r.dismissed {
        return Some(false);
    }
    r.inner
}
