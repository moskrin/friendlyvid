use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::clip::TrackKind;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Track {
    pub id: Uuid,
    pub kind: TrackKind,
    pub clips: Vec<Uuid>,
}

impl Track {
    pub fn new(kind: TrackKind) -> Self {
        Self {
            id: Uuid::new_v4(),
            kind,
            clips: Vec::new(),
        }
    }
}
