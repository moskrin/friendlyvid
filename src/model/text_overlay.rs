use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TextOverlay {
    pub id: Uuid,
    pub text: String,
    pub font_family: String,
    pub font_size: f32,
    pub color: [u8; 4],
    pub position: (f32, f32), // normalized (0..1, 0..1) in video rect
    #[serde(default)]
    pub bold: bool,
    #[serde(default)]
    pub italic: bool,
}

impl TextOverlay {
    pub fn new(text: String) -> Self {
        Self {
            id: Uuid::new_v4(),
            text,
            font_family: "Sans".to_string(),
            font_size: 48.0,
            color: [255, 255, 255, 255],
            position: (0.5, 0.5),
            bold: false,
            italic: false,
        }
    }
}
