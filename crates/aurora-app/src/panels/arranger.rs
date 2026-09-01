//! Arranger — ruler, track headers, waveform/MIDI clips, playhead, editing.
//! Handles thousands of tracks via row virtualization and precomputed peaks.

use crate::app::{AuroraApp, Tool};
use crate::theme::Theme;
use aurora_engine::project::{Clip, ClipId, TrackId, TrackKind};
use std::sync::atomic::Ordering;
use egui::{Color32, Pos2, Rect, Sense, Stroke, Vec2};

const HEADER_W: f32 = 176.0;
const RULER_H: f32 = 26.0;

impl AuroraApp {
    pub fn draw_arranger(&mut self, ctx: &egui::Context) {
        let mut edit_requests: Vec<ArrangerAction> = Vec::new();

        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(Theme::BG))
            .show(ctx, |ui| {
                let avail = ui.available_size();
                let arranger_h = if self.mixer_open {
                    avail.y * self.arranger_share
                } else {
                    avail.y
                };

                ui.allocate_ui(Vec2::new(avail.x, arranger_h), |ui| {
                    ui.vertical(|ui| {
                        // ---- toolbar row ----
                        ui.horizontal(|ui| {
                            ui.add_space(4.0);
                            for (i, (name, tool)) in [("SELECT", Tool::Select), ("CUT", Tool::Cut), ("DRAW", Tool::Draw)].iter().enumerate() {
                                let sel = self.tool == *tool;
                                let txt = egui::RichText::new(*name).size(9.0).color(if sel { Theme::BG } else { Theme::TEXT_DIM });
                                if ui.add(egui::Button::new(txt).fill(if sel { Theme::CYAN } else { Theme::CARD }).small()).clicked() {
                                    self.tool = *tool;
                                }
                                let _ = i;
                            }
                            ui.separator();
                            if ui.toggle_value(&mut self.snap, egui::RichText::new("SNAP").size(9.0)).changed() {}
                            ui.separator();
                            ui.label(egui::RichText::new("ZOOM").size(8.0).color(Theme::TEXT_FAINT));
                            ui.add(egui::Slider::new(&mut self.zoom, 8.0..=220.0).show_value(false));
                            ui.separator();
                            if ui.small_button(egui::RichText::new("+ AUDIO").size(9.0)).clicked() {
                                edit_requests.push(ArrangerAction::AddAudio);
                            }
                            if ui.small_button(egui::RichText::new("+ INSTRUMENT").size(9.0)).clicked() {
                                edit_requests.push(ArrangerAction::AddInstr);
                            }
                            if ui.small_button(egui::RichText::new("+ BUS").size(9.0)).clicked() {
                                edit_requests.push(ArrangerAction::AddBus);
                            }
                            ui.separator();
                            if ui.small_button(egui::RichText::new("SPLIT @ PLAYHEAD").size(9.0)).clicked() {
                                edit_requests.push(ArrangerAction::Split);
                            }
                            if ui.small_button(egui::RichText::new("DUPLICATE").size(9.0)).clicked() {
                                edit_requests.push(ArrangerAction::Dup);
                            }
                            if ui.small_button(egui::RichText::new("DELETE").size(9.0)).clicked() {
                                edit_requests.push(ArrangerAction::Del);
                            }
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                ui.label(
                                    egui::RichText::new(format!(
                                        "{} tracks · {} clips",
                                        self.project.tracks.len(),
                                        self.project.tracks.iter().map(|t| t.clips.len()).sum::<usize>()
                                    ))
                                    .size(9.0)
                                    .color(Theme::TEXT_FAINT),
                                );
                            });
                        });

                        // ---- timeline ----
                        let timeline = ui.allocate_ui(Vec2::new(ui.available_width(), ui.available_height()), |ui| {
                            self.draw_timeline(ui, &mut edit_requests);
                        });
                        let _ = timeline;
                    });
                });

                if self.mixer_open {
                    // divider drag handle
                    let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 6.0), Sense::drag());
                    let p = ui.painter_at(rect);
                    p.line_segment(
                        [Pos2::new(rect.left() + 2.0, rect.center().y), Pos2::new(rect.right() - 2.0, rect.center().y)],
                        Stroke::new(1.0, if resp.hovered() { Theme::CYAN } else { Theme::BORDER }),
                    );
                    if resp.dragged() {
                        let dy = -resp.drag_delta().y * 0.002;
                        self.arranger_share = (self.arranger_share + dy).clamp(0.25, 0.85);
                    }
                }
            });

        for a in edit_requests {
            match a {
                ArrangerAction::AddAudio => self.add_audio_track(),
                ArrangerAction::AddInstr => self.add_instrument_track(),
                ArrangerAction::AddBus => self.add_bus_track(),
                ArrangerAction::Split => {
                    let pos = self.engine_pos();
                    let (tid, cid) = match (self.selected_track, self.selected_clip) {
                        (Some(t), Some(c)) => (t, c),
                        _ => {
                            self.split_at_playhead_fallback(pos);
                            continue;
                        }
                    };
                    self.split_clip_at(tid, cid, pos);
                }
                ArrangerAction::Dup => self.duplicate_selected_clip(),
                ArrangerAction::Del => self.delete_selected_clip(),
                ArrangerAction::Select(tid, cid) => {
                    self.selected_track = Some(tid);
                    self.selected_clip = Some(cid);
                }
                ArrangerAction::OpenPiano(tid, cid) => {
                    self.piano_roll = Some((tid, cid));
                }
                ArrangerAction::DragStart(tid, cid, x) => {
                    self.selected_track = Some(tid);
                    self.selected_clip = Some(cid);
                    self.drag_clip = Some((tid, cid, x));
                }
                ArrangerAction::DragMove(tid, cid, dx_sec) => {
                    if let Some(t) = self.project.track_by_id_mut(tid) {
                        if let Some(c) = t.clips.iter_mut().find(|c| c.id == cid) {
                            c.start = (c.start + dx_sec).max(0.0);
                        }
                    }
                    if let Some((_, _, grab_x)) = self.drag_clip {
                        // keep grab anchored: store moved position
                        self.drag_clip = Some((tid, cid, grab_x));
                    }
                    self.mark_graph_dirty();
                }
                ArrangerAction::Seek(sec) => {
                    let snapped = if self.snap {
                        let bar = 60.0 / self.project.tempo * 4.0;
                        (sec / bar).round() * bar
                    } else {
                        sec
                    };
                    self.seek(snapped.max(0.0));
                }
                ArrangerAction::ToggleFlag(tid, k) => {
                    if let Some(t) = self.project.track_by_id_mut(tid) {
                        match k {
                            0 => t.solo = !t.solo,
                            1 => t.mute = !t.mute,
                            _ => t.armed = !t.armed,
                        }
                    }
                }
            }
        }
    }

    fn split_at_playhead_fallback(&mut self, pos: f64) {
        if let Some(tid) = self.selected_track {
            let cid = self.project.track_by_id(tid).and_then(|t| {
                t.clips
                    .iter()
                    .find(|c| pos > c.start + 0.01 && pos < c.end() - 0.01)
                    .map(|c| c.id)
            });
            if let Some(cid) = cid {
                self.split_clip_at(tid, cid, pos);
                self.status = "Clip split at playhead".into();
            }
        }
    }

    fn draw_timeline(&mut self, ui: &mut egui::Ui, actions: &mut Vec<ArrangerAction>) {
        let width = ui.available_width();
        let height = ui.available_height();
        let (rect, resp) = ui.allocate_exact_size(Vec2::new(width, height), Sense::click_and_drag());
        let painter = ui.painter_at(rect);
        let zoom = self.zoom;
        let px_per_sec = zoom;
        let scroll_x = self.h_scroll;

        let total_rows: usize = self.project.tracks.len();
        let row_heights: Vec<f32> = self.project.tracks.iter().map(|t| t.height).collect();
        let total_h: f32 = row_heights.iter().sum();

        // ---------------- wheel scrolling ----------------
        let scroll = ui.input(|i| i.smooth_scroll_delta);
        if scroll != egui::Vec2::ZERO && resp.hovered() {
            let total_h_clamped = (total_h - (height - RULER_H)).max(0.0);
            self.v_scroll = (self.v_scroll - scroll.y).clamp(0.0, total_h_clamped);
            self.h_scroll = (self.h_scroll + scroll.x as f64 / px_per_sec as f64).max(0.0);
        }

        // ---------------- track lanes + headers ----------------
        let content_x = rect.left() + HEADER_W;

        // visible time range
        let t0 = scroll_x;
        let t1 = scroll_x + (width - HEADER_W) as f64 / px_per_sec as f64;

        // ---- rows (visible window w/ vertical scroll) ----
        let mut acc = 0.0f32;
        let mut visible: Vec<(usize, f32)> = Vec::new(); // (track idx, y offset)
        for (i, h) in row_heights.iter().enumerate() {
            let y0 = rect.top() + RULER_H + acc - self.v_scroll;
            if y0 + *h >= rect.top() + RULER_H && y0 <= rect.bottom() {
                visible.push((i, y0));
            }
            acc += *h;
        }

        // playhead position
        let pos = self.engine_pos();
        let pos_x = content_x + ((pos - t0) * px_per_sec as f64) as f32;

        for &(ti, y0) in &visible {
            let t = &self.project.tracks[ti];
            let row = Rect::from_min_size(Pos2::new(rect.left(), y0), Vec2::new(width, t.height));
            // lane background
            let lane_fill = if Some(t.id) == self.selected_track {
                Color32::from_rgb(18, 26, 42)
            } else {
                Color32::from_rgb(12, 17, 28)
            };
            painter.rect_filled(row, 0.0, lane_fill);
            painter.line_segment(
                [Pos2::new(rect.left(), row.bottom()), Pos2::new(rect.right(), row.bottom())],
                Stroke::new(0.6, Theme::BORDER),
            );

            // ---- header ----
            let hdr = Rect::from_min_size(Pos2::new(rect.left() + 2.0, y0 + 1.0), Vec2::new(HEADER_W - 6.0, t.height - 3.0));
            let hcol = Color32::from_rgb(t.color[0], t.color[1], t.color[2]);
            let hdr_fill = if Some(t.id) == self.selected_track { Theme::CARD_HI } else { Theme::CARD };
            painter.rect_filled(hdr, Theme::R_SM, hdr_fill);
            painter.rect_filled(
                Rect::from_min_size(Pos2::new(hdr.left(), hdr.top()), Vec2::new(3.0, hdr.height())),
                2.0,
                hcol,
            );
            // index + name
            painter.text(
                Pos2::new(hdr.left() + 10.0, hdr.top() + 7.0),
                egui::Align2::LEFT_CENTER,
                format!("{}", ti + 1),
                crate::theme::Theme::mono(9.0),
                Theme::TEXT_FAINT,
            );
            let kind_tag = match t.kind {
                TrackKind::Instrument => "INST",
                TrackKind::Bus => "BUS",
                TrackKind::Audio => "AUD",
            };
            painter.text(
                Pos2::new(hdr.left() + 30.0, hdr.top() + 7.0),
                egui::Align2::LEFT_CENTER,
                &t.name,
                egui::FontId::proportional(10.5),
                Theme::TEXT,
            );
            painter.text(
                Pos2::new(hdr.right() - 4.0, hdr.top() + 7.0),
                egui::Align2::RIGHT_CENTER,
                kind_tag,
                egui::FontId::proportional(7.0),
                hcol,
            );
            if !t.subtitle.is_empty() {
                painter.text(
                    Pos2::new(hdr.left() + 30.0, hdr.top() + 18.0),
                    egui::Align2::LEFT_CENTER,
                    &t.subtitle,
                    egui::FontId::proportional(7.5),
                    Theme::TEXT_FAINT,
                );
            }
            // S M R chips + meter
            let chip_y = hdr.top() + hdr.height() - 12.0;
            let mut flags_clicked: Option<u8> = None;
            for (k, (label, active, col)) in [
                ("S", t.solo, Theme::YELLOW),
                ("M", t.mute, Theme::RECORD),
                ("R", t.armed, Theme::PLAY),
            ]
            .iter()
            .enumerate()
            {
                let cr = Rect::from_min_size(Pos2::new(hdr.left() + 8.0 + k as f32 * 21.0, chip_y), Vec2::new(17.0, 12.0));
                painter.rect_filled(cr, 2.0, if *active { *col } else { Theme::BG });
                painter.rect_stroke(cr, 2.0, Stroke::new(0.6, if *active { *col } else { Theme::BORDER }));
                painter.text(
                    cr.center(),
                    egui::Align2::CENTER_CENTER,
                    *label,
                    egui::FontId::proportional(7.5),
                    if *active { Theme::BG } else { Theme::TEXT_DIM },
                );
                // hit test (pointer inside rect + clicked this frame)
                if let Some(ptr) = resp.interact_pointer_pos() {
                    if resp.clicked() && cr.contains(ptr) {
                        flags_clicked = Some(k as u8);
                    }
                }
            }
            // level meter
            let lvl = f32::from_bits(self.parts.meters.track_peak[ti.min(4095)].load(std::sync::atomic::Ordering::Relaxed));
            let lvl_db = 20.0 * lvl.max(1e-6).log10();
            let m01 = crate::theme::db_to_meter(lvl_db);
            let mr = Rect::from_min_size(Pos2::new(hdr.left() + 74.0, chip_y + 2.0), Vec2::new(hdr.right() - hdr.left() - 82.0, 7.0));
            painter.rect_filled(mr, 2.0, Color32::from_rgb(14, 20, 32));
            let mw = mr.width() * m01;
            if mw > 1.0 {
                painter.rect_filled(
                    Rect::from_min_size(mr.left_top(), Vec2::new(mw, mr.height())),
                    2.0,
                    crate::theme::meter_color(m01),
                );
            }

            if let Some(k) = flags_clicked {
                actions.push(ArrangerAction::ToggleFlag(t.id, k));
            }

            // ---- clips ----
            let active_take = t.active_take;
            for c in t.clips.iter().filter(|c| c.take_id == active_take || c.take_id == 0) {
                let cx0 = content_x + ((c.start - t0) * px_per_sec as f64) as f32;
                let cx1 = content_x + ((c.end() - t0) * px_per_sec as f64) as f32;
                if cx1 < content_x - 4.0 || cx0 > rect.right() + 4.0 {
                    continue;
                }
                let clip_rect = Rect::from_min_max(
                    Pos2::new(cx0.max(content_x + 1.0), y0 + 2.0),
                    Pos2::new(cx1.min(rect.right() - 1.0), y0 + t.height - 2.0),
                );
                if clip_rect.width() < 2.0 {
                    continue;
                }
                let ccol = Color32::from_rgb(t.color[0], t.color[1], t.color[2]);
                let selected = self.selected_clip == Some(c.id) && self.selected_track == Some(t.id);
                // body
                painter.rect_filled(
                    clip_rect,
                    3.0,
                    Color32::from_rgb(
                        (ccol.r() as f32 * 0.32) as u8,
                        (ccol.g() as f32 * 0.30) as u8,
                        (ccol.b() as f32 * 0.36) as u8,
                    ),
                );
                painter.rect_stroke(
                    clip_rect,
                    3.0,
                    Stroke::new(if selected { 1.6 } else { 0.8 }, if selected { Color32::WHITE } else { ccol }),
                );
                // waveform / notes
                if clip_rect.width() > 4.0 {
                    let wave_rect = Rect::from_min_max(
                        Pos2::new(clip_rect.left() + 2.0, clip_rect.top() + 2.0),
                        Pos2::new(clip_rect.right() - 2.0, clip_rect.bottom() - 2.0),
                    );
                    if let (Some(peaks), Some(_audio)) = (&c.peaks, &c.audio) {
                        let n = peaks.len() as f64;
                        let cy = wave_rect.center().y;
                        let half = wave_rect.height() * 0.5;
                        let cols = (wave_rect.width() as i32).max(1) as usize;
                        let mut mesh = egui::Mesh::default();
                        let base_px = (c.start - t0) * px_per_sec as f64;
                        for px in 0..cols {
                            let sec = t0 + (base_px + px as f64) / px_per_sec as f64;
                            let rel = (sec - c.start) / c.length.max(1e-9);
                            let bi = ((rel * n) as usize).min(peaks.len() - 1);
                            let (mn, mx) = peaks[bi];
                            let x = wave_rect.left() + px as f32;
                            let y_top = cy - mx * half;
                            let y_bot = cy - mn * half;
                            let col = Color32::from_rgb(
                                (ccol.r() as f32 * 1.05).min(255.0) as u8,
                                (ccol.g() as f32 * 1.05).min(255.0) as u8,
                                (ccol.b() as f32 * 1.1).min(255.0) as u8,
                            );
                            let a = mesh.vertices.len() as u32;
                            mesh.colored_vertex(Pos2::new(x, y_top), col);
                            mesh.colored_vertex(Pos2::new(x, y_bot + 1.0), col);
                            mesh.colored_vertex(Pos2::new(x + 1.0, y_top), col);
                            mesh.colored_vertex(Pos2::new(x + 1.0, y_bot + 1.0), col);
                            mesh.add_triangle(a, a + 1, a + 2);
                            mesh.add_triangle(a + 2, a + 1, a + 3);
                        }
                        painter.add(egui::Shape::mesh(mesh));
                    } else if let Some(notes) = &c.notes {
                        // MIDI clip: draw note blocks
                        let beats_total = ((c.length * self.project.tempo / 60.0) as f32).max(1.0);
                        for note in notes {
                            let nx0 = wave_rect.left() + (note.start_beats / beats_total) * wave_rect.width();
                            let nx1 = wave_rect.left() + ((note.start_beats + note.len_beats) / beats_total) * wave_rect.width();
                            let nh = wave_rect.height() / 24.0;
                            let ny = wave_rect.top() + (1.0 - (note.key as f32 / 87.0)) * (wave_rect.height() - nh);
                            painter.rect_filled(
                                Rect::from_min_max(Pos2::new(nx0, ny), Pos2::new(nx1.max(nx0 + 2.0), ny + nh - 1.0)),
                                1.5,
                                ccol,
                            );
                        }
                    }
                }
                // label
                if clip_rect.width() > 40.0 && t.height > 26.0 {
                    painter.text(
                        Pos2::new(clip_rect.left() + 5.0, clip_rect.top() + 7.0),
                        egui::Align2::LEFT_CENTER,
                        &c.name,
                        egui::FontId::proportional(8.0),
                        Color32::from_rgb(240, 244, 250),
                    );
                }

                // clip interactions (hit test via pointer + click/drag)
                if let Some(ptr) = resp.interact_pointer_pos() {
                    if clip_rect.contains(ptr) {
                        if resp.clicked() {
                            actions.push(ArrangerAction::Select(t.id, c.id));
                        }
                        if resp.double_clicked() && c.notes.is_some() {
                            actions.push(ArrangerAction::OpenPiano(t.id, c.id));
                        }
                        if resp.drag_started() {
                            actions.push(ArrangerAction::DragStart(t.id, c.id, ptr.x));
                        }
                    }
                }
            }
        }

        // ---- drag move clip ----
        if resp.dragged() {
            if let Some((tid, cid, grab_x)) = self.drag_clip {
                let dx_sec = (resp.interact_pointer_pos().unwrap_or(Pos2::ZERO).x - grab_x) as f64 / px_per_sec as f64;
                actions.push(ArrangerAction::DragMove(tid, cid, dx_sec));
            } else if resp.drag_delta().x != 0.0 && self.drag_clip.is_none() && self.drag_lane.is_none() {
                // horizontal scroll on empty drag
                self.h_scroll = (self.h_scroll - resp.drag_delta().x as f64 / px_per_sec as f64).max(0.0);
            }
        }
        if resp.drag_stopped() {
            self.drag_clip = None;
        }

        // ---- ruler ----
        let ruler = Rect::from_min_max(Pos2::new(rect.left(), rect.top()), Pos2::new(rect.right(), rect.top() + RULER_H));
        painter.rect_filled(ruler, 0.0, Theme::PANEL2);
        painter.line_segment(
            [Pos2::new(rect.left(), ruler.bottom()), Pos2::new(rect.right(), ruler.bottom())],
            Stroke::new(1.0, Theme::BORDER),
        );
        // ---- loop region (under the numbers, subtle like the design) ----
        if self.project.loop_enabled {
            let lx0 = content_x + ((self.project.loop_range.0 - t0) * px_per_sec as f64) as f32;
            let lx1 = content_x + ((self.project.loop_range.1 - t0) * px_per_sec as f64) as f32;
            painter.rect_filled(
                Rect::from_min_max(Pos2::new(lx0.max(content_x), ruler.top() + 3.0), Pos2::new(lx1.min(rect.right()), ruler.bottom() - 4.0)),
                2.0,
                Theme::CYAN.linear_multiply(0.13),
            );
            painter.line_segment(
                [Pos2::new(lx0.max(content_x), ruler.top() + 3.0), Pos2::new(lx0.max(content_x), ruler.bottom() - 4.0)],
                Stroke::new(1.2, Theme::CYAN.linear_multiply(0.75)),
            );
            painter.line_segment(
                [Pos2::new(lx1.min(rect.right()), ruler.top() + 3.0), Pos2::new(lx1.min(rect.right()), ruler.bottom() - 4.0)],
                Stroke::new(1.2, Theme::CYAN.linear_multiply(0.75)),
            );
        }

        let beat = 60.0 / self.project.tempo;
        let bar = beat * 4.0;
        let first_bar = (t0 / bar).floor() as i64;
        let last_bar = ((t1 / bar).ceil() as i64).max(first_bar + 1);
        let bar_px = bar * px_per_sec as f64;
        let step = ((12.0 / bar_px.max(1.0)).ceil() as i64).max(1);
        for b in first_bar..=last_bar {
            if b % step != 0 {
                continue;
            }
            let x = content_x + ((b as f64 * bar - t0) * px_per_sec as f64) as f32;
            if x < content_x || x > rect.right() {
                continue;
            }
            painter.line_segment(
                [Pos2::new(x, ruler.top() + 8.0), Pos2::new(x, ruler.bottom())],
                Stroke::new(0.8, Theme::BORDER_HI),
            );
            painter.text(
                Pos2::new(x + 3.0, ruler.top() + 9.0),
                egui::Align2::LEFT_CENTER,
                format!("{}", b + 1),
                egui::FontId::proportional(8.5),
                Theme::TEXT_DIM,
            );
        }
        // ruler click => seek
        if let Some(ptr) = resp.interact_pointer_pos() {
            if resp.dragged() && ptr.y < ruler.bottom() + 2.0 {
                let sec = t0 + ((ptr.x - content_x) as f64 / px_per_sec as f64);
                actions.push(ArrangerAction::Seek(sec.max(0.0)));
            } else if resp.clicked() && ptr.y < ruler.bottom() {
                let sec = t0 + ((ptr.x - content_x) as f64 / px_per_sec as f64);
                actions.push(ArrangerAction::Seek(sec.max(0.0)));
            }
        }

        // ---- loop region ----
        if self.project.loop_enabled {
            let lx0 = content_x + ((self.project.loop_range.0 - t0) * px_per_sec as f64) as f32;
            let lx1 = content_x + ((self.project.loop_range.1 - t0) * px_per_sec as f64) as f32;
            painter.rect_filled(
                Rect::from_min_max(Pos2::new(lx0.max(content_x), ruler.top() + 2.0), Pos2::new(lx1.min(rect.right()), ruler.bottom() - 2.0)),
                3.0,
                Theme::CYAN.linear_multiply(0.18),
            );
            painter.rect_stroke(
                Rect::from_min_max(Pos2::new(lx0.max(content_x), ruler.top() + 2.0), Pos2::new(lx1.min(rect.right()), ruler.bottom() - 2.0)),
                3.0,
                Stroke::new(1.0, Theme::CYAN.linear_multiply(0.7)),
            );
        }

        // ---- vertical scrollbar (when needed) ----
        let visible_h = height - RULER_H;
        if total_h > visible_h + 1.0 {
            let bar_w = 5.0;
            let track = Rect::from_min_max(Pos2::new(rect.right() - bar_w - 2.0, rect.top() + RULER_H), Pos2::new(rect.right() - 2.0, rect.bottom()));
            painter.rect_filled(track, 2.0, Color32::from_rgb(18, 24, 38));
            let thumb_h = (visible_h / total_h * track.height()).clamp(24.0, track.height());
            let max_off = total_h - visible_h;
            let ty = track.top() + (self.v_scroll / max_off.max(1.0)) * (track.height() - thumb_h);
            painter.rect_filled(Rect::from_min_size(Pos2::new(track.left(), ty), Vec2::new(bar_w, thumb_h)), 2.0, Theme::BORDER_HI);
        }

        // ---- playhead ----
        if pos_x >= content_x && pos_x <= rect.right() {
            painter.line_segment(
                [Pos2::new(pos_x, rect.top()), Pos2::new(pos_x, rect.bottom())],
                Stroke::new(1.6, Theme::CYAN),
            );
            let tri = [
                Pos2::new(pos_x - 5.0, rect.top()),
                Pos2::new(pos_x + 5.0, rect.top()),
                Pos2::new(pos_x, rect.top() + 7.0),
            ];
            painter.add(egui::Shape::convex_polygon(tri.to_vec(), Theme::CYAN, Stroke::NONE));
        }

        // vertical grid lines (bars)
        for b in first_bar..=last_bar {
            let x = content_x + ((b as f64 * bar - t0) * px_per_sec as f64) as f32;
            if x < content_x || x > rect.right() || b % step != 0 {
                continue;
            }
            painter.line_segment(
                [Pos2::new(x, rect.top() + RULER_H), Pos2::new(x, rect.bottom())],
                Stroke::new(0.5, Theme::BORDER.linear_multiply(0.5)),
            );
        }
    }
}

pub enum ArrangerAction {
    AddAudio,
    AddInstr,
    AddBus,
    Split,
    Dup,
    Del,
    Select(TrackId, ClipId),
    OpenPiano(TrackId, ClipId),
    DragStart(TrackId, ClipId, f32),
    DragMove(TrackId, ClipId, f64),
    Seek(f64),
    ToggleFlag(TrackId, u8),
}
