//! Bottom mixer — channel strips (FX slots, pan knob, fader+meter, dB box)
//! plus the MASTER strip. Fully painter-drawn for precise strip layout.

use crate::app::AuroraApp;
use crate::theme::Theme;
use aurora_engine::project::TrackKind;
use std::sync::atomic::Ordering;
use egui::{Color32, Pos2, Rect, Sense, Stroke, Vec2};

const STRIP_W: f32 = 84.0;

impl AuroraApp {
    pub fn draw_mixer(&mut self, ctx: &egui::Context) {
        let mut open_fx: Option<(u64, usize)> = None;
        let mut actions: Vec<MixerAction> = Vec::new();

        egui::TopBottomPanel::bottom("mixer")
            .exact_height(262.0)
            .frame(egui::Frame::none().fill(Theme::BG).inner_margin(egui::Margin::same(4.0)))
            .show(ctx, |ui| {
                ui.allocate_ui(egui::vec2(ui.available_width(), 250.0), |ui| {
                    egui::ScrollArea::horizontal()
                        .auto_shrink(false)
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.set_min_height(250.0);
                                let n = self.project.tracks.len();
                                for i in 0..n {
                                    if let Some(a) = self.draw_strip(ui, i, &mut open_fx) {
                                        actions.push(a);
                                    }
                                    ui.add_space(2.0);
                                }
                                ui.add_space(6.0);
                                self.draw_master_strip(ui);
                            });
                        });
                });
            });

        if let Some((tid, slot)) = open_fx {
            if !self.fx_windows.contains(&tid) {
                self.fx_windows.push(tid);
            }
            self.fx_selected.insert(tid, slot);
        }
        for a in actions {
            match a {
                MixerAction::ToggleFxWin(tid) => {
                    if self.fx_windows.contains(&tid) {
                        self.fx_windows.retain(|x| *x != tid);
                    } else {
                        self.fx_windows.push(tid);
                    }
                }
                MixerAction::Select(tid) => self.selected_track = Some(tid),
                MixerAction::AddFx(tid) => {
                    let mut fx = aurora_engine::effects::EffectInstance::new(
                        aurora_engine::effects::EffectType::Compressor,
                        0,
                    );
                    fx.uid = self.project.alloc_id();
                    if let Some(t) = self.project.track_by_id_mut(tid) {
                        t.fx.push(fx);
                    }
                    self.mark_graph_dirty();
                    self.status = "Compressor added — FX window to change type".into();
                }
                MixerAction::ToggleSolo(tid) => {
                    if let Some(t) = self.project.track_by_id_mut(tid) {
                        t.solo = !t.solo;
                    }
                }
                MixerAction::ToggleMute(tid) => {
                    if let Some(t) = self.project.track_by_id_mut(tid) {
                        t.mute = !t.mute;
                    }
                }
                MixerAction::AddMasterFx(slot) => {
                    let et = [
                        aurora_engine::effects::EffectType::Limiter,
                        aurora_engine::effects::EffectType::Eq3,
                        aurora_engine::effects::EffectType::Compressor,
                    ][slot.min(2)];
                    let mut fx = aurora_engine::effects::EffectInstance::new(et, 0);
                    fx.uid = self.project.alloc_id();
                    self.project.master_fx.push(fx);
                    self.mark_graph_dirty();
                }
            }
        }
    }

    fn draw_strip(&mut self, ui: &mut egui::Ui, idx: usize, open_fx: &mut Option<(u64, usize)>) -> Option<MixerAction> {
        let mut default_action: Option<MixerAction> = None;
        let t = &self.project.tracks[idx];
        let selected = Some(t.id) == self.selected_track;
        let name = t.name.clone();
        let subtitle = t.subtitle.clone();
        let color = t.color;
        let kind = t.kind;
        let vol = t.volume_db;
        let pan = t.pan;
        let mute = t.mute;
        let solo = t.solo;
        let fx_count = t.fx.len();
        let tid = t.id;
        let fx_types: Vec<aurora_engine::effects::EffectType> = t.fx.iter().map(|f| f.etype).collect();

        let (rect, resp) = ui.allocate_exact_size(Vec2::new(STRIP_W, 250.0), Sense::click_and_drag());
        let ptr = resp.interact_pointer_pos();
        let painter = ui.painter_at(rect);
        let bg = if selected { Theme::PANEL2 } else { Theme::PANEL };
        painter.rect_filled(rect, 4.0, bg);
        let mut clicked_on = |r: Rect| -> bool {
            resp.clicked() && ptr.map(|p| r.contains(p)).unwrap_or(false)
        };
        let dragging_in = |r: Rect| -> bool {
            resp.dragged() && ptr.map(|p| r.contains(p)).unwrap_or(false)
        };
        let drag_delta = resp.drag_delta();

        // ---- FX slots (2 rows x 2) + add chip ----
        let slot_w = 35.0;
        let slot_h = 14.0;
        for s in 0..4 {
            let sr = Rect::from_min_size(
                Pos2::new(
                    rect.left() + 5.0 + (s % 2) as f32 * (slot_w + 3.0),
                    rect.top() + 5.0 + (s / 2) as f32 * (slot_h + 3.0),
                ),
                Vec2::new(slot_w, slot_h),
            );
            let filled = s < fx_count;
            let col = if filled {
                crate::panels::browser::fx_color(fx_types[s])
            } else {
                Theme::BORDER
            };
            painter.rect_filled(sr, 3.0, if filled { col.linear_multiply(0.28) } else { Color32::from_rgb(15, 21, 34) });
            painter.rect_stroke(sr, 3.0, Stroke::new(0.8, col));
            let label = if filled { fx_types[s].name()[..2].to_uppercase() } else { "+".into() };
            painter.text(sr.center(), egui::Align2::CENTER_CENTER, label, egui::FontId::proportional(7.5), if filled { col } else { Theme::TEXT_FAINT });
            if clicked_on(sr) {
                if filled {
                    open_fx.get_or_insert((tid, s));
                } else {
                    default_action = Some(MixerAction::AddFx(tid));
                }
            }
        }

        // ---- name / subtitle / color line ----
        painter.text(Pos2::new(rect.center().x, rect.top() + 41.0), egui::Align2::CENTER_CENTER, &name, egui::FontId::proportional(9.5), Theme::TEXT);
        if !subtitle.is_empty() {
            painter.text(Pos2::new(rect.center().x, rect.top() + 51.0), egui::Align2::CENTER_CENTER, &subtitle, egui::FontId::proportional(7.0), Theme::TEXT_FAINT);
        }
        painter.line_segment(
            [Pos2::new(rect.left() + 10.0, rect.top() + 58.0), Pos2::new(rect.right() - 10.0, rect.top() + 58.0)],
            Stroke::new(2.0, Color32::from_rgb(color[0], color[1], color[2])),
        );

        // ---- pan knob (manual) ----
        let pc = Pos2::new(rect.center().x, rect.top() + 80.0);
        draw_knob(&painter, pc, 12.0, (pan + 1.0) / 2.0, Theme::CYAN);
        painter.text(Pos2::new(pc.x, pc.y + 18.0), egui::Align2::CENTER_CENTER, format!("{:+.2}", pan), egui::FontId::proportional(7.0), Theme::TEXT_FAINT);
        let pan_rect = Rect::from_center_size(pc, Vec2::splat(28.0));
        if dragging_in(pan_rect) {
            let np = (pan + drag_delta.x * 0.01).clamp(-1.0, 1.0);
            self.project.tracks[idx].pan = np;
        } else if resp.double_clicked() && ptr.map(|p| pan_rect.contains(p)).unwrap_or(false) {
            self.project.tracks[idx].pan = 0.0;
        }

        // ---- fader + meter (manual) ----
        let fader_x = rect.left() + 20.0;
        let meter_x = rect.left() + 44.0;
        let f_top = rect.top() + 104.0;
        let f_bot = rect.bottom() - 44.0;
        // meter
        let meter_peak = f32::from_bits(self.parts.meters.track_peak[idx.min(4095)].load(std::sync::atomic::Ordering::Relaxed));
        let meter01 = crate::theme::db_to_meter(20.0 * meter_peak.max(1e-6).log10());
        painter.rect_filled(Rect::from_min_size(Pos2::new(meter_x - 3.0, f_top), Vec2::new(6.0, f_bot - f_top)), 2.0, Color32::from_rgb(16, 22, 36));
        let steps = 36;
        for i in 0..steps {
            let t = i as f32 / steps as f32;
            if t > meter01 {
                break;
            }
            let seg = (f_bot - f_top) / steps as f32;
            painter.rect_filled(
                Rect::from_min_size(Pos2::new(meter_x - 3.0, f_bot - seg * (i + 1) as f32 + 0.8), Vec2::new(6.0, seg - 1.3)),
                1.5,
                crate::theme::meter_color(t),
            );
        }
        // fader track + cap
        painter.line_segment([Pos2::new(fader_x, f_top), Pos2::new(fader_x, f_bot)], Stroke::new(3.0, Theme::BORDER));
        let norm = ((vol + 60.0) / 66.0).clamp(0.0, 1.0);
        let cap_y = f_bot - (f_bot - f_top) * norm;
        painter.rect_filled(Rect::from_center_size(Pos2::new(fader_x, cap_y), Vec2::new(22.0, 11.0)), 3.0, Theme::CARD_HI);
        painter.rect_stroke(Rect::from_center_size(Pos2::new(fader_x, cap_y), Vec2::new(22.0, 11.0)), 3.0, Stroke::new(1.0, Color32::from_rgb(color[0], color[1], color[2])));
        painter.line_segment([Pos2::new(fader_x - 7.0, cap_y), Pos2::new(fader_x + 7.0, cap_y)], Stroke::new(1.5, Theme::TEXT));
        let fader_rect = Rect::from_min_max(Pos2::new(fader_x - 14.0, f_top), Pos2::new(meter_x + 8.0, f_bot));
        if dragging_in(fader_rect) {
            let y = ptr.unwrap().y;
            let n2 = (1.0 - (y - f_top) / (f_bot - f_top)).clamp(0.0, 1.0);
            let mut db = -60.0 + n2 * 66.0;
            if (db - 0.0).abs() < 1.2 {
                db = 0.0;
            }
            self.project.tracks[idx].volume_db = db;
        }

        // ---- S/M chips ----
        for (k, (label, active, col)) in [("S", solo, Theme::YELLOW), ("M", mute, Theme::RECORD)].iter().enumerate() {
            let cr = Rect::from_min_size(Pos2::new(rect.left() + 8.0 + k as f32 * 24.0, rect.bottom() - 38.0), Vec2::new(20.0, 14.0));
            painter.rect_filled(cr, 3.0, if *active { *col } else { Theme::CARD });
            painter.rect_stroke(cr, 3.0, Stroke::new(0.7, if *active { *col } else { Theme::BORDER_HI }));
            painter.text(cr.center(), egui::Align2::CENTER_CENTER, *label, egui::FontId::proportional(8.5), if *active { Theme::BG } else { Theme::TEXT_DIM });
            if clicked_on(cr) {
                default_action = Some(if k == 0 { MixerAction::ToggleSolo(tid) } else { MixerAction::ToggleMute(tid) });
            }
        }
        // dB box
        let dbr = Rect::from_min_size(Pos2::new(rect.left() + 36.0, rect.bottom() - 38.0), Vec2::new(42.0, 14.0));
        painter.rect_filled(dbr, 2.0, Theme::BG);
        painter.rect_stroke(dbr, 2.0, Stroke::new(0.7, Theme::BORDER));
        let txt = if vol <= -59.5 { "-inf".into() } else { format!("{vol:.1}") };
        painter.text(dbr.center(), egui::Align2::CENTER_CENTER, &txt, crate::theme::Theme::mono(9.0), if vol > -0.1 { Theme::RECORD } else { Theme::TEXT_DIM });

        // ---- footer ----
        painter.text(
            Pos2::new(rect.center().x, rect.bottom() - 10.0),
            egui::Align2::CENTER_CENTER,
            if kind == TrackKind::Bus { format!("{name} [BUS]") } else { name },
            egui::FontId::proportional(7.5),
            Theme::TEXT_DIM,
        );
        if resp.clicked() && default_action.is_none() {
            default_action = Some(MixerAction::Select(tid));
        }
        default_action
    }

    fn draw_master_strip(&mut self, ui: &mut egui::Ui) {
        let (rect, resp) = ui.allocate_exact_size(Vec2::new(STRIP_W + 8.0, 250.0), Sense::click_and_drag());
        let ptr = resp.interact_pointer_pos();
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 4.0, Theme::PANEL2);
        painter.rect_stroke(rect, 4.0, Stroke::new(1.2, Theme::RECORD.linear_multiply(0.7)));
        let clicked_on = |r: Rect| -> bool { resp.clicked() && ptr.map(|p| r.contains(p)).unwrap_or(false) };

        painter.text(Pos2::new(rect.center().x, rect.top() + 12.0), egui::Align2::CENTER_CENTER, "MASTER", egui::FontId::proportional(10.5), Theme::TEXT);
        painter.text(Pos2::new(rect.center().x, rect.top() + 24.0), egui::Align2::CENTER_CENTER, "STEREO OUT", egui::FontId::proportional(7.0), Theme::RECORD);

        let fx = self.project.master_fx.len();
        for s in 0..3 {
            let sr = Rect::from_min_size(Pos2::new(rect.left() + 6.0 + s as f32 * 28.0, rect.top() + 32.0), Vec2::new(25.0, 14.0));
            let filled = s < fx;
            let et = self.project.master_fx.get(s).map(|f| f.etype);
            let col = et.map(crate::panels::browser::fx_color).unwrap_or(Theme::BORDER);
            painter.rect_filled(sr, 3.0, if filled { col.linear_multiply(0.28) } else { Color32::from_rgb(15, 21, 34) });
            painter.rect_stroke(sr, 3.0, Stroke::new(0.8, col));
            if filled {
                painter.text(sr.center(), egui::Align2::CENTER_CENTER, et.unwrap().name()[..2].to_uppercase(), egui::FontId::proportional(7.5), col);
            } else {
                painter.text(sr.center(), egui::Align2::CENTER_CENTER, "+", egui::FontId::proportional(8.0), Theme::TEXT_FAINT);
            }
            if clicked_on(sr) && !filled {
                // handled post-frame via direct push (safe: mutable here is fine)
                let et = [aurora_engine::effects::EffectType::Limiter, aurora_engine::effects::EffectType::Eq3, aurora_engine::effects::EffectType::Compressor][s];
                let mut fxn = aurora_engine::effects::EffectInstance::new(et, self.project.alloc_id());
                fxn.uid = self.project.alloc_id();
                self.project.master_fx.push(fxn);
                self.mark_graph_dirty();
            }
        }

        let pl = f32::from_bits(self.parts.meters.master_peak_l.load(std::sync::atomic::Ordering::Relaxed));
        let pr = f32::from_bits(self.parts.meters.master_peak_r.load(std::sync::atomic::Ordering::Relaxed));
        let ml = crate::theme::db_to_meter(20.0 * pl.max(1e-6).log10());
        let mr = crate::theme::db_to_meter(20.0 * pr.max(1e-6).log10());
        let meter_h = 120.0;
        let top = rect.top() + 54.0;
        let bot = top + meter_h;
        for (k, lvl) in [ml, mr].iter().enumerate() {
            let x = rect.left() + 14.0 + k as f32 * 20.0;
            painter.rect_filled(Rect::from_min_size(Pos2::new(x, top), Vec2::new(6.0, meter_h)), 2.0, Color32::from_rgb(16, 22, 36));
            let steps = 36;
            for i in 0..steps {
                let t = i as f32 / steps as f32;
                if t > *lvl {
                    break;
                }
                let seg = meter_h / steps as f32;
                painter.rect_filled(Rect::from_min_size(Pos2::new(x, bot - seg * (i + 1) as f32 + 0.8), Vec2::new(6.0, seg - 1.3)), 1.5, crate::theme::meter_color(t));
            }
        }
        // master fader
        let fader_x = rect.left() + 62.0;
        painter.line_segment([Pos2::new(fader_x, top), Pos2::new(fader_x, bot)], Stroke::new(3.0, Theme::BORDER));
        let norm = ((self.project.master_volume_db + 60.0) / 66.0).clamp(0.0, 1.0);
        let cap_y = bot - (bot - top) * norm;
        painter.rect_filled(Rect::from_center_size(Pos2::new(fader_x, cap_y), Vec2::new(22.0, 11.0)), 3.0, Theme::CARD_HI);
        painter.rect_stroke(Rect::from_center_size(Pos2::new(fader_x, cap_y), Vec2::new(22.0, 11.0)), 3.0, Stroke::new(1.0, Theme::RECORD));
        let fader_rect = Rect::from_min_max(Pos2::new(fader_x - 14.0, top), Pos2::new(fader_x + 14.0, bot));
        if resp.dragged() && ptr.map(|p| fader_rect.contains(p)).unwrap_or(false) {
            let n2 = (1.0 - (ptr.unwrap().y - top) / (bot - top)).clamp(0.0, 1.0);
            self.project.master_volume_db = -60.0 + n2 * 66.0;
        }

        // loudness readouts
        let lufs = f32::from_bits(self.parts.loudness.integrated_lu.load(std::sync::atomic::Ordering::Relaxed));
        let tp = f32::from_bits(self.parts.loudness.true_peak_db.load(std::sync::atomic::Ordering::Relaxed));
        painter.text(Pos2::new(rect.center().x, rect.bottom() - 34.0), egui::Align2::CENTER_CENTER, format!("{lufs:.1} LUFS"), crate::theme::Theme::mono(8.5), Theme::CYAN);
        painter.text(Pos2::new(rect.center().x, rect.bottom() - 23.0), egui::Align2::CENTER_CENTER, format!("{tp:.1} dBTP"), crate::theme::Theme::mono(8.5), Theme::TEXT_DIM);
        widgets_db_box(&painter, rect, self.project.master_volume_db);
        painter.text(Pos2::new(rect.center().x, rect.bottom() - 9.0), egui::Align2::CENTER_CENTER, "MASTER", egui::FontId::proportional(7.5), Theme::TEXT_DIM);
    }
}

fn widgets_db_box(p: &egui::Painter, rect: Rect, db: f32) {
    let dbr = Rect::from_center_size(Pos2::new(rect.center().x, rect.bottom() - 48.0), Vec2::new(54.0, 14.0));
    p.rect_filled(dbr, 2.0, Theme::BG);
    p.rect_stroke(dbr, 2.0, Stroke::new(0.7, Theme::BORDER));
    let txt = if db <= -59.5 { "-inf".into() } else { format!("{db:.1}") };
    p.text(dbr.center(), egui::Align2::CENTER_CENTER, &txt, crate::theme::Theme::mono(9.0), Theme::TEXT_DIM);
}

fn draw_knob(p: &egui::Painter, c: Pos2, r: f32, t: f32, col: Color32) {
    p.circle_filled(c, r, Theme::CARD_HI);
    p.circle_stroke(c, r, Stroke::new(1.2, Theme::BORDER_HI));
    let a0 = std::f32::consts::PI * 0.75;
    let a1 = std::f32::consts::PI * 2.25;
    let seg = 20;
    let tseg = (t.clamp(0.0, 1.0) * seg as f32).ceil() as usize;
    let mut pts = Vec::new();
    for i in 0..=seg {
        let a = a0 + (a1 - a0) * (i as f32 / seg as f32);
        pts.push(Pos2::new(c.x + a.cos() * r, c.y + a.sin() * r));
    }
    p.add(egui::Shape::line(pts, Stroke::new(2.0, Theme::BORDER)));
    let mut pts = Vec::new();
    for i in 0..=tseg {
        let a = a0 + (a1 - a0) * (i as f32 / seg as f32);
        pts.push(Pos2::new(c.x + a.cos() * r, c.y + a.sin() * r));
    }
    if pts.len() > 1 {
        p.add(egui::Shape::line(pts, Stroke::new(2.0, col)));
    }
    let pa = a0 + (a1 - a0) * t.clamp(0.0, 1.0);
    p.line_segment(
        [Pos2::new(c.x + pa.cos() * r * 0.2, c.y + pa.sin() * r * 0.2), Pos2::new(c.x + pa.cos() * r * 0.8, c.y + pa.sin() * r * 0.8)],
        Stroke::new(1.8, Theme::TEXT),
    );
}

#[derive(PartialEq)]
enum MixerAction {
    ToggleFxWin(u64),
    Select(u64),
    AddFx(u64),
    ToggleSolo(u64),
    ToggleMute(u64),
    AddMasterFx(usize),
}
