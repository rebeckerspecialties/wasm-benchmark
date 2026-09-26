# femtovg E2E — guest ⇄ host ABI (v1, approved 2026-09-22)

Every runtime runs the *same* femtovg guest, which does all of femtovg's
CPU work per frame: transforms, path flattening, stroke/fill tessellation,
and building the command and vertex buffers. The host owns the GPU (wgpu
on Metal, offscreen texture) and receives the buffers through one small
import ABI that is identical for every runtime.

The ABI below was approved as drafted. It was then built on femtovg
**master** (0.27.0, `1334764`) instead of the drafted
`show-multiple-guis-stack` branch. Master's `Command` set is larger
(clip-path commands, image filters), so the wire stream mirrors master.
The imports and exports are unchanged from the draft.

## Cut point: femtovg's `Renderer` trait

`femtovg::Canvas<R: Renderer>` hands its renderer
`render(output, images, verts: &[Vertex], commands: Vec<Command>)`, plus
image alloc/update/delete calls. `Command` and `Params` fields are
`pub(crate)`, so both ends of the wire live inside femtovg. The fork
([rebeckerspecialties/femtovg `wire-renderer`](https://github.com/rebeckerspecialties/femtovg/tree/wire-renderer),
submodule `femtovg/`) adds one module, `src/renderer/wire.rs`, behind a
`wire` cargo feature:

- `WireRenderer<S: WireSink>` is a `Renderer` + `SurfacelessRenderer` (the
  guest side). Each flush becomes the vertex array plus an encoded word
  stream, handed to the sink.
- `WireReplayer<R: Renderer>` is the host side. It owns the target
  renderer and its image store, hands out the image handles, decodes the
  stream back into real `Command`s, and calls the stock
  `WGPURenderer::render`, so the GPU work is exactly femtovg's native
  wgpu path.

A second fork commit makes femtovg's OpenGL backend (`opengl`) and its
wasm32 browser integration (`web`) optional default features. Without
them, a wasm32 build links no wasm-bindgen / web-sys. Otherwise their
descriptor exports and `__wbindgen_*` imports survive even when unused,
and the module cannot run outside a browser.

## Guest exports (`wasm32-unknown-unknown` cdylib, `workloads-rs-cargo/femtovg-guest`)

| export | signature | notes |
|---|---|---|
| `memory` | — | linear memory |
| `fvg_init` | `(scene: i32, width: i32, height: i32) -> i32` | parses the embedded SVG `scene` with usvg **once** (not timed) into a display list, with each path's absolute transform baked in; returns the path count, or < 0 on error |
| `fvg_frame` | `(index: i32, count: i32) -> i32` | draws frame `index` of `count`, then `canvas.flush()`, which calls the imports synchronously; returns the command count, or < 0 |
| `fvg_mem_pages` | `() -> i32` | `memory.size` in 64 KiB pages (for peak linear-memory tracking) |

The zoom schedule is computed in the guest from `(index, count)`, so it is
bit-identical everywhere: `z = 16^(1 - |2·index/(count-1) - 1|)`, which
goes 1× → 16× → 1× in equal log steps, zoomed about the canvas center.
The defaults are `count = 121` and a 1024×1024 canvas.

## Guest imports (module `"fvg"`)

All parameters are `i32`, so every runtime's host-function binding has the
same shape.

| import | signature | notes |
|---|---|---|
| `set_size` | `(width, height, dpi_milli) -> ()` | dpi × 1000 |
| `image_alloc` | `(width, height, format, flags) -> i32` | returns a host handle. Format: 0 Rgb8, 1 Rgba8, 2 Gray8; flags = femtovg `ImageFlags` bits |
| `image_update` | `(handle, x, y, width, height, format, data_ptr, data_len) -> i32` | 0 = ok; `data` is tightly packed rows |
| `image_delete` | `(handle) -> ()` | |
| `render` | `(verts_ptr, vert_count, cmds_ptr, cmds_len) -> ()` | once per flush. `verts` is femtovg's `#[repr(C)] Vertex {x,y,u,v: f32}` array, zero-copy; `cmds_len` counts `u32` words |

The host copies both spans out of linear memory inside `render` and
returns immediately. Decode, wgpu encoding and submit happen *after*
`fvg_frame` returns, so guest CPU time never includes GPU work.

## Wire command stream

The normative layout is the module docs of `femtovg/src/renderer/wire.rs`.
In summary: little-endian `u32` words, `NONE = 0xFFFF_FFFF`, and a leading
version word.

```
stream   := WIRE_VERSION n_commands command*
command  := kind clip_active fill_rule image filter_scratch
            glyph_kind glyph_image blend[4] tri_first tri_count
            n_drawables (fill_first fill_count stroke_first stroke_count)*
            payload
kind     := 0 ClipFill  1 ClipReset  2 SetRenderTarget  3 ClearRect  4 ConvexFill
            5 ConcaveFill  6 Stroke  7 StencilStroke  8 Triangles  9 RenderFilteredImage
payload  := ClipReset: visible | SetRenderTarget: target handle
            | ClearRect: f32 rgba[4] keep_clip | fills/strokes/triangles: 1 or 2 Params
            | RenderFilteredImage: target_image filter
Params   := 53 words (master's Params fields, in declaration order)
filter   := GaussianBlur | ColorMatrix[20] | Turbulence | LinearRgbToSrgb | SrgbToLinearRgb
```

A femtovg unit test (`round_trip_preserves_every_command`) draws a mixed
frame, decodes it, re-encodes the decoded commands and checks that the
words are unchanged.

## Host (native Rust; one runtime per process / app launch)

`crates/benchmark-core/src/femtovg_e2e.rs`, feature `femtovg-e2e`; CLI
`run_femtovg_e2e`, FFI `bench_femtovg_e2e`. The iOS app's `FEMTOVG_E2E=<scenes>`
mode calls it.

- wgpu 30, Metal backend, headless device, offscreen RGBA8 1024×1024.
- Per frame: `t_guest` = `fvg_frame()` wall time; `t_encode` = decode +
  `WGPURenderer::render` + `queue.submit`; `t_gpu` = `device.poll(Wait)`
  for that submission. Frame time is the sum. Reported: FPS (mean over the
  pass, and 1 / median frame time) and frame time p50/p95/p99/max.
- Two passes over the schedule, of which the last is reported. The first
  frame after canvas creation carries one extra command (render-target
  setup), and the warmup pass also absorbs pipeline creation.
- Peak memory: `task_vm_info.ledger_phys_footprint_peak` (the full rev3
  count is required, or the kernel leaves it 0), plus the max over frames
  of `fvg_mem_pages() × 64 KiB`.
- The whole run happens on one 16 MiB-stack thread carrying the caller's
  QoS class (a bare spawned thread would leave `.utility` for the default
  QoS and run on P-cores). The measured pass records its E-core share.

## Correctness

- FNV-1a-64 over `verts ‖ cmds` for every frame. **All seven runtimes and
  all three guest builds produce bit-identical frames**, and the same
  final-texture hash on the M4 Max and on the iPhone XS's A12 GPU.
- The native (aarch64, non-wasm) build of the same guest
  (`fvg_reference`) gives the same vertex and command counts on every
  frame and a byte-identical command stream. It is **not** bit-identical
  in the vertices: femtovg calls `sin_cos`, `tan`, `hypot`, `acos` and
  `atan2` while building paths. The wasm guest carries its own Rust libm
  port, while the native build uses Apple's libm, and they differ by 1 ulp
  on a few dozen of ~275K floats per frame.
- The relaxed build (`+relaxed-simd`) contains **no** relaxed-SIMD
  instruction. rustc only emits them through intrinsics, never by
  contracting `a*b+c` without fast-math. It is therefore effectively the
  simd128 build (identical code; only the target-features section
  differs). Each runtime runs the best build it supports: simd128 on
  Pulley, WAMR, WasmEdge and tinywasm; scalar on wasm3 and zwasm (no
  interpreter SIMD) and on wasmz (crashes on v128).

## Scenes

`workloads/femtovg/` (licenses in `LICENSES.md`):

- scene 0: femtovg's own `examples/assets/Ghostscript_Tiger.svg` (240
  paths, 2510 segments);
- scene 1: `combined-linking.svg` from the WebAssembly component-model
  design repository (Apache-2.0; 189 paths, 9341 segments of glyph
  outlines, plus a root `clip-path` that exercises `ClipFill` /
  `ClipReset`).
