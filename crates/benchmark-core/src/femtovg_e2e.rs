//! femtovg E2E host (docs/femtovg-e2e-abi.md).
//!
//! One runtime per process: the guest (`workloads/femtovg-*.wasm`) does all
//! of femtovg's CPU work per frame inside the chosen interpreter and sends
//! the vertex + command buffers out through the `fvg.render` import. This
//! host decodes them with femtovg's `WireReplayer` onto a real
//! `WGPURenderer` (Metal, headless, offscreen RGBA8 texture), submits, and
//! waits for the GPU every frame, so each frame is complete when its time
//! is taken:
//!
//!   t_guest   `fvg_frame()` wall time (femtovg CPU work in the guest, plus
//!             copying the two buffers out of linear memory in the import)
//!   t_encode  decode + `WGPURenderer::render` + `queue.submit`
//!   t_gpu     `device.poll(Wait)` for that submission
//!
//! Frame time is the sum. Every frame's buffers are hashed (FNV-1a 64 over
//! `verts ‖ commands`) so a runtime that computes a different frame is
//! caught, and the final frame's texture is read back, hashed and (on
//! request) written as a PNG.
//!
//! Runtime bindings for the five `fvg` imports live next to each runtime's
//! FFI (`femtovg_guest` in wamr.rs, wasm3.rs, ...); Pulley's is here. They
//! all call the `host_*` functions below with slices of guest memory.

use std::cell::RefCell;
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context, Result};
use femtovg::renderer::{Vertex, WGPURenderer, WireReplayer};
use femtovg::{ImageFlags, ImageInfo, PixelFormat};

use crate::{residency, Runtime};

pub const FEMTOVG_RELAXED_WASM: &[u8] =
    include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../workloads/femtovg-relaxed.wasm"));
pub const FEMTOVG_SIMD128_WASM: &[u8] =
    include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../workloads/femtovg-simd128.wasm"));
pub const FEMTOVG_SCALAR_WASM: &[u8] =
    include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../workloads/femtovg-scalar.wasm"));

/// Guest build. `Relaxed` is compiled with `+relaxed-simd` but contains no
/// relaxed-SIMD instruction (rustc never forms them without intrinsics), so
/// it is the `Simd128` code; both are kept to show that.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Variant {
    Relaxed,
    Simd128,
    Scalar,
}

impl Variant {
    pub fn name(self) -> &'static str {
        match self {
            Variant::Relaxed => "relaxed",
            Variant::Simd128 => "simd128",
            Variant::Scalar => "scalar",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "relaxed" => Some(Variant::Relaxed),
            "simd128" => Some(Variant::Simd128),
            "scalar" => Some(Variant::Scalar),
            _ => None,
        }
    }

    pub fn wasm(self) -> &'static [u8] {
        match self {
            Variant::Relaxed => FEMTOVG_RELAXED_WASM,
            Variant::Simd128 => FEMTOVG_SIMD128_WASM,
            Variant::Scalar => FEMTOVG_SCALAR_WASM,
        }
    }

    /// The best build `rt` runs: runtimes without an interpreter SIMD-128
    /// path (wasm3, zwasm) and wasmz (which crashes on v128) get the scalar
    /// build; the rest run the SIMD-128 build.
    pub fn best_for(rt: Runtime) -> Self {
        match rt {
            Runtime::Wasm3 | Runtime::Zwasm | Runtime::Wasmz => Variant::Scalar,
            _ => Variant::Simd128,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct E2eConfig {
    pub scene: u32,
    pub frames: u32,
    pub width: u32,
    pub height: u32,
    /// Full passes over the zoom schedule; all but the last are warmup
    /// (pipeline creation, first-touch allocation) and are not reported.
    pub passes: u32,
}

impl Default for E2eConfig {
    fn default() -> Self {
        E2eConfig { scene: 0, frames: 121, width: 1024, height: 1024, passes: 2 }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct FrameStats {
    pub t_guest: Duration,
    pub t_encode: Duration,
    pub t_gpu: Duration,
    pub verts: u32,
    pub commands: i32,
    pub hash: u64,
}

impl FrameStats {
    pub fn total(&self) -> Duration {
        self.t_guest + self.t_encode + self.t_gpu
    }
}

#[derive(Debug)]
pub struct E2eReport {
    pub runtime: Runtime,
    pub variant: Variant,
    pub config: E2eConfig,
    pub paths: i32,
    pub load_time: Duration,
    pub init_time: Duration,
    /// The measured (last) pass.
    pub frames: Vec<FrameStats>,
    /// FNV-1a over the measured pass's frame hashes, in order.
    pub all_hash: u64,
    pub max_mem_pages: i32,
    pub phys_footprint_peak: u64,
    pub final_texture_hash: u64,
    /// CPU accounting over the measured pass (whole process).
    pub cpu_ns: u64,
    pub p_cpu_ns: u64,
    pub instructions: u64,
    pub cycles: u64,
    pub adapter: String,
}

/// The guest's three exports, bound in some runtime.
pub trait Guest {
    fn init(&mut self, scene: i32, width: i32, height: i32) -> Result<i32>;
    fn frame(&mut self, index: i32, count: i32) -> Result<i32>;
    fn mem_pages(&mut self) -> Result<i32>;
}

// ---------------------------------------------------------------------------
// Host state shared with the import callbacks
// ---------------------------------------------------------------------------

struct HostState {
    replayer: WireReplayer<WGPURenderer>,
    verts: Vec<Vertex>,
    commands: Vec<u32>,
    hash: u64,
    rendered: bool,
}

thread_local! {
    static HOST: RefCell<Option<HostState>> = const { RefCell::new(None) };
}

fn with_host<T>(f: impl FnOnce(&mut HostState) -> T) -> Option<T> {
    HOST.with(|h| h.borrow_mut().as_mut().map(f))
}

pub(crate) fn host_set_size(width: i32, height: i32, dpi_milli: i32) {
    with_host(|h| h.replayer.set_size(width as u32, height as u32, dpi_milli as f32 / 1000.0));
}

pub(crate) fn host_image_alloc(width: i32, height: i32, format: i32, flags: i32) -> i32 {
    let Some(format) = pixel_format(format) else { return -1 };
    let info = ImageInfo::new(
        ImageFlags::from_bits_truncate(flags as u32),
        width.max(0) as usize,
        height.max(0) as usize,
        format,
    );
    with_host(|h| h.replayer.image_alloc(info).map(|x| x as i32).unwrap_or(-1)).unwrap_or(-1)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn host_image_update(
    handle: i32,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    format: i32,
    data: &[u8],
) -> i32 {
    let Some(format) = pixel_format(format) else { return -1 };
    with_host(|h| {
        h.replayer
            .image_update(handle as u32, x as usize, y as usize, width as usize, height as usize, format, data)
            .map_or(-1, |_| 0)
    })
    .unwrap_or(-1)
}

pub(crate) fn host_image_delete(handle: i32) {
    with_host(|h| h.replayer.image_delete(handle as u32));
}

/// `fvg.render`: copy both spans out of linear memory and return; decode,
/// encode and submit happen after `fvg_frame` returns.
pub(crate) fn host_render(verts: &[u8], commands: &[u8]) {
    // FVG_DUMP=path: write the first frame's raw buffers (for comparing a
    // runtime's output against the native reference byte by byte).
    if let Ok(path) = std::env::var("FVG_DUMP") {
        if !std::path::Path::new(&path).exists() {
            let _ = std::fs::write(&path, [verts, commands].concat());
        }
    }
    with_host(|h| {
        h.hash = fnv1a(fnv1a(FNV_OFFSET, verts), commands);
        h.verts.resize(verts.len() / std::mem::size_of::<Vertex>(), Vertex::default());
        // SAFETY: Vertex is four f32 (repr(C), no padding); the destination
        // holds exactly verts.len() bytes.
        unsafe {
            std::ptr::copy_nonoverlapping(verts.as_ptr(), h.verts.as_mut_ptr() as *mut u8, verts.len());
        }
        h.commands.clear();
        h.commands
            .extend(commands.chunks_exact(4).map(|w| u32::from_le_bytes([w[0], w[1], w[2], w[3]])));
        h.rendered = true;
    });
}

/// Bounds-checked `[ptr, ptr + len)` of a linear memory.
pub(crate) fn guest_span(mem: &[u8], ptr: i32, len: usize) -> Option<&[u8]> {
    let start = u32::try_from(ptr).ok()? as usize;
    mem.get(start..start.checked_add(len)?)
}

/// `(module, name)` of every import, in import-section order, for runtimes
/// that bind imports positionally. Only function imports are expected.
pub(crate) fn import_names(wasm: &[u8]) -> Result<Vec<(String, String)>> {
    fn uleb(b: &[u8], pos: &mut usize) -> Result<u32> {
        let (mut v, mut shift) = (0u32, 0);
        loop {
            let byte = *b.get(*pos).ok_or_else(|| anyhow!("truncated LEB128"))?;
            *pos += 1;
            v |= u32::from(byte & 0x7f) << shift;
            if byte & 0x80 == 0 {
                return Ok(v);
            }
            shift += 7;
        }
    }
    fn name(b: &[u8], pos: &mut usize) -> Result<String> {
        let n = uleb(b, pos)? as usize;
        let s = b.get(*pos..*pos + n).ok_or_else(|| anyhow!("truncated name"))?;
        *pos += n;
        Ok(String::from_utf8_lossy(s).into_owned())
    }
    let mut pos = 8;
    while pos < wasm.len() {
        let id = wasm[pos];
        pos += 1;
        let size = uleb(wasm, &mut pos)? as usize;
        let end = pos + size;
        if id == 2 {
            let n = uleb(wasm, &mut pos)?;
            let mut out = Vec::with_capacity(n as usize);
            for _ in 0..n {
                let module = name(wasm, &mut pos)?;
                let field = name(wasm, &mut pos)?;
                let kind = *wasm.get(pos).ok_or_else(|| anyhow!("truncated import"))?;
                pos += 1;
                if kind != 0 {
                    bail!("import {module}.{field} is not a function (kind {kind})");
                }
                uleb(wasm, &mut pos)?;
                out.push((module, field));
            }
            return Ok(out);
        }
        pos = end;
    }
    Ok(Vec::new())
}

fn pixel_format(code: i32) -> Option<PixelFormat> {
    match code {
        0 => Some(PixelFormat::Rgb8),
        1 => Some(PixelFormat::Rgba8),
        2 => Some(PixelFormat::Gray8),
        _ => None,
    }
}

const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

fn fnv1a(mut h: u64, bytes: &[u8]) -> u64 {
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(FNV_PRIME);
    }
    h
}

// ---------------------------------------------------------------------------
// GPU
// ---------------------------------------------------------------------------

struct Gpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
    target: wgpu::Texture,
    adapter: String,
}

fn gpu(width: u32, height: u32) -> Result<Gpu> {
    let mut desc = wgpu::InstanceDescriptor::new_without_display_handle();
    desc.backends = wgpu::Backends::METAL;
    let instance = wgpu::Instance::new(desc);
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::default(),
        force_fallback_adapter: false,
        compatible_surface: None,
        ..Default::default()
    }))
    .map_err(|e| anyhow!("no Metal adapter: {e}"))?;
    let info = adapter.get_info();
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("femtovg e2e"),
        required_features: wgpu::Features::empty(),
        required_limits: wgpu::Limits::downlevel_defaults().using_resolution(adapter.limits()),
        experimental_features: wgpu::ExperimentalFeatures::disabled(),
        memory_hints: wgpu::MemoryHints::MemoryUsage,
        trace: wgpu::Trace::default(),
    }))
    .map_err(|e| anyhow!("request_device: {e}"))?;
    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("femtovg e2e target"),
        size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    Ok(Gpu { device, queue, target, adapter: format!("{} ({:?})", info.name, info.backend) })
}

/// Reads the target texture back as tightly packed RGBA8 rows.
fn read_back(g: &Gpu, width: u32, height: u32) -> Result<Vec<u8>> {
    let unpadded = width * 4;
    let padded = unpadded.div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let buf = g.device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: (padded * height) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut enc = g.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
    enc.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: &g.target,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &buf,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded),
                rows_per_image: Some(height),
            },
        },
        wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
    );
    g.queue.submit([enc.finish()]);
    let slice = buf.slice(..);
    let (tx, rx) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |r| {
        let _ = tx.send(r);
    });
    g.device
        .poll(wgpu::PollType::wait_indefinitely())
        .map_err(|e| anyhow!("poll: {e}"))?;
    rx.recv().context("map_async callback")?.map_err(|e| anyhow!("map: {e}"))?;
    let mapped = slice.get_mapped_range().map_err(|e| anyhow!("mapped range: {e:?}"))?;
    let mut out = vec![0u8; (unpadded * height) as usize];
    for row in 0..height as usize {
        let s = row * padded as usize;
        let d = row * unpadded as usize;
        out[d..d + unpadded as usize].copy_from_slice(&mapped[s..s + unpadded as usize]);
    }
    drop(mapped);
    buf.unmap();
    Ok(out)
}

// ---------------------------------------------------------------------------
// Driver
// ---------------------------------------------------------------------------

/// Instantiates the guest in `rt`, binding the `fvg` imports to the host
/// functions above. Must be called with the HostState installed.
fn instantiate(rt: Runtime, wasm: &'static [u8]) -> Result<Box<dyn Guest>> {
    match rt {
        Runtime::Pulley => pulley_guest(wasm),
        Runtime::Wamr => crate::wamr::femtovg_guest(wasm),
        Runtime::Wasm3 => crate::wasm3::femtovg_guest(wasm),
        Runtime::WasmEdge => crate::wasmedge::femtovg_guest(wasm),
        Runtime::Zwasm => crate::zwasm::femtovg_guest(wasm),
        Runtime::Wasmz => crate::wasmz::femtovg_guest(wasm),
        Runtime::Tinywasm => crate::tinywasm::femtovg_guest(wasm),
    }
}

/// Runs the E2E on `rt` with guest build `variant`. `png` (if set) receives
/// the final frame of the measured pass.
///
/// Everything happens on one dedicated 16 MiB-stack thread with the
/// caller's QoS class: the import callbacks find the host state in a
/// thread-local, and zwasm's and wasmz's load paths overflow small (GCD
/// worker) stacks.
pub fn run_e2e(rt: Runtime, variant: Variant, cfg: E2eConfig, png: Option<&std::path::Path>)
    -> Result<E2eReport>
{
    let png = png.map(|p| p.to_path_buf());
    crate::run_on_thread("femtovg-e2e", 16 * 1024 * 1024, move || {
        run_e2e_on_this_thread(rt, variant, cfg, png.as_deref())
    })?
}

fn run_e2e_on_this_thread(rt: Runtime, variant: Variant, cfg: E2eConfig, png: Option<&std::path::Path>)
    -> Result<E2eReport>
{
    let g = gpu(cfg.width, cfg.height)?;
    HOST.with(|h| {
        *h.borrow_mut() = Some(HostState {
            replayer: WireReplayer::new(WGPURenderer::new(g.device.clone(), g.queue.clone())),
            verts: Vec::new(),
            commands: Vec::new(),
            hash: 0,
            rendered: false,
        })
    });
    struct Uninstall;
    impl Drop for Uninstall {
        fn drop(&mut self) {
            HOST.with(|h| h.borrow_mut().take());
        }
    }
    let _uninstall = Uninstall;

    let load_start = Instant::now();
    let mut guest = instantiate(rt, variant.wasm()).context("instantiate guest")?;
    let load_time = load_start.elapsed();
    let init_start = Instant::now();
    let paths = guest
        .init(cfg.scene as i32, cfg.width as i32, cfg.height as i32)
        .context("fvg_init")?;
    let init_time = init_start.elapsed();
    if paths < 0 {
        bail!("fvg_init returned {paths}");
    }

    let mut max_pages = guest.mem_pages()?;
    let mut frames = Vec::with_capacity(cfg.frames as usize);
    let mut usage_before = None;
    for pass in 0..cfg.passes.max(1) {
        let measured = pass + 1 == cfg.passes.max(1);
        if measured {
            usage_before = residency::proc_usage();
        }
        for i in 0..cfg.frames {
            with_host(|h| h.rendered = false);
            let t0 = Instant::now();
            let n = guest.frame(i as i32, cfg.frames as i32).with_context(|| format!("fvg_frame({i})"))?;
            let t_guest = t0.elapsed();
            if n < 0 {
                bail!("fvg_frame({i}) returned {n}");
            }
            let t1 = Instant::now();
            let (submission, verts, hash) = with_host(|h| -> Result<_> {
                if !h.rendered {
                    bail!("fvg_frame({i}) did not call fvg.render");
                }
                let cb = h
                    .replayer
                    .render(&g.target, &h.verts, &h.commands)
                    .map_err(|e| anyhow!("wire decode/render: {e:?}"))?;
                Ok((cb.map(|cb| g.queue.submit([cb])), h.verts.len() as u32, h.hash))
            })
            .ok_or_else(|| anyhow!("host state missing"))??;
            let t_encode = t1.elapsed();
            let t2 = Instant::now();
            if let Some(idx) = submission {
                g.device
                    .poll(wgpu::PollType::Wait { submission_index: Some(idx), timeout: None })
                    .map_err(|e| anyhow!("poll: {e}"))?;
            }
            let t_gpu = t2.elapsed();
            if measured {
                frames.push(FrameStats { t_guest, t_encode, t_gpu, verts, commands: n, hash });
            }
        }
        max_pages = max_pages.max(guest.mem_pages()?);
    }
    let usage = match (residency::proc_usage(), usage_before) {
        (Some(a), Some(b)) => a.since(&b),
        _ => residency::ProcUsage::default(),
    };
    let pixels = read_back(&g, cfg.width, cfg.height)?;
    let final_texture_hash = fnv1a(FNV_OFFSET, &pixels);
    if let Some(path) = png {
        write_png(path, cfg.width, cfg.height, &pixels).with_context(|| format!("write {}", path.display()))?;
    }
    let all_hash = frames.iter().fold(FNV_OFFSET, |a, f| (a ^ f.hash).wrapping_mul(FNV_PRIME));
    let phys_footprint_peak = residency::phys_footprint().map(|(_, peak)| peak).unwrap_or(0);
    Ok(E2eReport {
        runtime: rt,
        variant,
        config: cfg,
        paths,
        load_time,
        init_time,
        frames,
        all_hash,
        max_mem_pages: max_pages,
        phys_footprint_peak,
        final_texture_hash,
        cpu_ns: usage.cpu_ns,
        p_cpu_ns: usage.p_cpu_ns,
        instructions: usage.instructions,
        cycles: usage.cycles,
        adapter: g.adapter.clone(),
    })
}

fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return f64::NAN;
    }
    let idx = ((sorted.len() as f64 - 1.0) * p).round() as usize;
    sorted[idx.min(sorted.len() - 1)]
}

impl E2eReport {
    /// One JSON object: config, FPS (mean over the pass, and 1 / median frame
    /// time), frame-time percentiles, the three timer means, memory, hashes.
    pub fn to_json(&self) -> String {
        let ms = |d: Duration| d.as_secs_f64() * 1e3;
        let mut totals: Vec<f64> = self.frames.iter().map(|f| ms(f.total())).collect();
        totals.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let sum: f64 = totals.iter().sum();
        let n = totals.len().max(1) as f64;
        let mean = |f: fn(&FrameStats) -> Duration| self.frames.iter().map(|x| ms(f(x))).sum::<f64>() / n;
        let median = percentile(&totals, 0.5);
        let e_share = if self.cpu_ns > 0 { 1.0 - (self.p_cpu_ns as f64 / self.cpu_ns as f64).min(1.0) } else { f64::NAN };
        let frame_hashes: Vec<String> = self.frames.iter().map(|f| format!("\"{:016x}\"", f.hash)).collect();
        let frame_verts: Vec<String> = self.frames.iter().map(|f| f.verts.to_string()).collect();
        let frame_cmds: Vec<String> = self.frames.iter().map(|f| f.commands.to_string()).collect();
        format!(
            "{{\"runtime\":\"{}\",\"variant\":\"{}\",\"scene\":{},\"frames\":{},\"width\":{},\"height\":{},\
             \"paths\":{},\"load_ms\":{:.3},\"init_ms\":{:.3},\"fps_mean\":{:.3},\"fps_median\":{:.3},\
             \"frame_ms_p50\":{:.3},\"frame_ms_p95\":{:.3},\"frame_ms_p99\":{:.3},\"frame_ms_max\":{:.3},\
             \"guest_ms_mean\":{:.3},\"encode_ms_mean\":{:.3},\"gpu_ms_mean\":{:.3},\
             \"verts_total\":{},\"commands_total\":{},\"max_mem_pages\":{},\"linear_memory_peak_bytes\":{},\
             \"phys_footprint_peak_bytes\":{},\"cpu_ms\":{:.3},\"e_share\":{:.4},\"ipc\":{:.3},\
             \"all_hash\":\"{:016x}\",\"final_texture_hash\":\"{:016x}\",\"adapter\":{:?},\"frame_hashes\":[{}],\
             \"frame_verts\":[{}],\"frame_commands\":[{}]}}",
            crate::cases::runtime_token(self.runtime),
            self.variant.name(),
            self.config.scene,
            self.frames.len(),
            self.config.width,
            self.config.height,
            self.paths,
            ms(self.load_time),
            ms(self.init_time),
            n / (sum / 1e3),
            1e3 / median,
            median,
            percentile(&totals, 0.95),
            percentile(&totals, 0.99),
            totals.last().copied().unwrap_or(f64::NAN),
            mean(|f| f.t_guest),
            mean(|f| f.t_encode),
            mean(|f| f.t_gpu),
            self.frames.iter().map(|f| f.verts as u64).sum::<u64>(),
            self.frames.iter().map(|f| f.commands as i64).sum::<i64>(),
            self.max_mem_pages,
            self.max_mem_pages as u64 * 65536,
            self.phys_footprint_peak,
            self.cpu_ns as f64 / 1e6,
            e_share,
            if self.cycles > 0 { self.instructions as f64 / self.cycles as f64 } else { f64::NAN },
            self.all_hash,
            self.final_texture_hash,
            self.adapter,
            frame_hashes.join(","),
            frame_verts.join(","),
            frame_cmds.join(","),
        )
    }
}

/// Minimal PNG writer (stored deflate blocks), for the final-frame check.
fn write_png(path: &std::path::Path, width: u32, height: u32, rgba: &[u8]) -> std::io::Result<()> {
    fn crc32(data: &[u8]) -> u32 {
        let mut c = 0xffff_ffffu32;
        for &b in data {
            c ^= b as u32;
            for _ in 0..8 {
                c = if c & 1 != 0 { 0xedb8_8320 ^ (c >> 1) } else { c >> 1 };
            }
        }
        !c
    }
    fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        let mut body = kind.to_vec();
        body.extend_from_slice(data);
        out.extend_from_slice(&body);
        out.extend_from_slice(&crc32(&body).to_be_bytes());
    }
    let mut raw = Vec::with_capacity((width as usize * 4 + 1) * height as usize);
    for row in rgba.chunks_exact(width as usize * 4) {
        raw.push(0);
        raw.extend_from_slice(row);
    }
    let mut z = vec![0x78, 0x01];
    let blocks: Vec<&[u8]> = raw.chunks(65535).collect();
    for (i, block) in blocks.iter().enumerate() {
        z.push(u8::from(i + 1 == blocks.len()));
        z.extend_from_slice(&(block.len() as u16).to_le_bytes());
        z.extend_from_slice(&(!(block.len() as u16)).to_le_bytes());
        z.extend_from_slice(block);
    }
    let (mut a, mut b) = (1u32, 0u32);
    for &x in &raw {
        a = (a + x as u32) % 65521;
        b = (b + a) % 65521;
    }
    z.extend_from_slice(&((b << 16) | a).to_be_bytes());
    let mut png = vec![0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1a, b'\n'];
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&height.to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
    chunk(&mut png, b"IHDR", &ihdr);
    chunk(&mut png, b"IDAT", &z);
    chunk(&mut png, b"IEND", &[]);
    std::fs::write(path, png)
}

// ---------------------------------------------------------------------------
// Pulley binding
// ---------------------------------------------------------------------------

struct PulleyGuest {
    store: wasmtime::Store<()>,
    init: wasmtime::TypedFunc<(i32, i32, i32), i32>,
    frame: wasmtime::TypedFunc<(i32, i32), i32>,
    mem_pages: wasmtime::TypedFunc<(), i32>,
}

impl Guest for PulleyGuest {
    fn init(&mut self, scene: i32, width: i32, height: i32) -> Result<i32> {
        crate::into_anyhow(self.init.call(&mut self.store, (scene, width, height)))
    }
    fn frame(&mut self, index: i32, count: i32) -> Result<i32> {
        crate::into_anyhow(self.frame.call(&mut self.store, (index, count)))
    }
    fn mem_pages(&mut self) -> Result<i32> {
        crate::into_anyhow(self.mem_pages.call(&mut self.store, ()))
    }
}

fn pulley_guest(wasm: &[u8]) -> Result<Box<dyn Guest>> {
    use wasmtime::{Caller, Extern, Linker, Module, Store};
    let engine = crate::pulley_engine()?;
    let module = crate::into_anyhow(Module::from_binary(&engine, wasm)).context("compile guest")?;
    let mut linker: Linker<()> = Linker::new(&engine);
    fn memory<'a>(caller: &'a mut Caller<'_, ()>) -> &'a [u8] {
        match caller.get_export("memory") {
            Some(Extern::Memory(m)) => m.data(caller),
            _ => &[],
        }
    }
    let wrap = |r: Result<&mut Linker<()>, wasmtime::Error>| crate::into_anyhow(r.map(|_| ()));
    wrap(linker.func_wrap("fvg", "set_size", |w: i32, h: i32, dpi: i32| host_set_size(w, h, dpi)))?;
    wrap(linker.func_wrap("fvg", "image_alloc", |w: i32, h: i32, f: i32, fl: i32| host_image_alloc(w, h, f, fl)))?;
    wrap(linker.func_wrap(
        "fvg",
        "image_update",
        |mut c: Caller<'_, ()>, hd: i32, x: i32, y: i32, w: i32, h: i32, f: i32, p: i32, n: i32| -> i32 {
            match guest_span(memory(&mut c), p, n.max(0) as usize) {
                Some(data) => host_image_update(hd, x, y, w, h, f, &data.to_vec()),
                None => -1,
            }
        },
    ))?;
    wrap(linker.func_wrap("fvg", "image_delete", |hd: i32| host_image_delete(hd)))?;
    wrap(linker.func_wrap("fvg", "render", |mut c: Caller<'_, ()>, vp: i32, vn: i32, cp: i32, cn: i32| {
        let mem = memory(&mut c);
        let verts = guest_span(mem, vp, vn.max(0) as usize * std::mem::size_of::<Vertex>());
        let cmds = guest_span(mem, cp, cn.max(0) as usize * 4);
        if let (Some(v), Some(k)) = (verts, cmds) {
            host_render(v, k);
        }
    }))?;
    let mut store = Store::new(&engine, ());
    let instance = crate::into_anyhow(linker.instantiate(&mut store, &module)).context("instantiate")?;
    let get = |store: &mut Store<()>, name: &str| {
        instance.get_func(&mut *store, name).ok_or_else(|| anyhow!("export `{name}` missing"))
    };
    let init = crate::into_anyhow(get(&mut store, "fvg_init")?.typed(&store))?;
    let frame = crate::into_anyhow(get(&mut store, "fvg_frame")?.typed(&store))?;
    let mem_pages = crate::into_anyhow(get(&mut store, "fvg_mem_pages")?.typed(&store))?;
    Ok(Box::new(PulleyGuest { store, init, frame, mem_pages }))
}
