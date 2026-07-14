# tixel

Display images and videos directly in your terminal using Unicode half-block characters (`▄`) with true-color ANSI escape codes.

![platform](https://img.shields.io/badge/platform-linux-blue)
![Rust](https://img.shields.io/badge/rust-1.85+-orange)

## Demo

```bash
# Static image
cargo run -- rickroll.jpeg

# Video with native framerate and audio
cargo run -- video.mp4

# Custom width and framerate
cargo run -- video.mp4 --width 120 --fps 15
```

## Features

- **Images** — PNG, JPEG, BMP, GIF, WebP (via [`image`](https://crates.io/crates/image))
- **Videos** — MP4, MOV, AVI, WebM, MKV (via [`video-rs`](https://crates.io/crates/video-rs) / FFmpeg)
- **Audio** — automatic background playback via `ffplay` (best-effort, silent if unavailable)
- **True colour** — 24-bit RGB ANSI output
- **Terminal-sized** — auto-detects terminal dimensions and scales media to fit
- **Smooth playback** — synchronised output (DEC 2026) on supporting terminals (kitty, foot, WezTerm, iTerm2, ghostty), no tearing on others
- **Alternate screen** — clean enter/exit, restores your terminal contents on exit
- **Ctrl+C** — gracefully stops playback and restores the terminal

## Installation

### Prerequisites

**Rust toolchain** (1.85 or later):

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

**FFmpeg development libraries** (required for video decoding):

```bash
# Debian / Ubuntu
sudo apt install libavutil-dev libavcodec-dev libavformat-dev \
                 libswscale-dev libswresample-dev libavfilter-dev \
                 libavdevice-dev libpostproc-dev

# Fedora
sudo dnf install ffmpeg-devel

# Arch
sudo pacman -S ffmpeg
```

**FFplay** (optional, for audio playback, comes with `ffmpeg`):

```bash
sudo apt install ffmpeg    # Debian / Ubuntu
```

### Build

```bash
git clone https://github.com/Paolo-Comper/tixel.git
cd tixel
cargo build --release
```

The binary is at `target/release/tixel`. You can copy it anywhere:

```bash
cp target/release/tixel ~/.local/bin/
```

## Usage

```
Usage: tixel [OPTIONS] <INPUT>

Arguments:
  <INPUT>  Path to the input file (image or video)

Options:
  -w, --width <WIDTH>  Output width in terminal columns (0 = auto-detect)
  -f, --fps <FPS>      Playback framerate for videos (0 = native framerate)
  -h, --help           Print help
  -V, --version        Print version
```

### Examples

```bash
# Show an image at default width (terminal width - 2)
tixel photo.jpg

# Show an image at 80 columns
tixel photo.jpg --width 80

# Play a video at native framerate (audio plays in background)
tixel video.mp4

# Play a video at 15 fps, 120 columns wide
tixel video.mp4 --width 120 --fps 15

# Single image: press Ctrl+C to exit
tixel wallpaper.png
```

## How it works

1. Media is decoded into a `Vec<RgbaImage>` (all frames are pre-loaded)
2. Each frame is resized to fit the terminal dimensions while preserving aspect ratio
3. Frames are rendered using the Unicode lower-half-block character `▄` — each text row displays **two** pixel rows (upper pixel = background colour, lower pixel = foreground colour)
4. Output is built as a single `String` per frame and written atomically to stdout
5. For videos, frames are displayed at the requested (or detected) framerate with timing compensation

## Supported formats

| Type | Extensions |
|------|-----------|
| Video | `mp4`, `mov`, `avi`, `webm`, `mkv`, `m4v` |
| Image | `png`, `jpg`, `jpeg`, `bmp`, `gif`, `webp` |

