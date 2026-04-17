use anyhow::Result;
use crossbeam_channel::{bounded, Receiver, Sender, TrySendError};
use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer_app as gst_app;
use gstreamer_controller as gst_ctrl;
use gstreamer_controller::prelude::*;
use gstreamer_editing_services as ges;
use gstreamer_editing_services::prelude::*;
use gstreamer_pbutils as gst_pbutils;
use gstreamer_video as gst_video;
use pango::prelude::*;
use std::path::Path;
use std::time::Duration;
use uuid::Uuid;

use super::frame_grabber::VideoFrame;
use crate::model::{Project, TransitionKind, TransitionPosition};

/// Maps between GES time (with transition overlaps) and model time (clips end-to-end).
/// Each breakpoint represents a transition overlap region in GES.
struct TransitionBreakpoint {
    ges_overlap_start: Duration, // GES time when overlap begins (clip B starts)
    ges_overlap_end: Duration,   // GES time when overlap ends (clip A ends)
    offset_before: Duration,     // cumulative offset before this transition
    offset_after: Duration,      // cumulative offset after this transition
}

pub struct MediaInfo {
    pub duration: Duration,
    pub width: u32,
    pub height: u32,
    pub has_video: bool,
    pub has_audio: bool,
}

#[derive(Debug, Clone)]
pub enum ExportState {
    Idle,
    Exporting,
    Done,
    Error(String),
}

pub struct MediaEngine {
    ges_pipeline: Option<ges::Pipeline>,
    ges_timeline: Option<ges::Timeline>,
    ges_layer: Option<ges::Layer>,
    ges_audio_layer: Option<ges::Layer>,
    export_text_clips: Vec<ges::TextOverlayClip>,
    frame_rx: Receiver<VideoFrame>,
    frame_tx: Sender<VideoFrame>,
    transition_breakpoints: Vec<TransitionBreakpoint>,
    pub export_state: ExportState,
    export_path: Option<std::path::PathBuf>,
    // While a clip is being interactively cropped, its videocrop effect is
    // skipped so the preview sees the uncropped source. The UV crop in the
    // preview panel then renders the live framing on top of the raw frame.
    crop_bypass_clip: Option<Uuid>,
}

fn path_to_uri(path: &Path) -> Result<String> {
    let abs = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };
    Ok(format!("file://{}", abs.display()))
}

fn dur_to_clocktime(d: Duration) -> gst::ClockTime {
    gst::ClockTime::from_nseconds(d.as_nanos() as u64)
}

/// Extract a VideoFrame from a GStreamer Sample and send it on the channel.
/// Used by both the new_sample and new_preroll appsink callbacks.
fn send_sample_frame(
    tx: &Sender<VideoFrame>,
    sample: &gst::Sample,
) -> Result<gst::FlowSuccess, gst::FlowError> {
    let buffer = sample.buffer().ok_or(gst::FlowError::Error)?;
    let caps = sample.caps().ok_or(gst::FlowError::Error)?;
    let video_info =
        gst_video::VideoInfo::from_caps(caps).map_err(|_| gst::FlowError::Error)?;

    let map = buffer.map_readable().map_err(|_| gst::FlowError::Error)?;

    let frame = VideoFrame {
        data: map.as_slice().to_vec(),
        width: video_info.width(),
        height: video_info.height(),
        pts_ns: buffer.pts().map(|t| t.nseconds()).unwrap_or(0),
    };

    match tx.try_send(frame) {
        Ok(()) => {}
        Err(TrySendError::Full(_)) => {}
        Err(TrySendError::Disconnected(_)) => {
            return Err(gst::FlowError::Error);
        }
    }

    Ok(gst::FlowSuccess::Ok)
}

impl MediaEngine {
    pub fn new() -> Self {
        let (tx, rx) = bounded(3);
        Self {
            ges_pipeline: None,
            ges_timeline: None,
            ges_layer: None,
            ges_audio_layer: None,
            export_text_clips: Vec::new(),
            frame_rx: rx,
            frame_tx: tx,
            transition_breakpoints: Vec::new(),
            export_state: ExportState::Idle,
            export_path: None,
            crop_bypass_clip: None,
        }
    }

    /// When set, `sync_from_model` will skip the videocrop effect for this
    /// clip so the pipeline emits an uncropped frame. Used during interactive
    /// crop mode.
    pub fn set_crop_bypass(&mut self, clip_id: Option<Uuid>) {
        self.crop_bypass_clip = clip_id;
    }

    fn ensure_pipeline(&mut self) -> Result<()> {
        if self.ges_pipeline.is_some() {
            return Ok(());
        }

        let timeline = ges::Timeline::new_audio_video();
        let layer = timeline.append_layer();
        let audio_layer = timeline.append_layer();
        audio_layer.set_auto_transition(true);

        let pipeline = ges::Pipeline::new();
        pipeline.set_timeline(&timeline)?;

        // Appsink for RGBA frame extraction
        let caps = gst::Caps::builder("video/x-raw")
            .field("format", "RGBA")
            .build();

        let appsink = gst_app::AppSink::builder()
            .caps(&caps)
            .max_buffers(2)
            .drop(true)
            .build();

        let tx_sample = self.frame_tx.clone();
        let tx_preroll = self.frame_tx.clone();
        appsink.set_callbacks(
            gst_app::AppSinkCallbacks::builder()
                .new_sample(move |sink| {
                    let sample = sink.pull_sample().map_err(|_| gst::FlowError::Eos)?;
                    send_sample_frame(&tx_sample, &sample)
                })
                .new_preroll(move |sink| {
                    // Preroll fires when paused (e.g. after seek). Without this,
                    // seeking while paused never displays a frame.
                    let sample = sink.pull_preroll().map_err(|_| gst::FlowError::Eos)?;
                    send_sample_frame(&tx_preroll, &sample)
                })
                .build(),
        );

        pipeline.preview_set_video_sink(Some(&appsink));
        pipeline.set_mode(ges::PipelineFlags::FULL_PREVIEW)?;

        self.ges_timeline = Some(timeline);
        self.ges_layer = Some(layer);
        self.ges_audio_layer = Some(audio_layer);
        self.ges_pipeline = Some(pipeline);

        Ok(())
    }

    /// Discover file metadata without loading into pipeline.
    pub fn load_file(&mut self, path: &Path) -> Result<MediaInfo> {
        self.ensure_pipeline()?;
        log::info!("load_file: discovering {}", path.display());

        let uri = path_to_uri(path)?;
        let discoverer = gst_pbutils::Discoverer::new(gst::ClockTime::from_seconds(10))?;
        let info = discoverer.discover_uri(&uri)?;
        log::info!("load_file: discovery complete");

        let duration = info
            .duration()
            .map(|d| Duration::from_nanos(d.nseconds()))
            .unwrap_or_default();

        let mut width = 0u32;
        let mut height = 0u32;
        let has_video = !info.video_streams().is_empty();
        let has_audio = !info.audio_streams().is_empty();

        if has_video {
            for stream in info.video_streams() {
                if let Some(caps) = stream.caps() {
                    if let Some(s) = caps.structure(0) {
                        width = s.get::<i32>("width").unwrap_or(1920) as u32;
                        height = s.get::<i32>("height").unwrap_or(1080) as u32;
                        break;
                    }
                }
            }
        }

        log::info!(
            "load_file: {}x{}, {:?}, video={}, audio={}",
            width, height, duration, has_video, has_audio
        );

        Ok(MediaInfo {
            duration,
            width,
            height,
            has_video,
            has_audio,
        })
    }

    /// Rebuild the GES timeline from our model. Call after every edit.
    /// Model clip positions are NOT modified. GES clips are shifted to create
    /// overlaps where transitions exist, and breakpoints are stored for
    /// GES<->model time conversion.
    pub fn sync_from_model(&mut self, project: &Project) -> Result<()> {
        self.ensure_pipeline()?;
        log::debug!("sync_from_model: starting");

        let pipeline = self.ges_pipeline.as_ref().unwrap();
        let layer = self.ges_layer.as_ref().unwrap();
        let ges_timeline = self.ges_timeline.as_ref().unwrap();

        // Save playback state and position
        let (_, current_state, _) = pipeline.state(gst::ClockTime::ZERO);
        let was_playing = current_state == gst::State::Playing;
        let saved_pos = pipeline.query_position::<gst::ClockTime>();

        // Disable auto-transitions - we manually create TransitionClips with correct vtype
        layer.set_auto_transition(false);

        log::debug!("sync_from_model: clearing old clips");
        for clip in layer.clips() {
            let _ = layer.remove_clip(&clip);
        }

        // Build GES timeline with adjusted positions for transitions.
        // Model positions stay end-to-end; GES positions overlap where transitions exist.
        // We collect pending transitions to add after all UriClips are in place.
        let mut accumulated_offset = Duration::ZERO;
        let mut breakpoints: Vec<TransitionBreakpoint> = Vec::new();
        let mut pending_transitions: Vec<(Duration, Duration, TransitionKind)> = Vec::new();
        let mut prev_ges_end: Option<Duration> = None;
        let video_clips = &project.timeline.video_track.clips;

        log::debug!(
            "sync_from_model: adding {} clips from model",
            video_clips.len()
        );

        for (i, clip_id) in video_clips.iter().enumerate() {
            if let Some(clip) = project.timeline.get_clip(*clip_id) {
                // Check for transition with previous clip
                if i > 0 {
                    let prev_id = video_clips[i - 1];
                    let trans_pos = TransitionPosition::Between(prev_id, *clip_id);
                    if let Some(transition) = project.timeline.find_transition(&trans_pos) {
                        let offset_before = accumulated_offset;
                        accumulated_offset += transition.duration;

                        // Record the overlap breakpoint for time mapping
                        let ges_start = clip.start.saturating_sub(accumulated_offset);
                        if let Some(prev_end) = prev_ges_end {
                            breakpoints.push(TransitionBreakpoint {
                                ges_overlap_start: ges_start,
                                ges_overlap_end: prev_end,
                                offset_before,
                                offset_after: accumulated_offset,
                            });
                            // Queue a manual TransitionClip at the overlap
                            pending_transitions.push((
                                ges_start,
                                prev_end - ges_start,
                                transition.kind,
                            ));
                        }
                    }
                }

                let ges_start = clip.start.saturating_sub(accumulated_offset);

                if let Some(source) = project.get_source(clip.source_id) {
                    let uri = path_to_uri(&source.path)?;
                    log::debug!(
                        "sync_from_model: clip {} ges_start={:?} (model_start={:?}, offset={:?})",
                        source.filename(),
                        ges_start,
                        clip.start,
                        accumulated_offset
                    );
                    match ges::UriClip::new(&uri) {
                        Ok(ges_clip) => {
                            ges_clip.set_start(dur_to_clocktime(ges_start));
                            ges_clip.set_duration(dur_to_clocktime(clip.duration));
                            ges_clip.set_inpoint(dur_to_clocktime(clip.in_point));

                            if let Err(e) = layer.add_clip(&ges_clip) {
                                log::error!(
                                    "Failed to add clip {} to GES layer: {}",
                                    clip.id,
                                    e
                                );
                            }

                            // Apply videocrop effect if clip has zoom/crop,
                            // unless this clip is currently being interactively cropped.
                            // The videoscale+capsfilter after the crop rescales the cropped
                            // region back to the source's native dimensions so the GES
                            // compositor sees a constant frame size. Without this, the
                            // cropped frame is pasted at its smaller post-crop size onto
                            // the compositor canvas, appearing in the top-left with black
                            // around it.
                            let skip_crop = self.crop_bypass_clip == Some(clip.id);
                            if clip.transform.has_crop() && !skip_crop {
                                let (top, bottom, left, right) =
                                    clip.transform.crop_pixels(source.width, source.height);
                                let desc = format!(
                                    "videocrop top={} bottom={} left={} right={} ! videoscale ! capsfilter caps=video/x-raw,width={},height={}",
                                    top, bottom, left, right, source.width, source.height
                                );
                                match ges::Effect::new(&desc) {
                                    Ok(effect) => {
                                        if let Err(e) = ges_clip.add_top_effect(&effect, 0) {
                                            log::error!("Failed to add crop effect: {}", e);
                                        }
                                    }
                                    Err(e) => {
                                        log::error!("Failed to create crop effect: {}", e);
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            log::error!(
                                "Failed to create GES UriClip for {}: {}",
                                source.path.display(),
                                e
                            );
                        }
                    }
                }

                prev_ges_end = Some(ges_start + clip.duration);
            }
        }

        // Add manually created TransitionClips at each overlap
        for (start, duration, kind) in &pending_transitions {
            add_manual_transition(layer, *start, *duration, *kind);
        }

        self.transition_breakpoints = breakpoints;

        // Sync audio track clips to the audio layer (with transition-aware overlaps)
        if let Some(ref audio_layer) = self.ges_audio_layer {
            for clip in audio_layer.clips() {
                let _ = audio_layer.remove_clip(&clip);
            }

            let audio_clips = &project.timeline.audio_track.clips;
            let mut audio_accumulated_offset = Duration::ZERO;

            for (i, clip_id) in audio_clips.iter().enumerate() {
                if let Some(clip) = project.timeline.get_clip(*clip_id) {
                    // Check for transition with previous audio clip
                    if i > 0 {
                        let prev_id = audio_clips[i - 1];
                        let trans_pos = TransitionPosition::Between(prev_id, *clip_id);
                        if let Some(transition) = project.timeline.find_transition(&trans_pos) {
                            audio_accumulated_offset += transition.duration;
                            log::debug!(
                                "sync_from_model: audio transition between clips, offset now {:?}",
                                audio_accumulated_offset
                            );
                        }
                    }

                    let ges_start = clip.start.saturating_sub(audio_accumulated_offset);

                    if let Some(source) = project.get_source(clip.source_id) {
                        let uri = path_to_uri(&source.path)?;
                        match ges::UriClip::new(&uri) {
                            Ok(ges_clip) => {
                                ges_clip.set_start(dur_to_clocktime(ges_start));
                                ges_clip.set_duration(dur_to_clocktime(clip.duration));
                                ges_clip.set_inpoint(dur_to_clocktime(clip.in_point));
                                if let Err(e) = audio_layer.add_clip(&ges_clip) {
                                    log::error!(
                                        "Failed to add audio clip {} to GES layer: {}",
                                        clip.id,
                                        e
                                    );
                                }
                            }
                            Err(e) => {
                                log::error!(
                                    "Failed to create GES audio UriClip for {}: {}",
                                    source.path.display(),
                                    e
                                );
                            }
                        }
                    }
                }
            }
        }

        log::debug!("sync_from_model: committing timeline");
        ges_timeline.commit();

        // Ensure pipeline is at least paused so it can preroll.
        // Wait for the state change to complete so seeking works immediately.
        match current_state {
            gst::State::Null | gst::State::Ready => {
                let _ = pipeline.set_state(gst::State::Paused);
                let _ = pipeline.state(gst::ClockTime::from_seconds(5));
            }
            _ => {
                // Pipeline was already running - pause briefly so flush seek works
                if !was_playing {
                    let _ = pipeline.set_state(gst::State::Paused);
                    let _ = pipeline.state(gst::ClockTime::from_seconds(5));
                }
            }
        }

        // Always do a flush seek to reset compositor state after timeline rebuild.
        // Without this, transitions can render incorrectly on first play-through.
        let seek_pos = saved_pos.unwrap_or(gst::ClockTime::ZERO);
        let _ = pipeline.seek_simple(
            gst::SeekFlags::FLUSH | gst::SeekFlags::ACCURATE,
            seek_pos,
        );

        // Resume if was playing
        if was_playing {
            let _ = pipeline.set_state(gst::State::Playing);
        }

        log::debug!("sync_from_model: done");
        Ok(())
    }

    /// Convert GES pipeline time to model time.
    /// Uses linear interpolation during transition overlaps for smooth playhead movement.
    pub fn ges_to_model_time(&self, ges: Duration) -> Duration {
        for bp in &self.transition_breakpoints {
            if ges < bp.ges_overlap_start {
                return ges + bp.offset_before;
            }
            if ges <= bp.ges_overlap_end {
                // During transition: linear interpolation (playhead moves at ~2x through overlap)
                let ges_range = (bp.ges_overlap_end - bp.ges_overlap_start).as_secs_f64();
                if ges_range > 0.0 {
                    let frac = (ges - bp.ges_overlap_start).as_secs_f64() / ges_range;
                    let model_start =
                        (bp.ges_overlap_start + bp.offset_before).as_secs_f64();
                    let model_end =
                        (bp.ges_overlap_end + bp.offset_after).as_secs_f64();
                    return Duration::from_secs_f64(
                        model_start + frac * (model_end - model_start),
                    );
                }
                return ges + bp.offset_after;
            }
        }
        // After all transitions
        let final_offset = self
            .transition_breakpoints
            .last()
            .map(|bp| bp.offset_after)
            .unwrap_or(Duration::ZERO);
        ges + final_offset
    }

    /// Convert model time to GES pipeline time.
    fn model_to_ges_time(&self, model: Duration) -> Duration {
        for bp in &self.transition_breakpoints {
            let model_overlap_start = bp.ges_overlap_start + bp.offset_before;
            let model_overlap_end = bp.ges_overlap_end + bp.offset_after;

            if model < model_overlap_start {
                return model.saturating_sub(bp.offset_before);
            }
            if model <= model_overlap_end {
                // During transition: inverse interpolation
                let model_range = (model_overlap_end - model_overlap_start).as_secs_f64();
                if model_range > 0.0 {
                    let frac = (model - model_overlap_start).as_secs_f64() / model_range;
                    let ges_range =
                        (bp.ges_overlap_end - bp.ges_overlap_start).as_secs_f64();
                    return Duration::from_secs_f64(
                        bp.ges_overlap_start.as_secs_f64() + frac * ges_range,
                    );
                }
                return model.saturating_sub(bp.offset_after);
            }
        }
        let final_offset = self
            .transition_breakpoints
            .last()
            .map(|bp| bp.offset_after)
            .unwrap_or(Duration::ZERO);
        model.saturating_sub(final_offset)
    }

    pub fn play(&self) {
        if let Some(ref pipeline) = self.ges_pipeline {
            let _ = pipeline.set_state(gst::State::Playing);
        }
    }

    pub fn pause(&self) {
        if let Some(ref pipeline) = self.ges_pipeline {
            let _ = pipeline.set_state(gst::State::Paused);
        }
    }

    pub fn stop(&mut self) {
        if let Some(ref pipeline) = self.ges_pipeline {
            let _ = pipeline.set_state(gst::State::Null);
        }
        self.ges_pipeline = None;
        self.ges_timeline = None;
        self.ges_layer = None;
        self.ges_audio_layer = None;
        self.export_text_clips.clear();
        self.transition_breakpoints.clear();
        // Drain frame channel
        while self.frame_rx.try_recv().is_ok() {}
    }

    /// Seek to a model time position (converts to GES time internally).
    pub fn seek(&self, model_position: Duration) {
        if let Some(ref pipeline) = self.ges_pipeline {
            // Ensure pipeline is at least Paused so seeking produces a frame.
            let (_, current, _) = pipeline.state(gst::ClockTime::ZERO);
            if matches!(
                current,
                gst::State::Null | gst::State::Ready | gst::State::VoidPending
            ) {
                let _ = pipeline.set_state(gst::State::Paused);
                // Wait for preroll to complete so the seek has something to work with
                let _ = pipeline.state(gst::ClockTime::from_seconds(5));
            }

            let ges_pos = self.model_to_ges_time(model_position);
            let _ = pipeline.seek_simple(
                gst::SeekFlags::FLUSH | gst::SeekFlags::KEY_UNIT,
                dur_to_clocktime(ges_pos),
            );
        }
    }

    /// Returns current position in model time.
    pub fn position(&self) -> Option<Duration> {
        self.ges_pipeline.as_ref().and_then(|p| {
            p.query_position::<gst::ClockTime>()
                .map(|ct| self.ges_to_model_time(Duration::from_nanos(ct.nseconds())))
        })
    }

    /// Returns total duration in model time (adjusts for transition overlaps).
    pub fn model_duration(&self) -> Option<Duration> {
        self.ges_pipeline.as_ref().and_then(|p| {
            p.query_duration::<gst::ClockTime>()
                .map(|ct| self.ges_to_model_time(Duration::from_nanos(ct.nseconds())))
        })
    }

    /// Returns total GES pipeline duration (without model time adjustment).
    #[allow(dead_code)]
    pub fn duration(&self) -> Option<Duration> {
        self.ges_pipeline.as_ref().and_then(|p| {
            p.query_duration::<gst::ClockTime>()
                .map(|ct| Duration::from_nanos(ct.nseconds()))
        })
    }

    pub fn is_loaded(&self) -> bool {
        self.ges_pipeline.is_some()
    }

    pub fn try_recv_frame(&self) -> Option<VideoFrame> {
        let mut latest = None;
        while let Ok(frame) = self.frame_rx.try_recv() {
            latest = Some(frame);
        }
        latest
    }

    /// Start exporting to an MP4 file. The GES pipeline switches to RENDER mode.
    pub fn start_export(&mut self, output_path: &Path, project: &Project) -> Result<()> {
        self.ensure_pipeline()?;
        self.sync_from_model(project)?;

        // Add text overlays as TitleClips for export (not in preview)
        self.add_text_layer_for_export(project);

        let pipeline = self.ges_pipeline.as_ref().unwrap();

        // Stop playback first
        let _ = pipeline.set_state(gst::State::Null);
        let _ = pipeline.state(gst::ClockTime::from_seconds(5));

        // H264 video in MP4 container
        let video_caps = gst::Caps::builder("video/x-h264")
            .field("profile", "main")
            .build();
        let video_profile = gst_pbutils::EncodingVideoProfile::builder(&video_caps).build();

        // AAC audio
        let audio_caps = gst::Caps::builder("audio/mpeg")
            .field("mpegversion", 4i32)
            .build();
        let audio_profile = gst_pbutils::EncodingAudioProfile::builder(&audio_caps).build();

        // MP4 container with faststart (moov atom at beginning)
        let container_caps = gst::Caps::builder("video/quicktime")
            .field("variant", "iso")
            .build();
        let muxer_props = gst_pbutils::ElementProperties::builder_general()
            .field("faststart", true)
            .build();
        let profile = gst_pbutils::EncodingContainerProfile::builder(&container_caps)
            .element_properties(muxer_props)
            .add_profile(video_profile)
            .add_profile(audio_profile)
            .build();

        let uri = path_to_uri(output_path)?;
        pipeline.set_render_settings(&uri, &profile)?;
        pipeline.set_mode(ges::PipelineFlags::RENDER)?;

        // Preroll in Paused, then flush seek to 0 to reset compositor state.
        // Without this, wipe transitions glitch on the first frame (displaced quadrants).
        let _ = pipeline.set_state(gst::State::Paused);
        let _ = pipeline.state(gst::ClockTime::from_seconds(5));
        let _ = pipeline.seek_simple(
            gst::SeekFlags::FLUSH | gst::SeekFlags::ACCURATE,
            gst::ClockTime::ZERO,
        );

        let _ = pipeline.set_state(gst::State::Playing);
        let _ = pipeline.state(gst::ClockTime::from_seconds(10));

        self.export_path = Some(output_path.to_path_buf());
        self.export_state = ExportState::Exporting;
        log::info!("Export started: {}", output_path.display());

        Ok(())
    }

    /// Poll the export pipeline for progress. Returns (state, progress 0.0-1.0).
    /// Call this each frame while exporting.
    pub fn poll_export(&mut self) -> (ExportState, f64) {
        if !matches!(self.export_state, ExportState::Exporting) {
            return (self.export_state.clone(), 0.0);
        }

        let pipeline = match &self.ges_pipeline {
            Some(p) => p,
            None => {
                self.export_state = ExportState::Error("No pipeline".into());
                return (self.export_state.clone(), 0.0);
            }
        };

        // Check bus for EOS or Error
        if let Some(bus) = pipeline.bus() {
            while let Some(msg) = bus.pop() {
                use gst::MessageView;
                match msg.view() {
                    MessageView::Eos(_) => {
                        log::info!("Export complete (EOS received on bus)");
                        self.export_state = ExportState::Done;
                        return (ExportState::Done, 1.0);
                    }
                    MessageView::Error(err) => {
                        let msg = format!("{}", err.error());
                        log::error!("Export error: {}", msg);
                        self.export_state = ExportState::Error(msg.clone());
                        return (ExportState::Error(msg), 0.0);
                    }
                    _ => {}
                }
            }
        }

        // Query position for progress
        let progress = match (
            pipeline.query_position::<gst::ClockTime>(),
            pipeline.query_duration::<gst::ClockTime>(),
        ) {
            (Some(pos), Some(dur)) if dur.nseconds() > 0 => {
                pos.nseconds() as f64 / dur.nseconds() as f64
            }
            _ => 0.0,
        };

        (ExportState::Exporting, progress)
    }

    /// Cancel or finish export, restoring to preview mode.
    pub fn finish_export(&mut self) {
        if let Some(ref pipeline) = self.ges_pipeline {
            // Gradual state transition so the muxer fully finalizes the
            // container (writes moov atom) before elements are torn down.
            let _ = pipeline.set_state(gst::State::Paused);
            let _ = pipeline.state(gst::ClockTime::from_seconds(5));
            let _ = pipeline.set_state(gst::State::Ready);
            let _ = pipeline.state(gst::ClockTime::from_seconds(5));
            let _ = pipeline.set_state(gst::State::Null);
            let _ = pipeline.state(gst::ClockTime::from_seconds(5));
            let _ = pipeline.set_mode(ges::PipelineFlags::FULL_PREVIEW);
        }

        // Log output file size for debugging
        if let Some(ref path) = self.export_path {
            match std::fs::metadata(path) {
                Ok(meta) => log::info!("Export file size: {} bytes ({})", meta.len(), path.display()),
                Err(e) => log::warn!("Could not stat export file: {} ({})", e, path.display()),
            }
        }

        // Remove the text layer (only used during export)
        self.remove_text_layer();

        self.export_path = None;
        self.export_state = ExportState::Idle;
        log::info!("Export finished/cancelled, restored to preview mode");
    }

    /// Add TextOverlayClip objects to the video layer for export.
    /// Uses GES TextOverlayClip (an operation clip that uses GStreamer's
    /// textoverlay element) instead of TitleClip. This overlays text directly
    /// onto video frames without needing a separate layer, alpha compositing,
    /// or ARGB format caps.
    fn add_text_layer_for_export(&mut self, project: &Project) {
        let video_layer = match &self.ges_layer {
            Some(l) => l.clone(),
            None => return,
        };

        if project.timeline.text_overlays.is_empty() {
            return;
        }

        // Get composition height for font scaling. Preview uses:
        //   font_size * (display_h / 720.0) pixels
        let comp_h = project
            .source_files
            .iter()
            .find(|s| s.has_video)
            .map(|s| s.height as f32)
            .unwrap_or(project.output_settings.height as f32);

        for clip_id in &project.timeline.text_track.clips {
            if let Some(clip) = project.timeline.get_clip(*clip_id) {
                if let Some(overlay) = project.timeline.get_text_overlay(*clip_id) {
                    let Some(text_clip) = ges::TextOverlayClip::new() else {
                        log::error!("Failed to create TextOverlayClip");
                        continue;
                    };

                    // Font: scale from 720p reference to actual video height,
                    // then use Pango measurement to correct for rendering
                    // differences between egui (preview) and Pango (export).
                    let target_px = overlay.font_size * comp_h / 720.0;
                    let font_desc = pango_corrected_font_desc(
                        &overlay.text,
                        &overlay.font_family,
                        overlay.bold,
                        overlay.italic,
                        target_px,
                    );

                    // Text color (ARGB u32)
                    let color_argb = ((overlay.color[3] as u32) << 24)
                        | ((overlay.color[0] as u32) << 16)
                        | ((overlay.color[1] as u32) << 8)
                        | (overlay.color[2] as u32);

                    // Set ALL properties BEFORE add_clip so that
                    // create_track_element uses correct values (GES reads
                    // priv fields when constructing the track element).
                    text_clip.set_start(dur_to_clocktime(clip.start));
                    text_clip.set_duration(dur_to_clocktime(clip.duration));
                    text_clip.set_text(Some(&overlay.text));
                    text_clip.set_font_desc(Some(&font_desc));
                    text_clip.set_color(color_argb);
                    text_clip.set_halign(ges::TextHAlign::Position);
                    text_clip.set_valign(ges::TextVAlign::Position);
                    text_clip.set_xpos(overlay.position.0 as f64);
                    text_clip.set_ypos(overlay.position.1 as f64);

                    if let Err(e) = video_layer.add_clip(&text_clip) {
                        log::error!("Failed to add TextOverlayClip: {}", e);
                        continue;
                    }

                    // Navigate the GES/GStreamer element hierarchy to find the
                    // underlying textoverlay GstElement:
                    // TextOverlayClip → TrackElement → NleObject (Bin) → inner
                    // Bin → textoverlay GstElement.
                    for child in text_clip.children(false) {
                        if let Some(te) = child.dynamic_cast_ref::<ges::TrackElement>() {
                            let nle = te.nleobject();
                            if let Ok(nle_bin) = nle.clone().dynamic_cast::<gst::Bin>() {
                                let mut iter = nle_bin.iterate_recurse();
                                while let Ok(Some(el)) = iter.next() {
                                    let is_textoverlay = el
                                        .factory()
                                        .map(|f| f.name() == "textoverlay")
                                        .unwrap_or(false);
                                    if !is_textoverlay {
                                        continue;
                                    }

                                    el.set_property("draw-outline", false);
                                    el.set_property("draw-shadow", false);

                                    // Set up fade in/out via ARGB control binding
                                    // on the "color" property. We animate only the
                                    // alpha channel; RGB stays constant.
                                    let fade_in = project.timeline.find_transition(
                                        &TransitionPosition::FadeIn(*clip_id),
                                    );
                                    let fade_out = project.timeline.find_transition(
                                        &TransitionPosition::FadeOut(*clip_id),
                                    );

                                    if fade_in.is_some() || fade_out.is_some() {
                                        // Control point timestamps must be in
                                        // the NLE pipeline's running time, which
                                        // matches the GES timeline position.
                                        let t_start = clip.start.as_nanos() as u64;
                                        let t_end = clip.end().as_nanos() as u64;

                                        // Alpha channel: ramp in/out
                                        let cs_a = gst_ctrl::InterpolationControlSource::new();
                                        cs_a.set_mode(gst_ctrl::InterpolationMode::Linear);

                                        let base_alpha = overlay.color[3] as f64 / 255.0;

                                        if let Some(fi) = fade_in {
                                            let fade_ns = fi.duration.as_nanos() as u64;
                                            cs_a.set(
                                                gst::ClockTime::from_nseconds(t_start),
                                                0.0,
                                            );
                                            cs_a.set(
                                                gst::ClockTime::from_nseconds(t_start + fade_ns),
                                                base_alpha,
                                            );
                                        } else {
                                            cs_a.set(
                                                gst::ClockTime::from_nseconds(t_start),
                                                base_alpha,
                                            );
                                        }

                                        if let Some(fo) = fade_out {
                                            let fade_ns = fo.duration.as_nanos() as u64;
                                            let fade_start = t_end.saturating_sub(fade_ns);
                                            // Hold full alpha until fade-out begins
                                            if fade_in.is_none() || fade_start > t_start {
                                                cs_a.set(
                                                    gst::ClockTime::from_nseconds(fade_start),
                                                    base_alpha,
                                                );
                                            }
                                            cs_a.set(
                                                gst::ClockTime::from_nseconds(t_end),
                                                0.0,
                                            );
                                        } else if fade_in.is_some() {
                                            // Hold alpha through end
                                            cs_a.set(
                                                gst::ClockTime::from_nseconds(t_end),
                                                base_alpha,
                                            );
                                        }

                                        // RGB channels: constant at clip start
                                        let cs_r = gst_ctrl::InterpolationControlSource::new();
                                        cs_r.set_mode(gst_ctrl::InterpolationMode::None);
                                        cs_r.set(
                                            gst::ClockTime::from_nseconds(t_start),
                                            overlay.color[0] as f64 / 255.0,
                                        );

                                        let cs_g = gst_ctrl::InterpolationControlSource::new();
                                        cs_g.set_mode(gst_ctrl::InterpolationMode::None);
                                        cs_g.set(
                                            gst::ClockTime::from_nseconds(t_start),
                                            overlay.color[1] as f64 / 255.0,
                                        );

                                        let cs_b = gst_ctrl::InterpolationControlSource::new();
                                        cs_b.set_mode(gst_ctrl::InterpolationMode::None);
                                        cs_b.set(
                                            gst::ClockTime::from_nseconds(t_start),
                                            overlay.color[2] as f64 / 255.0,
                                        );

                                        let binding = gst_ctrl::ARGBControlBinding::new(
                                            &el, "color", &cs_a, &cs_r, &cs_g, &cs_b,
                                        );
                                        if let Err(e) = el.add_control_binding(&binding) {
                                            log::warn!("Failed to add ARGB control binding: {}", e);
                                        }
                                        log::info!(
                                            "  fade: in={:?} out={:?}, timeline [{:.2}s-{:.2}s]",
                                            fade_in.map(|t| t.duration),
                                            fade_out.map(|t| t.duration),
                                            t_start as f64 / 1e9,
                                            t_end as f64 / 1e9,
                                        );
                                    }

                                    log::info!(
                                        "Text '{}': font='{}', pos=({:.2},{:.2})",
                                        overlay.text,
                                        font_desc,
                                        overlay.position.0,
                                        overlay.position.1,
                                    );
                                    break;
                                }
                            }
                        }
                    }

                    self.export_text_clips.push(text_clip);
                }
            }
        }

        log::info!(
            "Added {} text overlay clips for export, comp_h={}",
            self.export_text_clips.len(),
            comp_h,
        );
    }

    /// Remove the text overlay clips from the video layer (called after export).
    fn remove_text_layer(&mut self) {
        if let Some(ref layer) = self.ges_layer {
            for clip in self.export_text_clips.drain(..) {
                let _ = layer.remove_clip(&clip);
            }
        }
        self.export_text_clips.clear();
        log::info!("Text overlay clips removed");
    }
}

/// Manually create a TransitionClip with the correct vtype and add it to the layer.
fn add_manual_transition(
    layer: &ges::Layer,
    start: Duration,
    duration: Duration,
    kind: TransitionKind,
) {
    let vtype = match kind {
        TransitionKind::Fade | TransitionKind::Dissolve => {
            ges::VideoStandardTransitionType::Crossfade
        }
        TransitionKind::WipeLeft => ges::VideoStandardTransitionType::BarWipeLr,
        TransitionKind::WipeRight => ges::VideoStandardTransitionType::BarWipeLr,
        TransitionKind::WipeDown => ges::VideoStandardTransitionType::BarWipeTb,
        TransitionKind::WipeUp => ges::VideoStandardTransitionType::BarWipeTb,
    };

    let needs_invert = matches!(kind, TransitionKind::WipeRight | TransitionKind::WipeUp);

    match ges::TransitionClip::new(vtype) {
        Some(trans_clip) => {
            trans_clip.set_start(dur_to_clocktime(start));
            trans_clip.set_duration(dur_to_clocktime(duration));

            if let Err(e) = layer.add_clip(&trans_clip) {
                log::error!("Failed to add TransitionClip to layer: {}", e);
                return;
            }

            if needs_invert {
                for child in trans_clip.children(false) {
                    if let Some(vt) = child.dynamic_cast_ref::<ges::VideoTransition>() {
                        #[allow(deprecated)]
                        vt.set_inverted(true);
                        break;
                    }
                }
            }

            log::debug!(
                "add_manual_transition: {:?} at {:?} dur {:?} (invert={})",
                kind,
                start,
                duration,
                needs_invert,
            );
        }
        None => {
            log::error!("Failed to create TransitionClip for {:?}", kind);
        }
    }
}

/// Build a Pango font description string with a corrected pixel size.
/// Measures what Pango actually renders at the target em-square size, then
/// adjusts so the rendered text height matches `target_px`. This compensates
/// for Pango rendering text taller than the nominal font size (line spacing,
/// metric differences vs egui's ab_glyph renderer).
fn pango_corrected_font_desc(
    text: &str,
    family: &str,
    bold: bool,
    italic: bool,
    target_px: f32,
) -> String {
    let initial_px = target_px.max(1.0) as u32;
    let initial_desc = build_pango_font_desc(family, bold, italic, initial_px);

    // Measure with Pango to see actual rendered height
    let font_map = pangocairo::FontMap::default();
    let context = font_map.create_context();
    let layout = pango::Layout::new(&context);
    let desc = pango::FontDescription::from_string(&initial_desc);
    layout.set_font_description(Some(&desc));
    layout.set_text(text);
    let (_w, h) = layout.pixel_size();

    if h > 0 {
        let correction = target_px / h as f32;
        let corrected_px = (initial_px as f32 * correction).max(1.0) as u32;
        let corrected_desc = build_pango_font_desc(family, bold, italic, corrected_px);
        log::info!(
            "Pango font correction: '{}' initial={}px measured_h={} target={:.1} corrected={}px",
            family, initial_px, h, target_px, corrected_px,
        );
        corrected_desc
    } else {
        log::warn!("Pango measurement returned 0 height, using uncorrected size");
        initial_desc
    }
}

fn build_pango_font_desc(family: &str, bold: bool, italic: bool, size_px: u32) -> String {
    let mut desc = family.to_string();
    if bold {
        desc.push_str(" Bold");
    }
    if italic {
        desc.push_str(" Italic");
    }
    desc.push_str(&format!(" {}px", size_px));
    desc
}

impl Drop for MediaEngine {
    fn drop(&mut self) {
        self.stop();
    }
}
