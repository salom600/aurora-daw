//! Top bar — logo, menus, time display, transport, tempo/key, CPU/DSP meters.

use crate::app::AuroraApp;
use crate::theme::Theme;
use crate::widgets::{self, TransportIcon};
use aurora_engine::io::ExportFormat;
use std::sync::atomic::Ordering;
use egui::{Color32, Pos2};

const PANEL_H: f32 = 46.0;

pub fn fmt_time_bars_beats(pos: f64, tempo: f64, sig: (u32, u32)) -> String {
    let beats = pos * tempo / 60.0;
    let per_bar = sig.1 as f64 * 4.0 / sig.0 as f64;
    let bar = (beats / per_bar).floor() as i64 + 1;
    let beat = (beats % per_bar).floor() as i64 + 1;
    let ticks = ((beats * 120.0) % 120.0).floor() as i64;
    format!("{:03}.{}.{}", bar, beat, ticks)
}

fn fmt_clock(pos: f64) -> String {
    let s = pos.floor();
    let m = (s / 60.0).floor();
    format!("{:02}:{:02}:{:02}", m as i64, (s % 60.0) as i64, ((pos - s) * 100.0).floor() as i64)
}

impl AuroraApp {
    pub fn draw_topbar(&mut self, ctx: &egui::Context) {
        let mut actions: Vec<Action> = Vec::new();
        egui::TopBottomPanel::top("topbar")
            .exact_height(PANEL_H)
            .frame(
                egui::Frame::none()
                    .fill(Theme::BG)
                    .inner_margin(egui::Margin::symmetric(10.0, 4.0)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    // ---- logo ----
                    draw_logo(ui);

                    // ---- menus (fixed width so transport fits) ----
                    ui.allocate_ui(egui::vec2(372.0, 28.0), |ui| {
                    egui::menu::bar(ui, |ui| {
                        ui.menu_button("File", |ui| {
                            if ui.button("New Project").clicked() {
                                actions.push(Action::NewProject);
                            }
                            if ui.button("Open Project…").clicked() {
                                actions.push(Action::OpenProject);
                            }
                            if ui.button("Save Project").clicked() {
                                actions.push(Action::SaveProject);
                            }
                            ui.separator();
                            if ui.button("Import Audio File…").clicked() {
                                actions.push(Action::ImportPrompt);
                            }
                            ui.separator();
                            if ui.button("Export Mix…").clicked() {
                                actions.push(Action::Export);
                            }
                            ui.separator();
                            if ui.button("Quit").clicked() {
                                actions.push(Action::Quit);
                            }
                        });
                        ui.menu_button("Edit", |ui| {
                            if ui.button("Duplicate Clip").clicked() {
                                actions.push(Action::DupClip);
                            }
                            if ui.button("Delete Clip").clicked() {
                                actions.push(Action::DelClip);
                            }
                        });
                        ui.menu_button("Track", |ui| {
                            if ui.button("Add Audio Track").clicked() {
                                actions.push(Action::AddAudio);
                            }
                            if ui.button("Add Instrument Track").clicked() {
                                actions.push(Action::AddInstr);
                            }
                            if ui.button("Add Bus").clicked() {
                                actions.push(Action::AddBus);
                            }
                            ui.separator();
                            if ui.button("Delete Selected Track").clicked() {
                                actions.push(Action::DelTrack);
                            }
                        });
                        ui.menu_button("View", |ui| {
                            if ui.checkbox(&mut self.mixer_open, "Show Mixer").changed() {}
                        });
                        ui.menu_button("Transport", |ui| {
                            if ui.button("Play  (Space)").clicked() {
                                actions.push(Action::Play);
                            }
                            if ui.button("Pause").clicked() {
                                actions.push(Action::Pause);
                            }
                            if ui.button("Stop  (Enter position 0)").clicked() {
                                actions.push(Action::Stop);
                            }
                            if ui.button("Toggle Loop  (L)").clicked() {
                                actions.push(Action::Loop);
                            }
                        });
                        ui.menu_button("AI Tools", |ui| {
                            if ui.button("One-Click Vocal Cleanup").clicked() {
                                actions.push(Action::AiClean);
                            }
                            if ui.button("Analyze Mix (AI Mix Assistant)").clicked() {
                                actions.push(Action::AiMix);
                            }
                            if ui.button("Apply AI Mix").clicked() {
                                actions.push(Action::AiApply);
                            }
                        });
                        ui.menu_button("Utilities", |ui| {
                            if ui.button("Load Aurora Demo Session").clicked() {
                                actions.push(Action::Demo);
                            }
                            if ui.button("Load Vocal Recording Session").clicked() {
                                actions.push(Action::VocalSession);
                            }
                            ui.separator();
                            ui.menu_button("Stress Test", |ui| {
                                for n in [100usize, 500, 1000, 2000] {
                                    if ui.button(format!("Generate {n}-Track Project")).clicked() {
                                        actions.push(Action::Stress(n));
                                    }
                                }
                            });
                            ui.separator();
                            if ui.button("About Aurora").clicked() {
                                self.about_open = true;
                            }
                        });
                    });
                    });

                    // ---- clock + bars|beats (fixed width) ----
                    ui.separator();
                    let pos = self.engine_pos();
                    let tempo = self.project.tempo;
                    let sig = self.project.time_sig;
                    ui.vertical(|ui| {
                        ui.set_min_size(egui::vec2(96.0, 20.0));
                        ui.label(
                            egui::RichText::new(fmt_time_bars_beats(pos, tempo, sig))
                                .text_style(egui::TextStyle::Monospace)
                                .size(14.0)
                                .color(Theme::CYAN),
                        );
                        ui.label(egui::RichText::new("BARS | BEATS | TICKS").size(6.0).color(Theme::TEXT_FAINT));
                    });

                    // ---- transport ----
                    if crate::widgets::transport_button(ui, TransportIcon::Rewind, false, Theme::TEXT, "To start").clicked() {
                        actions.push(Action::Seek(0.0));
                    }
                    if crate::widgets::transport_button(ui, TransportIcon::Play, self.playing, Theme::PLAY, "Play (Space)").clicked() {
                        if self.playing {
                            actions.push(Action::Pause);
                        } else {
                            actions.push(Action::Play);
                        }
                    }
                    if crate::widgets::transport_button(ui, TransportIcon::Stop, !self.playing && !self.recording, Theme::STOP, "Stop").clicked() {
                        actions.push(Action::Stop);
                    }
                    if crate::widgets::transport_button(ui, TransportIcon::Record, self.recording, Theme::RECORD, "Record (armed tracks)").clicked() {
                        if self.recording {
                            actions.push(Action::RecStop);
                        } else {
                            actions.push(Action::RecStart);
                        }
                    }
                    if crate::widgets::transport_button(ui, TransportIcon::Forward, false, Theme::TEXT, "To end").clicked() {
                        actions.push(Action::Seek(self.project.duration()));
                    }
                    if crate::widgets::transport_button(ui, TransportIcon::Loop, self.project.loop_enabled, Theme::CYAN, "Loop (L)").clicked() {
                        actions.push(Action::Loop);
                    }

                    // ---- tempo / time sig / key ----
                    ui.separator();
                    ui.add_sized([56.0, 18.0], egui::DragValue::new(&mut self.project.tempo)
                        .speed(0.5)
                        .range(40.0..=280.0)
                        .custom_formatter(|v, _| format!("{v:.2}")));
                    ui.vertical(|ui| {
                        ui.label(egui::RichText::new("BPM").size(6.5).color(Theme::TEXT_FAINT));
                    });
                    ui.label(egui::RichText::new(format!("{}/{}", self.project.time_sig.0, self.project.time_sig.1)).size(11.0).color(Theme::TEXT_DIM));
                    ui.vertical(|ui| {
                        ui.label(egui::RichText::new("SIG").size(6.5).color(Theme::TEXT_FAINT));
                    });
                    ui.label(egui::RichText::new(&self.project.key).size(11.0).color(Theme::TEXT_DIM));

                    // ---- master meter ----
                    let pl = f32::from_bits(self.parts.meters.master_peak_l.load(Ordering::Relaxed));
                    let pr = f32::from_bits(self.parts.meters.master_peak_r.load(Ordering::Relaxed));
                    let (mut ml, mut mr) = (crate::theme::db_to_meter(20.0 * pl.max(1e-6).log10()), crate::theme::db_to_meter(20.0 * pr.max(1e-6).log10()));
                    ui.separator();
                    ui.vertical(|ui| {
                        ui.set_min_size(egui::vec2(72.0, 30.0));
                        let rect = ui.max_rect();
                        let painter = ui.painter();
                        let w = rect.width() - 4.0;
                        for (i, (lvl, ch)) in [(ml, "L"), (mr, "R")].iter().enumerate() {
                            let y = rect.top() + 4.0 + i as f32 * 11.0;
                            painter.rect_filled(
                                egui::Rect::from_min_size(Pos2::new(rect.left(), y), egui::vec2(w, 7.0)),
                                2.0,
                                Color32::from_rgb(16, 22, 36),
                            );
                            let fw = w * lvl.clamp(0.0, 1.0);
                            if fw > 1.0 {
                                let steps = 24;
                                for s in 0..steps {
                                    let t = s as f32 / steps as f32;
                                    let sw = w / steps as f32;
                                    if s as f32 / steps as f32 > lvl.clamp(0.0, 1.0) {
                                        break;
                                    }
                                    painter.rect_filled(
                                        egui::Rect::from_min_size(Pos2::new(rect.left() + s as f32 * sw, y), egui::vec2(sw - 1.0, 7.0)),
                                        1.5,
                                        crate::theme::meter_color(t),
                                    );
                                }
                            }
                        }
                    });

                    // ---- CPU / DSP / driver ----
                    let budget_us = 512.0 / 48000.0 * 1e6;
                    let cb_us = self.parts.meters.callback_us.load(Ordering::Relaxed) as f32;
                    let cpu = ((cb_us / budget_us) * 100.0).clamp(0.0, 100.0);
                    let driver = self.parts.meters.driver_kind.load(Ordering::Relaxed);
                    ui.separator();
                    ui.vertical(|ui| {
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new("CPU").size(7.5).color(Theme::TEXT_FAINT));
                            bar_mini(ui, cpu / 100.0, Theme::CYAN, 54.0);
                            ui.label(egui::RichText::new(format!("{cpu:.0}%")).size(8.0).color(Theme::TEXT_DIM));
                        });
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new("RAM").size(7.5).color(Theme::TEXT_FAINT));
                            let ram_frac = (self.ram_mb / 8192.0).clamp(0.0, 1.0);
                            bar_mini(ui, ram_frac, Theme::PURPLE, 54.0);
                            ui.label(egui::RichText::new(format!("{:.0}MB", self.ram_mb)).size(8.0).color(Theme::TEXT_DIM));
                        });
                    });
                    let driver_chip = match driver {
                        1 => ("DEVICE", Theme::GREEN),
                        2 => ("SYNTH", Theme::YELLOW),
                        _ => ("—", Theme::TEXT_FAINT),
                    };
                    ui.separator();
                    ui.vertical(|ui| {
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new(format!("● {}", driver_chip.0)).size(9.0).color(driver_chip.1));
                            // live input meter — moves when the microphone hears sound
                            let ip = f32::from_bits(self.parts.meters.input_peak.load(Ordering::Relaxed));
                            bar_mini(ui, (ip * 2.5).clamp(0.0, 1.0), Theme::GREEN, 40.0);
                            if ui
                                .add(egui::Button::new(egui::RichText::new("?").size(8.0)).small())
                                .clicked()
                            {
                                self.show_welcome = true;
                            }
                        });
                        if let Some(a) = &self.audio {
                            let out_n = a.device_name.chars().take(26).collect::<String>();
                            let in_n = a.input_name.chars().take(26).collect::<String>();
                            let in_col = if a.input_kind == aurora_engine::audio_io::DriverKind::RealDevice {
                                Theme::GREEN
                            } else {
                                Theme::YELLOW
                            };
                            ui.label(egui::RichText::new(format!("OUT {out_n}")).size(7.5).color(Theme::TEXT_FAINT));
                            ui.label(egui::RichText::new(format!("IN  {in_n}")).size(7.5).color(in_col));
                        }
                    }).response.on_hover_text(format!(
                        "Audio engine status\n\nOutput: {}\nInput: {}{}{}\n\n● GREEN = real sound card (WASAPI/ALSA)\n● YELLOW = software fallback\n\nThe IN meter moves when your microphone hears sound.\nClick O on a track header to monitor yourself live.",
                        self.audio.as_ref().map(|a| a.device_name.as_str()).unwrap_or("—"),
                        self.audio.as_ref().map(|a| a.input_name.as_str()).unwrap_or("—"),
                        self.audio.as_ref().filter(|a| a.input_sample_rate > 0).map(|a| format!(" @ {} Hz", a.input_sample_rate)).unwrap_or_default(),
                        self.audio.as_ref().filter(|a| !a.inputs.is_empty()).map(|a| format!("\n\nAvailable inputs:\n{}", a.inputs.iter().take(8).map(|s| format!("• {s}")).collect::<Vec<_>>().join("\n"))).unwrap_or_default(),
                    ));

                    // ---- status ----
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let rec = self.recording;
                        if rec {
                            ui.label(egui::RichText::new("● REC").size(10.0).color(Theme::RECORD));
                        }
                    });
                });
            });

        // apply actions after the UI pass (avoid borrow conflicts)
        for a in actions {
            self.apply_action(a);
        }
    }

    pub fn apply_action(&mut self, a: Action) {
        match a {
            Action::NewProject => self.new_project(),
            Action::OpenProject => self.open_project(),
            Action::SaveProject => self.save_project(),
            Action::ImportPrompt => {
                self.status = "Type a path in the Browser > IMPORT field".into();
                self.browser_tab = 2;
            }
            Action::Export => {
                self.export_dlg = Some(crate::app::ExportDlg {
                    format: ExportFormat::Wav24,
                    sample_rate: 48000,
                    stems: false,
                    dir: crate::app::dirs_music().display().to_string(),
                    name: self.project.name.replace(' ', "_"),
                    range_full: true,
                    from: 0.0,
                    to: self.project.duration().max(4.0),
                });
            }
            Action::Quit => std::process::exit(0),
            Action::DupClip => self.duplicate_selected_clip(),
            Action::DelClip => self.delete_selected_clip(),
            Action::AddAudio => self.add_audio_track(),
            Action::AddInstr => self.add_instrument_track(),
            Action::AddBus => self.add_bus_track(),
            Action::DelTrack => self.delete_selected_track(),
            Action::Play => self.play(),
            Action::Pause => self.pause(),
            Action::Stop => self.stop(),
            Action::Loop => self.toggle_loop(),
            Action::Seek(p) => self.seek(p),
            Action::RecStart => self.record_start(),
            Action::RecStop => self.record_stop(),
            Action::AiClean => self.ai_clean_vocals(),
            Action::AiMix => self.ai_mix_analyze(),
            Action::AiApply => self.ai_mix_apply(),
            Action::Demo => self.load_demo(),
            Action::VocalSession => {
                let p = aurora_engine::demo::build_vocal_session();
                self.load_project_internal(p);
                self.status = "Vocal recording session loaded — vocal track armed".into();
            }
            Action::Stress(n) => self.generate_stress(n),
        }
    }
}

pub enum Action {
    NewProject,
    OpenProject,
    SaveProject,
    ImportPrompt,
    Export,
    Quit,
    DupClip,
    DelClip,
    AddAudio,
    AddInstr,
    AddBus,
    DelTrack,
    Play,
    Pause,
    Stop,
    Loop,
    Seek(f64),
    RecStart,
    RecStop,
    AiClean,
    AiMix,
    AiApply,
    Demo,
    VocalSession,
    Stress(usize),
}

fn bar_mini(ui: &mut egui::Ui, t: f32, col: Color32, w: f32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, 5.0), egui::Sense::hover());
    let p = ui.painter_at(rect);
    p.rect_filled(rect, 2.0, Color32::from_rgb(16, 22, 36));
    let fw = rect.width() * t.clamp(0.0, 1.0);
    if fw > 1.0 {
        p.rect_filled(
            egui::Rect::from_min_size(rect.left_top(), egui::vec2(fw, rect.height())),
            2.0,
            col,
        );
    }
}

fn draw_logo(ui: &mut egui::Ui) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(118.0, 34.0), egui::Sense::hover());
    let p = ui.painter_at(rect);
    // gradient "A" mark
    let x0 = rect.left() + 6.0;
    let y0 = rect.center().y;
    let steps = 14;
    for i in 0..steps {
        let t = i as f32 / steps as f32;
        let col = crate::theme::mix(Theme::BLUE, Theme::PURPLE, t);
        let yy = y0 - 9.0 + t * 18.0;
        p.line_segment(
            [Pos2::new(x0 + t * 9.0, yy - 1.2), Pos2::new(x0 + t * 9.0 + 0.4, yy + 1.2)],
            egui::Stroke::new(2.2, col),
        );
    }
    p.line_segment(
        [Pos2::new(x0 - 1.0, y0 + 5.0), Pos2::new(x0 + 10.0, y0 + 5.0)],
        egui::Stroke::new(1.6, Theme::CYAN),
    );
    p.text(
        Pos2::new(x0 + 22.0, y0 - 5.0),
        egui::Align2::LEFT_CENTER,
        "AURORA",
        egui::FontId::proportional(13.5),
        Theme::TEXT,
    );
    p.text(
        Pos2::new(x0 + 22.0, y0 + 7.5),
        egui::Align2::LEFT_CENTER,
        "PRODUCER SUITE",
        egui::FontId::proportional(6.5),
        Theme::TEXT_FAINT,
    );
}
