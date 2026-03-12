use egui::{self, Color32, Rect, Stroke, Vec2};
use std::time::Duration;
use uuid::Uuid;

use crate::model::{Timeline, TrackKind, TransitionKind, TransitionPosition};
use crate::state::Playhead;

const RULER_HEIGHT: f32 = 24.0;
const TRACK_HEIGHT: f32 = 48.0;
const TRACK_GAP: f32 = 2.0;
const TRACK_LABEL_WIDTH: f32 = 60.0;
const PLAYHEAD_GRAB_RADIUS: f32 = 12.0;
const JUNCTION_HIT_WIDTH: f32 = 10.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JunctionKind {
    Between(Uuid, Uuid),
    LeadingEdge(Uuid),
    TrailingEdge(Uuid),
}

pub enum TimelineAction {
    SelectClip(Uuid),
    #[allow(dead_code)]
    DeleteSelectedClip,
    SetTransition(JunctionKind, Option<TransitionKind>, Duration),
    CommitMove(Uuid, Duration),
    CommitTrim(Uuid, Duration, Duration, Duration), // clip_id, new_in_point, new_start, new_duration
    AddTextClip(Duration),
    SplitClip(Uuid),
}

const EDGE_HIT_WIDTH: f32 = 6.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Edge {
    Left,
    Right,
}

#[allow(dead_code)]
enum DragState {
    MovingClip {
        clip_id: Uuid,
        original_start: Duration,
        grab_offset_secs: f64,
    },
    ResizingLeft {
        clip_id: Uuid,
        original_start: Duration,
        original_duration: Duration,
        original_in_point: Duration,
    },
    ResizingRight {
        clip_id: Uuid,
        original_duration: Duration,
    },
}

pub struct TimelineView {
    pub zoom: f64,
    pub scroll_offset: f64,
    dragging_playhead: bool,
    pub selected_clip: Option<Uuid>,
    hovered_clip: Option<Uuid>,
    hovered_junction: Option<JunctionKind>,
    hovered_edge: Option<(Uuid, Edge)>,
    drag_state: Option<DragState>,
    junction_menu: Option<JunctionMenuState>,
    clip_context_menu: Option<ClipContextMenuState>,
    last_transition_duration: f32,
}

enum MenuPhase {
    MainMenu,
    WipeSubmenu,
    DurationDialog { kind: TransitionKind, seconds: f32 },
}

struct JunctionMenuState {
    junction: JunctionKind,
    is_edge: bool,
    click_pos: egui::Pos2,
    phase: MenuPhase,
    main_menu_rect: Option<egui::Rect>,
}

struct ClipContextMenuState {
    clip_id: Uuid,
    click_pos: egui::Pos2,
}

impl Default for TimelineView {
    fn default() -> Self {
        Self {
            zoom: 100.0,
            scroll_offset: 0.0,
            dragging_playhead: false,
            selected_clip: None,
            hovered_clip: None,
            hovered_junction: None,
            hovered_edge: None,
            drag_state: None,
            junction_menu: None,
            clip_context_menu: None,
            last_transition_duration: 2.0,
        }
    }
}

/// Find the clip under the playhead on the video track
pub fn clip_at_playhead(timeline: &Timeline, playhead_pos: Duration) -> Option<Uuid> {
    for clip_id in &timeline.video_track.clips {
        if let Some(clip) = timeline.get_clip(*clip_id) {
            if playhead_pos >= clip.start && playhead_pos < clip.end() {
                return Some(clip.id);
            }
        }
    }
    None
}

pub fn show_timeline(
    ui: &mut egui::Ui,
    timeline: &Timeline,
    playhead: &mut Playhead,
    view: &mut TimelineView,
    seek_requested: &mut Option<Duration>,
    actions: &mut Vec<TimelineAction>,
) {
    // Show context menus if open (must happen before allocate_painter so egui
    // processes the popup in the right layer)
    show_junction_menu(ui, timeline, view, actions);
    show_clip_context_menu(ui, timeline, view, actions, playhead);

    let (response, painter) =
        ui.allocate_painter(ui.available_size(), egui::Sense::click_and_drag());
    let rect = response.rect;

    if rect.width() < 10.0 || rect.height() < 10.0 {
        return;
    }

    painter.rect_filled(rect, 0.0, Color32::from_gray(30));

    let content_left = rect.left() + TRACK_LABEL_WIDTH;
    let content_width = rect.width() - TRACK_LABEL_WIDTH;

    // Ruler
    let ruler_rect = Rect::from_min_size(
        egui::pos2(content_left, rect.top()),
        Vec2::new(content_width, RULER_HEIGHT),
    );
    draw_ruler(&painter, ruler_rect, view);

    // Detect hover position
    let pointer_pos = ui.input(|i| i.pointer.hover_pos());
    let video_track_y = rect.top() + RULER_HEIGHT;

    // Build clip rects for hit testing
    struct ClipRect {
        id: Uuid,
        rect: Rect,
        #[allow(dead_code)]
        track_kind: TrackKind,
    }
    let mut clip_rects: Vec<ClipRect> = Vec::new();

    // Track info for drawing
    let tracks_info: &[(&str, Color32, &[Uuid], TrackKind)] = &[
        ("Video", Color32::from_rgb(70, 130, 180), &timeline.video_track.clips, TrackKind::Video),
        ("Audio", Color32::from_rgb(60, 179, 113), &timeline.audio_track.clips, TrackKind::Audio),
        ("Text", Color32::from_rgb(186, 85, 211), &timeline.text_track.clips, TrackKind::Text),
    ];

    for (i, (label, color, clip_ids, track_kind)) in tracks_info.iter().enumerate() {
        let y = rect.top() + RULER_HEIGHT + (i as f32) * (TRACK_HEIGHT + TRACK_GAP);

        // Track label
        let label_rect = Rect::from_min_size(
            egui::pos2(rect.left(), y),
            Vec2::new(TRACK_LABEL_WIDTH, TRACK_HEIGHT),
        );
        painter.rect_filled(label_rect, 0.0, Color32::from_gray(45));
        painter.text(
            label_rect.center(),
            egui::Align2::CENTER_CENTER,
            *label,
            egui::FontId::proportional(12.0),
            Color32::from_gray(180),
        );

        // Track background
        let track_rect = Rect::from_min_size(
            egui::pos2(content_left, y),
            Vec2::new(content_width, TRACK_HEIGHT),
        );
        painter.rect_filled(track_rect, 0.0, Color32::from_gray(38));

        // Draw clips
        for clip_id in clip_ids.iter() {
            if let Some(clip) = timeline.get_clip(*clip_id) {
                let mut x_start = time_to_x(clip.start, view, content_left);
                let mut x_end = time_to_x(clip.end(), view, content_left);

                // Adjust visual position during active drag
                if let Some(ref drag) = view.drag_state {
                    if let Some(pos) = pointer_pos {
                        match drag {
                            DragState::MovingClip { clip_id: did, grab_offset_secs, .. } if *did == *clip_id => {
                                let clip_width = x_end - x_start;
                                let mouse_time = x_to_time(pos.x, view, content_left);
                                let new_secs = (mouse_time.as_secs_f64() - grab_offset_secs).max(0.0);
                                x_start = time_to_x(Duration::from_secs_f64(new_secs), view, content_left);
                                x_end = x_start + clip_width;
                            }
                            DragState::ResizingLeft { clip_id: did, .. } if *did == *clip_id => {
                                x_start = pos.x.min(x_end - 5.0);
                            }
                            DragState::ResizingRight { clip_id: did, .. } if *did == *clip_id => {
                                x_end = pos.x.max(x_start + 5.0);
                            }
                            _ => {}
                        }
                    }
                }

                if x_end < content_left || x_start > rect.right() {
                    continue;
                }

                let clip_rect = Rect::from_min_max(
                    egui::pos2(x_start.max(content_left), y + 2.0),
                    egui::pos2(x_end.min(rect.right()), y + TRACK_HEIGHT - 2.0),
                );

                // Determine color based on hover/selection state
                let is_selected = view.selected_clip == Some(*clip_id);
                let is_hovered = view.hovered_clip == Some(*clip_id);
                let fill = if is_selected {
                    color.linear_multiply(1.5)
                } else if is_hovered {
                    color.linear_multiply(1.25)
                } else {
                    *color
                };

                painter.rect_filled(clip_rect, 4.0, fill);

                let stroke_color = if is_selected {
                    Color32::WHITE
                } else {
                    color.linear_multiply(1.4)
                };
                let stroke_width = if is_selected { 2.0 } else { 1.0 };
                painter.rect_stroke(
                    clip_rect,
                    4.0,
                    Stroke::new(stroke_width, stroke_color),
                    egui::StrokeKind::Outside,
                );

                if clip_rect.width() > 30.0 {
                    let name = if clip.track_kind == TrackKind::Text {
                        // Show text overlay content for text clips
                        timeline.get_text_overlay(clip.id)
                            .map(|o| {
                                let t = &o.text;
                                if t.len() > 20 { format!("{}...", &t[..17]) } else { t.clone() }
                            })
                            .unwrap_or_else(|| clip.display_name())
                    } else {
                        clip.display_name()
                    };
                    painter.text(
                        clip_rect.center(),
                        egui::Align2::CENTER_CENTER,
                        &name,
                        egui::FontId::proportional(11.0),
                        Color32::WHITE,
                    );
                }

                clip_rects.push(ClipRect {
                    id: *clip_id,
                    rect: clip_rect,
                    track_kind: *track_kind,
                });
            }
        }
    }

    // Re-draw selected text clip on top of other overlapping text clips
    if let Some(sel_id) = view.selected_clip {
        if let Some(clip) = timeline.get_clip(sel_id) {
            if clip.track_kind == TrackKind::Text {
                let y = rect.top() + RULER_HEIGHT + 2.0 * (TRACK_HEIGHT + TRACK_GAP);
                let x_start = time_to_x(clip.start, view, content_left);
                let x_end = time_to_x(clip.end(), view, content_left);
                if x_end >= content_left && x_start <= rect.right() {
                    let clip_rect = Rect::from_min_max(
                        egui::pos2(x_start.max(content_left), y + 2.0),
                        egui::pos2(x_end.min(rect.right()), y + TRACK_HEIGHT - 2.0),
                    );
                    let color = Color32::from_rgb(186, 85, 211);
                    painter.rect_filled(clip_rect, 4.0, color.linear_multiply(1.5));
                    painter.rect_stroke(
                        clip_rect,
                        4.0,
                        Stroke::new(2.0, Color32::WHITE),
                        egui::StrokeKind::Outside,
                    );
                    if clip_rect.width() > 30.0 {
                        let name = timeline
                            .get_text_overlay(sel_id)
                            .map(|o| {
                                let t = &o.text;
                                if t.len() > 20 {
                                    format!("{}...", &t[..17])
                                } else {
                                    t.clone()
                                }
                            })
                            .unwrap_or_else(|| clip.display_name());
                        painter.text(
                            clip_rect.center(),
                            egui::Align2::CENTER_CENTER,
                            &name,
                            egui::FontId::proportional(11.0),
                            Color32::WHITE,
                        );
                    }
                }
            }
        }
    }

    // -- Detect hovered clip and junction --
    view.hovered_clip = None;
    view.hovered_junction = None;

    if let Some(pos) = pointer_pos {
        if pos.x >= content_left {
            // Determine which track the pointer is over
            let hovered_track = determine_hovered_track(pos.y, video_track_y);

            // Get clips for the hovered track
            let track_clips: &[Uuid] = match hovered_track {
                Some(TrackKind::Video) => &timeline.video_track.clips,
                Some(TrackKind::Audio) => &timeline.audio_track.clips,
                Some(TrackKind::Text) => &timeline.text_track.clips,
                None => &[],
            };

            // Skip junction detection for text track (text overlays don't use GES transitions)
            if !track_clips.is_empty() && hovered_track != Some(TrackKind::Text) {
                // Check junctions first (they take priority over clip body)
                // Leading edge (before first clip)
                if let Some(first) = track_clips.first() {
                    if let Some(clip) = timeline.get_clip(*first) {
                        let edge_x = time_to_x(clip.start, view, content_left);
                        if (pos.x - edge_x).abs() < JUNCTION_HIT_WIDTH {
                            view.hovered_junction = Some(JunctionKind::LeadingEdge(*first));
                        }
                    }
                }
                // Trailing edge (after last clip)
                if let Some(last) = track_clips.last() {
                    if let Some(clip) = timeline.get_clip(*last) {
                        let edge_x = time_to_x(clip.end(), view, content_left);
                        if (pos.x - edge_x).abs() < JUNCTION_HIT_WIDTH {
                            view.hovered_junction = Some(JunctionKind::TrailingEdge(*last));
                        }
                    }
                }
                // Between adjacent clips (touching or overlapping)
                for pair in track_clips.windows(2) {
                    let (left_id, right_id) = (pair[0], pair[1]);
                    if let (Some(left), Some(right)) =
                        (timeline.get_clip(left_id), timeline.get_clip(right_id))
                    {
                        let boundary_x = time_to_x(left.end(), view, content_left);
                        let right_start_x = time_to_x(right.start, view, content_left);
                        // For touching clips, check left.end. For gapped clips,
                        // check both edges.
                        if (pos.x - boundary_x).abs() < JUNCTION_HIT_WIDTH
                            || (pos.x - right_start_x).abs() < JUNCTION_HIT_WIDTH
                        {
                            view.hovered_junction =
                                Some(JunctionKind::Between(left_id, right_id));
                        }
                    }
                }
            }

            // If not hovering a junction, check clip edges for resize, then clip bodies
            if view.hovered_junction.is_none() {
                // Check edges first (resize handles)
                view.hovered_edge = None;
                for cr in &clip_rects {
                    if pos.y >= cr.rect.top() && pos.y <= cr.rect.bottom() {
                        if (pos.x - cr.rect.left()).abs() < EDGE_HIT_WIDTH {
                            view.hovered_edge = Some((cr.id, Edge::Left));
                            break;
                        }
                        if (pos.x - cr.rect.right()).abs() < EDGE_HIT_WIDTH {
                            view.hovered_edge = Some((cr.id, Edge::Right));
                            break;
                        }
                    }
                }

                // Then clip bodies
                if view.hovered_edge.is_none() {
                    for cr in &clip_rects {
                        if cr.rect.contains(pos) {
                            view.hovered_clip = Some(cr.id);
                            // For text clips, keep iterating - overlapping clips
                            // are drawn later (on top), so the last match wins.
                            // Video/audio clips don't overlap so stop at first hit.
                            if cr.track_kind != TrackKind::Text {
                                break;
                            }
                        }
                    }
                }
            }
        }
    }

    // -- Drag handling --
    // Start drag
    if response.drag_started() && !view.dragging_playhead && view.junction_menu.is_none() {
        if let Some((edge_id, edge)) = view.hovered_edge {
            if let Some(clip) = timeline.get_clip(edge_id) {
                match edge {
                    Edge::Left => {
                        view.drag_state = Some(DragState::ResizingLeft {
                            clip_id: edge_id,
                            original_start: clip.start,
                            original_duration: clip.duration,
                            original_in_point: clip.in_point,
                        });
                    }
                    Edge::Right => {
                        view.drag_state = Some(DragState::ResizingRight {
                            clip_id: edge_id,
                            original_duration: clip.duration,
                        });
                    }
                }
            }
        } else if let Some(hovered_id) = view.hovered_clip {
            if let Some(clip) = timeline.get_clip(hovered_id) {
                // Only allow body drag for audio/text tracks (not video)
                if clip.track_kind != TrackKind::Video {
                    if let Some(pos) = response.interact_pointer_pos() {
                        let click_time = x_to_time(pos.x, view, content_left);
                        let offset = click_time.as_secs_f64() - clip.start.as_secs_f64();
                        view.drag_state = Some(DragState::MovingClip {
                            clip_id: hovered_id,
                            original_start: clip.start,
                            grab_offset_secs: offset,
                        });
                    }
                }
            }
        }
    }

    // End drag - commit changes
    if response.drag_stopped() {
        if let Some(drag) = view.drag_state.take() {
            if let Some(pos) = response.interact_pointer_pos() {
                match drag {
                    DragState::MovingClip {
                        clip_id,
                        original_start: _,
                        grab_offset_secs,
                    } => {
                        let mouse_time = x_to_time(pos.x, view, content_left);
                        let new_secs = (mouse_time.as_secs_f64() - grab_offset_secs).max(0.0);
                        let new_start = Duration::from_secs_f64(new_secs);
                        actions.push(TimelineAction::CommitMove(clip_id, new_start));
                    }
                    DragState::ResizingLeft {
                        clip_id,
                        original_start,
                        original_duration,
                        original_in_point,
                    } => {
                        let mouse_time = x_to_time(pos.x, view, content_left);
                        let original_end_secs =
                            original_start.as_secs_f64() + original_duration.as_secs_f64();
                        let new_start_secs =
                            mouse_time.as_secs_f64().max(0.0).min(original_end_secs - 0.1);
                        let delta = new_start_secs - original_start.as_secs_f64();
                        let new_start = Duration::from_secs_f64(new_start_secs);
                        let new_duration =
                            Duration::from_secs_f64((original_duration.as_secs_f64() - delta).max(0.1));
                        let new_in_point =
                            Duration::from_secs_f64((original_in_point.as_secs_f64() + delta).max(0.0));
                        actions.push(TimelineAction::CommitTrim(
                            clip_id,
                            new_in_point,
                            new_start,
                            new_duration,
                        ));
                    }
                    DragState::ResizingRight {
                        clip_id,
                        original_duration: _,
                    } => {
                        if let Some(clip) = timeline.get_clip(clip_id) {
                            let mouse_time = x_to_time(pos.x, view, content_left);
                            let new_dur_secs = (mouse_time.as_secs_f64()
                                - clip.start.as_secs_f64())
                            .max(0.1);
                            let new_duration = Duration::from_secs_f64(new_dur_secs);
                            actions.push(TimelineAction::CommitTrim(
                                clip_id,
                                clip.in_point,
                                clip.start,
                                new_duration,
                            ));
                        }
                    }
                }
            }
        }
    }

    // -- Draw junction highlights --
    if let Some(ref junction) = view.hovered_junction {
        let jx = junction_x(junction, timeline, view, content_left);
        let track_y = junction_track_y(junction, timeline, video_track_y);
        if let Some(x) = jx {
            painter.line_segment(
                [
                    egui::pos2(x, track_y + 2.0),
                    egui::pos2(x, track_y + TRACK_HEIGHT - 2.0),
                ],
                Stroke::new(3.0, Color32::YELLOW),
            );
            let cy = track_y + TRACK_HEIGHT * 0.5;
            let d = 6.0;
            let points = vec![
                egui::pos2(x, cy - d),
                egui::pos2(x + d, cy),
                egui::pos2(x, cy + d),
                egui::pos2(x - d, cy),
            ];
            painter.add(egui::Shape::convex_polygon(
                points,
                Color32::YELLOW,
                Stroke::NONE,
            ));
        }
    }

    // -- Draw existing transition indicators --
    for transition in &timeline.transitions {
        let tx = transition_position_x(transition, timeline, view, content_left);
        let track_y = transition_track_y(transition, timeline, video_track_y);
        if let Some(x) = tx {
            let cy = track_y + TRACK_HEIGHT * 0.5;
            let d = 5.0;
            let points = vec![
                egui::pos2(x, cy - d),
                egui::pos2(x + d, cy),
                egui::pos2(x, cy + d),
                egui::pos2(x - d, cy),
            ];
            painter.add(egui::Shape::convex_polygon(
                points,
                Color32::from_rgb(0, 200, 255),
                Stroke::new(1.0, Color32::WHITE),
            ));
            painter.text(
                egui::pos2(x, cy - d - 4.0),
                egui::Align2::CENTER_BOTTOM,
                transition.kind.label(),
                egui::FontId::proportional(9.0),
                Color32::from_rgb(0, 200, 255),
            );
        }
    }

    // -- Playhead interaction --
    let ph_x = time_to_x(playhead.position, view, content_left);
    let ph_handle_center = egui::pos2(ph_x, rect.top() + RULER_HEIGHT * 0.5);

    let near_handle = pointer_pos
        .map(|p| p.distance(ph_handle_center) < PLAYHEAD_GRAB_RADIUS)
        .unwrap_or(false);

    if response.drag_started() {
        if let Some(pos) = response.interact_pointer_pos() {
            if pos.distance(ph_handle_center) < PLAYHEAD_GRAB_RADIUS
                || (pos.y < rect.top() + RULER_HEIGHT && pos.x >= content_left)
            {
                view.dragging_playhead = true;
            }
        }
    }

    if view.dragging_playhead && response.dragged() {
        if let Some(pos) = response.interact_pointer_pos() {
            let time = x_to_time(pos.x, view, content_left);
            playhead.position = time;
            *seek_requested = Some(time);
        }
    }

    if response.drag_stopped() {
        view.dragging_playhead = false;
    }

    // Right-click: clip context menu (split) or text track add
    if response.secondary_clicked() {
        if let Some(pos) = response.interact_pointer_pos() {
            if pos.x >= content_left {
                // Check if right-clicking on a clip
                let mut clicked_clip = None;
                for cr in &clip_rects {
                    if cr.rect.contains(pos) {
                        clicked_clip = Some(cr.id);
                        break;
                    }
                }
                if let Some(clip_id) = clicked_clip {
                    view.clip_context_menu = Some(ClipContextMenuState {
                        clip_id,
                        click_pos: pos,
                    });
                } else {
                    // No clip under cursor: add text clip if on text track
                    let hovered_track = determine_hovered_track(pos.y, video_track_y);
                    if hovered_track == Some(TrackKind::Text) {
                        let click_time = x_to_time(pos.x, view, content_left);
                        actions.push(TimelineAction::AddTextClip(click_time));
                    }
                }
            }
        }
    }

    // Click handling (not drag)
    if response.clicked() && !view.dragging_playhead {
        if let Some(pos) = response.interact_pointer_pos() {
            if pos.x >= content_left {
                // Check if clicking a junction - open context menu
                if let Some(junction) = view.hovered_junction {
                    let is_edge = matches!(
                        junction,
                        JunctionKind::LeadingEdge(_) | JunctionKind::TrailingEdge(_)
                    );
                    view.junction_menu = Some(JunctionMenuState {
                        junction,
                        is_edge,
                        click_pos: pos,
                        phase: MenuPhase::MainMenu,
                        main_menu_rect: None,
                    });
                }
                // Check if clicking a clip - select it
                else if let Some(clip_id) = view.hovered_clip {
                    view.selected_clip = Some(clip_id);
                    actions.push(TimelineAction::SelectClip(clip_id));
                }
                // Otherwise set playhead
                else {
                    view.selected_clip = None;
                    let time = x_to_time(pos.x, view, content_left);
                    playhead.position = time;
                    *seek_requested = Some(time);
                }
            }
        }
    }

    // Cursor icon
    if view.drag_state.is_some() {
        match &view.drag_state {
            Some(DragState::MovingClip { .. }) => {
                ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
            }
            Some(DragState::ResizingLeft { .. } | DragState::ResizingRight { .. }) => {
                ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
            }
            None => {}
        }
    } else if view.hovered_edge.is_some() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
    } else if view.hovered_junction.is_some() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    } else if view.hovered_clip.is_some() {
        // Show grab cursor for audio/text clips (moveable), pointing hand for video
        if let Some(hovered_id) = view.hovered_clip {
            if let Some(clip) = timeline.get_clip(hovered_id) {
                if clip.track_kind != TrackKind::Video {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
                } else {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                }
            }
        }
    } else if near_handle || view.dragging_playhead {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
    }

    // Draw playhead (on top of everything)
    if ph_x >= content_left && ph_x <= rect.right() {
        painter.line_segment(
            [
                egui::pos2(ph_x, rect.top()),
                egui::pos2(ph_x, rect.bottom()),
            ],
            Stroke::new(2.0, Color32::RED),
        );
        let handle_color = if view.dragging_playhead || near_handle {
            Color32::from_rgb(255, 80, 80)
        } else {
            Color32::RED
        };
        painter.circle_filled(ph_handle_center, 8.0, handle_color);
    }

    // Zoom/scroll: vertical scroll = zoom, horizontal scroll = pan
    if response.hovered() {
        let scroll = ui.input(|i| i.smooth_scroll_delta);
        if scroll.y != 0.0 {
            let factor = 1.0 + (scroll.y as f64 * 0.005);
            view.zoom = (view.zoom * factor).clamp(20.0, 1000.0);
        }
        if scroll.x != 0.0 {
            view.scroll_offset -= scroll.x as f64 / view.zoom;
            view.scroll_offset = view.scroll_offset.max(0.0);
        }
    }
}

fn show_junction_menu(
    ui: &mut egui::Ui,
    timeline: &Timeline,
    view: &mut TimelineView,
    actions: &mut Vec<TimelineAction>,
) {
    let mut menu_state = match view.junction_menu.take() {
        Some(s) => s,
        None => return,
    };

    let popup_id = egui::Id::new("junction_transition_menu");
    #[allow(deprecated)]
    ui.memory_mut(|m| m.open_popup(popup_id));

    let escape_pressed = ui.input(|i| i.key_pressed(egui::Key::Escape));
    let enter_pressed = ui.input(|i| i.key_pressed(egui::Key::Enter));
    let mut keep_open = true;
    let mut chosen: Option<(Option<TransitionKind>, Duration)> = None;

    let trans_pos = junction_to_transition_pos(&menu_state.junction);
    let current = trans_pos
        .as_ref()
        .and_then(|p| timeline.find_transition(p))
        .map(|t| t.kind);
    let is_wipe = matches!(
        current,
        Some(TransitionKind::WipeLeft)
            | Some(TransitionKind::WipeRight)
            | Some(TransitionKind::WipeDown)
            | Some(TransitionKind::WipeUp)
    );

    let menu_frame = egui::Frame::new()
        .fill(egui::Color32::from_gray(40))
        .stroke(egui::Stroke::new(1.0, egui::Color32::from_gray(80)))
        .inner_margin(egui::Margin::same(4));

    // Main menu area
    let area_resp = egui::Area::new(popup_id)
        .order(egui::Order::Foreground)
        .fixed_pos(menu_state.click_pos)
        .show(ui.ctx(), |ui| {
            menu_frame.show(ui, |ui| {
                ui.set_min_width(130.0);

                // Title
                ui.label(
                    egui::RichText::new("Transition")
                        .strong()
                        .color(egui::Color32::from_gray(160)),
                );
                ui.separator();

                match &menu_state.phase {
                    MenuPhase::MainMenu | MenuPhase::WipeSubmenu => {
                        // Cut option
                        let cut_mark = if current.is_none() { "  *" } else { "" };
                        if ui
                            .selectable_label(false, format!("Cut{}", cut_mark))
                            .clicked()
                        {
                            chosen = Some((None, Duration::ZERO));
                            keep_open = false;
                        }

                        // Fade option
                        let fade_mark =
                            if current == Some(TransitionKind::Fade) { "  *" } else { "" };
                        if ui
                            .selectable_label(false, format!("Fade{}", fade_mark))
                            .clicked()
                        {
                            menu_state.phase = MenuPhase::DurationDialog {
                                kind: TransitionKind::Fade,
                                seconds: view.last_transition_duration,
                            };
                        }

                        // Wipe option (only for between-clip video junctions)
                        let is_video_track = junction_is_video(&menu_state.junction, timeline);
                        if !menu_state.is_edge && is_video_track {
                            let wipe_mark = if is_wipe { "  *" } else { "" };
                            let wipe_resp = ui.selectable_label(
                                matches!(menu_state.phase, MenuPhase::WipeSubmenu),
                                format!("Wipe{}  >", wipe_mark),
                            );
                            if wipe_resp.hovered() {
                                menu_state.phase = MenuPhase::WipeSubmenu;
                            }
                        }
                    }
                    MenuPhase::DurationDialog { .. } => {
                        // Rendered in a separate Area below
                    }
                }
            });
        });

    let main_rect = area_resp.response.rect;
    menu_state.main_menu_rect = Some(main_rect);

    // Handle DurationDialog phase: extract fields to locals to avoid borrow issues
    if let MenuPhase::DurationDialog { kind, seconds } = menu_state.phase {
        let dialog_kind = kind;
        let mut secs = seconds;
        let mut apply = false;
        let mut cancel = false;

        let title = match dialog_kind {
            TransitionKind::Fade | TransitionKind::Dissolve => "Fade Duration",
            _ => "Wipe Duration",
        };

        let dur_popup_id = egui::Id::new("junction_duration_dialog");
        let dur_resp = egui::Area::new(dur_popup_id)
            .order(egui::Order::Foreground)
            .fixed_pos(menu_state.click_pos)
            .show(ui.ctx(), |ui| {
                menu_frame.show(ui, |ui| {
                    ui.set_min_width(160.0);
                    ui.label(
                        egui::RichText::new(title)
                            .strong()
                            .color(egui::Color32::from_gray(160)),
                    );
                    ui.separator();
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        ui.add(
                            egui::DragValue::new(&mut secs)
                                .range(0.1..=5.0)
                                .speed(0.1)
                                .suffix(" sec")
                                .fixed_decimals(1),
                        );
                    });
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        if ui.button("Apply").clicked() || enter_pressed {
                            apply = true;
                        }
                        if ui.button("Cancel").clicked() || escape_pressed {
                            cancel = true;
                        }
                    });
                });
            });

        if apply {
            let dur = Duration::from_secs_f32(secs.clamp(0.1, 5.0));
            view.last_transition_duration = secs;
            chosen = Some((Some(dialog_kind), dur));
            keep_open = false;
        } else if cancel {
            menu_state.phase = MenuPhase::MainMenu;
        } else {
            menu_state.phase = MenuPhase::DurationDialog {
                kind: dialog_kind,
                seconds: secs,
            };
        }

        // Check click-outside against the duration popup
        let dur_rect = dur_resp.response.rect;
        if keep_open {
            let clicked_outside = ui.input(|i| {
                i.pointer.any_pressed()
                    && i.pointer
                        .latest_pos()
                        .map_or(false, |p| !dur_rect.contains(p))
            });
            if clicked_outside {
                keep_open = false;
            }
        }
    }

    // Wipe submenu
    if matches!(menu_state.phase, MenuPhase::WipeSubmenu) {
        let sub_id = egui::Id::new("junction_wipe_submenu");
        let sub_pos = egui::pos2(main_rect.right() + 2.0, main_rect.top());
        let sub_resp = egui::Area::new(sub_id)
            .order(egui::Order::Foreground)
            .fixed_pos(sub_pos)
            .show(ui.ctx(), |ui| {
                menu_frame.show(ui, |ui| {
                    ui.set_min_width(90.0);
                    let mk = |k: TransitionKind| if current == Some(k) { "  *" } else { "" };
                    let wipe_options = [
                        ("Left", TransitionKind::WipeLeft),
                        ("Right", TransitionKind::WipeRight),
                        ("Up", TransitionKind::WipeUp),
                        ("Down", TransitionKind::WipeDown),
                    ];
                    for (label, kind) in &wipe_options {
                        if ui
                            .selectable_label(false, format!("{}{}", label, mk(*kind)))
                            .clicked()
                        {
                            menu_state.phase = MenuPhase::DurationDialog {
                                kind: *kind,
                                seconds: view.last_transition_duration,
                            };
                        }
                    }
                });
            });

        // Check clicks outside both main menu and submenu
        let sub_rect = sub_resp.response.rect;
        if keep_open && !matches!(menu_state.phase, MenuPhase::DurationDialog { .. }) {
            let clicked_outside = ui.input(|i| {
                i.pointer.any_pressed()
                    && i.pointer.latest_pos().map_or(false, |p| {
                        !main_rect.contains(p) && !sub_rect.contains(p)
                    })
            });
            if clicked_outside || escape_pressed {
                keep_open = false;
            }
        }
    } else if matches!(menu_state.phase, MenuPhase::MainMenu) {
        // MainMenu: check click outside main menu only
        if keep_open {
            let clicked_outside = ui.input(|i| {
                i.pointer.any_pressed()
                    && i.pointer
                        .latest_pos()
                        .map_or(false, |p| !main_rect.contains(p))
            });
            if clicked_outside || escape_pressed {
                keep_open = false;
            }
        }
    }

    if let Some((kind, duration)) = chosen {
        actions.push(TimelineAction::SetTransition(
            menu_state.junction,
            kind,
            duration,
        ));
    } else if keep_open {
        view.junction_menu = Some(menu_state);
    }
}

fn show_clip_context_menu(
    ui: &mut egui::Ui,
    timeline: &Timeline,
    view: &mut TimelineView,
    actions: &mut Vec<TimelineAction>,
    playhead: &Playhead,
) {
    let menu_state = match view.clip_context_menu.take() {
        Some(s) => s,
        None => return,
    };

    let popup_id = egui::Id::new("clip_context_menu");
    #[allow(deprecated)]
    ui.memory_mut(|m| m.open_popup(popup_id));

    let escape_pressed = ui.input(|i| i.key_pressed(egui::Key::Escape));
    let mut keep_open = true;

    let menu_frame = egui::Frame::new()
        .fill(egui::Color32::from_gray(40))
        .stroke(egui::Stroke::new(1.0, egui::Color32::from_gray(80)))
        .inner_margin(egui::Margin::same(4));

    // Check if playhead is within the clip bounds
    let playhead_in_clip = timeline
        .get_clip(menu_state.clip_id)
        .map(|c| playhead.position >= c.start && playhead.position < c.end())
        .unwrap_or(false);

    let area_resp = egui::Area::new(popup_id)
        .order(egui::Order::Foreground)
        .fixed_pos(menu_state.click_pos)
        .show(ui.ctx(), |ui| {
            menu_frame.show(ui, |ui| {
                ui.set_min_width(130.0);

                let split_btn = ui.add_enabled(
                    playhead_in_clip,
                    egui::Button::new("Split at Playhead"),
                );
                if split_btn.clicked() {
                    actions.push(TimelineAction::SplitClip(menu_state.clip_id));
                    keep_open = false;
                }

                // Fade In/Out for text clips (junctions are disabled on text track)
                let is_text = timeline
                    .get_clip(menu_state.clip_id)
                    .map(|c| c.track_kind == TrackKind::Text)
                    .unwrap_or(false);
                if is_text {
                    ui.separator();

                    let has_fade_in = timeline
                        .find_transition(&TransitionPosition::FadeIn(menu_state.clip_id))
                        .is_some();
                    let fade_in_label = if has_fade_in { "Remove Fade In" } else { "Fade In" };
                    if ui.button(fade_in_label).clicked() {
                        let kind = if has_fade_in { None } else { Some(TransitionKind::Fade) };
                        actions.push(TimelineAction::SetTransition(
                            JunctionKind::LeadingEdge(menu_state.clip_id),
                            kind,
                            Duration::from_millis(500),
                        ));
                        keep_open = false;
                    }

                    let has_fade_out = timeline
                        .find_transition(&TransitionPosition::FadeOut(menu_state.clip_id))
                        .is_some();
                    let fade_out_label = if has_fade_out { "Remove Fade Out" } else { "Fade Out" };
                    if ui.button(fade_out_label).clicked() {
                        let kind = if has_fade_out { None } else { Some(TransitionKind::Fade) };
                        actions.push(TimelineAction::SetTransition(
                            JunctionKind::TrailingEdge(menu_state.clip_id),
                            kind,
                            Duration::from_millis(500),
                        ));
                        keep_open = false;
                    }
                }
            });
        });

    let menu_rect = area_resp.response.rect;
    if keep_open {
        let clicked_outside = ui.input(|i| {
            i.pointer.any_pressed()
                && i.pointer
                    .latest_pos()
                    .map_or(false, |p| !menu_rect.contains(p))
        });
        if clicked_outside || escape_pressed {
            keep_open = false;
        }
    }

    if keep_open {
        view.clip_context_menu = Some(menu_state);
    }
}

fn junction_to_transition_pos(junction: &JunctionKind) -> Option<TransitionPosition> {
    Some(match junction {
        JunctionKind::Between(a, b) => TransitionPosition::Between(*a, *b),
        JunctionKind::LeadingEdge(c) => TransitionPosition::FadeIn(*c),
        JunctionKind::TrailingEdge(c) => TransitionPosition::FadeOut(*c),
    })
}

fn junction_is_video(junction: &JunctionKind, timeline: &Timeline) -> bool {
    let clip_id = match junction {
        JunctionKind::Between(id, _) | JunctionKind::LeadingEdge(id) | JunctionKind::TrailingEdge(id) => *id,
    };
    timeline.get_clip(clip_id).map(|c| c.track_kind == TrackKind::Video).unwrap_or(false)
}

fn determine_hovered_track(pointer_y: f32, video_track_y: f32) -> Option<TrackKind> {
    let tracks = [TrackKind::Video, TrackKind::Audio, TrackKind::Text];
    for (i, kind) in tracks.iter().enumerate() {
        let y = video_track_y + (i as f32) * (TRACK_HEIGHT + TRACK_GAP);
        if pointer_y >= y && pointer_y <= y + TRACK_HEIGHT {
            return Some(*kind);
        }
    }
    None
}

fn track_y_for_kind(kind: TrackKind, video_track_y: f32) -> f32 {
    let idx = match kind {
        TrackKind::Video => 0,
        TrackKind::Audio => 1,
        TrackKind::Text => 2,
    };
    video_track_y + (idx as f32) * (TRACK_HEIGHT + TRACK_GAP)
}

fn junction_track_y(junction: &JunctionKind, timeline: &Timeline, video_track_y: f32) -> f32 {
    let clip_id = match junction {
        JunctionKind::Between(id, _) | JunctionKind::LeadingEdge(id) | JunctionKind::TrailingEdge(id) => *id,
    };
    if let Some(clip) = timeline.get_clip(clip_id) {
        track_y_for_kind(clip.track_kind, video_track_y)
    } else {
        video_track_y
    }
}

fn transition_track_y(
    transition: &crate::model::Transition,
    timeline: &Timeline,
    video_track_y: f32,
) -> f32 {
    let clip_id = match transition.position {
        TransitionPosition::Between(id, _) | TransitionPosition::FadeIn(id) | TransitionPosition::FadeOut(id) => id,
    };
    if let Some(clip) = timeline.get_clip(clip_id) {
        track_y_for_kind(clip.track_kind, video_track_y)
    } else {
        video_track_y
    }
}

fn junction_x(
    junction: &JunctionKind,
    timeline: &Timeline,
    view: &TimelineView,
    content_left: f32,
) -> Option<f32> {
    match junction {
        JunctionKind::Between(left_id, _) => {
            timeline.get_clip(*left_id).map(|c| time_to_x(c.end(), view, content_left))
        }
        JunctionKind::LeadingEdge(clip_id) => {
            timeline.get_clip(*clip_id).map(|c| time_to_x(c.start, view, content_left))
        }
        JunctionKind::TrailingEdge(clip_id) => {
            timeline.get_clip(*clip_id).map(|c| time_to_x(c.end(), view, content_left))
        }
    }
}

fn transition_position_x(
    transition: &crate::model::Transition,
    timeline: &Timeline,
    view: &TimelineView,
    content_left: f32,
) -> Option<f32> {
    match transition.position {
        TransitionPosition::Between(left_id, _) => {
            timeline.get_clip(left_id).map(|c| time_to_x(c.end(), view, content_left))
        }
        TransitionPosition::FadeIn(clip_id) => {
            timeline.get_clip(clip_id).map(|c| time_to_x(c.start, view, content_left))
        }
        TransitionPosition::FadeOut(clip_id) => {
            timeline.get_clip(clip_id).map(|c| time_to_x(c.end(), view, content_left))
        }
    }
}

fn draw_ruler(painter: &egui::Painter, rect: Rect, view: &TimelineView) {
    painter.rect_filled(rect, 0.0, Color32::from_gray(50));

    let pixels_per_sec = view.zoom as f32;
    let tick_interval_secs = if pixels_per_sec > 200.0 {
        0.5
    } else if pixels_per_sec > 50.0 {
        1.0
    } else if pixels_per_sec > 20.0 {
        5.0
    } else {
        10.0
    };

    let start_time = view.scroll_offset.max(0.0);
    let end_time = start_time + (rect.width() as f64 / view.zoom);

    let first_tick =
        (start_time / tick_interval_secs).floor() as i64 * tick_interval_secs as i64;
    let mut t = first_tick as f64;

    while t <= end_time + tick_interval_secs {
        let x = time_to_x(Duration::from_secs_f64(t.max(0.0)), view, rect.left());
        if x >= rect.left() && x <= rect.right() {
            painter.line_segment(
                [
                    egui::pos2(x, rect.bottom() - 6.0),
                    egui::pos2(x, rect.bottom()),
                ],
                Stroke::new(1.0, Color32::from_gray(120)),
            );

            let secs = t.max(0.0);
            let label = if secs < 60.0 {
                format!("{:.0}s", secs)
            } else {
                format!("{}:{:02}", (secs / 60.0) as u32, (secs % 60.0) as u32)
            };
            painter.text(
                egui::pos2(x + 2.0, rect.top() + 4.0),
                egui::Align2::LEFT_TOP,
                label,
                egui::FontId::proportional(10.0),
                Color32::from_gray(160),
            );
        }
        t += tick_interval_secs;
    }
}

fn time_to_x(time: Duration, view: &TimelineView, left: f32) -> f32 {
    left + ((time.as_secs_f64() - view.scroll_offset) * view.zoom) as f32
}

fn x_to_time(x: f32, view: &TimelineView, left: f32) -> Duration {
    let secs = ((x - left) as f64 / view.zoom) + view.scroll_offset;
    Duration::from_secs_f64(secs.max(0.0))
}
