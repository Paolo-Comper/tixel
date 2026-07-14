use clap::Parser;
use crossterm::terminal;
use image::{
    imageops::FilterType::Triangle,
    RgbaImage,
};
use std::{
    fmt::Write,
    io::{stdout, Write as IoWrite},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::atomic::{AtomicBool, Ordering},
    sync::Arc,
    time::{Duration, Instant},
};
use video_rs::{decode::Decoder, Location};

mod error;
use error::ImgToTerminalError as Error;

#[derive(Parser)]
#[command(name = "tixel", version, about = "Display images and videos in the terminal")]
struct Args {
    /// Path to the input file (image or video)
    input: PathBuf,

    /// Output width in terminal columns (0 = auto-detect)
    #[arg(short, long, default_value_t = 0)]
    width: u32,

    /// Playback framerate for videos (0 = use video's native framerate)
    #[arg(short, long, default_value_t = 0.0)]
    fps: f32,
}

fn main() {
    let args = Args::parse();

    if let Err(e) = run(&args) {
        eprintln!("\x1b[1;31mErrore:\x1b[0m {e}");
        std::process::exit(1);
    }
}

fn run(args: &Args) -> Result<(), Error> {
    if !args.input.exists() {
        return Err(Error::FileNotFound(args.input.display().to_string()));
    }

    let (term_cols, term_rows) = terminal::size()?;

    let target_width = if args.width > 0 {
        args.width
    } else {
        if term_cols < 20 {
            return Err(Error::TerminalTooSmall(term_cols, term_rows));
        }
        term_cols.saturating_sub(2).max(20) as u32
    };

    // One text row = 2 pixel rows via half-block ▄
    let max_height = (term_rows.saturating_sub(1) * 2) as u32;

    let (frames, detected_fps) =
        load_media(&args.input, target_width, max_height)?;

    if frames.is_empty() {
        return Err(Error::NoFrames(args.input.display().to_string()));
    }

    let fps = if args.fps > 0.0 { args.fps } else { detected_fps };

    // Background audio playback (ffplay) for videos only
    let audio = if fps > 0.0 {
        AudioPlayer::start(&args.input)
    } else {
        AudioPlayer(None)
    };

    render_loop(&frames, fps)?;

    audio.wait();
    Ok(())
}

fn ext_lower(path: &Path) -> Option<String> {
    path.extension()?.to_str().map(|s| s.to_ascii_lowercase())
}

fn is_video_file(path: &Path) -> bool {
    matches!(
        ext_lower(path).as_deref(),
        Some("mp4" | "mov" | "avi" | "webm" | "mkv" | "m4v")
    )
}

fn is_image_file(path: &Path) -> bool {
    matches!(
        ext_lower(path).as_deref(),
        Some("png" | "jpg" | "jpeg" | "bmp" | "gif" | "webp")
    )
}

fn detect_file_type(path: &Path) -> Result<(), Error> {
    match ext_lower(path) {
        Some(_) if is_video_file(path) || is_image_file(path) => Ok(()),
        Some(ext) => Err(Error::UnknownFormat(ext)),
        None => Err(Error::UnknownFormat("(nessuna estensione)".into())),
    }
}

fn load_media(
    path: &Path,
    target_width: u32,
    max_height: u32,
) -> Result<(Vec<RgbaImage>, f32), Error> {
    detect_file_type(path)?;

    if is_video_file(path) {
        load_video(path, target_width, max_height)
    } else {
        load_image(path, target_width, max_height)
    }
}

fn load_image(
    path: &Path,
    target_width: u32,
    max_height: u32,
) -> Result<(Vec<RgbaImage>, f32), Error> {
    let img = image::open(path)?;
    let (w, h) = (img.width(), img.height());
    let (out_w, out_h) = proportional_size(w, h, target_width, max_height);
    let resized = img.resize_exact(out_w, out_h, Triangle).to_rgba8();
    Ok((vec![resized], 0.0))
}

fn load_video(
    path: &Path,
    target_width: u32,
    max_height: u32,
) -> Result<(Vec<RgbaImage>, f32), Error> {
    video_rs::init().map_err(|e| Error::VideoInit(e.to_string()))?;

    let mut decoder =
        Decoder::new(Location::from(path)).map_err(|e| Error::VideoOpen(e.to_string()))?;

    let fps = decoder.frame_rate();
    let detected_fps = if fps > 0.0 { fps } else { 10.0 };

    let (in_w, in_h) = decoder.size();
    let (out_w, out_h) = proportional_size(in_w, in_h, target_width, max_height);

    let mut frames = Vec::new();

    for result in decoder.decode_raw_iter() {
        let raw = match result {
            Ok(frame) => frame,
            Err(_) => break,
        };

        let w = raw.width();
        let h = raw.height();
        let stride = raw.stride(0);
        let src = raw.data(0);

        // RGB24 with optional stride padding
        let pix_bytes = w as usize * 3;

        let rgb_data: Vec<u8> = if stride == pix_bytes {
            src[..(pix_bytes * h as usize).min(src.len())].to_vec()
        } else {
            let mut data = Vec::with_capacity(pix_bytes * h as usize);
            for y in 0..h as usize {
                let start = y * stride;
                let end = start + pix_bytes;
                if end <= src.len() {
                    data.extend_from_slice(&src[start..end]);
                }
            }
            data
        };

        let rgb_img = image::RgbImage::from_raw(w, h, rgb_data)
            .ok_or_else(|| Error::VideoDecode("dimensioni frame non valide".into()))?;

        let resized = image::imageops::resize(&rgb_img, out_w, out_h, Triangle);
        let rgba = image::DynamicImage::ImageRgb8(resized).to_rgba8();
        frames.push(rgba);
    }

    if frames.is_empty() {
        return Err(Error::NoFrames(path.display().to_string()));
    }

    Ok((frames, detected_fps))
}

/// Aspect-ratio-preserving size that fits within both constraints.
/// Height is rounded up to an even number (half-block needs 2 pixel rows).
fn proportional_size(in_w: u32, in_h: u32, max_width: u32, max_height: u32) -> (u32, u32) {
    if in_w == 0 || in_h == 0 {
        return (max_width.max(4), 2);
    }

    let w = max_width.max(4);
    let h = ((w as f64 * in_h as f64 / in_w as f64).round() as u32).max(2);

    if h > max_height {
        let h = (max_height & !1).max(2);
        let w = ((h as f64 * in_w as f64 / in_h as f64).round() as u32).max(4);
        return (w, h);
    }

    (w, h + (h & 1))
}

/// Render a single frame to an ANSI escape-code string using half-block
/// characters (▄).  Wraps output in DEC 2026 synchronised mode to reduce
/// tearing on supporting terminals.
fn render_frame(img: &RgbaImage) -> String {
    let (w, h) = img.dimensions();

    let cap = (w as usize * 38 + 10) * (h as usize / 2 + 1);
    let mut out = String::with_capacity(cap);

    out.push_str("\x1b[?2026h");
    out.push_str("\x1b[H");

    if h < 2 {
        out.push_str("\x1b[0J\x1b[?2026l");
        return out;
    }

    for y in (0..h - 1).step_by(2) {
        for x in 0..w {
            let upper = img.get_pixel(x, y);
            let lower = img.get_pixel(x, y + 1);

            // foreground = lower pixel, background = upper pixel
            write!(
                out,
                "\x1b[38;2;{};{};{}m\x1b[48;2;{};{};{}m\u{2584}",
                lower[0], lower[1], lower[2],
                upper[0], upper[1], upper[2],
            )
            .unwrap();
        }
        out.push_str("\x1b[0m\n");
    }

    out.push_str("\x1b[0J\x1b[?2026l");
    out
}

fn render_loop(frames: &[RgbaImage], fps: f32) -> Result<(), Error> {
    if frames.is_empty() {
        return Ok(());
    }

    let running = Arc::new(AtomicBool::new(true));
    let r = running.clone();
    let _ = ctrlc::set_handler(move || r.store(false, Ordering::SeqCst));

    let _screen = AltScreen::enter();

    if frames.len() == 1 {
        // Single image — show until Ctrl+C
        let out = render_frame(&frames[0]);
        print!("{out}");
        stdout().flush()?;
        while running.load(Ordering::SeqCst) {
            std::thread::sleep(Duration::from_millis(200));
        }
    } else {
        // Video — play at the requested framerate
        let frame_duration = if fps > 0.0 {
            Duration::from_secs_f32(1.0 / fps)
        } else {
            Duration::from_millis(100)
        };

        print!("\x1b[?25l");
        stdout().flush().ok();

        for frame in frames {
            if !running.load(Ordering::SeqCst) {
                break;
            }

            let start = Instant::now();
            print!("{}", render_frame(frame));
            stdout().flush()?;

            let elapsed = start.elapsed();
            if elapsed < frame_duration {
                std::thread::sleep(frame_duration - elapsed);
            }
        }
    }

    // Restore cursor and leave alternate screen
    print!("\x1b[?25h");
    stdout().flush().ok();

    Ok(())
}

/// RAII guard: alternate screen buffer on construction, restored on drop.
struct AltScreen;

impl AltScreen {
    fn enter() -> Self {
        print!("\x1b[?1049h");
        stdout().flush().ok();
        AltScreen
    }
}

impl Drop for AltScreen {
    fn drop(&mut self) {
        print!("\x1b[?1049l");
        stdout().flush().ok();
    }
}

/// Background audio playback via `ffplay -vn` (best-effort, silent no-op
/// if ffplay is not available).
struct AudioPlayer(Option<Child>);

impl AudioPlayer {
    fn start(path: &Path) -> Self {
        let child = Command::new("ffplay")
            .args(["-nodisp", "-autoexit", "-loglevel", "quiet", "-vn"])
            .arg(path.as_os_str())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .ok();
        AudioPlayer(child)
    }

    fn wait(mut self) {
        if let Some(ref mut child) = self.0 {
            let _ = child.wait();
        }
    }
}

impl Drop for AudioPlayer {
    fn drop(&mut self) {
        if let Some(ref mut child) = self.0 {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // ── proportional_size ────────────────────────────────────────────────

    #[test]
    fn test_size_normal() {
        // 1920×1080, 80 cols wide, 120 rows tall (∞) → width‑constrained
        let (w, h) = proportional_size(1920, 1080, 80, 9999);
        assert_eq!(w, 80);
        // 80 * 1080 / 1920 = 45.0 → rounded up to even = 46
        assert_eq!(h, 46);
    }

    #[test]
    fn test_size_height_constrained() {
        // 100×500 in 80 cols but only 40 px tall → height‑constrained
        let (w, h) = proportional_size(100, 500, 80, 40);
        // Uses max_height=40 → 40*100/500 = 8
        assert_eq!(w, 8);
        assert_eq!(h, 40);
    }

    #[test]
    fn test_size_height_constrained_odd() {
        // max_height has an odd value which gets rounded down to even
        let (w, h) = proportional_size(100, 200, 80, 41);
        assert_eq!(h, 40); // 41 & !1 = 40
        assert_eq!(w, 20); // 40*100/200 = 20
    }

    #[test]
    fn test_size_min_width_clamp() {
        let (w, h) = proportional_size(100, 100, 1, 9999);
        assert_eq!(w, 4); // clamped to minimum
        assert_eq!(h, 4);
    }

    #[test]
    fn test_size_odd_height_rounded() {
        // 80 * 51 / 100 = 40.8 → 41 → round up to even = 42
        let (_, h) = proportional_size(100, 51, 80, 9999);
        assert_eq!(h % 2, 0);
    }

    #[test]
    fn test_size_zero_input_width() {
        let (w, h) = proportional_size(0, 100, 80, 9999);
        assert_eq!(w, 80);
        assert_eq!(h, 2); // fallback minimum
    }

    #[test]
    fn test_size_tall_image() {
        let (_, h) = proportional_size(100, 500, 80, 9999);
        assert_eq!(h % 2, 0);
        assert!(h > 80);
    }

    #[test]
    fn test_size_wide_image() {
        let (_, h) = proportional_size(500, 100, 80, 9999);
        assert_eq!(h, 16); // 80 * 100 / 500 = 16
    }

    // ── File-type detection ──────────────────────────────────────────────

    #[test]
    fn test_ext_lower_upper() {
        assert_eq!(ext_lower(Path::new("video.MP4")).as_deref(), Some("mp4"));
        assert_eq!(ext_lower(Path::new("Photo.JPEG")).as_deref(), Some("jpeg"));
    }

    #[test]
    fn test_ext_lower_no_ext() {
        assert_eq!(ext_lower(Path::new("Makefile")), None);
    }

    #[test]
    fn test_is_video_true() {
        for ext in &["mp4", "mov", "avi", "webm", "mkv", "m4v"] {
            assert!(is_video_file(Path::new(&format!("f.{ext}"))), "{ext}");
        }
    }

    #[test]
    fn test_is_video_upper() {
        assert!(is_video_file(Path::new("f.MP4")));
        assert!(is_video_file(Path::new("f.MOV")));
    }

    #[test]
    fn test_is_video_false() {
        assert!(!is_video_file(Path::new("f.png")));
        assert!(!is_video_file(Path::new("f.jpg")));
    }

    #[test]
    fn test_is_image_true() {
        for ext in &["png", "jpg", "jpeg", "bmp", "gif", "webp"] {
            assert!(is_image_file(Path::new(&format!("f.{ext}"))), "{ext}");
        }
    }

    #[test]
    fn test_is_image_false() {
        assert!(!is_image_file(Path::new("f.mp4")));
        assert!(!is_image_file(Path::new("f.txt")));
    }

    // ── render_frame ─────────────────────────────────────────────────────

    #[test]
    fn test_render_frame_starts_and_ends() {
        let img = RgbaImage::from_raw(4, 4, vec![
            255, 0, 0, 255,   0, 255, 0, 255,   0, 0, 255, 255,   255, 255, 0, 255,
            0, 255, 255, 255, 255, 0, 255, 255, 128, 128, 128, 255, 64, 64, 64, 255,
            100, 50, 0, 255,   50, 100, 0, 255,   0, 100, 50, 255,   50, 0, 100, 255,
            200, 150, 100, 255, 150, 200, 100, 255, 100, 150, 200, 255, 150, 100, 200, 255,
        ]).unwrap();

        let out = render_frame(&img);
        assert!(out.starts_with("\x1b[?2026h"), "should start with sync begin");
        assert!(out.contains("\x1b[H"), "should contain cursor home");
        assert!(out.ends_with("\x1b[0J\x1b[?2026l"), "should end with clear-to-EOS + sync end");
        assert_eq!(out.matches('\n').count(), 2, "4 rows → 2 char rows");
    }

    #[test]
    fn test_render_frame_too_short() {
        let img = RgbaImage::from_raw(1, 1, vec![255, 0, 0, 255]).unwrap();
        assert_eq!(render_frame(&img), "\x1b[?2026h\x1b[H\x1b[0J\x1b[?2026l");
    }

    #[test]
    fn test_render_frame_pixel_colors() {
        // 2×2 image → 1 row with 2 half-block chars.
        // Upper row = [255,0,0], [0,255,0]
        // Lower row = [0,0,255], [255,255,0]
        let img = RgbaImage::from_raw(2, 2, vec![
            255, 0, 0, 255,   0, 255, 0, 255,
            0, 0, 255, 255, 255, 255, 0, 255,
        ]).unwrap();

        let out = render_frame(&img);
        assert_eq!(out.matches('\u{2584}').count(), 2, "two half-block chars");

        // First pixel: lower=[0,0,255] → fg blue, upper=[255,0,0] → bg red
        assert!(out.contains("38;2;0;0;255"), "fg = blue");
        assert!(out.contains("48;2;255;0;0"), "bg = red");

        // Third pixel: lower=[255,255,0] → fg yellow, upper=[0,255,0] → bg green
        // (second half of the line)
        assert!(out.contains("38;2;255;255;0"), "fg = yellow");
        assert!(out.contains("48;2;0;255;0"), "bg = green");
    }

    #[test]
    fn test_render_frame_odd_height() {
        // 2×3 image → only 1 char row (rows 0+1), row 2 is dropped
        let img = RgbaImage::from_raw(2, 3, vec![
            255, 0, 0, 255,   0, 255, 0, 255,
            0, 0, 255, 255, 255, 255, 0, 255,
            128, 128, 128, 255, 64, 64, 64, 255,
        ]).unwrap();

        let out = render_frame(&img);
        assert_eq!(out.matches('\n').count(), 1, "3 rows → 1 char row");
    }

    // ── Error display ────────────────────────────────────────────────────

    #[test]
    fn test_error_file_not_found() {
        let e = Error::FileNotFound("x.mp4".into());
        let s = e.to_string();
        assert!(s.contains("x.mp4") && s.contains("non trovato"));
    }

    #[test]
    fn test_error_unknown_format() {
        let e = Error::UnknownFormat("xyz".into());
        let s = e.to_string();
        assert!(s.contains("xyz") && s.contains("non supportato"));
    }

    #[test]
    fn test_error_no_frames() {
        let e = Error::NoFrames("x.mp4".into());
        let s = e.to_string();
        assert!(s.contains("x.mp4") && s.contains("Nessun frame"));
    }

    #[test]
    fn test_error_terminal_too_small() {
        let e = Error::TerminalTooSmall(5, 3);
        let s = e.to_string();
        assert!(s.contains("5") && s.contains("3") && s.contains("piccolo"));
    }

    #[test]
    fn test_error_io_conversion() {
        let io = std::io::Error::new(std::io::ErrorKind::NotFound, "test");
        let e: Error = io.into();
        assert!(matches!(e, Error::IoError(_)));
    }

    // ── AltScreen ────────────────────────────────────────────────────────

    #[test]
    fn test_alt_screen_enter_drop() {
        // Just check that entering and dropping doesn't panic
        let screen = AltScreen::enter();
        drop(screen);
    }
}
