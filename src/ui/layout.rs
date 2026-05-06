use std::time::{Duration, Instant};

use crate::commands::clip_commands::{RemoveClipCommand, SplitClipCommand};
use crate::commands::transform_commands::SetCropCommand;
use crate::commands::transition_commands::SetTransitionCommand;
use crate::commands::CommandHistory;
use crate::media::engine::MediaEngine;
use crate::model::{Project, TransitionPosition};
use crate::state::AppState;
use crate::ui::font_manager::FontManager;
use crate::ui::{media_browser, preview, timeline_widget, toolbar};
use crate::ui::timeline_widget::{JunctionKind, TimelineAction};

/// Minimum time the busy/disabled state stays visible after a click, so the
/// user always sees their click register even when the underlying work is
/// near-instant.
const MIN_BUSY_DISPLAY: Duration = Duration::from_millis(150);

#[derive(Clone, Copy)]
enum PendingOp {
    Split,
    SplitClip(uuid::Uuid),
    Crop,
}

/// Tracks one-frame-deferred operations. A click sets `pending`; the next
/// frame's prologue runs the work and starts a `visible_until` timer that
/// keeps the toolbar button disabled long enough to be perceptible.
#[derive(Default)]
pub struct BusyState {
    pending: Option<PendingOp>,
    visible_until: Option<Instant>,
}

impl BusyState {
    fn is_busy(&self) -> bool {
        self.pending.is_some()
            || self.visible_until.is_some_and(|t| Instant::now() < t)
    }

    fn start(&mut self, op: PendingOp, ctx: &egui::Context) {
        if self.is_busy() {
            return;
        }
        self.pending = Some(op);
        ctx.request_repaint();
    }

    fn take_pending(&mut self) -> Option<PendingOp> {
        let op = self.pending.take();
        if op.is_some() {
            self.visible_until = Some(Instant::now() + MIN_BUSY_DISPLAY);
        }
        op
    }
}

pub struct LayoutState {
    pub preview_panel: preview::PreviewPanel,
    pub timeline_view: timeline_widget::TimelineView,
    pub exporting: bool,
    pub export_progress: f64,
    pub font_manager: FontManager,
    pub busy: BusyState,
}

impl LayoutState {
    pub fn new() -> Self {
        Self {
            preview_panel: preview::PreviewPanel::new(),
            timeline_view: timeline_widget::TimelineView::default(),
            exporting: false,
            export_progress: 0.0,
            font_manager: FontManager::new(),
            busy: BusyState::default(),
        }
    }
}

pub fn show_main_layout(
    ctx: &egui::Context,
    state: &mut AppState,
    commands: &mut CommandHistory,
    engine: &mut MediaEngine,
    layout: &mut LayoutState,
) {
    // Fonts loaded last frame are now in the atlas after begin_frame rebuilt it
    layout.font_manager.begin_frame();

    // Run any operation queued by last frame's click. We defer by one frame
    // so the toolbar can render the button disabled before potentially-slow
    // work runs, giving the user immediate visual confirmation that their
    // click registered. visible_until then keeps the disabled appearance up
    // for at least MIN_BUSY_DISPLAY even if the work was instant.
    if let Some(op) = layout.busy.take_pending() {
        match op {
            PendingOp::Split => {
                do_split(state, commands);
                sync_engine(engine, &state.project);
            }
            PendingOp::SplitClip(clip_id) => {
                let pos = state.playhead.position;
                let cmd = SplitClipCommand::new(clip_id, pos);
                if let Err(e) = commands.execute(Box::new(cmd), state) {
                    log::error!("Split clip failed: {}", e);
                }
                sync_engine(engine, &state.project);
            }
            PendingOp::Crop => {
                do_crop_toggle(layout, state, commands, engine);
            }
        }
        ctx.request_repaint_after(MIN_BUSY_DISPLAY);
    }

    let busy = layout.busy.is_busy();
    let can_split = !busy
        && timeline_widget::clip_at_playhead(&state.project.timeline, state.playhead.position)
            .is_some();
    let has_selection = layout.timeline_view.selected_clip.is_some();
    let can_crop = !busy && (has_selection || layout.preview_panel.crop_mode);
    let crop_mode = layout.preview_panel.crop_mode;
    let has_clips = !state.project.timeline.video_track.clips.is_empty();

    // Poll export progress each frame
    if layout.exporting {
        use crate::media::engine::ExportState;
        let (export_state, progress) = engine.poll_export();
        layout.export_progress = progress;
        match export_state {
            ExportState::Done => {
                layout.exporting = false;
                engine.finish_export();
                log::info!("Export complete!");
            }
            ExportState::Error(msg) => {
                layout.exporting = false;
                engine.finish_export();
                log::error!("Export failed: {}", msg);
            }
            ExportState::Exporting => {
                ctx.request_repaint();
            }
            ExportState::Idle => {
                layout.exporting = false;
            }
        }
    }

    // Toolbar
    let toolbar_action = egui::TopBottomPanel::top("toolbar").show(ctx, |ui| {
        toolbar::show_toolbar(
            ui,
            commands.can_undo(),
            commands.can_redo(),
            can_split,
            has_selection,
            can_crop,
            crop_mode,
            has_clips && !layout.exporting,
        )
    });

    handle_toolbar_actions(
        toolbar_action.inner,
        state,
        commands,
        engine,
        layout,
        ctx,
    );

    // Timeline
    let mut seek_requested: Option<Duration> = None;
    let mut timeline_actions: Vec<TimelineAction> = Vec::new();
    egui::TopBottomPanel::bottom("timeline")
        .resizable(true)
        .min_height(120.0)
        .default_height(220.0)
        .show(ctx, |ui| {
            timeline_widget::show_timeline(
                ui,
                &state.project.timeline,
                &mut state.playhead,
                &mut layout.timeline_view,
                &mut seek_requested,
                &mut timeline_actions,
            );
        });

    if let Some(pos) = seek_requested {
        engine.seek(pos);
    }

    // Process timeline actions
    for action in timeline_actions {
        match action {
            TimelineAction::SelectClip(id) => {
                state.selection.select_clip(id);
                // Enter text edit mode when selecting a text clip
                let is_text = state.project.timeline.get_clip(id)
                    .map(|c| c.track_kind == crate::model::TrackKind::Text)
                    .unwrap_or(false);
                if is_text {
                    layout.preview_panel.text_edit_clip_id = Some(id);
                } else {
                    layout.preview_panel.text_edit_clip_id = None;
                }
            }
            TimelineAction::DeleteSelectedClip => {
                // Handled via toolbar/keyboard
            }
            TimelineAction::SetTransition(junction, kind, duration) => {
                if let Some(pos) = junction_to_transition_pos(&junction) {
                    let cmd = SetTransitionCommand::new(pos, kind, duration);
                    if let Err(e) = commands.execute(Box::new(cmd), state) {
                        log::error!("Set transition failed: {}", e);
                    }
                    sync_engine(engine, &state.project);
                }
            }
            TimelineAction::CommitMove(clip_id, new_start) => {
                use crate::commands::clip_commands::MoveClipCommand;
                let cmd = MoveClipCommand::new(clip_id, new_start);
                if let Err(e) = commands.execute(Box::new(cmd), state) {
                    log::error!("Move clip failed: {}", e);
                }
                sync_engine(engine, &state.project);
            }
            TimelineAction::CommitTrim(clip_id, new_in_point, new_start, new_duration) => {
                use crate::commands::clip_commands::TrimClipCommand;
                let cmd = TrimClipCommand::new(clip_id, new_in_point, new_start, new_duration);
                if let Err(e) = commands.execute(Box::new(cmd), state) {
                    log::error!("Trim clip failed: {}", e);
                }
                sync_engine(engine, &state.project);
            }
            TimelineAction::SplitClip(clip_id) => {
                layout.busy.start(PendingOp::SplitClip(clip_id), ctx);
            }
            TimelineAction::AddTextClip(start) => {
                use crate::commands::clip_commands::AddTextClipCommand;
                // End of content = max of video and audio track durations
                let video_end = state.project.timeline.track_duration(crate::model::TrackKind::Video);
                let audio_end = state.project.timeline.track_duration(crate::model::TrackKind::Audio);
                let end_of_content = video_end.max(audio_end);
                let cmd = AddTextClipCommand::new(start, end_of_content);
                if let Err(e) = commands.execute(Box::new(cmd), state) {
                    log::error!("Add text clip failed: {}", e);
                }
                sync_engine(engine, &state.project);
            }
        }
    }

    // Get selected clip's transform for preview display
    let selected_transform = layout
        .timeline_view
        .selected_clip
        .and_then(|id| state.project.timeline.get_clip(id))
        .map(|c| c.transform.clone());

    // Main area
    let mut browser_open_requested = false;
    let mut export_cancel_requested = false;
    let mut remove_source_id: Option<uuid::Uuid> = None;
    egui::CentralPanel::default().show(ctx, |ui| {
        let available = ui.available_size();
        let browser_width = (available.x * 0.25).clamp(150.0, 300.0);

        ui.horizontal(|ui| {
            ui.allocate_ui(egui::vec2(browser_width, available.y), |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    match media_browser::show_media_browser(ui, &state.project) {
                        media_browser::MediaBrowserAction::OpenFile => {
                            browser_open_requested = true;
                        }
                        media_browser::MediaBrowserAction::RemoveSource(id) => {
                            remove_source_id = Some(id);
                        }
                        media_browser::MediaBrowserAction::None => {}
                    }
                });
            });

            ui.separator();

            ui.allocate_ui(
                egui::vec2(available.x - browser_width - 10.0, available.y),
                |ui| {
                    if layout.exporting {
                        if show_export_progress(ui, layout.export_progress) {
                            export_cancel_requested = true;
                        }
                    } else {
                        // Destructure to avoid double-borrow of layout
                        let (preview_panel, font_manager) = (
                            &mut layout.preview_panel,
                            &mut layout.font_manager,
                        );
                        let preview_action = preview_panel.show(
                            ui,
                            engine,
                            &mut state.playhead,
                            selected_transform.as_ref(),
                            &state.project.timeline,
                            ctx,
                            font_manager,
                        );
                        match preview_action {
                            preview::PreviewAction::UpdateTextOverlay { clip_id, text, position } => {
                                if let Some(overlay) = state.project.timeline.get_text_overlay_mut(clip_id) {
                                    overlay.text = text;
                                    overlay.position = position;
                                }
                            }
                            preview::PreviewAction::SetTextFont { clip_id, font_family } => {
                                if let Some(overlay) = state.project.timeline.get_text_overlay_mut(clip_id) {
                                    overlay.font_family = font_family;
                                }
                            }
                            preview::PreviewAction::SetTextColor { clip_id, color } => {
                                if let Some(overlay) = state.project.timeline.get_text_overlay_mut(clip_id) {
                                    overlay.color = color;
                                }
                            }
                            preview::PreviewAction::SetTextFontSize { clip_id, font_size } => {
                                if let Some(overlay) = state.project.timeline.get_text_overlay_mut(clip_id) {
                                    overlay.font_size = font_size;
                                }
                            }
                            preview::PreviewAction::SetTextBold { clip_id, bold } => {
                                if let Some(overlay) = state.project.timeline.get_text_overlay_mut(clip_id) {
                                    overlay.bold = bold;
                                }
                            }
                            preview::PreviewAction::SetTextItalic { clip_id, italic } => {
                                if let Some(overlay) = state.project.timeline.get_text_overlay_mut(clip_id) {
                                    overlay.italic = italic;
                                }
                            }
                            preview::PreviewAction::None => {}
                        }

                        // When text editing finishes (Enter/Escape), deselect everything
                        if layout.preview_panel.text_edit_finished {
                            layout.preview_panel.text_edit_finished = false;
                            layout.timeline_view.selected_clip = None;
                            state.selection.clear();
                        }
                    }
                },
            );
        });
    });

    if export_cancel_requested {
        engine.finish_export();
        layout.exporting = false;
        log::info!("Export cancelled by user");
    }

    if browser_open_requested {
        open_file_dialog(state, engine);
    }

    if let Some(source_id) = remove_source_id {
        remove_source_from_project(source_id, state, commands, engine);
    }

    if !layout.exporting {
        handle_keyboard_shortcuts(ctx, state, commands, engine, layout);
    }

    // Apply any newly loaded fonts so the atlas rebuilds on next frame's begin_frame
    layout.font_manager.end_frame(ctx);
}

fn sync_engine(engine: &mut MediaEngine, project: &Project) {
    if let Err(e) = engine.sync_from_model(project) {
        log::error!("GES sync failed: {}", e);
    }
}

fn finish_crop(
    layout: &mut LayoutState,
    state: &mut AppState,
    commands: &mut CommandHistory,
    engine: &mut MediaEngine,
) {
    let result = layout.preview_panel.exit_crop_mode();
    // Clear the bypass before syncing so the pipeline picks up the new
    // (or original, if no change) videocrop state.
    engine.set_crop_bypass(None);
    if let Some(result) = result {
        let cmd = SetCropCommand::new(result.clip_id, result.old_transform, result.new_transform);
        if let Err(e) = commands.execute(Box::new(cmd), state) {
            log::error!("Set crop failed: {}", e);
        }
    }
    sync_engine(engine, &state.project);
}

fn handle_toolbar_actions(
    action: toolbar::ToolbarAction,
    state: &mut AppState,
    commands: &mut CommandHistory,
    engine: &mut MediaEngine,
    layout: &mut LayoutState,
    ctx: &egui::Context,
) {
    if action.new_project {
        new_project(state, commands, engine, layout);
    }
    if action.open_file {
        open_file_dialog(state, engine);
    }
    if action.exit {
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }
    if action.split {
        layout.busy.start(PendingOp::Split, ctx);
    }
    if action.delete {
        // Clear text edit if deleting the text clip being edited
        if let Some(clip_id) = layout.timeline_view.selected_clip {
            if layout.preview_panel.text_edit_clip_id == Some(clip_id) {
                layout.preview_panel.text_edit_clip_id = None;
            }
        }
        do_delete(state, commands, &mut layout.timeline_view);
        sync_engine(engine, &state.project);
    }
    if action.undo {
        let _ = commands.undo(state);
        sync_engine(engine, &state.project);
    }
    if action.redo {
        let _ = commands.redo(state);
        sync_engine(engine, &state.project);
    }
    if action.crop {
        layout.busy.start(PendingOp::Crop, ctx);
    }
    if action.export {
        start_export_dialog(state, engine, layout);
    }
    if action.save_project {
        save_project(state);
    }
    if action.save_project_as {
        save_project_as(state);
    }
    if action.open_project {
        open_project(state, commands, engine, layout);
    }
}

fn new_project(
    state: &mut AppState,
    commands: &mut CommandHistory,
    engine: &mut MediaEngine,
    layout: &mut LayoutState,
) {
    engine.stop();
    *state = AppState::new();
    *commands = CommandHistory::new();
    layout.preview_panel = preview::PreviewPanel::new();
    layout.timeline_view = timeline_widget::TimelineView::default();
    layout.exporting = false;
    layout.export_progress = 0.0;
}

fn open_file_dialog(state: &mut AppState, engine: &mut MediaEngine) {
    let file = rfd::FileDialog::new()
        .add_filter(
            "Media",
            &[
                "mp4", "mkv", "avi", "mov", "webm", "flv", "wmv", "mp3", "wav", "ogg", "flac",
                "aac", "m4a",
            ],
        )
        .pick_file();

    if let Some(path) = file {
        match engine.load_file(&path) {
            Ok(info) => {
                let source = crate::model::SourceFile::new(
                    path.clone(),
                    info.duration,
                    info.width,
                    info.height,
                    info.has_video,
                    info.has_audio,
                );
                let source_id = source.id;
                let is_audio_only = source.is_audio_only();
                state.project.source_files.push(source);

                let track_kind = if is_audio_only {
                    crate::model::TrackKind::Audio
                } else {
                    crate::model::TrackKind::Video
                };

                // Audio clips append at end of audio track content,
                // video clips append at end of video track content
                let append_at = if is_audio_only {
                    state.project.timeline.track_duration(crate::model::TrackKind::Audio)
                } else {
                    state.project.timeline.track_duration(crate::model::TrackKind::Video)
                };

                let clip = crate::model::Clip::new(
                    source_id,
                    track_kind,
                    append_at,
                    info.duration,
                );
                state.project.timeline.add_clip(clip);

                sync_engine(engine, &state.project);

                log::info!("Loaded: {}", path.display());
            }
            Err(e) => {
                log::error!("Failed to load file: {}", e);
            }
        }
    }
}

fn remove_source_from_project(
    source_id: uuid::Uuid,
    state: &mut AppState,
    commands: &mut CommandHistory,
    engine: &mut MediaEngine,
) {
    use crate::commands::clip_commands::RemoveSourceCommand;
    let cmd = RemoveSourceCommand::new(source_id);
    if let Err(e) = commands.execute(Box::new(cmd), state) {
        log::error!("Failed to remove source: {}", e);
    }
    sync_engine(engine, &state.project);
}

fn save_project(state: &mut AppState) {
    if state.project.project_path.is_some() {
        write_project_file(state);
    } else {
        save_project_as(state);
    }
}

fn save_project_as(state: &mut AppState) {
    let file = rfd::FileDialog::new()
        .set_file_name("project.fvid")
        .add_filter("FriendlyVid Project", &["fvid"])
        .save_file();

    if let Some(path) = file {
        state.project.project_path = Some(path);
        write_project_file(state);
    }
}

fn write_project_file(state: &AppState) {
    if let Some(ref path) = state.project.project_path {
        match serde_json::to_string_pretty(&state.project) {
            Ok(json) => match std::fs::write(path, json) {
                Ok(()) => log::info!("Project saved: {}", path.display()),
                Err(e) => log::error!("Failed to write project file: {}", e),
            },
            Err(e) => log::error!("Failed to serialize project: {}", e),
        }
    }
}

fn open_project(
    state: &mut AppState,
    commands: &mut CommandHistory,
    engine: &mut MediaEngine,
    layout: &mut LayoutState,
) {
    let file = rfd::FileDialog::new()
        .add_filter("FriendlyVid Project", &["fvid"])
        .pick_file();

    if let Some(path) = file {
        match std::fs::read_to_string(&path) {
            Ok(json) => match serde_json::from_str::<crate::model::Project>(&json) {
                Ok(mut project) => {
                    project.project_path = Some(path.clone());

                    // Reset everything
                    engine.stop();
                    *commands = CommandHistory::new();
                    layout.preview_panel = preview::PreviewPanel::new();
                    layout.timeline_view = timeline_widget::TimelineView::default();
                    layout.exporting = false;
                    layout.export_progress = 0.0;

                    // Re-discover each source file so the GES pipeline knows about them
                    for source in &project.source_files {
                        if let Err(e) = engine.load_file(&source.path) {
                            log::error!(
                                "Failed to reload source {}: {}",
                                source.path.display(),
                                e
                            );
                        }
                    }


                    state.project = project;
                    state.selection = Default::default();
                    state.playhead = Default::default();

                    sync_engine(engine, &state.project);
                    log::info!("Project loaded: {}", path.display());
                }
                Err(e) => log::error!("Failed to parse project file: {}", e),
            },
            Err(e) => log::error!("Failed to read project file: {}", e),
        }
    }
}

fn show_export_progress(ui: &mut egui::Ui, progress: f64) -> bool {
    let mut cancel = false;
    ui.vertical_centered(|ui| {
        ui.add_space(ui.available_height() * 0.3);
        ui.heading("Exporting...");
        ui.add_space(10.0);
        let pct = (progress * 100.0) as u32;
        ui.add(egui::ProgressBar::new(progress as f32).text(format!("{}%", pct)));
        ui.add_space(20.0);
        if ui.button("Cancel").clicked() {
            cancel = true;
        }
    });
    cancel
}

fn start_export_dialog(state: &mut AppState, engine: &mut MediaEngine, layout: &mut LayoutState) {
    let file = rfd::FileDialog::new()
        .set_file_name("export.mp4")
        .add_filter("MP4", &["mp4"])
        .save_file();

    if let Some(path) = file {
        match engine.start_export(&path, &state.project) {
            Ok(()) => {
                layout.exporting = true;
                layout.export_progress = 0.0;
                log::info!("Exporting to: {}", path.display());
            }
            Err(e) => {
                log::error!("Failed to start export: {}", e);
            }
        }
    }
}

fn do_split(state: &mut AppState, commands: &mut CommandHistory) {
    let pos = state.playhead.position;
    if let Some(clip_id) =
        timeline_widget::clip_at_playhead(&state.project.timeline, pos)
    {
        let cmd = SplitClipCommand::new(clip_id, pos);
        if let Err(e) = commands.execute(Box::new(cmd), state) {
            log::error!("Split failed: {}", e);
        }
    }
}

fn do_crop_toggle(
    layout: &mut LayoutState,
    state: &mut AppState,
    commands: &mut CommandHistory,
    engine: &mut MediaEngine,
) {
    if layout.preview_panel.crop_mode {
        finish_crop(layout, state, commands, engine);
    } else if let Some(clip_id) = layout.timeline_view.selected_clip {
        if let Some(clip) = state.project.timeline.get_clip(clip_id) {
            layout
                .preview_panel
                .enter_crop_mode(clip_id, clip.transform.clone());
            // Bypass the clip's videocrop while in crop mode so the preview's
            // UV crop operates on the uncropped source.
            engine.set_crop_bypass(Some(clip_id));
            sync_engine(engine, &state.project);
        }
    }
}

fn do_delete(
    state: &mut AppState,
    commands: &mut CommandHistory,
    view: &mut timeline_widget::TimelineView,
) {
    if let Some(clip_id) = view.selected_clip {
        let cmd = RemoveClipCommand::new(clip_id);
        if let Err(e) = commands.execute(Box::new(cmd), state) {
            log::error!("Delete failed: {}", e);
        }
        view.selected_clip = None;
        state.selection.clear();
    }
}

fn junction_to_transition_pos(junction: &JunctionKind) -> Option<TransitionPosition> {
    Some(match junction {
        JunctionKind::Between(a, b) => TransitionPosition::Between(*a, *b),
        JunctionKind::LeadingEdge(c) => TransitionPosition::FadeIn(*c),
        JunctionKind::TrailingEdge(c) => TransitionPosition::FadeOut(*c),
    })
}

fn handle_keyboard_shortcuts(
    ctx: &egui::Context,
    state: &mut AppState,
    commands: &mut CommandHistory,
    engine: &mut MediaEngine,
    layout: &mut LayoutState,
) {
    // While inline text editing is active, only Escape (cancel) and Enter (commit)
    // should be processed. All other keys belong to the text field.
    if layout.preview_panel.text_editing_inline {
        // Escape and Enter are handled by preview.rs (lost_focus / key checks).
        // Nothing else should fire here.
        return;
    }

    let escape = ctx.input(|i| i.key_pressed(egui::Key::Escape));
    let enter = ctx.input(|i| i.key_pressed(egui::Key::Enter));
    let space = ctx.input(|i| i.key_pressed(egui::Key::Space));
    let ctrl_z = ctx.input(|i| i.modifiers.ctrl && i.key_pressed(egui::Key::Z) && !i.modifiers.shift);
    let ctrl_shift_z = ctx.input(|i| i.modifiers.ctrl && i.modifiers.shift && i.key_pressed(egui::Key::Z));
    let ctrl_s = ctx.input(|i| i.modifiers.ctrl && i.key_pressed(egui::Key::S));
    let ctrl_b = ctx.input(|i| i.modifiers.ctrl && i.key_pressed(egui::Key::B));
    let delete = ctx.input(|i| i.key_pressed(egui::Key::Delete) || i.key_pressed(egui::Key::Backspace));

    // Enter confirms crop mode (saves changes)
    if enter && layout.preview_panel.crop_mode {
        finish_crop(layout, state, commands, engine);
        return;
    }

    // Escape cancels crop mode (discards changes) or text edit mode
    if escape && layout.preview_panel.crop_mode {
        layout.preview_panel.cancel_crop_mode();
        engine.set_crop_bypass(None);
        sync_engine(engine, &state.project);
        return;
    }
    if escape && layout.preview_panel.text_edit_clip_id.is_some() {
        layout.preview_panel.text_edit_clip_id = None;
        layout.preview_panel.text_editing_inline = false;
        layout.timeline_view.selected_clip = None;
        state.selection.clear();
        return;
    }

    if space {
        if state.playhead.playing {
            engine.pause();
            state.playhead.playing = false;
        } else {
            engine.play();
            state.playhead.playing = true;
        }
    }

    if ctrl_s {
        save_project(state);
    }

    if ctrl_z {
        let _ = commands.undo(state);
        sync_engine(engine, &state.project);
    }

    if ctrl_shift_z {
        let _ = commands.redo(state);
        sync_engine(engine, &state.project);
    }

    if ctrl_b {
        layout.busy.start(PendingOp::Split, ctx);
    }

    if delete {
        do_delete(state, commands, &mut layout.timeline_view);
        sync_engine(engine, &state.project);
    }
}
