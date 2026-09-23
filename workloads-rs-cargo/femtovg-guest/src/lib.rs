//! femtovg E2E guest (docs/femtovg-e2e-abi.md).
//!
//! `fvg_init` parses one of the embedded SVG scenes with usvg once and turns
//! it into a femtovg display list, with each path's absolute transform baked
//! into its points. Every `fvg_frame` then redraws the whole list at the
//! frame's zoom (`z = 16^(1 - |2·index/(count-1) - 1|)`, about the canvas
//! center) on a `Canvas<WireRenderer<_>>` and flushes it. That flush is all of
//! femtovg's CPU work for the frame (transforms, flattening, tessellation,
//! command building) and ends in one `fvg.render` import call carrying the
//! vertex and command buffers. The host owns the GPU.
//!
//! The same code builds natively (rlib); `src/bin/fvg_reference.rs` uses it
//! to produce reference hashes of every frame's buffers.

use femtovg::{Canvas, Color, FillRule, LineCap, LineJoin, Paint, Path, Renderer};
use usvg::tiny_skia_path::{PathSegment, Transform};

pub const SCENES: [&[u8]; 2] = [
    include_bytes!("../../../workloads/femtovg/Ghostscript_Tiger.svg"),
    include_bytes!("../../../workloads/femtovg/combined-linking.svg"),
];

/// One entry of the display list.
pub enum Item {
    Draw {
        path: Path,
        fill: Option<Paint>,
        stroke: Option<Paint>,
    },
    /// `save()` + `clip_path()`, undone by the matching `Restore`.
    Clip { path: Path, rule: FillRule },
    Restore,
}

pub struct Scene {
    pub items: Vec<Item>,
    pub width: f32,
    pub height: f32,
    /// Path segments across all items (a size measure for reports).
    pub segments: usize,
}

impl Scene {
    pub fn paths(&self) -> usize {
        self.items.iter().filter(|i| matches!(i, Item::Draw { .. })).count()
    }
}

/// Parses scene `index` (0 = Ghostscript tiger, 1 = combined-linking).
pub fn load_scene(index: usize) -> Result<Scene, String> {
    let data = SCENES.get(index).ok_or("no such scene")?;
    let tree = usvg::Tree::from_data(data, &usvg::Options::default()).map_err(|e| e.to_string())?;
    let size = tree.size();
    let mut scene = Scene {
        items: Vec::new(),
        width: size.width(),
        height: size.height(),
        segments: 0,
    };
    walk(tree.root(), 1.0, &mut scene);
    Ok(scene)
}

fn walk(group: &usvg::Group, parent_opacity: f32, scene: &mut Scene) {
    let opacity = parent_opacity * group.opacity().get();
    let clip = group.clip_path().map(|cp| clip_geometry(cp, group.abs_transform(), scene));
    let clipped = clip.is_some();
    if let Some((path, rule)) = clip {
        scene.items.push(Item::Clip { path, rule });
    }
    for node in group.children() {
        match node {
            usvg::Node::Group(g) => walk(g, opacity, scene),
            usvg::Node::Path(p) if p.is_visible() => {
                let (path, n) = convert_path(p.data(), p.abs_transform());
                scene.segments += n;
                let scale = transform_scale(p.abs_transform());
                let fill = p.fill().and_then(|f| {
                    paint(f.paint(), f.opacity().get() * opacity, p.abs_transform()).map(|paint| {
                        paint.with_anti_alias(true).with_fill_rule(match f.rule() {
                            usvg::FillRule::NonZero => FillRule::NonZero,
                            usvg::FillRule::EvenOdd => FillRule::EvenOdd,
                        })
                    })
                });
                let stroke = p.stroke().and_then(|s| {
                    paint(s.paint(), s.opacity().get() * opacity, p.abs_transform()).map(|mut paint| {
                        paint.set_anti_alias(true);
                        paint.set_line_width(s.width().get() * scale);
                        paint.set_line_cap(match s.linecap() {
                            usvg::LineCap::Butt => LineCap::Butt,
                            usvg::LineCap::Round => LineCap::Round,
                            usvg::LineCap::Square => LineCap::Square,
                        });
                        paint.set_line_join(match s.linejoin() {
                            usvg::LineJoin::Miter | usvg::LineJoin::MiterClip => LineJoin::Miter,
                            usvg::LineJoin::Round => LineJoin::Round,
                            usvg::LineJoin::Bevel => LineJoin::Bevel,
                        });
                        paint.set_miter_limit(s.miterlimit().get());
                        if let Some(dash) = s.dasharray() {
                            let dash: Vec<f32> = dash.iter().map(|d| d * scale).collect();
                            paint.set_line_dash(&dash);
                            paint.set_line_dash_offset(s.dashoffset() * scale);
                        }
                        paint
                    })
                });
                if fill.is_some() || stroke.is_some() {
                    scene.items.push(Item::Draw { path, fill, stroke });
                }
            }
            // Text needs fonts (usvg's `text` feature is off) and neither
            // scene has images.
            _ => {}
        }
    }
    if clipped {
        scene.items.push(Item::Restore);
    }
}

/// A clip path's children, in canvas space, merged into one path.
fn clip_geometry(cp: &usvg::ClipPath, owner: Transform, scene: &mut Scene) -> (Path, FillRule) {
    let base = owner.pre_concat(cp.transform());
    let mut out = Path::new();
    let mut rule = FillRule::NonZero;
    for node in cp.root().children() {
        if let usvg::Node::Path(p) = node {
            let n = append_path(&mut out, p.data(), base.pre_concat(p.abs_transform()));
            scene.segments += n;
            if let Some(f) = p.fill() {
                if f.rule() == usvg::FillRule::EvenOdd {
                    rule = FillRule::EvenOdd;
                }
            }
        }
    }
    (out, rule)
}

fn convert_path(data: &usvg::tiny_skia_path::Path, ts: Transform) -> (Path, usize) {
    let mut path = Path::new();
    let n = append_path(&mut path, data, ts);
    (path, n)
}

fn append_path(path: &mut Path, data: &usvg::tiny_skia_path::Path, ts: Transform) -> usize {
    let map = |p: usvg::tiny_skia_path::Point| {
        let mut q = p;
        ts.map_point(&mut q);
        q
    };
    let mut n = 0;
    for seg in data.segments() {
        n += 1;
        match seg {
            PathSegment::MoveTo(p) => {
                let p = map(p);
                path.move_to(p.x, p.y)
            }
            PathSegment::LineTo(p) => {
                let p = map(p);
                path.line_to(p.x, p.y)
            }
            PathSegment::QuadTo(c, p) => {
                let (c, p) = (map(c), map(p));
                path.quad_to(c.x, c.y, p.x, p.y)
            }
            PathSegment::CubicTo(c1, c2, p) => {
                let (c1, c2, p) = (map(c1), map(c2), map(p));
                path.bezier_to(c1.x, c1.y, c2.x, c2.y, p.x, p.y)
            }
            PathSegment::Close => path.close(),
        }
    }
    n
}

/// Uniform scale factor of an affine transform (for stroke widths, radii).
fn transform_scale(ts: Transform) -> f32 {
    (ts.sx * ts.sy - ts.kx * ts.ky).abs().sqrt()
}

fn color(c: usvg::Color, alpha: f32) -> Color {
    Color::rgba(c.red, c.green, c.blue, (alpha.clamp(0.0, 1.0) * 255.0).round() as u8)
}

fn paint(p: &usvg::Paint, alpha: f32, ts: Transform) -> Option<Paint> {
    match p {
        usvg::Paint::Color(c) => Some(Paint::color(color(*c, alpha))),
        usvg::Paint::LinearGradient(g) => {
            let gts = ts.pre_concat(g.transform());
            let mut a = usvg::tiny_skia_path::Point::from_xy(g.x1(), g.y1());
            let mut b = usvg::tiny_skia_path::Point::from_xy(g.x2(), g.y2());
            gts.map_point(&mut a);
            gts.map_point(&mut b);
            let stops = g
                .stops()
                .iter()
                .map(|s| (s.offset().get(), color(s.color(), s.opacity().get() * alpha)));
            Some(Paint::linear_gradient_stops(a.x, a.y, b.x, b.y, stops))
        }
        usvg::Paint::RadialGradient(g) => {
            let gts = ts.pre_concat(g.transform());
            let mut c = usvg::tiny_skia_path::Point::from_xy(g.cx(), g.cy());
            gts.map_point(&mut c);
            let s = transform_scale(gts);
            let stops = g
                .stops()
                .iter()
                .map(|st| (st.offset().get(), color(st.color(), st.opacity().get() * alpha)));
            Some(Paint::radial_gradient_stops(c.x, c.y, g.fr().get() * s, g.r().get() * s, stops))
        }
        usvg::Paint::Pattern(_) => None,
    }
}

/// Zoom of frame `index` of `count`: 1× → 16× → 1× in equal log steps.
pub fn zoom(index: u32, count: u32) -> f32 {
    let t = if count > 1 { index as f32 / (count - 1) as f32 } else { 0.0 };
    16f32.powf(1.0 - (2.0 * t - 1.0).abs())
}

/// Draws frame `index` of `count` (does not flush).
pub fn draw_frame<R: Renderer>(canvas: &mut Canvas<R>, scene: &Scene, index: u32, count: u32) {
    let (w, h) = (canvas.width(), canvas.height());
    canvas.reset();
    canvas.clear_rect(0, 0, w, h, Color::rgbf(1.0, 1.0, 1.0));
    let (wf, hf) = (w as f32, h as f32);
    let fit = (wf / scene.width).min(hf / scene.height) * zoom(index, count);
    canvas.save();
    canvas.translate(wf * 0.5, hf * 0.5);
    canvas.scale(fit, fit);
    canvas.translate(-scene.width * 0.5, -scene.height * 0.5);
    for item in &scene.items {
        match item {
            Item::Draw { path, fill, stroke } => {
                if let Some(fill) = fill {
                    canvas.fill_path(path, fill);
                }
                if let Some(stroke) = stroke {
                    canvas.stroke_path(path, stroke);
                }
            }
            Item::Clip { path, rule } => {
                canvas.save();
                canvas.clip_path(path, *rule);
            }
            Item::Restore => canvas.restore(),
        }
    }
    canvas.restore();
}

/// FNV-1a 64 over `verts ‖ commands`, both as little-endian bytes: the frame
/// hash the host checks across runtimes.
pub fn frame_hash(verts: &[femtovg::renderer::Vertex], commands: &[u32]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut eat = |b: u8| {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    };
    for v in verts {
        for f in [v.x, v.y, v.u, v.v] {
            f.to_bits().to_le_bytes().into_iter().for_each(&mut eat);
        }
    }
    for w in commands {
        w.to_le_bytes().into_iter().for_each(&mut eat);
    }
    h
}

/// Command count of an encoded flush (its second word).
pub fn command_count(commands: &[u32]) -> i32 {
    commands.get(1).map_or(0, |&n| n as i32)
}

// ---------------------------------------------------------------------------
// wasm32 guest ABI
// ---------------------------------------------------------------------------

#[cfg(target_arch = "wasm32")]
mod guest {
    use super::*;
    use femtovg::{
        renderer::{WireRenderer, WireSink},
        ErrorKind, ImageInfo, PixelFormat,
    };

    #[link(wasm_import_module = "fvg")]
    extern "C" {
        #[link_name = "set_size"]
        fn fvg_set_size(width: i32, height: i32, dpi_milli: i32);
        #[link_name = "image_alloc"]
        fn fvg_image_alloc(width: i32, height: i32, format: i32, flags: i32) -> i32;
        #[link_name = "image_update"]
        fn fvg_image_update(
            handle: i32,
            x: i32,
            y: i32,
            width: i32,
            height: i32,
            format: i32,
            data_ptr: i32,
            data_len: i32,
        ) -> i32;
        #[link_name = "image_delete"]
        fn fvg_image_delete(handle: i32);
        #[link_name = "render"]
        fn fvg_render(verts_ptr: i32, vert_count: i32, cmds_ptr: i32, cmds_len: i32);
    }

    fn format_code(f: PixelFormat) -> i32 {
        match f {
            PixelFormat::Rgb8 => 0,
            PixelFormat::Rgba8 => 1,
            PixelFormat::Gray8 => 2,
        }
    }

    /// The `fvg` imports as a [`WireSink`]. femtovg's Canvas does not hand
    /// its renderer back, so the flush's command count goes out through a
    /// static for `fvg_frame` to return.
    pub struct Imports;

    static LAST_COMMANDS: core::sync::atomic::AtomicI32 = core::sync::atomic::AtomicI32::new(0);

    impl WireSink for Imports {
        fn set_size(&mut self, width: u32, height: u32, dpi: f32) {
            unsafe { fvg_set_size(width as i32, height as i32, (dpi * 1000.0).round() as i32) }
        }
        fn image_alloc(&mut self, info: ImageInfo) -> Result<u32, ErrorKind> {
            let h = unsafe {
                fvg_image_alloc(
                    info.width() as i32,
                    info.height() as i32,
                    format_code(info.format()),
                    info.flags().bits() as i32,
                )
            };
            if h < 0 {
                Err(ErrorKind::GeneralError("fvg.image_alloc failed".into()))
            } else {
                Ok(h as u32)
            }
        }
        fn image_update(
            &mut self,
            handle: u32,
            x: usize,
            y: usize,
            width: usize,
            height: usize,
            format: PixelFormat,
            data: &[u8],
        ) -> Result<(), ErrorKind> {
            let rc = unsafe {
                fvg_image_update(
                    handle as i32,
                    x as i32,
                    y as i32,
                    width as i32,
                    height as i32,
                    format_code(format),
                    data.as_ptr() as i32,
                    data.len() as i32,
                )
            };
            if rc == 0 {
                Ok(())
            } else {
                Err(ErrorKind::GeneralError("fvg.image_update failed".into()))
            }
        }
        fn image_delete(&mut self, handle: u32) {
            unsafe { fvg_image_delete(handle as i32) }
        }
        fn render(&mut self, verts: &[femtovg::renderer::Vertex], commands: &[u32]) {
            LAST_COMMANDS.store(command_count(commands), core::sync::atomic::Ordering::Relaxed);
            unsafe {
                fvg_render(
                    verts.as_ptr() as i32,
                    verts.len() as i32,
                    commands.as_ptr() as i32,
                    commands.len() as i32,
                )
            }
        }
    }

    struct State {
        canvas: Canvas<WireRenderer<Imports>>,
        scene: Scene,
    }

    static mut STATE: Option<State> = None;

    #[allow(static_mut_refs)]
    fn state() -> Option<&'static mut State> {
        unsafe { STATE.as_mut() }
    }

    /// Parses scene `scene` and sets up a `width` × `height` canvas. Returns
    /// the number of drawn paths, or < 0 on error.
    #[no_mangle]
    pub extern "C" fn fvg_init(scene: i32, width: i32, height: i32) -> i32 {
        let Ok(scene) = load_scene(scene as usize) else { return -1 };
        let Ok(mut canvas) = Canvas::new(WireRenderer::new(Imports)) else {
            return -2;
        };
        canvas.set_size(width as u32, height as u32, 1.0);
        let paths = scene.paths() as i32;
        unsafe { STATE = Some(State { canvas, scene }) };
        paths
    }

    /// Draws and flushes frame `index` of `count`. Returns the flush's
    /// command count, or < 0 if `fvg_init` has not run.
    #[no_mangle]
    pub extern "C" fn fvg_frame(index: i32, count: i32) -> i32 {
        let Some(st) = state() else { return -1 };
        draw_frame(&mut st.canvas, &st.scene, index as u32, count as u32);
        st.canvas.flush();
        LAST_COMMANDS.load(core::sync::atomic::Ordering::Relaxed)
    }

    /// Current linear-memory size in 64 KiB pages.
    #[no_mangle]
    pub extern "C" fn fvg_mem_pages() -> i32 {
        core::arch::wasm32::memory_size(0) as i32
    }

}
