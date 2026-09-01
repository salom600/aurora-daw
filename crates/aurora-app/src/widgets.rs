//! AURORA custom-painted widgets — transport icons, knobs, faders, meters,
//! progress rings. All drawn with egui's Painter for a crisp, professional
//! look identical to the reference design (no external icon fonts needed).

use egui::{Color32, Pos2, Rect, Response, Rounding, Sense, Stroke, Vec2};

use crate::theme::{self, Theme};

// ---------------------------------------------------------------------------
// Painter-drawn icons (16x16 cell inside given rect)
// ---------------------------------------------------------------------------

pub fn icon_play(p: &egui::Painter, c: Pos2, s: f32, col: Color32) {
    let s = s * 0.5;
    let pts = [
        Pos2::new(c.x - s * 0.55, c.y - s),
        Pos2::new(c.x - s * 0.55, c.y + s),
        Pos2::new(c.x + s, c.y),
    ];
    p.add(egui::Shape::convex_polygon(pts.to_vec(), col, Stroke::NONE));
}

pub fn icon_stop(p: &egui::Painter, c: Pos2, s: f32, col: Color32) {
    let s = s * 0.42;
    p.rect_filled(Rect::from_center_size(c, Vec2::splat(s * 2.0)), 1.0, col);
}

pub fn icon_record(p: &egui::Painter, c: Pos2, s: f32, col: Color32) {
    p.circle_filled(c, s * 0.52, col);
}

pub fn icon_rewind(p: &egui::Painter, c: Pos2, s: f32, col: Color32) {
    let s = s * 0.5;
    let pts = [
        Pos2::new(c.x - s, c.y),
        Pos2::new(c.x + s * 0.55, c.y - s),
        Pos2::new(c.x + s * 0.55, c.y + s),
    ];
    p.add(egui::Shape::convex_polygon(pts.to_vec(), col, Stroke::NONE));
    p.line_segment(
        [Pos2::new(c.x - s - 1.0, c.y - s), Pos2::new(c.x - s - 1.0, c.y + s)],
        Stroke::new(1.6, col),
    );
}

pub fn icon_forward(p: &egui::Painter, c: Pos2, s: f32, col: Color32) {
    let s = s * 0.5;
    let pts = [
        Pos2::new(c.x + s, c.y),
        Pos2::new(c.x - s * 0.55, c.y - s),
        Pos2::new(c.x - s * 0.55, c.y + s),
    ];
    p.add(egui::Shape::convex_polygon(pts.to_vec(), col, Stroke::NONE));
    p.line_segment(
        [Pos2::new(c.x + s + 1.0, c.y - s), Pos2::new(c.x + s + 1.0, c.y + s)],
        Stroke::new(1.6, col),
    );
}

pub fn icon_loop(p: &egui::Painter, c: Pos2, s: f32, col: Color32) {
    let r: f32 = s * 0.45;
    let center = c;
    let stroke = Stroke::new(1.6, col);
    let center = c;
    let stroke = Stroke::new(1.6, col);
    // arc with arrowheads
    let n = 24;
    let mut pts = Vec::new();
    for i in 0..=n {
        let a: f32 = -0.6 + (i as f32 / n as f32) * std::f32::consts::TAU * 0.85;
        pts.push(Pos2::new(center.x + a.cos() * r, center.y + a.sin() * r));
    }
    p.add(egui::Shape::line(pts, stroke));
    let a1: f32 = -0.6;
    let tip = Pos2::new(center.x + a1.cos() * r, center.y + a1.sin() * r);
    p.circle_filled(tip, 1.8, col);
}

pub fn icon_mic(p: &egui::Painter, c: Pos2, s: f32, col: Color32) {
    let stroke = Stroke::new(1.5, col);
    p.rect_filled(
        Rect::from_center_size(Pos2::new(c.x, c.y - s * 0.15), Vec2::new(s * 0.7, s * 1.0)),
        s * 0.35,
        col,
    );
    p.line_segment(
        [Pos2::new(c.x - s * 0.5, c.y - s * 0.05), Pos2::new(c.x - s * 0.5, c.y + s * 0.05)],
        stroke,
    );
    p.line_segment(
        [Pos2::new(c.x - s * 0.5, c.y + s * 0.05), Pos2::new(c.x + s * 0.5, c.y + s * 0.05)],
        stroke,
    );
    p.line_segment(
        [Pos2::new(c.x, c.y + s * 0.05), Pos2::new(c.x, c.y + s * 0.55)],
        stroke,
    );
    p.line_segment(
        [Pos2::new(c.x - s * 0.3, c.y + s * 0.55), Pos2::new(c.x + s * 0.3, c.y + s * 0.55)],
        stroke,
    );
}

// ---------------------------------------------------------------------------
// Round transport buttons (like 89.png)
// ---------------------------------------------------------------------------

pub enum TransportIcon {
    Play,
    Stop,
    Record,
    Rewind,
    Forward,
    Loop,
}

pub fn transport_button(
    ui: &mut egui::Ui,
    icon: TransportIcon,
    active: bool,
    active_color: Color32,
    tip: &str,
) -> Response {
    let size = Vec2::new(34.0, 26.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click_and_drag());
    let painter = ui.painter_at(rect.intersect(ui.clip_rect()));
    let c = rect.center();
    let bg = if resp.hovered() {
        Theme::CARD_HI
    } else {
        Color32::TRANSPARENT
    };
    if bg != Color32::TRANSPARENT {
        painter.rect_filled(rect, Theme::R_SM, bg);
    }
    let s = 12.0;
    if active {
        // glow dot under
        painter.circle_filled(c, 2.0, active_color);
    }
    let col = if active { active_color } else { Theme::TEXT_DIM };
    match icon {
        TransportIcon::Play => icon_play(&painter, c, s, col),
        TransportIcon::Stop => icon_stop(&painter, c, s, col),
        TransportIcon::Record => icon_record(&painter, c, s, col),
        TransportIcon::Rewind => icon_rewind(&painter, c, s, col),
        TransportIcon::Forward => icon_forward(&painter, c, s, col),
        TransportIcon::Loop => icon_loop(&painter, c, s * 1.1, col),
    }
    resp.on_hover_text(tip)
}

/// Large colored transport pill (PLAY / RECORD / STOP) like 28.png top bar.
pub fn transport_pill(
    ui: &mut egui::Ui,
    label: &str,
    icon: TransportIcon,
    color: Color32,
    active: bool,
) -> Response {
    let size = Vec2::new(92.0, 30.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let painter = ui.painter_at(rect);
    let fill = if active {
        color
    } else {
        Color32::from_rgb(
            (color.r() as f32 * 0.28 + Theme::CARD.r() as f32 * 0.72) as u8,
            (color.g() as f32 * 0.28 + Theme::CARD.g() as f32 * 0.72) as u8,
            (color.b() as f32 * 0.28 + Theme::CARD.b() as f32 * 0.72) as u8,
        )
    };
    painter.rect_filled(rect, Theme::R, fill);
    painter.rect_stroke(rect.shrink(0.5), Theme::R, Stroke::new(1.0, color.linear_multiply(0.6)));
    let c = Pos2::new(rect.left() + 20.0, rect.center().y);
    let icon_col = if active { Color32::WHITE } else { color };
    match icon {
        TransportIcon::Play => icon_play(&painter, c, 11.0, icon_col),
        TransportIcon::Stop => icon_stop(&painter, c, 10.0, icon_col),
        TransportIcon::Record => icon_record(&painter, c, 9.0, icon_col),
        _ => {}
    }
    painter.text(
        Pos2::new(rect.center().x + 8.0, rect.center().y),
        egui::Align2::CENTER_CENTER,
        label,
        FontId_::bold(12.0),
        if active { Color32::WHITE } else { Theme::TEXT },
    );
    resp
}

mod FontId_ {
    use egui::FontId;
    pub fn bold(size: f32) -> FontId {
        FontId::proportional(size) // weight comes from default font family
    }
}

// ---------------------------------------------------------------------------
// Rotary knob
// ---------------------------------------------------------------------------

pub fn knob(ui: &mut egui::Ui, label: &str, value: &mut f32, min: f32, max: f32, color: Color32) -> Response {
    let size = Vec2::splat(34.0);
    let resp_cell: std::cell::RefCell<Option<Response>> = std::cell::RefCell::new(None);
    let out = ui.vertical(|ui| {
        let (rect, resp) = ui.allocate_exact_size(size, Sense::click_and_drag());
        let painter = ui.painter_at(rect);
        let c = rect.center();
        let r = size.x * 0.42;
        // drag handling
        if resp.dragged() {
            let dy = -resp.drag_delta().y;
            *value += dy * (max - min) / 140.0;
            *value = value.clamp(min, max);
        }
        if resp.double_clicked() {
            *value = (min + max) * 0.5;
        }
        let t = if max > min { (*value - min) / (max - min) } else { 0.0 };
        // arc from 135° to 405°
        let a0 = std::f32::consts::PI * 0.75;
        let a1 = std::f32::consts::PI * 2.25;
        let segments = 28;
        let mut pts = Vec::new();
        for i in 0..=segments {
            let a = a0 + (a1 - a0) * (i as f32 / segments as f32);
            pts.push(Pos2::new(c.x + a.cos() * r, c.y + a.sin() * r));
        }
        painter.add(egui::Shape::line(pts, Stroke::new(2.5, Theme::BORDER_HI)));
        let tseg = (t * segments as f32).ceil() as usize;
        let mut pts = Vec::new();
        for i in 0..=tseg.min(segments) {
            let a = a0 + (a1 - a0) * (i as f32 / segments as f32);
            pts.push(Pos2::new(c.x + a.cos() * r, c.y + a.sin() * r));
        }
        if pts.len() > 1 {
            painter.add(egui::Shape::line(pts, Stroke::new(2.5, color)));
        }
        // pointer
        let pa = a0 + (a1 - a0) * t;
        painter.line_segment(
            [
                Pos2::new(c.x + (pa.cos()) * r * 0.25, c.y + (pa.sin()) * r * 0.25),
                Pos2::new(c.x + (pa.cos()) * r * 0.8, c.y + (pa.sin()) * r * 0.8),
            ],
            Stroke::new(2.0, Theme::TEXT),
        );
        resp_cell.borrow_mut().replace(resp.on_hover_text(format!("{label}: {:.2}", *value)));
        ui.label(
            egui::RichText::new(label)
                .size(8.5)
                .color(Theme::TEXT_FAINT),
        );
    });
    let _ = out;
    resp_cell.into_inner().unwrap_or_else(|| ui.button(label))
}

// ---------------------------------------------------------------------------
// Vertical fader + meter (mixer strip)
// ---------------------------------------------------------------------------

pub fn fader_with_meter(
    ui: &mut egui::Ui,
    db: &mut f32,
    meter01: f32,
    height: f32,
    accent: Color32,
) -> Response {
    let width = 44.0;
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(width, height), Sense::click_and_drag());
    let painter = ui.painter_at(rect);
    let track_x = rect.left() + 15.0;
    let meter_x = rect.left() + 32.0;
    let top = rect.top() + 8.0;
    let bot = rect.bottom() - 8.0;
    // fader track
    painter.line_segment(
        [Pos2::new(track_x, top), Pos2::new(track_x, bot)],
        Stroke::new(3.0, Theme::BORDER),
    );
    // meter background + fill (bottom-up)
    painter.rect_filled(
        Rect::from_min_size(Pos2::new(meter_x - 2.5, top), Vec2::new(5.0, bot - top)),
        2.0,
        Color32::from_rgb(16, 22, 36),
    );
    let mh = (bot - top) * meter01.clamp(0.0, 1.0);
    let steps = 40;
    for i in 0..steps {
        let t0 = i as f32 / steps as f32;
        let seg_h = (bot - top) / steps as f32;
        let y0 = bot - seg_h * i as f32;
        if y0 < bot - mh {
            break;
        }
        painter.rect_filled(
            Rect::from_min_size(Pos2::new(meter_x - 2.5, y0 - seg_h + 0.8), Vec2::new(5.0, seg_h - 1.2)),
            1.5,
            theme::meter_color(t0),
        );
    }
    // fader cap position: 0dB at 75% height, -60dB at bottom, +6dB at top
    let db_to_y = |db: f32| -> f32 {
        let norm = ((db + 60.0) / 66.0).clamp(0.0, 1.0); // 0..1 bottom->top
        bot - (bot - top) * norm
    };
    let cy = db_to_y(*db);
    painter.rect_filled(
        Rect::from_center_size(Pos2::new(track_x, cy), Vec2::new(22.0, 11.0)),
        3.0,
        Theme::CARD_HI,
    );
    painter.rect_stroke(
        Rect::from_center_size(Pos2::new(track_x, cy), Vec2::new(22.0, 11.0)),
        3.0,
        Stroke::new(1.0, accent),
    );
    painter.line_segment(
        [Pos2::new(track_x - 7.0, cy), Pos2::new(track_x + 7.0, cy)],
        Stroke::new(1.5, Theme::TEXT),
    );
    // interaction
    if resp.dragged() {
        let norm = 1.0 - ((resp.interact_pointer_pos().unwrap_or(rect.center()).y - top) / (bot - top));
        *db = -60.0 + norm.clamp(0.0, 1.0) * 66.0;
        if (*db - 0.0).abs() < 1.2 {
            *db = 0.0; // snap unity
        }
    }
    resp.on_hover_text(format!("Gain: {:.1} dB", *db))
}

// ---------------------------------------------------------------------------
// Horizontal mini meter (track headers)
// ---------------------------------------------------------------------------

pub fn mini_meter(ui: &mut egui::Ui, level01: f32, width: f32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, 4.0), Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 2.0, Color32::from_rgb(16, 22, 36));
    let w = rect.width() * level01.clamp(0.0, 1.0);
    if w > 0.5 {
        painter.rect_filled(
            Rect::from_min_size(rect.left_top(), Vec2::new(w, rect.height())),
            2.0,
            theme::meter_color(level01),
        );
    }
}

// ---------------------------------------------------------------------------
// Circular progress ring (AI smart modules)
// ---------------------------------------------------------------------------

pub fn ring(ui: &mut egui::Ui, radius: f32, t: f32, color: Color32, label: &str) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(radius * 2.0), Sense::hover());
    let painter = ui.painter_at(rect);
    let c = rect.center();
    painter.circle_stroke(c, radius - 1.0, Stroke::new(2.5, Theme::BORDER));
    let segments = 32;
    let tseg = (t.clamp(0.0, 1.0) * segments as f32).ceil() as usize;
    let mut pts = Vec::new();
    let a0 = -std::f32::consts::FRAC_PI_2;
    for i in 0..=tseg {
        let a = a0 + std::f32::consts::TAU * (i as f32 / segments as f32);
        pts.push(Pos2::new(c.x + a.cos() * (radius - 1.0), c.y + a.sin() * (radius - 1.0)));
    }
    if pts.len() > 1 {
        painter.add(egui::Shape::line(pts, Stroke::new(2.5, color)));
    }
    painter.text(
        c,
        egui::Align2::CENTER_CENTER,
        label,
        theme::Theme::mono(10.0),
        Theme::TEXT,
    );
}

// ---------------------------------------------------------------------------
// Chip buttons (S / M / R)
// ---------------------------------------------------------------------------

pub fn chip(ui: &mut egui::Ui, label: &str, active: bool, active_col: Color32) -> Response {
    let size = Vec2::new(17.0, 15.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let painter = ui.painter_at(rect);
    let fill = if active { active_col } else { Theme::CARD };
    painter.rect_filled(rect, Theme::R_SM, fill);
    if !active {
        painter.rect_stroke(rect, Theme::R_SM, Stroke::new(0.8, Theme::BORDER_HI));
    }
    painter.text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        label,
        egui::FontId::proportional(9.0),
        if active { Color32::from_rgb(10, 14, 23) } else { Theme::TEXT_DIM },
    );
    resp
}

// ---------------------------------------------------------------------------
// Level readout (dB display box, like mixer strips)
// ---------------------------------------------------------------------------

pub fn db_box(ui: &mut egui::Ui, db: f32, width: f32) {
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(width, 16.0), Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, Theme::R_SM, Theme::BG);
    painter.rect_stroke(rect, Theme::R_SM, Stroke::new(0.8, Theme::BORDER));
    let txt = if db <= -59.5 { "-inf".to_string() } else { format!("{db:.1}") };
    painter.text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        &txt,
        theme::Theme::mono(9.5),
        if db > -0.1 { Theme::RECORD } else { Theme::TEXT_DIM },
    );
    let _ = resp;
}

// ---------------------------------------------------------------------------
// Color-dot + text list row (browser sections)
// ---------------------------------------------------------------------------

pub fn browser_row(ui: &mut egui::Ui, dot: Color32, title: &str, sub: &str) -> Response {
    let resp = ui
        .allocate_ui(Vec2::new(ui.available_width(), 30.0), |ui| {
            let (rect, r) = ui.allocate_exact_size(
                Vec2::new(ui.available_width(), 28.0),
                Sense::click(),
            );
            let painter = ui.painter_at(rect);
            if r.hovered() {
                painter.rect_filled(rect, Theme::R_SM, Theme::CARD);
            }
            painter.circle_filled(Pos2::new(rect.left() + 12.0, rect.center().y), 4.5, dot);
            painter.text(
                Pos2::new(rect.left() + 26.0, rect.center().y - 6.0),
                egui::Align2::LEFT_CENTER,
                title,
                egui::FontId::proportional(11.0),
                Theme::TEXT,
            );
            painter.text(
                Pos2::new(rect.left() + 26.0, rect.center().y + 6.5),
                egui::Align2::LEFT_CENTER,
                sub,
                egui::FontId::proportional(8.5),
                Theme::TEXT_FAINT,
            );
            r
        })
        .inner;
    resp
}
