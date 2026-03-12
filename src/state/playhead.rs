use std::time::Duration;

#[derive(Debug, Clone)]
pub struct Playhead {
    pub position: Duration,
    pub playing: bool,
}

impl Default for Playhead {
    fn default() -> Self {
        Self {
            position: Duration::ZERO,
            playing: false,
        }
    }
}
