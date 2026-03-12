use anyhow::Result;
use uuid::Uuid;

use super::Command;
use crate::model::Transform;
use crate::state::AppState;

pub struct SetCropCommand {
    clip_id: Uuid,
    old_transform: Transform,
    new_transform: Transform,
}

impl SetCropCommand {
    pub fn new(clip_id: Uuid, old_transform: Transform, new_transform: Transform) -> Self {
        Self {
            clip_id,
            old_transform,
            new_transform,
        }
    }
}

impl Command for SetCropCommand {
    fn execute(&mut self, state: &mut AppState) -> Result<()> {
        if let Some(clip) = state.project.timeline.get_clip_mut(self.clip_id) {
            clip.transform = self.new_transform.clone();
        }
        Ok(())
    }

    fn undo(&mut self, state: &mut AppState) -> Result<()> {
        if let Some(clip) = state.project.timeline.get_clip_mut(self.clip_id) {
            clip.transform = self.old_transform.clone();
        }
        Ok(())
    }

    fn description(&self) -> &str {
        "Set crop"
    }
}
