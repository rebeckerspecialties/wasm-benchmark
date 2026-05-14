// 64×64 f32 matrix multiply using wasm simd128 (v128) intrinsics.
//
// Computes C = A * B where A and B are deterministic 64×64 f32 matrices
// filled from index-derived values, then returns the i32-truncated value
// of trace(C) so the host can verify. The inner loop uses f32x4 fused-style
// multiply-accumulate and v128.load / v128.store to exercise Pulley's SIMD
// dispatch.

#![no_std]
#![no_main]

use core::arch::wasm32::*;

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    loop {}
}

const N: usize = 64;
static mut A: [f32; N * N] = [0.0; N * N];
static mut B: [f32; N * N] = [0.0; N * N];
static mut C: [f32; N * N] = [0.0; N * N];

fn fill(seed: i32) {
    unsafe {
        let mut i = 0;
        while i < N {
            let mut j = 0;
            while j < N {
                // Cheap deterministic float fill that touches both halves.
                A[i * N + j] = ((i as i32 + seed) ^ (j as i32 * 7)) as f32 * 0.001;
                B[i * N + j] = (((i as i32) * 13 + j as i32 + seed) & 0xFF) as f32 * 0.002;
                j += 1;
            }
            i += 1;
        }
    }
}

#[target_feature(enable = "simd128")]
unsafe fn matmul_simd_inner() {
    unsafe {
        let mut i = 0;
        while i < N {
            let mut j = 0;
            // Process j in chunks of 4 (one v128 = 4 f32 lanes).
            while j < N {
                let mut acc = f32x4_splat(0.0);
                let mut k = 0;
                while k < N {
                    let a = f32x4_splat(A[i * N + k]);
                    // Load four contiguous B[k][j..j+4] elements.
                    let bp = (&B[k * N + j] as *const f32) as *const v128;
                    let b = v128_load(bp);
                    acc = f32x4_add(acc, f32x4_mul(a, b));
                    k += 1;
                }
                let cp = (&mut C[i * N + j] as *mut f32) as *mut v128;
                v128_store(cp, acc);
                j += 4;
            }
            i += 1;
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn matmul(seed: i32) -> i32 {
    fill(seed);
    unsafe {
        matmul_simd_inner();
        // Return i32-truncation of trace(C), which depends on every row.
        let mut sum: f32 = 0.0;
        let mut i = 0;
        while i < N {
            sum += C[i * N + i];
            i += 1;
        }
        sum as i32
    }
}
