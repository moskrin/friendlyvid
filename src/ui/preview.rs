use egui::{self, TextureHandle, Vec2};
use std::time::Duration;
use uuid::Uuid;

use crate::media::engine::MediaEngine;
use crate::model::{Timeline, Transform};
use crate::state::Playhead;
use crate::ui::font_manager::FontManager;
use crate::util::time::format_duration;

const HANDLE_SIZE: f32 = 14.0;
const HANDLE_HIT_EXPAND: f32 = 10.0;

#[derive(Default)]
enum TextDragMode {
    #[default]
    None,
    Moving,
    Resizing {
        original_font_size: f32,
        original_dist: f32,
    },
}

struct TextStyleMenuState {
    clip_id: Uuid,
    click_pos: egui::Pos2,
    current_font: String,
    current_bold: bool,
    current_italic: bool,
    picker_color: egui::Color32,
}

pub struct PreviewPanel {
    texture: Option<TextureHandle>,
    pub crop_mode: bool,
    crop_clip_id: Option<Uuid>,
    crop_original: Transform,
    crop_current: Transform,
    crop_changed: bool,
    pub text_edit_clip_id: Option<Uuid>,
    pub text_editing_inline: bool,
    text_edit_buffer: String,
    text_edit_needs_focus: bool,
    text_drag_mode: TextDragMode,
    text_style_menu: Option<TextStyleMenuState>,
    pub text_edit_finished: bool,
}

pub enum PreviewAction {
    None,
    UpdateTextOverlay {
        clip_id: Uuid,
        text: String,
        position: (f32, f32),
    },
    SetTextFont {
        clip_id: Uuid,
        font_family: String,
    },
    SetTextColor {
        clip_id: Uuid,
        color: [u8; 4],
    },
    SetTextFontSize {
        clip_id: Uuid,
        font_size: f32,
    },
    SetTextBold {
        clip_id: Uuid,
        bold: bool,
    },
    SetTextItalic {
        clip_id: Uuid,
        italic: bool,
    },
}

/// Returned when crop mode ends with changes.
pub struct CropResult {
    pub clip_id: Uuid,
    pub old_transform: Transform,
    pub new_transform: Transform,
}

impl PreviewPanel {
    pub fn new() -> Self {
        Self {
            texture: None,
            crop_mode: false,
            crop_clip_id: None,
            crop_original: Transform::default(),
            crop_current: Transform::default(),
            crop_changed: false,
            text_edit_clip_id: None,
            text_editing_inline: false,
            text_edit_buffer: String::new(),
            text_edit_needs_focus: false,
            text_drag_mode: TextDragMode::None,
            text_style_menu: None,
            text_edit_finished: false,
        }
    }

    pub fn enter_crop_mode(&mut self, clip_id: Uuid, transform: Transform) {
        self.crop_mode = true;
        self.crop_clip_id = Some(clip_id);
        self.crop_original = transform.clone();
        self.crop_current = transform;
        self.crop_changed = false;
    }

    pub fn exit_crop_mode(&mut self) -> Option<CropResult> {
        self.crop_mode = false;
        if self.crop_changed {
            self.crop_changed = false;
            Some(CropResult {
                clip_id: self.crop_clip_id.take()?,
                old_transform: self.crop_original.clone(),
                new_transform: self.crop_current.clone(),
            })
        } else {
            self.crop_clip_id = None;
            None
        }
    }

    pub fn cancel_crop_mode(&mut self) {
        self.crop_mode = false;
        self.crop_clip_id = None;
        self.crop_changed = false;
    }

    #[allow(clippy::too_many_arguments)]
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        engine: &mut MediaEngine,
        playhead: &mut Playhead,
        selected_transform: Option<&Transform>,
        timeline: &Timeline,
        ctx: &egui::Context,
        font_manager: &mut FontManager,
    ) -> PreviewAction {
        let mut action = PreviewAction::None;

        // Pull latest frame from engine
        if let Some(frame) = engine.try_recv_frame() {
            let image = egui::ColorImage::from_rgba_unmultiplied(
                [frame.width as usize, frame.height as usize],
                &frame.data,
            );
            if let Some(handle) = &mut self.texture {
                handle.set(image, egui::TextureOptions::LINEAR);
            } else {
                self.texture =
                    Some(ctx.load_texture("preview", image, egui::TextureOptions::LINEAR));
            }

            // Convert frame PTS from GES time to model time
            playhead.position = engine.ges_to_model_time(Duration::from_nanos(frame.pts_ns));
        }

        // Update position from pipeline query. When playing, this catches
        // audio-only regions after video ends. When paused, this picks up
        // seek results.
        if let Some(pos) = engine.position() {
            playhead.position = pos;
        }

        // Clone the transform for UV calculation to avoid borrow conflict with mutation
        let display_transform: Option<Transform> = if self.crop_mode {
            Some(self.crop_current.clone())
        } else {
            selected_transform.cloned()
        };

        ui.vertical(|ui| {
            // Video display
            let available = ui.available_size();
            let video_area_height = available.y - 50.0;

            if let Some(tex) = &self.texture {
                let tex_size = tex.size_vec2();
                let aspect = tex_size.x / tex_size.y;
                let max_w = available.x;
                let max_h = video_area_height.max(100.0);
                let (w, h) = fit_size(max_w, max_h, aspect);

                // UV cropping is only used during live crop mode, where the pipeline
                // has NOT yet had the new crop baked in. Once committed, the GES
                // videocrop effect emits an already-cropped frame — applying UV
                // crop on top of that would double-crop the preview.
                let uv = if self.crop_mode {
                    if let Some(ref t) = display_transform {
                        if t.has_crop() {
                            let (l, t, r, b) = t.crop_uv();
                            egui::Rect::from_min_max(egui::pos2(l, t), egui::pos2(r, b))
                        } else {
                            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0))
                        }
                    } else {
                        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0))
                    }
                } else {
                    egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0))
                };

                let sense = if self.crop_mode || self.text_edit_clip_id.is_some() {
                    egui::Sense::click_and_drag()
                } else {
                    egui::Sense::hover()
                };
                let (response, _painter) = ui.allocate_painter(
                    Vec2::new(available.x, video_area_height.max(100.0)),
                    sense,
                );

                // Center the image within the allocated area
                let img_rect = egui::Rect::from_center_size(
                    response.rect.center(),
                    Vec2::new(w, h),
                );

                // Draw the video frame with UV crop
                ui.painter().image(tex.id(), img_rect, uv, egui::Color32::WHITE);

                // Crop mode interactions
                if self.crop_mode {
                    // Crop mode border
                    ui.painter().rect_stroke(
                        img_rect,
                        0.0,
                        egui::Stroke::new(2.0, egui::Color32::YELLOW),
                        egui::StrokeKind::Outside,
                    );

                    // Scroll to zoom
                    if response.hovered() {
                        let scroll = ui.input(|i| i.smooth_scroll_delta.y);
                        if scroll != 0.0 {
                            let zoom_factor = 1.0 + scroll * 0.005;
                            let old_zoom = self.crop_current.zoom;
                            self.crop_current.zoom =
                                (self.crop_current.zoom * zoom_factor).clamp(1.0, 5.0);
                            if (self.crop_current.zoom - old_zoom).abs() > 0.001 {
                                self.crop_changed = true;
                            }
                        }
                    }

                    // Drag to pan
                    if response.dragged() {
                        let delta = response.drag_delta();
                        if delta.length() > 0.0 && self.crop_current.zoom > 1.0 {
                            // Scale drag pixels to pan units
                            let sensitivity = 1.0 / (w * self.crop_current.zoom);
                            self.crop_current.pan_x =
                                (self.crop_current.pan_x - delta.x * sensitivity).clamp(0.0, 1.0);
                            self.crop_current.pan_y =
                                (self.crop_current.pan_y - delta.y * sensitivity).clamp(0.0, 1.0);
                            self.crop_changed = true;
                        }
                    }

                    // Escape exits crop mode (handled by layout via keyboard check)
                }

                // Render text overlays on top of video
                if !self.crop_mode {
                    let pos = playhead.position;
                    let mut selected_text_rect: Option<egui::Rect> = None;
                    let mut selected_font_size: f32 = 48.0;
                    let mut visible_text_rects: Vec<(Uuid, egui::Rect)> = Vec::new();

                    for clip_id in &timeline.text_track.clips {
                        if let Some(clip) = timeline.get_clip(*clip_id) {
                            if pos >= clip.start && pos < clip.end() {
                                if let Some(overlay) = timeline.get_text_overlay(*clip_id) {
                                    let is_selected = self.text_edit_clip_id == Some(*clip_id);

                                    // Apply fade in/out transitions as opacity
                                    let mut alpha = overlay.color[3] as f32 / 255.0;
                                    if let Some(t) = timeline.find_transition(
                                        &crate::model::TransitionPosition::FadeIn(*clip_id),
                                    ) {
                                        let fade_dur = t.duration.as_secs_f64();
                                        let elapsed = (pos - clip.start).as_secs_f64();
                                        if fade_dur > 0.0 && elapsed < fade_dur {
                                            alpha *= (elapsed / fade_dur) as f32;
                                        }
                                    }
                                    if let Some(t) = timeline.find_transition(
                                        &crate::model::TransitionPosition::FadeOut(*clip_id),
                                    ) {
                                        let fade_dur = t.duration.as_secs_f64();
                                        let remaining = (clip.end() - pos).as_secs_f64();
                                        if fade_dur > 0.0 && remaining < fade_dur {
                                            alpha *= (remaining / fade_dur) as f32;
                                        }
                                    }
                                    let final_alpha = (alpha.clamp(0.0, 1.0) * 255.0) as u8;

                                    let color = egui::Color32::from_rgba_unmultiplied(
                                        overlay.color[0],
                                        overlay.color[1],
                                        overlay.color[2],
                                        final_alpha,
                                    );

                                    // Scale font size relative to video display size
                                    let scale = img_rect.height() / 720.0;
                                    let font_size = overlay.font_size * scale;

                                    let display_text = if is_selected && self.text_editing_inline {
                                        self.text_edit_buffer.clone()
                                    } else {
                                        overlay.text.clone()
                                    };

                                    let text_x = img_rect.left() + overlay.position.0 * img_rect.width();
                                    let text_y = img_rect.top() + overlay.position.1 * img_rect.height();
                                    let text_pos = egui::pos2(text_x, text_y);

                                    // Load and use system font (font becomes
                                    // available next frame after atlas rebuild)
                                    font_manager.ensure_loaded(
                                        &overlay.font_family,
                                        overlay.bold,
                                        overlay.italic,
                                    );
                                    let font_id = font_manager.font_id(
                                        &overlay.font_family,
                                        font_size,
                                        overlay.bold,
                                        overlay.italic,
                                    );

                                    let galley = ui.painter().layout_no_wrap(
                                        display_text,
                                        font_id,
                                        color,
                                    );

                                    let text_rect = egui::Rect::from_center_size(
                                        text_pos,
                                        galley.size(),
                                    );

                                    // Selection highlight + resize handles
                                    if is_selected {
                                        let sel_rect = text_rect.expand(4.0);
                                        ui.painter().rect_stroke(
                                            sel_rect,
                                            2.0,
                                            egui::Stroke::new(1.5, egui::Color32::from_rgb(0, 180, 255)),
                                            egui::StrokeKind::Outside,
                                        );

                                        // Corner resize handles (not during inline edit)
                                        if !self.text_editing_inline {
                                            for hr in corner_handle_rects(sel_rect) {
                                                ui.painter().rect_filled(hr, 1.0, egui::Color32::WHITE);
                                                ui.painter().rect_stroke(
                                                    hr,
                                                    1.0,
                                                    egui::Stroke::new(1.0, egui::Color32::from_rgb(0, 120, 215)),
                                                    egui::StrokeKind::Outside,
                                                );
                                            }
                                        }

                                        selected_text_rect = Some(text_rect);
                                        selected_font_size = overlay.font_size;
                                    }

                                    ui.painter().galley(
                                        text_rect.left_top(),
                                        galley,
                                        color,
                                    );

                                    visible_text_rects.push((*clip_id, text_rect));
                                }
                            }
                        }
                    }

                    // Text interaction (drag, resize, double-click, right-click)
                    if !self.text_editing_inline {
                        // Drag start: check handles first, then body
                        if response.drag_started() {
                            if let Some(click_pos) = response.interact_pointer_pos() {
                                let mut hit_handle = false;
                                if let Some(sel_rect) = selected_text_rect {
                                    let sel_outer = sel_rect.expand(4.0);
                                    for hr in corner_handle_rects(sel_outer) {
                                        if hr.expand(HANDLE_HIT_EXPAND).contains(click_pos) {
                                            let center = sel_rect.center();
                                            self.text_drag_mode = TextDragMode::Resizing {
                                                original_font_size: selected_font_size,
                                                original_dist: center.distance(click_pos),
                                            };
                                            hit_handle = true;
                                            break;
                                        }
                                    }
                                }
                                if !hit_handle
                                    && selected_text_rect
                                        .is_some_and(|r| r.expand(4.0).contains(click_pos))
                                {
                                    self.text_drag_mode = TextDragMode::Moving;
                                }
                            }
                        }

                        // During drag
                        if response.dragged() {
                            match &self.text_drag_mode {
                                TextDragMode::Moving => {
                                    if let Some(edit_id) = self.text_edit_clip_id {
                                        if let Some(overlay) = timeline.get_text_overlay(edit_id) {
                                            let delta = response.drag_delta();
                                            if delta.length() > 0.0 {
                                                let new_px = (overlay.position.0 + delta.x / img_rect.width()).clamp(0.0, 1.0);
                                                let new_py = (overlay.position.1 + delta.y / img_rect.height()).clamp(0.0, 1.0);
                                                action = PreviewAction::UpdateTextOverlay {
                                                    clip_id: edit_id,
                                                    text: overlay.text.clone(),
                                                    position: (new_px, new_py),
                                                };
                                            }
                                        }
                                    }
                                }
                                TextDragMode::Resizing { original_font_size, original_dist } => {
                                    if let Some(mouse_pos) = response.interact_pointer_pos() {
                                        if let Some(sel_rect) = selected_text_rect {
                                            let center = sel_rect.center();
                                            let new_dist = center.distance(mouse_pos);
                                            if *original_dist > 1.0 {
                                                let scale = new_dist / *original_dist;
                                                let new_size = (*original_font_size * scale).clamp(8.0, 200.0);
                                                if let Some(edit_id) = self.text_edit_clip_id {
                                                    action = PreviewAction::SetTextFontSize {
                                                        clip_id: edit_id,
                                                        font_size: new_size,
                                                    };
                                                }
                                            }
                                        }
                                    }
                                }
                                TextDragMode::None => {}
                            }
                        }

                        // Drag end
                        if response.drag_stopped() {
                            self.text_drag_mode = TextDragMode::None;
                        }

                        // Right-click on text overlay: open style menu
                        if response.secondary_clicked() {
                            if let Some(click_pos) = response.interact_pointer_pos() {
                                for (cid, rect) in &visible_text_rects {
                                    if rect.expand(4.0).contains(click_pos) {
                                        // Initialize menu state from current overlay
                                        let overlay = timeline.get_text_overlay(*cid);
                                        let (font, bold, italic, color) = overlay
                                            .map(|o| (
                                                o.font_family.clone(),
                                                o.bold,
                                                o.italic,
                                                egui::Color32::from_rgba_unmultiplied(
                                                    o.color[0], o.color[1], o.color[2], o.color[3],
                                                ),
                                            ))
                                            .unwrap_or((
                                                "Sans".to_string(),
                                                false,
                                                false,
                                                egui::Color32::WHITE,
                                            ));
                                        self.text_style_menu = Some(TextStyleMenuState {
                                            clip_id: *cid,
                                            click_pos,
                                            current_font: font,
                                            current_bold: bold,
                                            current_italic: italic,
                                            picker_color: color,
                                        });
                                        break;
                                    }
                                }
                            }
                        }

                        // Double-click enters inline text edit
                        if response.double_clicked() {
                            if let Some(edit_id) = self.text_edit_clip_id {
                                if let Some(overlay) = timeline.get_text_overlay(edit_id) {
                                    self.text_editing_inline = true;
                                    self.text_edit_needs_focus = true;
                                    self.text_edit_buffer = overlay.text.clone();
                                }
                            }
                        }
                    }

                    // Cursor icons for resize handles
                    if let Some(sel_rect) = selected_text_rect {
                        if !self.text_editing_inline {
                            if let Some(hover_pos) = ui.input(|i| i.pointer.hover_pos()) {
                                let sel_outer = sel_rect.expand(4.0);
                                let handles = corner_handle_rects(sel_outer);
                                // TL=0 BR=2 -> NwSe, TR=1 BL=3 -> NeSw
                                for (i, hr) in handles.iter().enumerate() {
                                    if hr.expand(HANDLE_HIT_EXPAND).contains(hover_pos) {
                                        let icon = if i == 0 || i == 2 {
                                            egui::CursorIcon::ResizeNwSe
                                        } else {
                                            egui::CursorIcon::ResizeNeSw
                                        };
                                        ui.ctx().set_cursor_icon(icon);
                                        break;
                                    }
                                }
                            }
                        }
                    }
                }
            } else {
                ui.allocate_ui_with_layout(
                    Vec2::new(available.x, video_area_height.max(100.0)),
                    egui::Layout::centered_and_justified(egui::Direction::TopDown),
                    |ui| {
                        ui.label("No video loaded. Use File > Open to load a video.");
                    },
                );
            }

            // Crop mode label
            if self.crop_mode {
                ui.horizontal(|ui| {
                    ui.colored_label(
                        egui::Color32::YELLOW,
                        format!(
                            "Crop Mode - scroll to zoom ({:.1}x), drag to pan",
                            self.crop_current.zoom
                        ),
                    );
                });
            }

            // Text edit mode
            if self.text_edit_clip_id.is_some() && !self.crop_mode {
                if self.text_editing_inline {
                    ui.horizontal(|ui| {
                        ui.colored_label(
                            egui::Color32::from_rgb(0, 180, 255),
                            "Editing text:",
                        );
                        let te = ui.text_edit_singleline(&mut self.text_edit_buffer);

                        // Only request focus on the first frame of editing
                        if self.text_edit_needs_focus {
                            te.request_focus();
                            self.text_edit_needs_focus = false;
                        }

                        let escaped = ui.input(|i| i.key_pressed(egui::Key::Escape));

                        if escaped {
                            // Cancel: discard changes, exit completely
                            self.text_editing_inline = false;
                            self.text_edit_clip_id = None;
                            self.text_edit_finished = true;
                        } else if te.lost_focus() {
                            // Commit: save text, exit completely (Enter or click-away)
                            if let Some(edit_id) = self.text_edit_clip_id {
                                if let Some(overlay) = timeline.get_text_overlay(edit_id) {
                                    if self.text_edit_buffer != overlay.text {
                                        action = PreviewAction::UpdateTextOverlay {
                                            clip_id: edit_id,
                                            text: self.text_edit_buffer.clone(),
                                            position: overlay.position,
                                        };
                                    }
                                }
                            }
                            self.text_editing_inline = false;
                            self.text_edit_clip_id = None;
                            self.text_edit_finished = true;
                        }
                    });
                } else {
                    ui.horizontal(|ui| {
                        ui.colored_label(
                            egui::Color32::from_rgb(0, 180, 255),
                            "Text selected - double-click preview to edit, drag to move, right-click for style",
                        );
                    });
                }
            }

            // Text style popup menu (rendered as Area, appears on top)
            if self.text_style_menu.is_some() {
                let style_action = self.show_text_style_menu(ui, font_manager);
                if !matches!(style_action, PreviewAction::None) {
                    action = style_action;
                }
            }

            ui.separator();

            // Transport controls - include text track duration (text clips
            // aren't on the GES pipeline, so engine duration alone isn't enough)
            let engine_dur = engine.model_duration().unwrap_or(Duration::ZERO);
            let text_dur = timeline.track_duration(crate::model::TrackKind::Text);
            let duration = engine_dur.max(text_dur);
            let pos = playhead.position;

            ui.horizontal(|ui| {
                let label = if playhead.playing { "||" } else { ">" };
                if ui.button(label).clicked() {
                    if playhead.playing {
                        engine.pause();
                        playhead.playing = false;
                    } else {
                        engine.play();
                        playhead.playing = true;
                    }
                }

                ui.label(format!(
                    "{} / {}",
                    format_duration(pos),
                    format_duration(duration)
                ));

                if !duration.is_zero() {
                    let mut pos_frac = pos.as_secs_f64() / duration.as_secs_f64();
                    let slider = egui::Slider::new(&mut pos_frac, 0.0..=1.0)
                        .show_value(false)
                        .trailing_fill(true);

                    let response = ui.add(slider);
                    if response.changed() {
                        let new_pos = Duration::from_secs_f64(pos_frac * duration.as_secs_f64());
                        engine.seek(new_pos);
                        playhead.position = new_pos;
                    }
                }
            });
        });

        if playhead.playing {
            ctx.request_repaint();
        } else if engine.is_loaded() {
            // Poll at low rate when paused to pick up seek frames
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }

        action
    }

    fn show_text_style_menu(
        &mut self,
        ui: &mut egui::Ui,
        font_manager: &mut FontManager,
    ) -> PreviewAction {
        let mut action = PreviewAction::None;
        let mut menu_state = match self.text_style_menu.take() {
            Some(s) => s,
            None => return action,
        };

        let popup_id = egui::Id::new("text_style_menu");
        let escape = ui.input(|i| i.key_pressed(egui::Key::Escape));
        let mut keep_open = true;

        let menu_frame = egui::Frame::new()
            .fill(egui::Color32::from_gray(40))
            .stroke(egui::Stroke::new(1.0, egui::Color32::from_gray(80)))
            .inner_margin(egui::Margin::same(8));

        let _area_resp = egui::Area::new(popup_id)
            .order(egui::Order::Foreground)
            .fixed_pos(menu_state.click_pos)
            .show(ui.ctx(), |ui| {
                menu_frame.show(ui, |ui| {
                    ui.set_min_width(220.0);

                    // -- Font selector --
                    ui.label(
                        egui::RichText::new("Font")
                            .strong()
                            .color(egui::Color32::from_gray(160)),
                    );

                    let families = font_manager.families().to_vec();
                    let mut selected_font = menu_state.current_font.clone();
                    egui::ComboBox::from_id_salt("text_font_combo")
                        .selected_text(&selected_font)
                        .width(200.0)
                        .height(300.0)
                        .show_ui(ui, |ui| {
                            for family in &families {
                                ui.selectable_value(&mut selected_font, family.clone(), family);
                            }
                        });
                    if selected_font != menu_state.current_font {
                        menu_state.current_font = selected_font.clone();
                        action = PreviewAction::SetTextFont {
                            clip_id: menu_state.clip_id,
                            font_family: selected_font,
                        };
                    }

                    ui.add_space(4.0);

                    // -- Bold / Italic toggles --
                    ui.horizontal(|ui| {
                        let bold_text = if menu_state.current_bold {
                            egui::RichText::new("Bold").strong()
                        } else {
                            egui::RichText::new("Bold")
                        };
                        if ui.selectable_label(menu_state.current_bold, bold_text).clicked() {
                            menu_state.current_bold = !menu_state.current_bold;
                            action = PreviewAction::SetTextBold {
                                clip_id: menu_state.clip_id,
                                bold: menu_state.current_bold,
                            };
                        }

                        let italic_text = if menu_state.current_italic {
                            egui::RichText::new("Italic").italics()
                        } else {
                            egui::RichText::new("Italic")
                        };
                        if ui.selectable_label(menu_state.current_italic, italic_text).clicked() {
                            menu_state.current_italic = !menu_state.current_italic;
                            action = PreviewAction::SetTextItalic {
                                clip_id: menu_state.clip_id,
                                italic: menu_state.current_italic,
                            };
                        }
                    });

                    ui.separator();

                    // -- Color presets + picker --
                    ui.label(
                        egui::RichText::new("Color")
                            .strong()
                            .color(egui::Color32::from_gray(160)),
                    );

                    let colors: &[(&str, [u8; 4])] = &[
                        ("White", [255, 255, 255, 255]),
                        ("Black", [0, 0, 0, 255]),
                        ("Red", [255, 0, 0, 255]),
                        ("Green", [0, 200, 0, 255]),
                        ("Blue", [0, 100, 255, 255]),
                        ("Yellow", [255, 255, 0, 255]),
                        ("Cyan", [0, 255, 255, 255]),
                    ];

                    ui.horizontal_wrapped(|ui| {
                        for (name, color) in colors {
                            let c = egui::Color32::from_rgba_unmultiplied(
                                color[0], color[1], color[2], color[3],
                            );
                            let is_current = menu_state.picker_color == c;
                            let size = Vec2::splat(20.0);
                            let (rect, resp) = ui.allocate_exact_size(size, egui::Sense::click());
                            ui.painter().rect_filled(rect, 2.0, c);
                            if is_current {
                                ui.painter().rect_stroke(
                                    rect,
                                    2.0,
                                    egui::Stroke::new(2.0, egui::Color32::WHITE),
                                    egui::StrokeKind::Outside,
                                );
                            }
                            if resp.clicked() {
                                menu_state.picker_color = c;
                                action = PreviewAction::SetTextColor {
                                    clip_id: menu_state.clip_id,
                                    color: *color,
                                };
                            }
                            resp.on_hover_text(*name);
                        }

                        // Color picker button as last swatch
                        let old_color = menu_state.picker_color;
                        egui::color_picker::color_edit_button_srgba(
                            ui,
                            &mut menu_state.picker_color,
                            egui::color_picker::Alpha::Opaque,
                        );
                        if menu_state.picker_color != old_color {
                            let c = menu_state.picker_color;
                            action = PreviewAction::SetTextColor {
                                clip_id: menu_state.clip_id,
                                color: [c.r(), c.g(), c.b(), c.a()],
                            };
                        }
                    });
                });
            });

        // Dismiss only on Escape (not clicked_outside, because ComboBox and
        // color picker open child popups that register as "outside" clicks)
        if escape {
            keep_open = false;
        }

        if keep_open {
            self.text_style_menu = Some(menu_state);
        }

        action
    }
}

fn fit_size(max_w: f32, max_h: f32, aspect: f32) -> (f32, f32) {
    let w = max_w.min(max_h * aspect);
    let h = w / aspect;
    (w, h)
}

fn corner_handle_rects(rect: egui::Rect) -> [egui::Rect; 4] {
    let s = HANDLE_SIZE;
    [
        egui::Rect::from_center_size(rect.left_top(), Vec2::splat(s)),     // TL
        egui::Rect::from_center_size(rect.right_top(), Vec2::splat(s)),    // TR
        egui::Rect::from_center_size(rect.right_bottom(), Vec2::splat(s)), // BR
        egui::Rect::from_center_size(rect.left_bottom(), Vec2::splat(s)),  // BL
    ]
}
