use std::time::Duration;

use anyhow::Result;

use super::Command;
use crate::model::{TransitionKind, TransitionPosition};
use crate::state::AppState;

pub struct SetTransitionCommand {
    position: TransitionPosition,
    new_kind: Option<TransitionKind>,
    new_duration: Duration,
    old_kind: Option<TransitionKind>,
    old_duration: Duration,
}

impl SetTransitionCommand {
    pub fn new(
        position: TransitionPosition,
        kind: Option<TransitionKind>,
        duration: Duration,
    ) -> Self {
        Self {
            position,
            new_kind: kind,
            new_duration: duration,
            old_kind: None,
            old_duration: Duration::ZERO,
        }
    }
}

impl Command for SetTransitionCommand {
    fn execute(&mut self, state: &mut AppState) -> Result<()> {
        let existing = state.project.timeline.find_transition(&self.position);
        self.old_kind = existing.map(|t| t.kind);
        self.old_duration = existing.map(|t| t.duration).unwrap_or(Duration::ZERO);
        state
            .project
            .timeline
            .set_transition(self.position, self.new_kind, self.new_duration);
        Ok(())
    }

    fn undo(&mut self, state: &mut AppState) -> Result<()> {
        state
            .project
            .timeline
            .set_transition(self.position, self.old_kind, self.old_duration);
        Ok(())
    }

    fn description(&self) -> &str {
        "Set transition"
    }
}
