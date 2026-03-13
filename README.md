# FriendlyVid

A dead-simple video editor for Linux. Load video, trim, split, reorder, add transitions and titles, crop, export. No feature creep, no pro-editor complexity.

Built in Rust with [egui](https://github.com/emilk/egui) and [GStreamer](https://gstreamer.freedesktop.org/) (GES).

![Screenshot](/screenshot.png)

## Features

- **Multi-track timeline** - Video, Audio, and Text tracks with clip rendering and time ruler
- **Non-linear editing** - Split clips (Ctrl+B), drag to move/resize, delete with gap closing (video) or free placement (audio/text)
- **Transitions** - Fade, dissolve, and wipe transitions between clips; fade in/out at timeline edges
- **Text overlays** - Add styled text with font selection, color, bold/italic; drag to position, resize handles, inline editing
- **Crop and zoom** - Scroll to zoom, drag to pan, per-clip transforms
- **Audio support** - Import audio-only files (mp3, wav, ogg, flac, aac, m4a) to a dedicated audio track
- **Export** - H.264 MP4 with burned-in text overlays, Pango-matched font rendering
- **Undo/redo** - Full command history for all operations (Ctrl+Z / Ctrl+Shift+Z)
- **Project files** - Save/load `.fvid` project files (Ctrl+S)

## Keyboard Shortcuts

| Key | Action |
|-----|--------|
| Space | Play/pause |
| Ctrl+B | Split clip at playhead |
| Delete / Backspace | Remove selected clip |
| Ctrl+Z | Undo |
| Ctrl+Shift+Z | Redo |
| Ctrl+S | Save project |
| Ctrl+Scroll | Zoom timeline |
| Scroll | Pan timeline |

## Requirements

**System libraries** (Debian/Ubuntu):

```bash
sudo apt install libgstreamer1.0-dev libgstreamer-plugins-base1.0-dev \
  libges-1.0-dev gstreamer1.0-plugins-good gstreamer1.0-plugins-bad \
  gstreamer1.0-plugins-ugly gstreamer1.0-libav \
  libpango1.0-dev libcairo2-dev
```

**Rust** edition 2021, toolchain 1.93.1+.

## Build and Run

```bash
cargo run
```

Or for an optimized build:

```bash
cargo run --release
```

## Install

To install a .desktop file with accompanying icons:

```bash
friendlyvid --install
```

## License

[MIT](LICENSE)
