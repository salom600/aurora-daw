//! Left browser panel — BROWSER / PROJECT / PLUGINS tabs, AI collections,
//! sound library, import field, recent projects, AI assistant status.

use crate::app::AuroraApp;
use crate::theme::{section_label, Theme};
use crate::widgets;
use aurora_engine::project::TrackKind;
use egui::{Color32, Pos2};

impl AuroraApp {
    pub fn draw_browser(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("browser")
            .exact_width(226.0)
            .frame(egui::Frame::none().fill(Theme::PANEL).inner_margin(egui::Margin::same(8.0)))
            .show(ctx, |ui| {
                ui.vertical(|ui| {
                    // ---- tabs ----
                    ui.horizontal(|ui| {
                        for (i, tab) in ["BROWSER", "PROJECT", "PLUGINS"].iter().enumerate() {
                            let sel = self.browser_tab == i;
                            let txt = egui::RichText::new(*tab).size(9.5).color(if sel {
                                Theme::CYAN
                            } else {
                                Theme::TEXT_DIM
                            });
                            if ui.add(egui::Button::new(txt).fill(Color32::TRANSPARENT)).clicked() {
                                self.browser_tab = i;
                            }
                            if sel {
                                let rect = ui.max_rect();
                                ui.painter().line_segment(
                                    [
                                        Pos2::new(rect.right() - 28.0, rect.bottom() - 2.0),
                                        Pos2::new(rect.right() - 8.0, rect.bottom() - 2.0),
                                    ],
                                    egui::Stroke::new(2.0, Theme::CYAN),
                                );
                            }
                            ui.add_space(4.0);
                        }
                    });
                    ui.separator();
                    ui.add_space(4.0);

                    match self.browser_tab {
                        0 => self.browser_content(ui),
                        1 => self.project_content(ui),
                        _ => self.plugins_content(ui),
                    }

                    ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
                        ui.separator();
                        ui.horizontal(|ui| {
                            let (rect, _) = ui.allocate_exact_size(egui::vec2(26.0, 26.0), egui::Sense::hover());
                            let p = ui.painter_at(rect);
                            p.circle_filled(rect.center(), 12.0, Theme::BLUE.linear_multiply(0.25));
                            p.circle_filled(rect.center(), 6.0, Theme::CYAN);
                            crate::widgets::icon_play(&p, rect.center(), 6.0, Theme::BG);
                            ui.vertical(|ui| {
                                ui.label(egui::RichText::new("AI Assistant").size(10.0).color(Theme::TEXT));
                                ui.label(
                                    egui::RichText::new(match self.ai_mix.analyzing {
                                        true => "Analyzing your mix…",
                                        false => "Ready to assist your mix",
                                    })
                                    .size(8.5)
                                    .color(Theme::TEXT_FAINT),
                                );
                            });
                        });
                    });
                });
            });
    }

    fn browser_content(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
            // search
            let search = ui.add(
                egui::TextEdit::singleline(&mut "")
                    .hint_text("Search…")
                    .desired_width(ui.available_width()),
            );
            let _ = search;

            section_label(ui, "Favorites");
            for (dot, name, sub, action) in [
                (Theme::TEAL, "Drum Kit Modern", "Loops & one-shots", 0),
                (Theme::BLUE, "Cinematic Textures", "Atmos & pads", 1),
                (Theme::GREEN, "Vocal Chain Pro", "FX preset", 2),
                (Theme::PURPLE, "Master Bus Chain", "FX preset", 3),
            ] {
                if crate::widgets::browser_row(ui, dot, name, sub).clicked() {
                    match action {
                        2 => {
                            // apply vocal chain to selected track
                            if let Some(id) = self.selected_track {
                                let chain: Vec<aurora_engine::effects::EffectInstance> = [
                                    aurora_engine::effects::EffectType::Eq3,
                                    aurora_engine::effects::EffectType::Compressor,
                                    aurora_engine::effects::EffectType::DeEsser,
                                    aurora_engine::effects::EffectType::Reverb,
                                ]
                                .iter()
                                .map(|et| {
                                    let mut fx = aurora_engine::effects::EffectInstance::new(*et, 0);
                                    fx.uid = self.project.alloc_id();
                                    fx
                                })
                                .collect();
                                if let Some(t) = self.project.track_by_id_mut(id) {
                                    t.fx.clear();
                                    t.fx.extend(chain);
                                }
                                self.mark_graph_dirty();
                                self.status = "Vocal Chain Pro applied to selected track".into();
                            }
                        }
                        3 => {
                            self.project.master_fx = vec![
                                aurora_engine::effects::EffectInstance::new(aurora_engine::effects::EffectType::Eq3, 8001),
                                aurora_engine::effects::EffectInstance::new(aurora_engine::effects::EffectType::Compressor, 8002),
                                aurora_engine::effects::EffectInstance::new(aurora_engine::effects::EffectType::Limiter, 8003),
                            ];
                            self.mark_graph_dirty();
                            self.status = "Master Bus Chain applied (EQ + Glue + Limiter)".into();
                        }
                        _ => {
                            self.status = "Sound kit loaded into browser preview".into();
                        }
                    }
                }
            }

            section_label(ui, "AI Collections");
            for (dot, name, sub) in [
                (Theme::CYAN, "AI Mastering Suite", "One-click polish"),
                (Theme::GREEN, "AI Vocal Enhancer", "Clean & brighten"),
                (Theme::BLUE, "AI Noise Cleaner", "De-noise takes"),
                (Theme::PURPLE, "AI Instrument Balance", "Level matching"),
            ] {
                if crate::widgets::browser_row(ui, dot, name, sub).clicked() {
                    match sub {
                        "Clean & brighten" => self.ai_clean_vocals(),
                        "One-click polish" => {
                            if !self.ai_mix.analyzed {
                                self.ai_mix_analyze();
                            }
                            self.status = "AI Mastering Suite: analyzing, then Apply AI Mix".into();
                        }
                        "Level matching" => {
                            self.ai_mix_analyze();
                            self.status = "Analyzing levels for AI balance…".into();
                        }
                        _ => self.ai_clean_vocals(),
                    }
                }
            }

            section_label(ui, "Sound Library");
            for (dot, name, kind) in [
                (Theme::BLUE, "Bass", 0),
                (Theme::TEAL, "Drums", 1),
                (Theme::PURPLE, "Instruments", 2),
                (Theme::GREEN, "Vocals", 3),
                (Theme::PINK, "FX", 4),
                (Theme::ORANGE, "Loops", 5),
            ] {
                if crate::widgets::browser_row(ui, dot, name, "Click to add instrument track").clicked() {
                    match kind {
                        1 => {
                            self.add_audio_track();
                            if let Some(id) = self.selected_track {
                                if let Some(t) = self.project.track_by_id_mut(id) {
                                    t.name = format!("DRUMS {}", t.id % 100);
                                    t.subtitle = "Drum Kit Modern".into();
                                }
                            }
                        }
                        3 => {
                            self.add_audio_track();
                            if let Some(id) = self.selected_track {
                                if let Some(t) = self.project.track_by_id_mut(id) {
                                    t.name = format!("VOCAL {}", t.id % 100);
                                    t.subtitle = "Vocal Chain Pro".into();
                                    t.armed = true;
                                }
                            }
                            self.mark_graph_dirty();
                        }
                        _ => {
                            self.add_instrument_track();
                        }
                    }
                }
            }

            section_label(ui, "Recent Projects");
            for (dot, name) in [
                (Theme::CYAN, "Aurora Project 2026"),
                (Theme::BLUE, "Cinematic Trailer"),
                (Theme::PURPLE, "Neon City Soundtrack"),
                (Theme::GREEN, "Vocal Session Pro"),
            ] {
                if crate::widgets::browser_row(ui, dot, name, "Saved in Music/Aurora").clicked() {
                    self.open_project();
                }
            }
        });
    }

    fn project_content(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
            section_label(ui, "Session");
            ui.add(
                egui::TextEdit::singleline(&mut self.project.name)
                    .desired_width(ui.available_width()),
            );
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Tempo").size(10.0).color(Theme::TEXT_DIM));
                ui.add(
                    egui::DragValue::new(&mut self.project.tempo)
                        .speed(0.5)
                        .range(40.0..=280.0),
                );
                ui.label(egui::RichText::new("Key").size(10.0).color(Theme::TEXT_DIM));
                ui.add(
                    egui::TextEdit::singleline(&mut self.project.key)
                        .desired_width(60.0),
                );
            });
            ui.separator();

            section_label(ui, "Import");
            ui.add(
                egui::TextEdit::singleline(&mut self.import_path)
                    .hint_text("Path to WAV / MP3 / FLAC…")
                    .desired_width(ui.available_width()),
            );
            ui.horizontal(|ui| {
                if ui.button("Import to Timeline").clicked() {
                    let path = self.import_path.clone();
                    if !path.trim().is_empty() {
                        self.import_audio(&path);
                    }
                }
                if ui.button("Load Demo Session").clicked() {
                    self.load_demo();
                }
            });
            ui.separator();

            section_label(ui, "Tracks");
            egui::Grid::new("trkgrid").num_columns(2).spacing([8.0, 3.0]).show(ui, |ui| {
                for t in &self.project.tracks {
                    let col = Color32::from_rgb(t.color[0], t.color[1], t.color[2]);
                    let (rect, _) = ui.allocate_exact_size(egui::vec2(8.0, 8.0), egui::Sense::hover());
                    ui.painter().circle_filled(rect.center(), 3.5, col);
                    let resp = ui.label(
                        egui::RichText::new(&t.name).size(10.5).color(if Some(t.id) == self.selected_track {
                            Theme::CYAN
                        } else {
                            Theme::TEXT_DIM
                        }),
                    );
                    if resp.clicked() {
                        self.selected_track = Some(t.id);
                    }
                    ui.end_row();
                }
            });
            ui.separator();
            ui.horizontal(|ui| {
                if ui.small_button("+ Audio").clicked() {
                    self.add_audio_track();
                }
                if ui.small_button("+ Instr").clicked() {
                    self.add_instrument_track();
                }
            });
            ui.horizontal(|ui| {
                if ui.small_button("+ Bus").clicked() {
                    self.add_bus_track();
                }
                if ui.small_button("Delete").clicked() {
                    self.delete_selected_track();
                }
            });

            section_label(ui, "AI Cleanup History");
            if self.project.settings.ai_clean_history.is_empty() && self.cleaner.last_reports.is_empty() {
                ui.label(egui::RichText::new("No cleanup runs yet").size(10.0).color(Theme::TEXT_FAINT));
            }
            for (name, r) in self.cleaner.last_reports.iter().rev().take(6) {
                ui.label(
                    egui::RichText::new(format!(
                        "{name}: −{:.0} dB noise, {} clicks, {} breaths",
                        r.noise_reduction_est_db, r.clicks_fixed, r.breaths_removed
                    ))
                    .size(9.5)
                    .color(Theme::TEXT_DIM),
                );
            }
        });
    }

    fn plugins_content(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
            section_label(ui, "Installed Plugins");
            ui.label(egui::RichText::new("AURORA native rack").size(10.0).color(Theme::TEXT_FAINT));
            ui.add_space(4.0);
            for et in [
                aurora_engine::effects::EffectType::Eq3,
                aurora_engine::effects::EffectType::Compressor,
                aurora_engine::effects::EffectType::Gate,
                aurora_engine::effects::EffectType::DeEsser,
                aurora_engine::effects::EffectType::Reverb,
                aurora_engine::effects::EffectType::Delay,
                aurora_engine::effects::EffectType::Chorus,
                aurora_engine::effects::EffectType::Flanger,
                aurora_engine::effects::EffectType::Phaser,
                aurora_engine::effects::EffectType::Saturation,
                aurora_engine::effects::EffectType::Limiter,
            ] {
                let cat = et.category();
                let col = fx_color(et);
                let (rect, resp) = ui
                    .allocate_exact_size(egui::vec2(ui.available_width(), 26.0), egui::Sense::click())
                    ;
                let p = ui.painter_at(rect);
                if resp.hovered() {
                    p.rect_filled(rect, Theme::R_SM, Theme::CARD);
                }
                p.rect_filled(
                    egui::Rect::from_min_size(Pos2::new(rect.left() + 4.0, rect.center().y - 8.0), egui::vec2(16.0, 16.0)),
                    4.0,
                    col.linear_multiply(0.25),
                );
                p.rect_stroke(
                    egui::Rect::from_min_size(Pos2::new(rect.left() + 4.0, rect.center().y - 8.0), egui::vec2(16.0, 16.0)),
                    4.0,
                    egui::Stroke::new(1.0, col),
                );
                p.text(
                    Pos2::new(rect.left() + 12.0, rect.center().y),
                    egui::Align2::CENTER_CENTER,
                    &et.name()[..1],
                    egui::FontId::proportional(9.0),
                    col,
                );
                p.text(
                    Pos2::new(rect.left() + 28.0, rect.center().y - 5.0),
                    egui::Align2::LEFT_CENTER,
                    et.name(),
                    egui::FontId::proportional(11.0),
                    Theme::TEXT,
                );
                p.text(
                    Pos2::new(rect.left() + 28.0, rect.center().y + 6.5),
                    egui::Align2::LEFT_CENTER,
                    cat,
                    egui::FontId::proportional(8.0),
                    Theme::TEXT_FAINT,
                );
                if resp.clicked() {
                    // add to selected track
                    if let Some(id) = self.selected_track {
                        let mut fx = aurora_engine::effects::EffectInstance::new(et, 0);
                        fx.uid = self.project.alloc_id();
                        if let Some(t) = self.project.track_by_id_mut(id) {
                            t.fx.push(fx);
                        }
                        self.mark_graph_dirty();
                        self.status = format!("{} added to selected track", et.name());
                    }
                }
            }
        });
    }
}

pub fn fx_color(et: aurora_engine::effects::EffectType) -> Color32 {
    use aurora_engine::effects::EffectType::*;
    match et {
        Eq3 => Theme::CYAN,
        Compressor => Theme::BLUE,
        Reverb => Theme::PURPLE,
        Delay => Theme::PINK,
        Chorus => Theme::TEAL,
        Saturation => Theme::ORANGE,
        Limiter => Theme::YELLOW,
        Gate => Theme::GREEN,
        Flanger => Theme::BLUE,
        Phaser => Theme::PURPLE,
        DeEsser => Theme::GREEN,
    }
}
