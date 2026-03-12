use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;
use uuid::Uuid;

use super::{Clip, TextOverlay, Track, TrackKind, Transition, TransitionPosition};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Timeline {
    pub video_track: Track,
    pub audio_track: Track,
    pub text_track: Track,
    pub transitions: Vec<Transition>,
    pub text_overlays: HashMap<Uuid, TextOverlay>,
    pub clips: HashMap<Uuid, Clip>,
}

impl Timeline {
    pub fn new() -> Self {
        Self {
            video_track: Track::new(TrackKind::Video),
            audio_track: Track::new(TrackKind::Audio),
            text_track: Track::new(TrackKind::Text),
            transitions: Vec::new(),
            text_overlays: HashMap::new(),
            clips: HashMap::new(),
        }
    }

    pub fn add_clip(&mut self, clip: Clip) {
        let id = clip.id;
        let kind = clip.track_kind;
        self.clips.insert(id, clip);
        let track = match kind {
            TrackKind::Video => &mut self.video_track,
            TrackKind::Audio => &mut self.audio_track,
            TrackKind::Text => &mut self.text_track,
        };
        track.clips.push(id);
        self.sort_track(kind);
    }

    pub fn remove_clip(&mut self, id: Uuid) -> Option<Clip> {
        if let Some(clip) = self.clips.remove(&id) {
            let track = match clip.track_kind {
                TrackKind::Video => &mut self.video_track,
                TrackKind::Audio => &mut self.audio_track,
                TrackKind::Text => &mut self.text_track,
            };
            track.clips.retain(|c| *c != id);
            self.transitions.retain(|t| !t.references_clip(id));
            // Text overlays are keyed by clip_id; removal is handled by commands
            // that explicitly save/restore them for undo.
            Some(clip)
        } else {
            None
        }
    }

    /// Remove a clip and shift all subsequent clips on the same track left to close the gap.
    /// Returns the removed clip and a vec of (clip_id, old_start) for undo.
    pub fn remove_clip_and_close_gap(&mut self, id: Uuid) -> Option<(Clip, Vec<(Uuid, Duration)>)> {
        let clip = self.clips.get(&id)?;
        let gap = clip.duration;
        let removed_start = clip.start;
        let kind = clip.track_kind;

        let removed = self.remove_clip(id)?;

        // Shift subsequent clips left
        let track = match kind {
            TrackKind::Video => &self.video_track,
            TrackKind::Audio => &self.audio_track,
            TrackKind::Text => &self.text_track,
        };
        let mut shifts = Vec::new();
        for clip_id in &track.clips {
            if let Some(c) = self.clips.get_mut(clip_id) {
                if c.start > removed_start {
                    shifts.push((*clip_id, c.start));
                    c.start = c.start.saturating_sub(gap);
                }
            }
        }

        Some((removed, shifts))
    }

    pub fn get_clip(&self, id: Uuid) -> Option<&Clip> {
        self.clips.get(&id)
    }

    pub fn get_clip_mut(&mut self, id: Uuid) -> Option<&mut Clip> {
        self.clips.get_mut(&id)
    }

    fn sort_track(&mut self, kind: TrackKind) {
        let track = match kind {
            TrackKind::Video => &mut self.video_track,
            TrackKind::Audio => &mut self.audio_track,
            TrackKind::Text => &mut self.text_track,
        };
        let clips = &self.clips;
        track.clips.sort_by(|a, b| {
            let ca = clips.get(a).map(|c| c.start).unwrap_or_default();
            let cb = clips.get(b).map(|c| c.start).unwrap_or_default();
            ca.cmp(&cb)
        });
    }

    #[allow(dead_code)]
    pub fn duration(&self) -> Duration {
        self.clips
            .values()
            .map(|c| c.end())
            .max()
            .unwrap_or_default()
    }

    pub fn track_duration(&self, kind: TrackKind) -> Duration {
        let track = match kind {
            TrackKind::Video => &self.video_track,
            TrackKind::Audio => &self.audio_track,
            TrackKind::Text => &self.text_track,
        };
        track
            .clips
            .iter()
            .filter_map(|id| self.clips.get(id))
            .map(|c| c.end())
            .max()
            .unwrap_or_default()
    }

    pub fn clips_using_source(&self, source_id: Uuid) -> Vec<Uuid> {
        self.clips
            .values()
            .filter(|c| c.source_id == source_id)
            .map(|c| c.id)
            .collect()
    }

    pub fn find_transition(&self, position: &TransitionPosition) -> Option<&Transition> {
        self.transitions.iter().find(|t| t.position == *position)
    }

    pub fn add_text_overlay(&mut self, clip_id: Uuid, overlay: TextOverlay) {
        self.text_overlays.insert(clip_id, overlay);
    }

    pub fn remove_text_overlay(&mut self, clip_id: Uuid) -> Option<TextOverlay> {
        self.text_overlays.remove(&clip_id)
    }

    pub fn get_text_overlay(&self, clip_id: Uuid) -> Option<&TextOverlay> {
        self.text_overlays.get(&clip_id)
    }

    pub fn get_text_overlay_mut(&mut self, clip_id: Uuid) -> Option<&mut TextOverlay> {
        self.text_overlays.get_mut(&clip_id)
    }

    pub fn set_transition(
        &mut self,
        position: TransitionPosition,
        kind: Option<super::TransitionKind>,
        duration: Duration,
    ) {
        // Remove existing transition at this position
        self.transitions.retain(|t| t.position != position);
        // Add new one if kind is Some
        if let Some(kind) = kind {
            self.transitions
                .push(Transition::with_duration(kind, position, duration));
        }
    }

}
