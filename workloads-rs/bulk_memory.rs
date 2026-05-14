// Bulk-memory workload: exercises wasm `memory.copy` and `memory.fill`.
//
// rustc with `-C target-feature=+bulk-memory` lowers `slice::copy_from_slice`
// and `<slice>::fill` directly to those wasm instructions instead of emitting
// scalar copy loops. Pulley's bytecode has dedicated bulk-memory ops so the
// interpreter can hand off to memcpy / memset rather than running the byte
// loop one opcode at a time.
//
// Workload: 64 KiB source filled by an LCG; alternately memcpy chunks
// of varying sizes (powers of two from 16 B to 4 KiB) and memset chunks
// in a destination buffer for ROUNDS rounds. Return a checksum so the
// host can verify.

#![no_std]
#![no_main]

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    loop {}
}

const N: usize = 64 * 1024;
const ROUNDS: usize = 200;
static mut SRC: [u8; N] = [0; N];
static mut DST: [u8; N] = [0; N];

fn fill_lcg(seed: u32) {
    let mut s = seed;
    unsafe {
        let mut i = 0;
        while i < N {
            s = s.wrapping_mul(1664525).wrapping_add(1013904223);
            SRC[i] = (s >> 24) as u8;
            i += 1;
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn bulk_memory(seed: i32) -> i32 {
    fill_lcg(seed as u32);
    unsafe {
        // Cycle through chunk sizes that LLVM is comfortable lowering to
        // `memory.copy` / `memory.fill`.
        let sizes = [16usize, 64, 256, 1024, 4096];
        let mut round = 0usize;
        while round < ROUNDS {
            let size = sizes[round % sizes.len()];
            // Stride through dst, copying a chunk from src.
            let mut off = 0usize;
            while off + size <= N {
                let src_off = (off + (round * 17)) & (N - size);
                let dst = &raw mut DST[off];
                let src = &raw const SRC[src_off];
                core::ptr::copy_nonoverlapping(src, dst, size); // wasm: memory.copy
                off += size * 2;
            }
            // memory.fill: zero a stripe based on round
            let fill_off = (round * 1024) & (N - size);
            let fill_byte = (round & 0xFF) as u8;
            let region = &raw mut DST[fill_off];
            core::ptr::write_bytes(region, fill_byte, size); // wasm: memory.fill
            round += 1;
        }
        // Checksum DST.
        let mut sum: u32 = 0;
        let mut i = 0;
        while i < N {
            sum = sum.wrapping_add(DST[i] as u32);
            i += 1;
        }
        (sum & 0x7FFF_FFFF) as i32
    }
}
