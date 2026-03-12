# FriendlyVid - Simple Video Editor for Linux

A CapCut-simple video editor for Linux, built in Rust with egui + GStreamer.

## Session Notes - 2026-03-08

### Task Summary

Building a dead-simple GUI video editor from scratch. The goal is CapCut-level simplicity: load video, trim, split, reorder, transitions, titles, crop, scale, export. No feature creep, no pro-editor complexity.

### Current State

**Builds and runs.** Core editing features are functional across all three tracks (Video, Audio, Text).

Working features:
- Open media files (video + audio) via File > Open, plays with audio in preview panel
- Audio-only files (mp3, wav, ogg, flac, aac, m4a) route to Audio track
- Play/pause (Space), seek slider, draggable playhead on timeline
- Multi-track timeline (Video/Audio/Text) with clip rendering and time ruler
- Split clips at playhead (Split button or Ctrl+B)
- Click clips to select (highlight) on any track, Delete/Backspace to remove
- Video clips: gap-closing delete. Audio/text clips: no gap closing (free placement)
- Drag-to-move audio/text clips, drag-to-resize all clip edges
- Junction hover highlighting (yellow diamond) between clips on all tracks
- Click junction: transition menu (Cut/Fade for all tracks, Wipe for video only)
- Edge transitions: Fade from/to black at start/end of timeline
- Transition indicators shown as cyan diamonds with labels
- Undo/redo for all operations (Ctrl+Z / Ctrl+Shift+Z)
- Media browser: text list with X remove button per source, removes source + all clips
- Right-click text track to add text clips (positioned from click through end of content)
- Text overlay rendering on preview panel (scaled, positioned, with selection highlight)
- Double-click text in preview to edit inline, drag to reposition
- Crop mode: scroll to zoom, drag to pan (Enter to confirm, Escape to cancel)
- H264 MP4 export with text overlays via GES TitleClip
- Save/Load .fvid project files (Ctrl+S, File > Save/Save As/Open Project)
- File menu with New, Open, Save, Save As, Open Project, Export, Exit
- Zoom (Ctrl+scroll) and pan (scroll) on timeline

### Architecture

```
FriendlyVidApp (app.rs)
  +- AppState (state/)       -- project model, selection, playhead
  +- CommandHistory           -- undo/redo stack (separate from AppState)
  +- MediaEngine (media/)     -- GES Timeline+Pipeline, multi-layer (video+audio+text)
  +- LayoutState (ui/)        -- preview panel, timeline view state
```

**Key pattern:** All edits go through `Command` objects with `execute()`/`undo()`. The UI never modifies the model directly.

**GES Pipeline:** Three layers - video (manual transitions), audio (auto-transition for crossfade), text (TitleClip, export only). Preview uses appsink for RGBA frames; text overlays rendered via egui in preview.

**Timeline widget:** Custom-painted with `allocate_painter()`. Multi-track interaction with DragState enum for move/resize. Returns `Vec<TimelineAction>` to the layout.

**Text system:** Clip on text track (source_id = Uuid::nil()) + TextOverlay in HashMap<Uuid, TextOverlay> keyed by clip_id. Timing from Clip, content/styling from TextOverlay. Preview renders via egui painter, export via GES TitleClip.

### Key Decisions

- **egui with glow backend** (not wgpu) - wgpu panicked on the target machine.
- **winit pinned to 0.30.12** - 0.30.13 has type inference bugs with Rust 1.93.
- **GES multi-layer** - Video layer (manual TransitionClip), audio layer (auto-transition), text layer (export only).
- **CommandHistory separate from AppState** - Avoids double-mutable-borrow.
- **Track-aware delete** - Video: gap-closing. Audio/Text: free placement (no gap closing).
- **Text overlay timing from Clip** - TextOverlay has no start/duration, those come from the Clip.
- **Text preview via egui, export via GES** - egui overlay for interactive editing, GES TitleClip for burned-in export.
- **Backward-compatible serialization** - `#[serde(default = "default_true")]` for new SourceFile fields.

### Files to Read First

1. `src/ui/timeline_widget.rs` - Custom widget: multi-track interaction, drag state, junction menus, clip rendering.
2. `src/ui/layout.rs` - Wires everything: toolbar, keyboard, timeline actions, text edit mode.
3. `src/media/engine.rs` - GES integration: multi-layer pipeline, sync_from_model, export with text.
4. `src/model/timeline.rs` - Data model: clips HashMap, tracks, text_overlays HashMap, gap-closing.
5. `src/commands/clip_commands.rs` - All commands: Add/Remove/Trim/Split/Move clip, AddTextClip, RemoveSource.

### System Requirements

```bash
sudo apt install libgstreamer1.0-dev libgstreamer-plugins-base1.0-dev \
  libges-1.0-dev gstreamer1.0-plugins-good gstreamer1.0-plugins-bad \
  gstreamer1.0-plugins-ugly gstreamer1.0-libav
```

Rust edition 2021, toolchain 1.93.1+.

### Plan File

Implementation plan at `~/.claude/plans/snoopy-hopping-corbato.md` - Batch 1-3 (audio, multi-track interaction, text) are complete.
