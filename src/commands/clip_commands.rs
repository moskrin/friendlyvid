use anyhow::Result;
use std::time::Duration;
use uuid::Uuid;

use super::Command;
use crate::model::{Clip, SourceFile, TextOverlay, Transition, TrackKind};
use crate::state::AppState;

#[allow(dead_code)]
pub struct AddClipCommand {
    pub clip: Option<Clip>,
    clip_id: Uuid,
}

impl AddClipCommand {
    #[allow(dead_code)]
    pub fn new(clip: Clip) -> Self {
        let clip_id = clip.id;
        Self {
            clip: Some(clip),
            clip_id,
        }
    }
}

impl Command for AddClipCommand {
    fn execute(&mut self, state: &mut AppState) -> Result<()> {
        if let Some(clip) = self.clip.take() {
            state.project.timeline.add_clip(clip);
        }
        Ok(())
    }

    fn undo(&mut self, state: &mut AppState) -> Result<()> {
        if let Some(clip) = state.project.timeline.remove_clip(self.clip_id) {
            self.clip = Some(clip);
        }
        Ok(())
    }

    fn description(&self) -> &str {
        "Add clip"
    }
}

pub struct RemoveClipCommand {
    clip_id: Uuid,
    removed_clip: Option<Clip>,
    removed_text_overlay: Option<TextOverlay>,
    shifts: Vec<(Uuid, Duration)>,
}

impl RemoveClipCommand {
    pub fn new(clip_id: Uuid) -> Self {
        Self {
            clip_id,
            removed_clip: None,
            removed_text_overlay: None,
            shifts: Vec::new(),
        }
    }
}

impl Command for RemoveClipCommand {
    fn execute(&mut self, state: &mut AppState) -> Result<()> {
        // Check track kind before removing to decide whether to close gaps
        let is_video = state
            .project
            .timeline
            .get_clip(self.clip_id)
            .map(|c| c.track_kind == TrackKind::Video)
            .unwrap_or(false);

        // Save text overlay if this is a text clip
        self.removed_text_overlay = state.project.timeline.remove_text_overlay(self.clip_id);

        if is_video {
            // Video track: close gaps (shift subsequent clips left)
            if let Some((clip, shifts)) =
                state.project.timeline.remove_clip_and_close_gap(self.clip_id)
            {
                self.removed_clip = Some(clip);
                self.shifts = shifts;
            }
        } else {
            // Audio/text tracks: just remove, no gap closing
            if let Some(clip) = state.project.timeline.remove_clip(self.clip_id) {
                self.removed_clip = Some(clip);
            }
        }
        Ok(())
    }

    fn undo(&mut self, state: &mut AppState) -> Result<()> {
        // Restore shifts first (move clips back to original positions)
        for (id, old_start) in &self.shifts {
            if let Some(c) = state.project.timeline.get_clip_mut(*id) {
                c.start = *old_start;
            }
        }
        // Re-add the removed clip
        if let Some(clip) = self.removed_clip.take() {
            let clip_id = clip.id;
            state.project.timeline.add_clip(clip);
            // Restore text overlay if there was one
            if let Some(overlay) = self.removed_text_overlay.take() {
                state.project.timeline.add_text_overlay(clip_id, overlay);
            }
        }
        self.shifts.clear();
        Ok(())
    }

    fn description(&self) -> &str {
        "Remove clip"
    }
}

pub struct TrimClipCommand {
    clip_id: Uuid,
    new_in_point: Duration,
    new_start: Duration,
    new_duration: Duration,
    old_in_point: Option<Duration>,
    old_start: Option<Duration>,
    old_duration: Option<Duration>,
}

impl TrimClipCommand {
    pub fn new(
        clip_id: Uuid,
        new_in_point: Duration,
        new_start: Duration,
        new_duration: Duration,
    ) -> Self {
        Self {
            clip_id,
            new_in_point,
            new_start,
            new_duration,
            old_in_point: None,
            old_start: None,
            old_duration: None,
        }
    }
}

impl Command for TrimClipCommand {
    fn execute(&mut self, state: &mut AppState) -> Result<()> {
        if let Some(clip) = state.project.timeline.get_clip_mut(self.clip_id) {
            self.old_in_point = Some(clip.in_point);
            self.old_start = Some(clip.start);
            self.old_duration = Some(clip.duration);
            clip.in_point = self.new_in_point;
            clip.start = self.new_start;
            clip.duration = self.new_duration;
        }
        Ok(())
    }

    fn undo(&mut self, state: &mut AppState) -> Result<()> {
        if let Some(clip) = state.project.timeline.get_clip_mut(self.clip_id) {
            if let (Some(ip), Some(s), Some(d)) =
                (self.old_in_point, self.old_start, self.old_duration)
            {
                clip.in_point = ip;
                clip.start = s;
                clip.duration = d;
            }
        }
        Ok(())
    }

    fn description(&self) -> &str {
        "Trim clip"
    }
}

pub struct SplitClipCommand {
    clip_id: Uuid,
    split_at: Duration,
    new_clip_id: Uuid,
    original_duration: Option<Duration>,
}

impl SplitClipCommand {
    pub fn new(clip_id: Uuid, split_at: Duration) -> Self {
        Self {
            clip_id,
            split_at,
            new_clip_id: Uuid::new_v4(),
            original_duration: None,
        }
    }
}

impl Command for SplitClipCommand {
    fn execute(&mut self, state: &mut AppState) -> Result<()> {
        let new_clip = {
            let clip = state
                .project
                .timeline
                .get_clip(self.clip_id)
                .ok_or_else(|| anyhow::anyhow!("clip not found"))?;

            let offset = self.split_at.saturating_sub(clip.start);
            if offset.is_zero() || offset >= clip.duration {
                anyhow::bail!("split point outside clip bounds");
            }

            self.original_duration = Some(clip.duration);

            Clip {
                id: self.new_clip_id,
                source_id: clip.source_id,
                track_kind: clip.track_kind,
                start: self.split_at,
                duration: clip.duration - offset,
                in_point: clip.in_point + offset,
                transform: clip.transform.clone(),
            }
        };

        if let Some(clip) = state.project.timeline.get_clip_mut(self.clip_id) {
            let offset = self.split_at.saturating_sub(clip.start);
            clip.duration = offset;
        }
        state.project.timeline.add_clip(new_clip);
        Ok(())
    }

    fn undo(&mut self, state: &mut AppState) -> Result<()> {
        state.project.timeline.remove_clip(self.new_clip_id);
        if let Some(clip) = state.project.timeline.get_clip_mut(self.clip_id) {
            if let Some(dur) = self.original_duration {
                clip.duration = dur;
            }
        }
        Ok(())
    }

    fn description(&self) -> &str {
        "Split clip"
    }
}

pub struct MoveClipCommand {
    clip_id: Uuid,
    new_start: Duration,
    old_start: Option<Duration>,
}

impl MoveClipCommand {
    pub fn new(clip_id: Uuid, new_start: Duration) -> Self {
        Self {
            clip_id,
            new_start,
            old_start: None,
        }
    }
}

impl Command for MoveClipCommand {
    fn execute(&mut self, state: &mut AppState) -> Result<()> {
        if let Some(clip) = state.project.timeline.get_clip_mut(self.clip_id) {
            self.old_start = Some(clip.start);
            clip.start = self.new_start;
        }
        Ok(())
    }

    fn undo(&mut self, state: &mut AppState) -> Result<()> {
        if let Some(clip) = state.project.timeline.get_clip_mut(self.clip_id) {
            if let Some(s) = self.old_start {
                clip.start = s;
            }
        }
        Ok(())
    }

    fn description(&self) -> &str {
        "Move clip"
    }
}

pub struct RemoveSourceCommand {
    source_id: Uuid,
    removed_source: Option<SourceFile>,
    removed_clips: Vec<Clip>,
    removed_transitions: Vec<Transition>,
}

impl RemoveSourceCommand {
    pub fn new(source_id: Uuid) -> Self {
        Self {
            source_id,
            removed_source: None,
            removed_clips: Vec::new(),
            removed_transitions: Vec::new(),
        }
    }
}

impl Command for RemoveSourceCommand {
    fn execute(&mut self, state: &mut AppState) -> Result<()> {
        // Find and remove all clips using this source
        let clip_ids = state.project.timeline.clips_using_source(self.source_id);
        for clip_id in &clip_ids {
            if let Some(clip) = state.project.timeline.remove_clip(*clip_id) {
                self.removed_clips.push(clip);
            }
        }

        // Remove transitions referencing any removed clip
        let removed_clip_ids: Vec<Uuid> = self.removed_clips.iter().map(|c| c.id).collect();
        let mut removed_transitions = Vec::new();
        state.project.timeline.transitions.retain(|t| {
            if removed_clip_ids.iter().any(|id| t.references_clip(*id)) {
                removed_transitions.push(t.clone());
                false
            } else {
                true
            }
        });
        self.removed_transitions = removed_transitions;

        // Remove source file
        if let Some(idx) = state.project.source_files.iter().position(|s| s.id == self.source_id) {
            self.removed_source = Some(state.project.source_files.remove(idx));
        }

        // Clear selection if it pointed to a removed clip
        if let Some(sel) = state.selection.selected_clip {
            if removed_clip_ids.contains(&sel) {
                state.selection.clear();
            }
        }

        Ok(())
    }

    fn undo(&mut self, state: &mut AppState) -> Result<()> {
        // Restore source file
        if let Some(source) = self.removed_source.take() {
            state.project.source_files.push(source);
        }

        // Restore clips
        for clip in self.removed_clips.drain(..) {
            state.project.timeline.add_clip(clip);
        }

        // Restore transitions
        for transition in self.removed_transitions.drain(..) {
            state.project.timeline.transitions.push(transition);
        }

        Ok(())
    }

    fn description(&self) -> &str {
        "Remove source"
    }
}

pub struct AddTextClipCommand {
    clip: Option<Clip>,
    overlay: Option<TextOverlay>,
    clip_id: Uuid,
}

impl AddTextClipCommand {
    pub fn new(start: Duration, end_of_content: Duration) -> Self {
        let clip_id = Uuid::new_v4();
        let duration = end_of_content.saturating_sub(start);
        // Minimum 1 second
        let duration = if duration < Duration::from_secs(1) {
            Duration::from_secs(3)
        } else {
            duration
        };

        let clip = Clip {
            id: clip_id,
            source_id: Uuid::nil(), // text clips have no source file
            track_kind: TrackKind::Text,
            start,
            duration,
            in_point: Duration::ZERO,
            transform: crate::model::Transform::default(),
        };
        let overlay = TextOverlay::new("Text".to_string());

        Self {
            clip: Some(clip),
            overlay: Some(overlay),
            clip_id,
        }
    }
}

impl Command for AddTextClipCommand {
    fn execute(&mut self, state: &mut AppState) -> Result<()> {
        if let Some(clip) = self.clip.take() {
            state.project.timeline.add_clip(clip);
        }
        if let Some(overlay) = self.overlay.take() {
            state.project.timeline.add_text_overlay(self.clip_id, overlay);
        }
        Ok(())
    }

    fn undo(&mut self, state: &mut AppState) -> Result<()> {
        self.overlay = state.project.timeline.remove_text_overlay(self.clip_id);
        if let Some(clip) = state.project.timeline.remove_clip(self.clip_id) {
            self.clip = Some(clip);
        }
        Ok(())
    }

    fn description(&self) -> &str {
        "Add text clip"
    }
}
