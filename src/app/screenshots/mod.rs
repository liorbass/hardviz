//! README screenshots, rendered headlessly from demo data.
//!
//! ```text
//! cargo test --lib readme_screenshots -- --ignored
//! ```
//!
//! writes every image in `docs/images/`. Nothing opens a window or reads
//! this machine's sensors: the app runs on a thread-less poller fed by
//! [`demo`] sources, and frames are rasterised on the CPU by [`raster`].

// Pixel and sample arithmetic: coordinates, sizes and channel values move
// between float and integer types constantly, always within small known
// ranges; triangle maths reads best with the textbook single-letter names.
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    clippy::cast_possible_wrap,
    clippy::many_single_char_names,
    clippy::float_cmp
)]

mod demo;
mod raster;
mod terminal;

use super::HardVizApp;
use crate::config::{AppConfig, ViewMode};
use crate::metrics::HISTORY_LEN;
use crate::poller::Poller;
use crate::theme::ThemeName;
use crate::widgets::default_registry;
use eframe::egui;
use raster::{Canvas, Textures};
use std::path::{Path, PathBuf};

/// Logical size of the main window in the shots.
const WINDOW: egui::Vec2 = egui::vec2(1400.0, 960.0);
/// Logical size of the compact overlay (as `window::COMPACT_SIZE`).
const COMPACT: egui::Vec2 = egui::vec2(640.0, 96.0);
/// Render at this many pixels per point, then box-filter down.
const SUPERSAMPLE: usize = 2;

/// What one screenshot shows.
struct Shot {
    theme: ThemeName,
    mode: ViewMode,
    edit_layout: bool,
    settings: bool,
}

impl Shot {
    fn dashboard(theme: ThemeName) -> Self {
        Self { theme, mode: ViewMode::Windowed, edit_layout: false, settings: false }
    }
}

/// An app on demo data with a full history, set up as `shot` describes.
fn demo_app(ctx: &egui::Context, shot: &Shot) -> HardVizApp {
    let registry = default_registry();
    let mut config = AppConfig::with_default_widgets(&registry.default_layout());
    config.theme = shot.theme;
    config.view_mode = shot.mode;
    config.widgets_locked = !shot.edit_layout;
    let poller = Poller::manual(demo::sources());
    for _ in 0..HISTORY_LEN {
        poller.sample_now();
    }
    poller.set_processes(demo::processes());
    let mut app = HardVizApp::with_poller(ctx, config, poller);
    app.show_settings = shot.settings;
    app
}

/// Run `app` until its gauges have settled, then rasterise the last frame
/// at `scale` output pixels per point.
fn render(app: &mut HardVizApp, ctx: &egui::Context, size: egui::Vec2, scale: f32) -> Canvas {
    render_ui(ctx, size, scale, |ui| app.frame(ui))
}

/// Run `ui` for enough frames to settle, then rasterise the last one.
fn render_ui(ctx: &egui::Context, size: egui::Vec2, scale: f32, mut ui: impl FnMut(&mut egui::Ui)) -> Canvas {
    let ppp = scale * SUPERSAMPLE as f32;
    ctx.set_pixels_per_point(ppp);
    let mut textures = Textures::default();
    let mut output = None;
    for frame in 0..60 {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
            time: Some(f64::from(frame) * 0.05),
            predicted_dt: 0.05,
            ..Default::default()
        };
        let mut out = ctx.run_ui(input, &mut ui);
        textures.update(&out.textures_delta);
        out.textures_delta.clear();
        output = Some(out);
    }
    let out = output.expect("at least one frame");
    let prims = ctx.tessellate(out.shapes, out.pixels_per_point);
    let mut canvas = Canvas::new((size.x * ppp).round() as usize, (size.y * ppp).round() as usize);
    canvas.draw(&prims, &textures, out.pixels_per_point);
    canvas.downsample(SUPERSAMPLE)
}

/// Output frames in the animated demos; at `ANIM_FPS` this is a six-second loop.
const ANIM_FRAMES: usize = 72;
/// Frames per second, both for the demo clock and for the GIF made from them.
const ANIM_FPS: f64 = 12.0;

/// The surface an animation draws into. The compact strip is cropped to its
/// contents; the windowed dashboard is not.
struct Scene {
    size: egui::Vec2,
    ppp: f32,
    compact: bool,
}

impl Scene {
    fn new(shot: &Shot, scale: f32) -> Self {
        let compact = matches!(shot.mode, ViewMode::Compact);
        Self { size: if compact { COMPACT } else { WINDOW }, ppp: scale * SUPERSAMPLE as f32, compact }
    }
}

/// Run one frame of `app`, writing it to `path` when given.
fn tick(
    app: &mut HardVizApp,
    ctx: &egui::Context,
    textures: &mut Textures,
    scene: &Scene,
    t: f64,
    path: Option<&Path>,
) {
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, scene.size)),
        time: Some(t),
        predicted_dt: (1.0 / ANIM_FPS) as f32,
        ..Default::default()
    };
    let mut out = ctx.run_ui(input, &mut |ui: &mut egui::Ui| app.frame(ui));
    textures.update(&out.textures_delta);
    out.textures_delta.clear();
    let Some(path) = path else { return };
    let prims = ctx.tessellate(out.shapes, out.pixels_per_point);
    let mut canvas =
        Canvas::new((scene.size.x * scene.ppp).round() as usize, (scene.size.y * scene.ppp).round() as usize);
    canvas.draw(&prims, textures, out.pixels_per_point);
    let canvas = canvas.downsample(SUPERSAMPLE);
    let bounds = if scene.compact { canvas.content_bounds(6) } else { [0, 0, canvas.width, canvas.height] };
    canvas.save_png(path, bounds).unwrap_or_else(|e| panic!("writing {}: {e}", path.display()));
}

/// Render `shot` as a stream of PNG frames in `dir`, one demo sample per frame.
///
/// Unlike [`shoot`], the demo machine keeps polling while the frames are
/// captured, so the charts scroll and the gauges tween as they do on screen.
fn animate(shot: &Shot, dir: &Path, scale: f32) {
    let ctx = egui::Context::default();
    let mut app = demo_app(&ctx, shot);
    let scene = Scene::new(shot, scale);
    ctx.set_pixels_per_point(scene.ppp);
    let mut textures = Textures::default();
    std::fs::create_dir_all(dir).unwrap_or_else(|e| panic!("creating {}: {e}", dir.display()));
    let dt = 1.0 / ANIM_FPS;
    // Settle the layout and the gauge tweens before capturing anything.
    for i in 0..60u32 {
        tick(&mut app, &ctx, &mut textures, &scene, f64::from(i) * dt, None);
    }
    for n in 0..ANIM_FRAMES {
        app.poller.sample_now();
        let t = f64::from((60 + n) as u32) * dt;
        tick(&mut app, &ctx, &mut textures, &scene, t, Some(&dir.join(format!("frame_{n:03}.png"))));
    }
    println!("wrote {ANIM_FRAMES} frames to {}", dir.display());
}

fn shoot(shot: &Shot, path: &Path, scale: f32) {
    let ctx = egui::Context::default();
    let mut app = demo_app(&ctx, shot);
    let compact = matches!(shot.mode, ViewMode::Compact);
    let canvas = render(&mut app, &ctx, if compact { COMPACT } else { WINDOW }, scale);
    // The compact strip floats in its window; keep just the strip.
    let bounds = if compact { canvas.content_bounds(6) } else { [0, 0, canvas.width, canvas.height] };
    canvas.save_png(path, bounds).unwrap_or_else(|e| panic!("writing {}: {e}", path.display()));
    println!("wrote {}", path.display());
}

fn images_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("docs").join("images")
}

#[test]
#[ignore = "writes docs/images; run with `cargo test --lib readme_screenshots -- --ignored`"]
fn readme_screenshots() {
    let dir = images_dir();
    std::fs::create_dir_all(&dir).expect("create docs/images");
    shoot(&Shot::dashboard(ThemeName::MidnightDark), &dir.join("dashboard.png"), 1.0);
    shoot(
        &Shot { theme: ThemeName::Cyberpunk, mode: ViewMode::Windowed, edit_layout: true, settings: false },
        &dir.join("edit-layout.png"),
        1.0,
    );
    shoot(
        &Shot { theme: ThemeName::PureLight, mode: ViewMode::Windowed, edit_layout: false, settings: true },
        &dir.join("settings.png"),
        1.0,
    );
    shoot(
        &Shot { theme: ThemeName::Solarized, mode: ViewMode::Compact, edit_layout: false, settings: false },
        &dir.join("compact.png"),
        1.5,
    );
    terminal::shoot(ThemeName::OledBlack, &dir.join("tui.png"));
    // Theme gallery: half-size dashboards.
    for theme in ThemeName::ALL {
        let name = format!("theme-{}.png", theme.label().to_lowercase().replace(' ', "-"));
        shoot(&Shot::dashboard(theme), &dir.join(name), 0.5);
    }
}

/// Where `readme_animation` puts its frames: build output, not a git artifact.
fn frames_dir(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("target").join("readme-anim").join(name)
}

/// Renders the streaming frames behind the README's animated demos.
///
/// Run it in release (the CPU rasteriser is the bottleneck, ~1 min for both),
/// then turn the frames into the GIFs in `docs/images/` with `ffmpeg`. The two
/// passes matter: a single pass quantises each frame on its own and the
/// Cyberpunk gradients band.
///
/// ```sh
/// cargo test --release --lib readme_animation -- --ignored
///
/// # 1200px wide, 12 fps: 3.8 MB for the 6-second dashboard loop.
/// F=target/readme-anim/dashboard
/// ffmpeg -y -framerate 12 -i $F/frame_%03d.png -vf "scale=1200:-2:flags=lanczos,palettegen" $F/pal.png
/// ffmpeg -y -framerate 12 -i $F/frame_%03d.png -i $F/pal.png \
///   -lavfi "scale=1200:-2:flags=lanczos[x];[x][1:v]paletteuse=dither=bayer:bayer_scale=5:diff_mode=rectangle" \
///   docs/images/demo-dashboard.gif
///
/// # The strip is small enough to need no scaling: 37 KB.
/// C=target/readme-anim/compact
/// ffmpeg -y -framerate 12 -i $C/frame_%03d.png \
///   -lavfi "[0:v]split[a][b];[a]palettegen[pal];[b][pal]paletteuse=dither=bayer:bayer_scale=5" \
///   docs/images/demo-compact.gif
/// ```
#[test]
#[ignore = "writes target/readme-anim; run with `cargo test --lib readme_animation -- --ignored`"]
fn readme_animation() {
    animate(&Shot::dashboard(ThemeName::Cyberpunk), &frames_dir("dashboard"), 1.0);
    animate(
        &Shot { theme: ThemeName::Cyberpunk, mode: ViewMode::Compact, edit_layout: false, settings: false },
        &frames_dir("compact"),
        1.5,
    );
}

#[test]
fn a_demo_frame_renders_headlessly() {
    let ctx = egui::Context::default();
    let mut app = demo_app(&ctx, &Shot::dashboard(ThemeName::MidnightDark));
    let canvas = render(&mut app, &ctx, egui::vec2(400.0, 300.0), 0.5);
    assert_eq!((canvas.width, canvas.height), (200, 150));
    // Cards and text: far from a single flat colour.
    let [x0, y0, x1, y1] = canvas.content_bounds(0);
    assert!(x1 - x0 > 100 && y1 - y0 > 50, "nothing drawn: {:?}", [x0, y0, x1, y1]);
}
