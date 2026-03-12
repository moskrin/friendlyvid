use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::Duration;
use uuid::Uuid;

use super::Timeline;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub name: String,
    pub project_path: Option<PathBuf>,
    pub source_files: Vec<SourceFile>,
    pub timeline: Timeline,
    pub output_settings: OutputSettings,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceFile {
    pub id: Uuid,
    pub path: PathBuf,
    pub duration: Duration,
    pub width: u32,
    pub height: u32,
    #[serde(default = "default_true")]
    pub has_video: bool,
    #[serde(default = "default_true")]
    pub has_audio: bool,
}

fn default_true() -> bool {
    true
}

impl SourceFile {
    pub fn new(
        path: PathBuf,
        duration: Duration,
        width: u32,
        height: u32,
        has_video: bool,
        has_audio: bool,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            path,
            duration,
            width,
            height,
            has_video,
            has_audio,
        }
    }

    pub fn is_audio_only(&self) -> bool {
        !self.has_video && self.has_audio
    }

    pub fn filename(&self) -> String {
        self.path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "unknown".to_string())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputSettings {
    pub width: u32,
    pub height: u32,
    pub framerate_num: i32,
    pub framerate_den: i32,
}

impl Default for OutputSettings {
    fn default() -> Self {
        Self {
            width: 1920,
            height: 1080,
            framerate_num: 30,
            framerate_den: 1,
        }
    }
}

impl Project {
    pub fn new(name: String) -> Self {
        Self {
            name,
            project_path: None,
            source_files: Vec::new(),
            timeline: Timeline::new(),
            output_settings: OutputSettings::default(),
        }
    }

    pub fn get_source(&self, id: Uuid) -> Option<&SourceFile> {
        self.source_files.iter().find(|s| s.id == id)
    }
}
