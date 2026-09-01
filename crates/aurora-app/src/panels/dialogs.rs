//! Floating FX rack windows (per track), export dialog, about, piano roll,
//! toasts, import handling.

use crate::app::{AuroraApp, ExportDlg};
use crate::theme::Theme;
use crate::widgets;
use aurora_engine::effects::{effect_defs, EffectType};
use aurora_engine::io::ExportFormat;
use aurora_engine::project::{Note, TrackKind};
use egui::{Color32, Pos2, Rect, Sense, Stroke, Vec2};

impl AuroraApp {
    pub fn draw_fx_windows(&mut self, ctx: &egui::Context) {
        let mut to_remove: Vec<u64> = Vec::new();
        let ids: Vec<u64> = self.fx_windows.clone();
        for &tid in &ids {
            let Some(track) = self.project.track_by_id(tid) else {
                to_remove.push(tid);
                continue;
            };
            let name = track.name.clone();
            let mut keep_open = true;
            egui::Window::new(format!("FX — {name}"))
                .open(&mut keep_open)
                .default_width(420.0)
                .default_height(300.0)
                .collapsible(false)
                .frame(
                    egui::Frame::none()
                        .fill(Theme::PANEL2)
                        .stroke(Stroke::new(1.0, Theme::BORDER_HI))
                        .rounding(Theme::R)
                        .inner_margin(egui::Margin::same(10.0)),
                )
                .show(ctx, |ui| {
                    self.fx_window_body(ui, tid);
                });
            if !keep_open {
                to_remove.push(tid);
            }
        }
        for tid in to_remove {
            self.fx_windows.retain(|x| *x != tid);
        }
    }

    fn fx_window_body(&mut self, ui: &mut egui::Ui, tid: u64) {
        let types: [EffectType; 11] = [
            EffectType::Eq3,
            EffectType::Compressor,
            EffectType::DeEsser,
            EffectType::Reverb,
            EffectType::Delay,
            EffectType::Chorus,
            EffectType::Flanger,
            EffectType::Phaser,
            EffectType::Saturation,
            EffectType::Gate,
            EffectType::Limiter,
        ];
        // chain row (snapshot)
        let chain_info: Vec<(EffectType, bool)> = self
            .project
            .track_by_id(tid)
            .map(|t| t.fx.iter().map(|fx| (fx.etype, fx.enabled)).collect())
            .unwrap_or_default();
        ui.horizontal(|ui| {
            for (i, (fx_type, _en)) in chain_info.iter().enumerate() {
                let _fx = ();
                let sel = self.fx_selected.get(&tid).copied() == Some(i);
                let col = crate::panels::browser::fx_color(*fx_type);
                let txt = egui::RichText::new(fx_type.name()).size(10.0).color(if sel { Theme::BG } else { Theme::TEXT });
                let b = egui::Button::new(txt).fill(if sel { col } else { Theme::CARD }).small();
                if ui.add(b).clicked() {
                    self.fx_selected.insert(tid, i);
                }
            }
            if ui.small_button("+").clicked() {
                let mut fx = aurora_engine::effects::EffectInstance::new(EffectType::Eq3, 0);
                fx.uid = self.project.alloc_id();
                if let Some(t) = self.project.track_by_id_mut(tid) {
                    t.fx.push(fx);
                }
                self.mark_graph_dirty();
            }
        });
        ui.separator();

        let Some(idx) = self.fx_selected.get(&tid).copied() else {
            ui.label("Select or add an effect");
            return;
        };
        let (etype, params, enabled, fx_len) = {
            let Some(track) = self.project.track_by_id(tid) else { return };
            if idx >= track.fx.len() {
                return;
            }
            let fx = &track.fx[idx];
            (fx.etype, fx.params.clone(), fx.enabled, track.fx.len())
        };
        let _ = fx_len;
        let defs = effect_defs(etype);
        let col = crate::panels::browser::fx_color(etype);

        ui.horizontal(|ui| {
            let mut en = enabled;
            let label = if en { "ON" } else { "OFF" };
            if ui.toggle_value(&mut en, egui::RichText::new(label).size(9.0)).changed() {
                if let Some(t) = self.project.track_by_id_mut(tid) {
                    t.fx[idx].enabled = en;
                }
                self.mark_graph_dirty();
            }
            ui.label(egui::RichText::new(etype.name()).size(14.0).color(col));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button("Remove").clicked() {
                    if let Some(t) = self.project.track_by_id_mut(tid) {
                        t.fx.remove(idx);
                    }
                    self.mark_graph_dirty();
                    return;
                }
            });
        });

        // EQ gets a response curve
        if etype == EffectType::Eq3 {
            let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 90.0), Sense::hover());
            let p = ui.painter_at(rect);
            p.rect_filled(rect, 4.0, Color32::from_rgb(11, 15, 26));
            let low = params.first().copied().unwrap_or(0.0);
            let mid = params.get(2).copied().unwrap_or(0.0);
            let high = params.get(4).copied().unwrap_or(0.0);
            let eq = aurora_engine::dsp::Eq3::new(48000.0);
            let mut eq = eq;
            eq.mid_freq = params.get(1).copied().unwrap_or(1000.0);
            eq.update(48000.0, low, mid, high, params.get(3).copied().unwrap_or(0.9));
            let steps = 64;
            let mut pts = Vec::new();
            for s in 0..=steps {
                let f = 20.0 * (20000.0 / 20.0f32).powf(s as f32 / steps as f32);
                let db = 20.0 * eq.response(f, 48000.0).log10();
                let y = rect.center().y - db * 1.6;
                let x = rect.left() + rect.width() * s as f32 / steps as f32;
                pts.push(Pos2::new(x, y.clamp(rect.top() + 4.0, rect.bottom() - 4.0)));
            }
            p.add(egui::Shape::line(pts, Stroke::new(2.0, col)));
            p.line_segment(
                [Pos2::new(rect.left(), rect.center().y), Pos2::new(rect.right(), rect.center().y)],
                Stroke::new(0.6, Theme::BORDER_HI),
            );
        }

        // knobs grid
        let mut changed_params = params.clone();
        let mut any = false;
        egui::Grid::new("fxknobs").num_columns(5).spacing([14.0, 6.0]).show(ui, |ui| {
            for (i, d) in defs.iter().enumerate() {
                let mut v = changed_params[i];
                // knob with label
                let size = Vec2::splat(46.0);
                let (rect, resp) = ui.allocate_exact_size(size, Sense::click_and_drag());
                let painter = ui.painter_at(rect);
                let c = rect.center();
                let r = 15.0;
                let t = ((v - d.min) / (d.max - d.min)).clamp(0.0, 1.0);
                let tt = if d.log { ((v.max(d.min) / d.min).log10() / (d.max / d.min).log10()).clamp(0.0, 1.0) } else { t };
                if resp.dragged() {
                    let dy = -resp.drag_delta().y * 0.006;
                    let mut nt = (tt + dy).clamp(0.0, 1.0);
                    if d.log {
                        nt = nt.clamp(0.0, 1.0);
                        v = d.min * (d.max / d.min).powf(nt);
                    } else {
                        v = d.min + nt * (d.max - d.min);
                    }
                    changed_params[i] = v;
                    any = true;
                }
                if resp.double_clicked() {
                    v = d.default;
                    changed_params[i] = v;
                    any = true;
                }
                let t_show = ((changed_params[i] - d.min) / (d.max - d.min)).clamp(0.0, 1.0);
                let a0 = std::f32::consts::PI * 0.75;
                let a1 = std::f32::consts::PI * 2.25;
                let seg = 24;
                let tseg = (t_show * seg as f32).ceil() as usize;
                let mut pts = Vec::new();
                for k in 0..=seg {
                    let a = a0 + (a1 - a0) * (k as f32 / seg as f32);
                    pts.push(Pos2::new(c.x + a.cos() * r, c.y + a.sin() * r));
                }
                painter.add(egui::Shape::line(pts, Stroke::new(3.0, Theme::BORDER)));
                let mut pts = Vec::new();
                for k in 0..=tseg {
                    let a = a0 + (a1 - a0) * (k as f32 / seg as f32);
                    pts.push(Pos2::new(c.x + a.cos() * r, c.y + a.sin() * r));
                }
                if pts.len() > 1 {
                    painter.add(egui::Shape::line(pts, Stroke::new(3.0, col)));
                }
                let pa = a0 + (a1 - a0) * t_show;
                painter.line_segment(
                    [Pos2::new(c.x + pa.cos() * r * 0.2, c.y + pa.sin() * r * 0.2), Pos2::new(c.x + pa.cos() * r * 0.82, c.y + pa.sin() * r * 0.82)],
                    Stroke::new(2.0, Theme::TEXT),
                );
                painter.text(
                    Pos2::new(c.x, rect.bottom() - 4.0),
                    egui::Align2::CENTER_CENTER,
                    d.name,
                    egui::FontId::proportional(8.0),
                    Theme::TEXT_DIM,
                );
                let unit = if d.unit.is_empty() { String::new() } else { format!(" {}", d.unit) };
                painter.text(
                    Pos2::new(c.x, rect.bottom() + 8.0),
                    egui::Align2::CENTER_CENTER,
                    format!("{:.1}{unit}", changed_params[i]),
                    crate::theme::Theme::mono(8.5),
                    Theme::TEXT,
                );
                if (i + 1) % 5 == 0 {
                    ui.end_row();
                }
            }
        });

        // sends
        let (rs0, ds0) = self
            .project
            .track_by_id(tid)
            .map(|t| (t.reverb_send, t.delay_send))
            .unwrap_or((0.0, 0.0));
        ui.separator();
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Reverb send").size(9.5).color(Theme::TEXT_DIM));
            let mut rs = rs0;
            if ui.add(egui::Slider::new(&mut rs, 0.0..=1.0).show_value(false)).changed() {
                if let Some(t) = self.project.track_by_id_mut(tid) {
                    t.reverb_send = rs;
                }
                self.mark_graph_dirty();
            }
            ui.label(egui::RichText::new("Delay send").size(9.5).color(Theme::TEXT_DIM));
            let mut ds = ds0;
            if ui.add(egui::Slider::new(&mut ds, 0.0..=1.0).show_value(false)).changed() {
                if let Some(t) = self.project.track_by_id_mut(tid) {
                    t.delay_send = ds;
                }
                self.mark_graph_dirty();
            }
        });

        if any {
            if let Some(t) = self.project.track_by_id_mut(tid) {
                t.fx[idx].params = changed_params;
            }
            self.mark_graph_dirty();
        }
        let _ = types;
    }

    // ------------------------------------------------------------------
    // Dialogs
    // ------------------------------------------------------------------

    pub fn draw_dialogs(&mut self, ctx: &egui::Context) {
        // export dialog
        if let Some(mut dlg) = self.export_dlg.take() {
            let mut cancel = false;
            let mut start = false;
            egui::Window::new("Export / Bounce")
                .open(&mut {
                    let mut open = true;
                    cancel = !open;
                    open
                })
                .resizable(false)
                .collapsible(false)
                .frame(
                    egui::Frame::none()
                        .fill(Theme::PANEL2)
                        .stroke(Stroke::new(1.0, Theme::BORDER_HI))
                        .rounding(Theme::R)
                        .inner_margin(egui::Margin::same(14.0)),
                )
                .show(ctx, |ui| {
                    ui.set_width(380.0);
                    ui.label(egui::RichText::new("Bounce your mix to a release-ready file").size(12.0).color(Theme::TEXT));
                    ui.add_space(6.0);
                    egui::Grid::new("expgrid").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                        ui.label("Format");
                        ui.horizontal(|ui| {
                            for f in [ExportFormat::Wav16, ExportFormat::Wav24, ExportFormat::Wav32F, ExportFormat::Mp3] {
                                if ui.selectable_label(dlg.format == f, f.name()).clicked() {
                                    dlg.format = f;
                                }
                            }
                        });
                        ui.end_row();
                        ui.label("Sample rate");
                        ui.horizontal(|ui| {
                            for sr in [44100u32, 48000u32] {
                                if ui.selectable_label(dlg.sample_rate == sr, format!("{sr}")).clicked() {
                                    dlg.sample_rate = sr;
                                }
                            }
                        });
                        ui.end_row();
                        ui.label("Stems");
                        ui.checkbox(&mut dlg.stems, "Export each track separately");
                        ui.end_row();
                        ui.label("File name");
                        ui.add(egui::TextEdit::singleline(&mut dlg.name).desired_width(200.0));
                        ui.end_row();
                        ui.label("Directory");
                        ui.add(egui::TextEdit::singleline(&mut dlg.dir).desired_width(200.0));
                        ui.end_row();
                        ui.label("Range");
                        ui.horizontal(|ui| {
                            ui.checkbox(&mut dlg.range_full, "Full project");
                            if !dlg.range_full {
                                ui.add(egui::DragValue::new(&mut dlg.from).speed(0.1).suffix("s"));
                                ui.add(egui::DragValue::new(&mut dlg.to).speed(0.1).suffix("s"));
                            }
                        });
                        ui.end_row();
                    });
                    ui.add_space(8.0);
                    // running jobs
                    for (label, h) in &self.active_jobs {
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new(format!("{} {}%", label, h.percent())).size(9.5).color(Theme::CYAN));
                            ui.add(egui::ProgressBar::new(h.percent() as f32 / 100.0).desired_height(8.0));
                        });
                    }
                    ui.separator();
                    ui.horizontal(|ui| {
                        if ui.button("Cancel").clicked() {
                            cancel = true;
                        }
                        if ui
                            .add(
                                egui::Button::new(egui::RichText::new("Start Bounce").color(Theme::BG))
                                    .fill(Theme::CYAN),
                            )
                            .clicked()
                        {
                            start = true;
                        }
                    });
                });
            if start {
                let d2 = dlg.clone();
                self.export_dlg = None;
                self.start_export(&d2);
            } else if cancel {
                self.export_dlg = None;
            } else {
                self.export_dlg = Some(dlg);
            }
        }

        // about
        if self.about_open {
            let mut open = true;
            egui::Window::new("About").open(&mut open).resizable(false).show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("AURORA").size(22.0).color(Theme::CYAN));
                    ui.label(egui::RichText::new("Producer Suite").size(14.0).color(Theme::TEXT_DIM));
                });
                ui.label(format!("Version {}", aurora_engine::version()));
                ui.label("Rust real-time audio engine · AI vocal cleanup · professional FX rack");
                ui.label("Engine: lock-free mixer graph · BS.1770 loudness · offline bounce parity");
            });
            self.about_open = open;
        }
    }

    pub fn draw_toasts(&mut self, ctx: &egui::Context) {
        let now = std::time::Instant::now();
        self.toasts.retain(|(_, t)| now.duration_since(*t).as_secs_f64() < 4.0);
        egui::Area::new(egui::Id::new("toasts"))
            .anchor(egui::Align2::CENTER_BOTTOM, [0.0, -20.0])
            .show(ctx, |ui| {
                ui.vertical(|ui| {
                    for (msg, _) in &self.toasts {
                        crate::theme::card(ui, |ui| {
                            ui.label(egui::RichText::new(msg).size(11.0).color(Theme::TEXT));
                        });
                    }
                });
            });
    }

    // ------------------------------------------------------------------
    // Piano roll
    // ------------------------------------------------------------------

    pub fn draw_piano_roll(&mut self, ctx: &egui::Context) {
        let mut close = false;
        if let Some((tid, cid)) = self.piano_roll {
            let (name, notes_len) = self
                .project
                .track_by_id(tid)
                .and_then(|t| t.clips.iter().find(|c| c.id == cid))
                .map(|c| (c.name.clone(), c.notes.as_ref().map(|n| n.len()).unwrap_or(0)))
                .unwrap_or_default();
            if self.project.track_by_id(tid).map(|t| t.kind) != Some(TrackKind::Instrument) {
                close = true;
            } else {
                let mut open = true;
                egui::Window::new(format!("Piano Roll — {name} ({notes_len} notes)"))
                    .open(&mut open)
                    .default_size(Vec2::new(860.0, 460.0))
                    .frame(
                        egui::Frame::none()
                            .fill(Theme::PANEL2)
                            .stroke(Stroke::new(1.0, Theme::BORDER_HI))
                            .rounding(Theme::R)
                            .inner_margin(egui::Margin::same(10.0)),
                    )
                    .show(ctx, |ui| {
                        if self.piano_roll_body(ui, tid, cid) {
                            close = true;
                        }
                    });
                if !open {
                    close = true;
                }
            }
        }
        if close {
            self.piano_roll = None;
        }
    }

    fn piano_roll_body(&mut self, ui: &mut egui::Ui, tid: u64, cid: u64) -> bool {
        const KEYS: u8 = 36; // 3 octaves visible
        const KEY_H: f32 = 12.0;
        let Some(track) = self.project.track_by_id(tid) else { return true };
        let Some(clip) = track.clips.iter().find(|c| c.id == cid) else { return true };
        let length = clip.length.max(1.0);
        let tempo = self.project.tempo;
        let grid: Vec<Note> = clip.notes.clone().unwrap_or_default();
        let col = Color32::from_rgb(track.color[0], track.color[1], track.color[2]);

        let mut out_notes = grid.clone();
        let mut actions: Vec<PianoAction> = Vec::new();

        let (rect, resp) = ui.allocate_exact_size(
            Vec2::new(ui.available_width(), KEYS as f32 * KEY_H + 26.0),
            Sense::click_and_drag(),
        );
        let painter = ui.painter_at(rect);
        let keys_w = 44.0;
        let grid_rect = Rect::from_min_max(Pos2::new(rect.left() + keys_w, rect.top() + 22.0), Pos2::new(rect.right(), rect.bottom()));

        // note grid background
        for k in 0..KEYS {
            let y = grid_rect.top() + k as f32 * KEY_H;
            let is_black = matches!(k % 12, 1 | 3 | 6 | 8 | 10);
            painter.rect_filled(
                Rect::from_min_max(Pos2::new(grid_rect.left(), y), Pos2::new(grid_rect.right(), y + KEY_H)),
                0.0,
                if is_black { Color32::from_rgb(14, 19, 31) } else { Color32::from_rgb(18, 24, 38) },
            );
        }
        // beat lines
        let beats = length * tempo / 60.0;
        let px_per_beat = grid_rect.width() / beats as f32;
        let mut b = 0.0;
        while b <= beats {
            let x = grid_rect.left() + b as f32 * px_per_beat;
            painter.line_segment(
                [Pos2::new(x, grid_rect.top()), Pos2::new(x, grid_rect.bottom())],
                Stroke::new(if b as i64 % 4 == 0 { 1.0 } else { 0.4 }, Theme::BORDER),
            );
            b += 1.0;
        }
        // notes
        for (i, n) in out_notes.iter().enumerate() {
            let key_idx = n.key.min(KEYS - 1);
            let nx0 = grid_rect.left() + n.start_beats * px_per_beat;
            let nx1 = grid_rect.left() + (n.start_beats + n.len_beats) * px_per_beat;
            let ny = grid_rect.top() + key_idx as f32 * KEY_H;
            let nr = Rect::from_min_max(Pos2::new(nx0 + 1.0, ny + 1.0), Pos2::new(nx1.max(nx0 + 3.0), ny + KEY_H - 1.0));
            let selected = self.selected_clip == Some(cid);
            painter.rect_filled(nr, 2.0, if selected { col } else { col.linear_multiply(0.8) });
            let _ = i;
        }
        // keys column
        for k in 0..KEYS {
            let y = grid_rect.top() + k as f32 * KEY_H;
            let is_black = matches!(k % 12, 1 | 3 | 6 | 8 | 10);
            painter.rect_filled(
                Rect::from_min_max(Pos2::new(rect.left(), y), Pos2::new(rect.left() + keys_w - 4.0, y + KEY_H)),
                0.0,
                if is_black { Theme::CARD } else { Theme::TEXT },
            );
            if k % 12 == 0 {
                painter.text(
                    Pos2::new(rect.left() + 4.0, y + KEY_H / 2.0),
                    egui::Align2::LEFT_CENTER,
                    format!("C{}", 2 + (KEYS - 1 - k) / 12),
                    egui::FontId::proportional(8.0),
                    Theme::BG,
                );
            }
        }

        // interactions
        if let Some(ptr) = resp.interact_pointer_pos() {
            if grid_rect.contains(ptr) {
                let beat = ((ptr.x - grid_rect.left()) / px_per_beat).max(0.0);
                let key = (((ptr.y - grid_rect.top()) / KEY_H) as u8).min(KEYS - 1);
                if resp.secondary_clicked() {
                    actions.push(PianoAction::Delete(beat, key));
                } else if resp.clicked() {
                    actions.push(PianoAction::Add(beat, key));
                }
            }
        }

        let mut changed = false;
        for a in actions {
            match a {
                PianoAction::Add(beat, key) => {
                    out_notes.push(Note {
                        start_beats: (beat * 2.0).round() / 2.0,
                        len_beats: 0.5,
                        key,
                        vel: 0.9,
                    });
                    changed = true;
                }
                PianoAction::Delete(beat, key) => {
                    let before = out_notes.len();
                    out_notes.retain(|n| {
                        let overlap = beat >= n.start_beats && beat <= n.start_beats + n.len_beats;
                        !(overlap && (n.key == key || ((beat - n.start_beats) * px_per_beat) < 8.0))
                    });
                    changed |= out_notes.len() != before;
                }
            }
        }
        if changed {
            if let Some(t) = self.project.track_by_id_mut(tid) {
                if let Some(c) = t.clips.iter_mut().find(|c| c.id == cid) {
                    c.notes = Some(out_notes.clone());
                }
            }
            self.mark_graph_dirty();
        }

        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new("Click: add note · Right-click: delete · Plays through the synth engine")
                    .size(9.0)
                    .color(Theme::TEXT_FAINT),
            );
            let mut do_close = false;
            let mut do_close_flag = false;
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button("Close").clicked() {
                    do_close = true;
                }
                if ui.small_button("Clear").clicked() {
                    if let Some(t) = self.project.track_by_id_mut(tid) {
                        if let Some(c) = t.clips.iter_mut().find(|c| c.id == cid) {
                            c.notes = Some(Vec::new());
                        }
                    }
                    self.mark_graph_dirty();
                }
            });
            do_close_flag = do_close;
        });
        false
    }
}

enum PianoAction {
    Add(f32, u8),
    Delete(f32, u8),
}
