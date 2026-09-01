//! AURORA design system — colors, typography, chrome.
//! Faithful to the reference: deep navy chrome, cyan accent, soft cards.

use egui::{Color32, Context, FontId, Rounding, Style, Visuals};

pub struct Theme;

impl Theme {
    // base surfaces
    pub const BG: Color32 = Color32::from_rgb(10, 14, 23); // #0A0E17
    pub const PANEL: Color32 = Color32::from_rgb(13, 18, 32); // #0D1220
    pub const PANEL2: Color32 = Color32::from_rgb(17, 24, 39); // #111827
    pub const CARD: Color32 = Color32::from_rgb(21, 29, 46); // #151D2E
    pub const CARD_HI: Color32 = Color32::from_rgb(27, 37, 58);
    pub const BORDER: Color32 = Color32::from_rgb(31, 42, 61); // #1F2A3D
    pub const BORDER_HI: Color32 = Color32::from_rgb(45, 60, 88);

    // text
    pub const TEXT: Color32 = Color32::from_rgb(229, 233, 240);
    pub const TEXT_DIM: Color32 = Color32::from_rgb(139, 148, 167);
    pub const TEXT_FAINT: Color32 = Color32::from_rgb(94, 103, 124);

    // accents
    pub const CYAN: Color32 = Color32::from_rgb(34, 211, 238);
    pub const BLUE: Color32 = Color32::from_rgb(59, 130, 246);
    pub const PURPLE: Color32 = Color32::from_rgb(167, 139, 250);
    pub const GREEN: Color32 = Color32::from_rgb(52, 211, 153);
    pub const PLAY: Color32 = Color32::from_rgb(16, 185, 129);
    pub const RECORD: Color32 = Color32::from_rgb(239, 68, 68);
    pub const STOP: Color32 = Color32::from_rgb(245, 158, 11);
    pub const YELLOW: Color32 = Color32::from_rgb(234, 179, 8);
    pub const ORANGE: Color32 = Color32::from_rgb(251, 146, 60);
    pub const RED: Color32 = Color32::from_rgb(248, 113, 113);
    pub const PINK: Color32 = Color32::from_rgb(244, 114, 182);
    pub const TEAL: Color32 = Color32::from_rgb(45, 212, 191);

    pub const R: Rounding = Rounding::same(6.0);
    pub const R_SM: Rounding = Rounding::same(4.0);
    pub const R_LG: Rounding = Rounding::same(10.0);

    pub fn font(size: f32) -> FontId {
        FontId::proportional(size)
    }
    pub fn mono(size: f32) -> FontId {
        FontId::monospace(size)
    }

    pub fn apply(ctx: &Context) {
        let mut style = Style::default();
        style.visuals = Visuals::dark();
        style.visuals.dark_mode = true;
        style.visuals.panel_fill = Self::PANEL;
        style.visuals.window_fill = Self::PANEL2;
        style.visuals.extreme_bg_color = Self::BG;
        style.visuals.faint_bg_color = Color32::from_rgb(18, 25, 41);
        style.visuals.window_stroke = egui::Stroke::new(1.0, Self::BORDER);
        style.visuals.widgets.noninteractive.bg_stroke = egui::Stroke::new(0.5, Self::BORDER);
        style.visuals.widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0, Self::TEXT_DIM);
        style.visuals.widgets.inactive.bg_fill = Self::CARD;
        style.visuals.widgets.inactive.fg_stroke = egui::Stroke::new(1.0, Self::TEXT);
        style.visuals.widgets.inactive.bg_stroke = egui::Stroke::new(0.5, Self::BORDER);
        style.visuals.widgets.hovered.bg_fill = Self::CARD_HI;
        style.visuals.widgets.hovered.fg_stroke = egui::Stroke::new(1.0, Self::TEXT);
        style.visuals.widgets.hovered.bg_stroke = egui::Stroke::new(1.0, Self::CYAN.linear_multiply(0.4));
        style.visuals.widgets.active.bg_fill = Self::CARD_HI;
        style.visuals.widgets.active.fg_stroke = egui::Stroke::new(1.0, Self::CYAN);
        style.visuals.selection.bg_fill = Self::CYAN.linear_multiply(0.35);
        style.visuals.selection.stroke = egui::Stroke::new(1.0, Self::CYAN);
        style.visuals.override_text_color = Some(Self::TEXT);
        style.text_styles = [
            (egui::TextStyle::Body, FontId::proportional(12.5)),
            (egui::TextStyle::Small, FontId::proportional(10.0)),
            (egui::TextStyle::Button, FontId::proportional(12.0)),
            (egui::TextStyle::Heading, FontId::proportional(17.0)),
            (egui::TextStyle::Monospace, FontId::monospace(12.0)),
        ].into_iter().collect();
        style.spacing.item_spacing = egui::vec2(6.0, 4.0);
        style.spacing.button_padding = egui::vec2(8.0, 4.0);
        style.spacing.menu_margin = egui::Margin::same(6.0);
        ctx.set_style(style);
    }
}

/// Small-caps section label, like the design's "AI ASSISTANT", "SUGGESTED ACTIONS".
pub fn section_label(ui: &mut egui::Ui, text: &str) {
    ui.add_space(6.0);
    ui.label(
        egui::RichText::new(text.to_uppercase())
            .size(9.5)
            .color(Theme::TEXT_DIM),
    );
    ui.add_space(2.0);
}

pub fn card(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::none()
        .fill(Theme::CARD)
        .stroke(egui::Stroke::new(1.0, Theme::BORDER))
        .rounding(Theme::R)
        .inner_margin(egui::Margin::same(10.0))
        .outer_margin(egui::Margin::symmetric(0.0, 4.0))
        .show(ui, |ui| add(ui));
}

/// Vertical meter gradient used across faders: green -> yellow -> red.
pub fn meter_color(v: f32) -> Color32 {
    let v = v.clamp(0.0, 1.0);
    if v < 0.6 {
        mix(Color32::from_rgb(38, 175, 118), Color32::from_rgb(74, 222, 128), v / 0.6)
    } else if v < 0.85 {
        mix(Color32::from_rgb(74, 222, 128), Theme::YELLOW, (v - 0.6) / 0.25)
    } else {
        mix(Theme::YELLOW, Theme::RECORD, (v - 0.85) / 0.15)
    }
}

pub fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let l = |x: u8, y: u8| -> u8 { (x as f32 + (y as f32 - x as f32) * t) as u8 };
    Color32::from_rgb(l(a.r(), b.r()), l(a.g(), b.g()), l(a.b(), b.b()))
}

pub fn db_to_meter(db: f32) -> f32 {
    // -60..0 dB -> 0..1
    ((db + 60.0) / 60.0).clamp(0.0, 1.0)
}
