use serde::{Deserialize, Serialize};
use std::time::Duration;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Clip {
    pub id: Uuid,
    pub source_id: Uuid,
    pub track_kind: TrackKind,
    pub start: Duration,
    pub duration: Duration,
    pub in_point: Duration,
    pub transform: Transform,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TrackKind {
    Video,
    Audio,
    Text,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transform {
    pub zoom: f32,   // >= 1.0. 1.0 = no crop, 2.0 = 2x zoom (50% of source visible)
    pub pan_x: f32,  // 0.0..=1.0, 0.5 = centered. 0.0 = left edge, 1.0 = right edge
    pub pan_y: f32,  // 0.0..=1.0, 0.5 = centered. 0.0 = top edge, 1.0 = bottom edge
}

impl Default for Transform {
    fn default() -> Self {
        Self {
            zoom: 1.0,
            pan_x: 0.5,
            pan_y: 0.5,
        }
    }
}

impl Transform {
    pub fn has_crop(&self) -> bool {
        self.zoom > 1.001
    }

    /// Returns UV coordinates (left, top, right, bottom) for rendering the cropped view.
    pub fn crop_uv(&self) -> (f32, f32, f32, f32) {
        let viewport = 1.0 / self.zoom;
        let pan_range = 1.0 - viewport;
        let left = self.pan_x * pan_range;
        let top = self.pan_y * pan_range;
        (left, top, left + viewport, top + viewport)
    }

    /// Returns (top, bottom, left, right) pixel crop values for GES videocrop.
    pub fn crop_pixels(&self, source_w: u32, source_h: u32) -> (i32, i32, i32, i32) {
        let total_x = source_w as f32 * (1.0 - 1.0 / self.zoom);
        let total_y = source_h as f32 * (1.0 - 1.0 / self.zoom);
        let left = (self.pan_x * total_x) as i32;
        let right = ((1.0 - self.pan_x) * total_x) as i32;
        let top = (self.pan_y * total_y) as i32;
        let bottom = ((1.0 - self.pan_y) * total_y) as i32;
        (top, bottom, left, right)
    }
}

impl Clip {
    pub fn new(source_id: Uuid, track_kind: TrackKind, start: Duration, duration: Duration) -> Self {
        Self {
            id: Uuid::new_v4(),
            source_id,
            track_kind,
            start,
            duration,
            in_point: Duration::ZERO,
            transform: Transform::default(),
        }
    }

    pub fn end(&self) -> Duration {
        self.start + self.duration
    }

    pub fn display_name(&self) -> String {
        format!("Clip {}", &self.id.to_string()[..8])
    }
}
