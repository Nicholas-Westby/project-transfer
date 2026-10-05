//! The app's palette and type, applied to egui's dark and light styles.

use crate::model::ThemeChoice;
use egui::epaint::Shadow;
use egui::{
    Color32, Context, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Margin, Stroke,
    TextStyle, Theme, ThemePreference, Vec2,
};
use std::sync::Arc;

pub const CONTROL_RADIUS: u8 = 6;
pub const SHEET_RADIUS: u8 = 10;

/// The family for headings and the project title.
pub fn semibold() -> FontFamily {
    FontFamily::Name("semibold".into())
}

pub fn title_style() -> TextStyle {
    TextStyle::Name("title".into())
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Palette {
    pub ground: Color32,
    pub surface: Color32,
    pub ink: Color32,
    pub route: Color32,
    pub added: Color32,
    pub changed: Color32,
    pub removed: Color32,
    pub dark: bool,
}

fn hex(rgb: u32) -> Color32 {
    Color32::from_rgb((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8)
}

/// `a` moved toward `b` by `t`, kept opaque so text over it stays crisp.
pub fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let m = |x: u8, y: u8| (f32::from(x) + (f32::from(y) - f32::from(x)) * t).round() as u8;
    Color32::from_rgb(m(a.r(), b.r()), m(a.g(), b.g()), m(a.b(), b.b()))
}

impl Palette {
    pub fn dark() -> Palette {
        Palette {
            ground: hex(0x161A1F),
            surface: hex(0x1F242B),
            ink: hex(0xE3E8EE),
            route: hex(0x6FA8E6),
            added: hex(0x5FC48C),
            changed: hex(0xE3B256),
            removed: hex(0xEE6B5D),
            dark: true,
        }
    }

    pub fn light() -> Palette {
        Palette {
            ground: hex(0xEEF1F4),
            surface: hex(0xFFFFFF),
            ink: hex(0x1C232B),
            route: hex(0x2F6FB2),
            added: hex(0x2E8B57),
            changed: hex(0xB7791F),
            removed: hex(0xC0392B),
            dark: false,
        }
    }

    pub fn of(ctx: &Context) -> Palette {
        match ctx.theme() {
            Theme::Dark => Palette::dark(),
            Theme::Light => Palette::light(),
        }
    }

    /// Secondary text: ink at about 60% over the surface.
    pub fn muted(&self) -> Color32 {
        mix(self.surface, self.ink, 0.62)
    }

    /// Placeholder text in empty fields, well below secondary text so it
    /// never reads as a typed value.
    pub fn faint(&self) -> Color32 {
        mix(self.ground, self.ink, if self.dark { 0.34 } else { 0.40 })
    }

    /// Hairlines between areas: the surface nudged toward the ink.
    pub fn border(&self) -> Color32 {
        mix(self.surface, self.ink, if self.dark { 0.14 } else { 0.16 })
    }

    /// The session bar: Route at low opacity over the ground.
    pub fn bar(&self) -> Color32 {
        mix(self.ground, self.route, if self.dark { 0.14 } else { 0.12 })
    }

    /// Text drawn on a Route or Removed fill.
    pub fn on_fill(&self) -> Color32 {
        if self.dark {
            self.ground
        } else {
            Color32::WHITE
        }
    }

    /// A quiet tint for selected rows and badges.
    pub fn route_tint(&self) -> Color32 {
        mix(self.surface, self.route, 0.18)
    }

    fn visuals(&self) -> egui::Visuals {
        let mut v = if self.dark {
            egui::Visuals::dark()
        } else {
            egui::Visuals::light()
        };
        let radius = CornerRadius::same(CONTROL_RADIUS);
        let control = mix(self.surface, self.ink, if self.dark { 0.07 } else { 0.05 });
        v.panel_fill = self.ground;
        v.window_fill = self.surface;
        v.window_stroke = Stroke::new(1.0, self.border());
        v.window_corner_radius = CornerRadius::same(SHEET_RADIUS);
        v.menu_corner_radius = radius;
        v.window_shadow = Shadow {
            offset: [0, 8],
            blur: 28,
            spread: 0,
            color: Color32::from_black_alpha(if self.dark { 110 } else { 40 }),
        };
        v.popup_shadow = Shadow {
            offset: [0, 4],
            blur: 12,
            spread: 0,
            color: Color32::from_black_alpha(if self.dark { 90 } else { 30 }),
        };
        v.hyperlink_color = self.route;
        v.selection.bg_fill = mix(self.surface, self.route, 0.35);
        v.selection.stroke = Stroke::new(1.0, self.ink);
        v.faint_bg_color = mix(self.surface, self.ink, 0.03);
        v.extreme_bg_color = self.ground;
        v.text_edit_bg_color = Some(self.ground);
        v.code_bg_color = self.ground;
        v.warn_fg_color = self.changed;
        v.error_fg_color = self.removed;
        // egui paints field hints in this color, whatever the hint asks for.
        v.weak_text_color = Some(self.faint());
        v.text_cursor.stroke = Stroke::new(2.0, self.route);
        v.text_cursor.blink = false;
        // Every button shows it can be clicked, not only the frameless ones.
        v.interact_cursor = Some(egui::CursorIcon::PointingHand);

        let w = &mut v.widgets;
        w.noninteractive.bg_fill = self.surface;
        w.noninteractive.weak_bg_fill = self.surface;
        w.noninteractive.bg_stroke = Stroke::new(1.0, self.border());
        w.noninteractive.fg_stroke = Stroke::new(1.0, self.ink);
        w.noninteractive.corner_radius = radius;
        for (state, fill, stroke) in [
            (&mut w.inactive, control, Stroke::new(1.0, self.border())),
            (
                &mut w.hovered,
                mix(control, self.ink, 0.14),
                Stroke::new(1.0, mix(self.border(), self.route, 0.8)),
            ),
            // Pressed and keyboard-focused widgets share this, so the focus
            // ring is Route.
            (
                &mut w.active,
                mix(control, self.ink, 0.12),
                Stroke::new(1.5, self.route),
            ),
            (
                &mut w.open,
                mix(control, self.ink, 0.08),
                Stroke::new(1.0, self.border()),
            ),
        ] {
            state.bg_fill = fill;
            state.weak_bg_fill = fill;
            state.bg_stroke = stroke;
            state.fg_stroke = Stroke::new(1.0, self.ink);
            state.corner_radius = radius;
            state.expansion = 0.0;
        }
        v
    }
}

fn style_for(p: Palette) -> egui::Style {
    let mut style = egui::Style {
        visuals: p.visuals(),
        ..Default::default()
    };
    let prop = |size| FontId::new(size, FontFamily::Proportional);
    style.text_styles = [
        (TextStyle::Small, prop(12.0)),
        (TextStyle::Body, prop(14.0)),
        (TextStyle::Button, prop(14.0)),
        (TextStyle::Heading, FontId::new(20.0, semibold())),
        (
            TextStyle::Monospace,
            FontId::new(13.0, FontFamily::Monospace),
        ),
        (title_style(), FontId::new(24.0, semibold())),
    ]
    .into();
    let s = &mut style.spacing;
    s.item_spacing = Vec2::new(8.0, 6.0);
    s.button_padding = Vec2::new(12.0, 5.0);
    s.interact_size.y = 26.0;
    s.window_margin = Margin::same(24);
    s.menu_margin = Margin::same(6);
    s.icon_width = 16.0;
    s.icon_width_inner = 9.0;
    s.text_edit_width = 320.0;
    // Solid bars take their own room and show whenever content overflows,
    // so a clipped list always looks scrollable.
    s.scroll = egui::style::ScrollStyle::solid();
    style.animation_time = 0.0;
    style
}

fn fonts() -> FontDefinitions {
    let mut f = FontDefinitions::default();
    let add = |f: &mut FontDefinitions, name: &str, bytes: &'static [u8]| {
        f.font_data
            .insert(name.into(), Arc::new(FontData::from_static(bytes)));
    };
    add(
        &mut f,
        "inter",
        include_bytes!("../../assets/fonts/Inter-Regular.ttf"),
    );
    add(
        &mut f,
        "inter-semibold",
        include_bytes!("../../assets/fonts/Inter-SemiBold.ttf"),
    );
    add(
        &mut f,
        "jetbrains-mono",
        include_bytes!("../../assets/fonts/JetBrainsMono-Regular.ttf"),
    );
    // egui's own fonts stay behind ours for glyphs Inter lacks, such as ⇄.
    let fallbacks = f.families[&FontFamily::Proportional].clone();
    let chain = |first: &[&str]| {
        let mut v: Vec<String> = first.iter().map(|s| s.to_string()).collect();
        v.extend(fallbacks.iter().cloned());
        v
    };
    let prop = chain(&["inter"]);
    let semi = chain(&["inter-semibold", "inter"]);
    let mono = chain(&["jetbrains-mono"]);
    f.families.insert(FontFamily::Proportional, prop);
    f.families.insert(semibold(), semi);
    f.families.insert(FontFamily::Monospace, mono);
    f
}

/// Installs fonts and both styles; call once per context.
pub fn install(ctx: &Context) {
    ctx.set_fonts(fonts());
    ctx.set_style_of(Theme::Dark, style_for(Palette::dark()));
    ctx.set_style_of(Theme::Light, style_for(Palette::light()));
}

pub fn preference(choice: ThemeChoice) -> ThemePreference {
    match choice {
        ThemeChoice::Dark => ThemePreference::Dark,
        ThemeChoice::Light => ThemePreference::Light,
        ThemeChoice::System => ThemePreference::System,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn palettes_use_the_spec_values() {
        let d = Palette::dark();
        assert_eq!(d.ground, Color32::from_rgb(0x16, 0x1A, 0x1F));
        assert_eq!(d.route, Color32::from_rgb(0x6F, 0xA8, 0xE6));
        assert_eq!(d.removed, Color32::from_rgb(0xEE, 0x6B, 0x5D));
        let l = Palette::light();
        assert_eq!(l.surface, Color32::WHITE);
        assert_eq!(l.changed, Color32::from_rgb(0xB7, 0x79, 0x1F));
    }

    #[test]
    fn mixing_lands_between_the_two_colors() {
        let m = mix(Color32::BLACK, Color32::WHITE, 0.5);
        assert_eq!(m, Color32::from_rgb(128, 128, 128));
        let muted = Palette::dark().muted();
        assert!(muted.r() > Palette::dark().surface.r() && muted.r() < Palette::dark().ink.r());
    }
}
