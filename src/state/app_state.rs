use crate::model::Project;

use super::{Playhead, Selection};

pub struct AppState {
    pub project: Project,
    pub selection: Selection,
    pub playhead: Playhead,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            project: Project::new("Untitled".to_string()),
            selection: Selection::default(),
            playhead: Playhead::default(),
        }
    }
}
