//! Native (non-wasm) run of the guest's frame loop for reference hashes:
//! the same scene conversion, zoom schedule and femtovg CPU work, into a
//! WireRenderer whose sink hashes each flush (FNV-1a 64 over verts ‖
//! commands) instead of sending it to a GPU.
//!
//!   fvg_reference [scene=0] [count=121] [width=1024] [height=1024]
//!
//! Prints one JSON line per frame and a summary line.

use std::cell::Cell;

use femtovg::{
    renderer::{Vertex, WireRenderer, WireSink},
    Canvas, ErrorKind, ImageInfo, PixelFormat,
};
use femtovg_guest::{command_count, draw_frame, frame_hash, load_scene};

thread_local! {
    /// (hash, vertex count, command count) of the last flush. femtovg's
    /// Canvas does not hand its renderer back, so the sink reports here.
    static LAST: Cell<Option<(u64, usize, i32)>> = const { Cell::new(None) };
}

#[derive(Default)]
struct HashSink {
    next_image: u32,
}

impl WireSink for HashSink {
    fn set_size(&mut self, _: u32, _: u32, _: f32) {}
    fn image_alloc(&mut self, _: ImageInfo) -> Result<u32, ErrorKind> {
        self.next_image += 1;
        Ok(self.next_image - 1)
    }
    fn image_update(
        &mut self,
        _: u32,
        _: usize,
        _: usize,
        _: usize,
        _: usize,
        _: PixelFormat,
        _: &[u8],
    ) -> Result<(), ErrorKind> {
        Ok(())
    }
    fn image_delete(&mut self, _: u32) {}
    fn render(&mut self, verts: &[Vertex], commands: &[u32]) {
        // FVG_DUMP=path: write the first frame's raw buffers.
        if let Ok(path) = std::env::var("FVG_DUMP") {
            if !std::path::Path::new(&path).exists() {
                let mut bytes: Vec<u8> = verts
                    .iter()
                    .flat_map(|v| [v.x, v.y, v.u, v.v])
                    .flat_map(|f| f.to_bits().to_le_bytes())
                    .collect();
                bytes.extend(commands.iter().flat_map(|w| w.to_le_bytes()));
                let _ = std::fs::write(&path, bytes);
            }
        }
        LAST.with(|l| l.set(Some((frame_hash(verts, commands), verts.len(), command_count(commands)))));
    }
}

fn main() {
    let args: Vec<u32> = std::env::args()
        .skip(1)
        .map(|a| a.parse().expect("integer argument"))
        .collect();
    let arg = |i: usize, d: u32| args.get(i).copied().unwrap_or(d);
    let (scene_idx, count, width, height) = (arg(0, 0), arg(1, 121), arg(2, 1024), arg(3, 1024));
    let scene = load_scene(scene_idx as usize).expect("scene");
    eprintln!(
        "scene {scene_idx}: {} paths, {} segments, {}x{} user units",
        scene.paths(),
        scene.segments,
        scene.width,
        scene.height
    );
    let mut canvas = Canvas::new(WireRenderer::new(HashSink::default())).expect("canvas");
    canvas.set_size(width, height, 1.0);
    let mut all: u64 = 0xcbf2_9ce4_8422_2325;
    for i in 0..count {
        draw_frame(&mut canvas, &scene, i, count);
        canvas.flush();
        let (h, nv, nc) = LAST.with(|l| l.take()).expect("flush reached the sink");
        all = (all ^ h).wrapping_mul(0x0000_0100_0000_01b3);
        println!("{{\"frame\":{i},\"hash\":\"{h:016x}\",\"verts\":{nv},\"commands\":{nc}}}");
    }
    println!(
        "{{\"scene\":{scene_idx},\"frames\":{count},\"width\":{width},\"height\":{height},\"all\":\"{all:016x}\"}}"
    );
}
