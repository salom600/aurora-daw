//! Right inspector — AI Mix Assistant, Smart Modules, Spectral Analyzer,
//! Loudness Meter, Vocal Cleaner card. Matches the reference design.

use crate::app::AuroraApp;
use crate::theme::{section_label, Theme};
use crate::widgets;
use egui::{Color32, Pos2, Rect, Stroke, Vec2};
use rustfft::num_complex::Complex;

impl AuroraApp {
    pub fn draw_inspector(&mut self, ctx: &egui::Context) {
        let mut run_clean = false;
        let mut run_mix = false;
        let mut apply_mix = false;

        egui::SidePanel::right("inspector")
            .exact_width(284.0)
            .frame(egui::Frame::none().fill(Theme::PANEL).inner_margin(egui::Margin::same(8.0)))
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
                    // ================= AI MIX ASSISTANT =================
                    section_label(ui, "AI Mix Assistant");
                    crate::theme::card(ui, |ui| {
                        ui.vertical(|ui| {
                            // circular visual
                            ui.with_layout(egui::Layout::top_down_justified(egui::Align::Center), |ui| {
                                let (rect, _) = ui.allocate_exact_size(Vec2::splat(74.0), egui::Sense::hover());
                                let p = ui.painter_at(rect);
                                let c = rect.center();
                                let pulse = 0.5 + 0.5 * (self.start_time.elapsed().as_secs_f32() * 2.0).sin();
                                p.circle_filled(c, 30.0 + pulse * 2.0, Theme::BLUE.linear_multiply(0.15));
                                p.circle_stroke(c, 26.0, Stroke::new(1.5, Theme::BLUE.linear_multiply(0.7)));
                                // waveform ring
                                let n = 40;
                                let mut pts = Vec::new();
                                let pl = f32::from_bits(self.parts.meters.master_rms_l.load(std::sync::atomic::Ordering::Relaxed));
                                for i in 0..=n {
                                    let a = i as f32 / n as f32 * std::f32::consts::TAU;
                                    let r = 14.0 + (a * 5.0).sin().abs() * 6.0 * (pl * 14.0 + 0.2);
                                    pts.push(Pos2::new(c.x + a.cos() * r, c.y + a.sin() * r * 0.55));
                                }
                                p.add(egui::Shape::line(pts, Stroke::new(1.4, Theme::CYAN)));
                                p.circle_filled(c, 3.0, Theme::PURPLE);
                            });
                            ui.add_space(4.0);
                            let status_text = if self.ai_mix.analyzing {
                                "Analyzing your mix…"
                            } else if self.ai_mix.analyzed {
                                if self.ai_mix.applied {
                                    "Applied — AI has optimized your mix"
                                } else {
                                    "Analysis Complete — ready to optimize"
                                }
                            } else {
                                "Let AI analyze levels, EQ and image"
                            };
                            ui.label(
                                egui::RichText::new(status_text)
                                    .size(10.5)
                                    .color(if self.ai_mix.analyzed { Theme::CYAN } else { Theme::TEXT_DIM }),
                            );
                            ui.add_space(6.0);
                            ui.with_layout(egui::Layout::top_down_justified(egui::Align::Center), |ui| {
                                let label = if self.ai_mix.analyzed && !self.ai_mix.applied {
                                    format!("Apply AI Mix ({} fixes)", self.ai_mix.suggestions.len())
                                } else if self.ai_mix.applied {
                                    "Re-analyze Mix".to_string()
                                } else {
                                    "Analyze Mix".to_string()
                                };
                                if ui
                                    .add(
                                        egui::Button::new(egui::RichText::new(label).size(11.0).color(Theme::BG))
                                            .fill(Theme::CYAN)
                                            .min_size(Vec2::new(ui.available_width() - 8.0, 24.0)),
                                    )
                                    .clicked()
                                {
                                    if self.ai_mix.analyzed && !self.ai_mix.applied && !self.ai_mix.suggestions.is_empty() {
                                        apply_mix = true;
                                    } else {
                                        run_mix = true;
                                    }
                                }
                            });
                            if self.ai_mix.analyzed {
                                ui.label(
                                    egui::RichText::new(format!(
                                        "Confidence {:.0}% · {} suggestions",
                                        self.ai_mix.confidence * 100.0,
                                        self.ai_mix.suggestions.len()
                                    ))
                                    .size(8.5)
                                    .color(Theme::TEXT_FAINT),
                                );
                            }
                        });
                    });

                    // suggestion list
                    if self.ai_mix.analyzed && !self.ai_mix.suggestions.is_empty() {
                        crate::theme::card(ui, |ui| {
                            egui::ScrollArea::vertical().max_height(108.0).show(ui, |ui| {
                                for s in self.ai_mix.suggestions.iter().take(10) {
                                    ui.horizontal(|ui| {
                                        let col = match s.kind.as_str() {
                                            "Gain" => Theme::GREEN,
                                            "Pan" => Theme::BLUE,
                                            "EQ" => Theme::CYAN,
                                            _ => Theme::PURPLE,
                                        };
                                        let (r, _) = ui.allocate_exact_size(Vec2::new(6.0, 6.0), egui::Sense::hover());
                                        ui.painter().circle_filled(r.center(), 3.0, col);
                                        ui.label(egui::RichText::new(&s.description).size(9.0).color(Theme::TEXT_DIM));
                                    });
                                }
                            });
                        });
                    }

                    // ================= SMART MODULES =================
                    section_label(ui, "Smart Modules");
                    let modules = [
                        ("AI EQ", "Balanced", self.ai_mix.analyzed, 0.65, Theme::CYAN),
                        ("AI Compression", "Optimal", self.ai_mix.analyzed, 0.72, Theme::BLUE),
                        ("AI Reverb", "Cinematic Hall", self.ai_mix.analyzed, 0.40, Theme::PURPLE),
                        ("AI Stereo Width", "Wide", self.ai_mix.analyzed, 0.80, Theme::PINK),
                        ("AI Loudness", "-14 LUFS", self.ai_mix.analyzed, 0.90, Theme::GREEN),
                    ];
                    for (name, sub, active, val, col) in modules {
                        crate::theme::card(ui, |ui| {
                            ui.horizontal(|ui| {
                                let (r, _) = ui.allocate_exact_size(Vec2::splat(22.0), egui::Sense::hover());
                                let p = ui.painter_at(r);
                                p.rect_filled(r, 4.0, col.linear_multiply(0.2));
                                p.rect_stroke(r, 4.0, Stroke::new(1.0, col));
                                p.text(r.center(), egui::Align2::CENTER_CENTER, "AI", egui::FontId::proportional(8.5), col);
                                ui.vertical(|ui| {
                                    ui.label(egui::RichText::new(name).size(10.5).color(Theme::TEXT));
                                    ui.label(egui::RichText::new(sub).size(8.5).color(Theme::TEXT_FAINT));
                                });
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    crate::widgets::ring(ui, 15.0, if active { val } else { 0.0 }, col, &format!("{:.0}", val * 100.0));
                                });
                            });
                        });
                    }

                    // ================= VOCAL CLEANER =================
                    section_label(ui, "AI Vocal Cleaner");
                    crate::theme::card(ui, |ui| {
                        ui.vertical(|ui| {
                            ui.label(
                                egui::RichText::new("One-click: noise · hum · clicks · breaths · sibilance")
                                    .size(9.5)
                                    .color(Theme::TEXT_DIM),
                            );
                            ui.add_space(4.0);
                            ui.horizontal(|ui| {
                                ui.label(egui::RichText::new("Intensity").size(9.0).color(Theme::TEXT_FAINT));
                                ui.add(egui::Slider::new(&mut self.cleaner.options.intensity, 0.2..=1.0).show_value(false));
                            });
                            ui.horizontal(|ui| {
                                ui.checkbox(&mut self.cleaner.options.remove_noise, egui::RichText::new("Noise").size(9.0));
                                ui.checkbox(&mut self.cleaner.options.remove_hum, egui::RichText::new("Hum").size(9.0));
                                ui.checkbox(&mut self.cleaner.options.remove_clicks, egui::RichText::new("Clicks").size(9.0));
                            });
                            ui.horizontal(|ui| {
                                ui.checkbox(&mut self.cleaner.options.remove_breaths, egui::RichText::new("Breaths").size(9.0));
                                ui.checkbox(&mut self.cleaner.options.de_ess, egui::RichText::new("De-Ess").size(9.0));
                                ui.checkbox(&mut self.cleaner.options.de_harsh, egui::RichText::new("De-Harsh").size(9.0));
                            });
                            ui.add_space(2.0);
                            // active jobs progress
                            for (label, h) in &self.active_jobs {
                                if h.kind == aurora_engine::jobs::JobKind::VocalCleanup {
                                    ui.horizontal(|ui| {
                                        ui.label(egui::RichText::new(format!("{:.0}%", h.percent())).size(8.5).color(Theme::CYAN));
                                        ui.add(egui::ProgressBar::new(h.percent() as f32 / 100.0).desired_height(6.0));
                                    });
                                    let _ = label;
                                }
                            }
                            ui.with_layout(egui::Layout::top_down_justified(egui::Align::Center), |ui| {
                                if ui
                                    .add(
                                        egui::Button::new(egui::RichText::new("Clean All Vocals (1-Click)").size(11.0).color(Color32::from_rgb(10, 14, 23)))
                                            .fill(Theme::GREEN)
                                            .min_size(Vec2::new(ui.available_width() - 8.0, 24.0)),
                                    )
                                    .clicked()
                                {
                                    run_clean = true;
                                }
                            });
                            // last reports
                            if let Some((name, r)) = self.cleaner.last_reports.last() {
                                ui.separator();
                                ui.label(egui::RichText::new(format!("Last run: {name}")).size(9.0).color(Theme::TEXT_DIM));
                                ui.label(
                                    egui::RichText::new(format!(
                                        "Noise −{:.1} dB · {} clicks · {} breaths · hum {:?}",
                                        r.noise_reduction_est_db,
                                        r.clicks_fixed,
                                        r.breaths_removed,
                                        r.hum_freqs
                                    ))
                                    .size(8.5)
                                    .color(Theme::GREEN),
                                );
                            }
                        });
                    });

                    // ================= SPECTRAL ANALYZER =================
                    section_label(ui, "Spectral Analyzer");
                    crate::theme::card(ui, |ui| {
                        let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 96.0), egui::Sense::hover());
                        let p = ui.painter_at(rect);
                        p.rect_filled(rect, 4.0, Color32::from_rgb(11, 15, 26));
                        // fetch spectrum
                        let spectrum = self.compute_spectrum();
                        let bars = 48;
                        let bw = rect.width() / bars as f32;
                        for b in 0..bars {
                            let t = b as f32 / bars as f32;
                            let v = spectrum.get(b).copied().unwrap_or(0.0);
                            let h = (rect.height() - 8.0) * v.clamp(0.0, 1.0);
                            let col = crate::theme::mix(Theme::BLUE, Theme::PURPLE, t);
                            p.rect_filled(
                                Rect::from_min_size(
                                    Pos2::new(rect.left() + b as f32 * bw + 1.0, rect.bottom() - 4.0 - h),
                                    Vec2::new(bw - 2.0, h),
                                ),
                                1.5,
                                col.linear_multiply(0.85),
                            );
                        }
                        // freq labels
                        for (f, lbl) in [(0, "50"), (12, "250"), (24, "1k"), (36, "4k"), (46, "16k")] {
                            p.text(
                                Pos2::new(rect.left() + f as f32 * bw + bw / 2.0, rect.bottom() - 8.0),
                                egui::Align2::CENTER_CENTER,
                                lbl,
                                egui::FontId::proportional(6.5),
                                Theme::TEXT_FAINT,
                            );
                        }
                    });

                    // ================= LOUDNESS METER =================
                    section_label(ui, "Loudness Meter");
                    crate::theme::card(ui, |ui| {
                        let lufs = f32::from_bits(self.parts.loudness.integrated_lu.load(std::sync::atomic::Ordering::Relaxed));
                        let m = f32::from_bits(self.parts.loudness.momentary_lu.load(std::sync::atomic::Ordering::Relaxed));
                        let tp = f32::from_bits(self.parts.loudness.true_peak_db.load(std::sync::atomic::Ordering::Relaxed));
                        ui.horizontal(|ui| {
                            ui.vertical(|ui| {
                                ui.label(egui::RichText::new(format!("{lufs:.1}")).size(19.0).color(Theme::CYAN));
                                ui.label(egui::RichText::new("LUFS INTEGRATED").size(6.5).color(Theme::TEXT_FAINT));
                            });
                            ui.separator();
                            ui.vertical(|ui| {
                                ui.label(egui::RichText::new(format!("{tp:.1}")).size(15.0).color(if tp > -1.0 { Theme::RECORD } else { Theme::TEXT }));
                                ui.label(egui::RichText::new("dBTP TRUE PEAK").size(6.5).color(Theme::TEXT_FAINT));
                            });
                            ui.separator();
                            ui.vertical(|ui| {
                                ui.label(egui::RichText::new(format!("{m:.1}")).size(15.0).color(Theme::TEXT_DIM));
                                ui.label(egui::RichText::new("LU MOMENTARY").size(6.5).color(Theme::TEXT_FAINT));
                            });
                        });
                        // history waveform (momentary)
                        let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 40.0), egui::Sense::hover());
                        let p = ui.painter_at(rect);
                        p.rect_filled(rect, 3.0, Color32::from_rgb(11, 15, 26));
                        let hist = self.loudness_history();
                        let n = hist.len().max(1);
                        let cy = rect.center().y;
                        let mut pts_top = Vec::new();
                        let mut pts_bot = Vec::new();
                        for (i, v) in hist.iter().enumerate() {
                            let x = rect.left() + rect.width() * (i as f32 / n as f32);
                            let h = v.clamp(0.0, 1.0) * (rect.height() / 2.0 - 2.0);
                            pts_top.push(Pos2::new(x, cy - h));
                            pts_bot.push(Pos2::new(x, cy + h));
                        }
                        pts_bot.reverse();
                        pts_top.extend(pts_bot);
                        if pts_top.len() > 2 {
                            p.add(egui::Shape::convex_polygon(pts_top, Theme::CYAN.linear_multiply(0.45), Stroke::NONE));
                        }
                        let target = -14.0f32;
                        let yn = rect.top() + rect.height() * (1.0 - ((target + 40.0) / 60.0).clamp(0.0, 1.0));
                        p.line_segment([Pos2::new(rect.left(), yn), Pos2::new(rect.right(), yn)], Stroke::new(0.8, Theme::YELLOW.linear_multiply(0.7)));
                    });

                    // session stats
                    section_label(ui, "Session");
                    ui.label(
                        egui::RichText::new(format!(
                            "Tracks: {}   Clips: {}   Duration: {:.1}s",
                            self.project.tracks.len(),
                            self.project.tracks.iter().map(|t| t.clips.len()).sum::<usize>(),
                            self.project.duration()
                        ))
                        .size(9.5)
                        .color(Theme::TEXT_DIM),
                    );
                    ui.label(
                        egui::RichText::new(format!(
                            "Engine: {} blocks · {} xruns · uptime {:.0}s",
                            self.parts.meters.blocks.load(std::sync::atomic::Ordering::Relaxed),
                            self.parts.meters.xruns.load(std::sync::atomic::Ordering::Relaxed),
                            self.start_time.elapsed().as_secs_f32()
                        ))
                        .size(9.0)
                        .color(Theme::TEXT_FAINT),
                    );
                    if let Some(exp) = &self.last_export {
                        ui.label(egui::RichText::new(format!("Last export: {exp}")).size(9.0).color(Theme::GREEN));
                    }
                    ui.add_space(10.0);
                });
            });

        if run_clean {
            self.ai_clean_vocals();
        }
        if run_mix {
            self.ai_mix_analyze();
        }
        if apply_mix {
            self.ai_mix_apply();
        }
    }

    fn compute_spectrum(&mut self) -> Vec<f32> {
        use std::sync::atomic::Ordering;
        let mut out = vec![0.0f32; 48];
        let snap: Vec<f32> = match self.parts.spectral.buf.try_lock() {
            Ok(b) => b.clone(),
            Err(_) => return out,
        };
        let _version = self.parts.spectral.version.load(Ordering::Relaxed);
        let n = snap.len();
        if n == 0 {
            return out;
        }
        let mut planner = rustfft::FftPlanner::<f32>::new();
        let fft = planner.plan_fft_forward(n);
        let mut buf: Vec<Complex<f32>> = snap
            .iter()
            .enumerate()
            .map(|(i, v)| {
                let w = 0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / n as f32).cos();
                Complex::new(v * w, 0.0)
            })
            .collect();
        fft.process(&mut buf);
        // log-ish band grouping: 48 bands from 20 Hz to 20 kHz
        let nyq = 24000.0;
        for b in 0..48 {
            let f0 = 20.0 * (nyq / 20.0f32).powf(b as f32 / 48.0);
            let f1 = 20.0 * (nyq / 20.0f32).powf((b + 1) as f32 / 48.0);
            let b0 = ((f0 / nyq) * (n / 2) as f32) as usize;
            let b1 = (((f1 / nyq) * (n / 2) as f32) as usize).clamp(b0 + 1, n / 2);
            let mag: f32 = buf[b0..b1].iter().map(|c| c.norm()).sum::<f32>() / (b1 - b0) as f32;
            out[b] = ((20.0 * mag.max(1e-7).log10()) + 60.0) / 70.0; // -60..+10 dB -> 0..1
        }
        // smooth in time
        for (o, n) in out.iter_mut().zip(self.spec_smooth.iter_mut()) {
            *n = *n * 0.75 + *o * 0.25;
            *o = *n;
        }
        out
    }

    fn loudness_history(&mut self) -> Vec<f32> {
        let m = f32::from_bits(self.parts.loudness.momentary_lu.load(std::sync::atomic::Ordering::Relaxed));
        self.lufs_history.push(((m + 40.0) / 60.0).clamp(0.0, 1.0));
        if self.lufs_history.len() > 160 {
            self.lufs_history.remove(0);
        }
        self.lufs_history.clone()
    }
}
