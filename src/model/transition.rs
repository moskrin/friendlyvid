use serde::{Deserialize, Serialize};
use std::time::Duration;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transition {
    pub id: Uuid,
    pub kind: TransitionKind,
    pub duration: Duration,
    pub position: TransitionPosition,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TransitionKind {
    Fade,
    Dissolve,
    WipeLeft,
    WipeRight,
    WipeDown,
    WipeUp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TransitionPosition {
    Between(Uuid, Uuid),
    FadeIn(Uuid),
    FadeOut(Uuid),
}

impl TransitionKind {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Fade => "Fade",
            Self::Dissolve => "Dissolve",
            Self::WipeLeft => "Wipe Left to Right",
            Self::WipeRight => "Wipe Right to Left",
            Self::WipeDown => "Wipe Top to Bottom",
            Self::WipeUp => "Wipe Bottom to Top",
        }
    }
}

impl Transition {
    #[allow(dead_code)]
    pub fn new(kind: TransitionKind, position: TransitionPosition) -> Self {
        Self {
            id: Uuid::new_v4(),
            kind,
            duration: Duration::from_millis(500),
            position,
        }
    }

    pub fn with_duration(
        kind: TransitionKind,
        position: TransitionPosition,
        duration: Duration,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            kind,
            duration,
            position,
        }
    }

    pub fn references_clip(&self, clip_id: Uuid) -> bool {
        match self.position {
            TransitionPosition::Between(a, b) => a == clip_id || b == clip_id,
            TransitionPosition::FadeIn(c) | TransitionPosition::FadeOut(c) => c == clip_id,
        }
    }
}
