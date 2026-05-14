// 64×64 f32 matrix multiply using the wasm relaxed-simd `f32x4_relaxed_madd`
// intrinsic. Pulley lowers this to its `Vfma32x4` bytecode (one rounding
// per FMA) versus the simd128-only `matmul_simd` workload, which uses
// `f32x4_mul` + `f32x4_add` (two roundings). Both produce a verifiable
// result; the reference function uses `f32::mul_add` so it shares Pulley's
// rounding behavior on aarch64.

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
                A[i * N + j] = ((i as i32 + seed) ^ (j as i32 * 7)) as f32 * 0.001;
                B[i * N + j] = (((i as i32) * 13 + j as i32 + seed) & 0xFF) as f32 * 0.002;
                j += 1;
            }
            i += 1;
        }
    }
}

#[target_feature(enable = "simd128,relaxed-simd")]
unsafe fn matmul_fma_inner() {
    unsafe {
        let mut i = 0;
        while i < N {
            let mut j = 0;
            while j < N {
                let mut acc = f32x4_splat(0.0);
                let mut k = 0;
                while k < N {
                    let a = f32x4_splat(A[i * N + k]);
                    let bp = (&B[k * N + j] as *const f32) as *const v128;
                    let b = v128_load(bp);
                    // Relaxed FMA: acc = a*b + acc with single rounding.
                    acc = f32x4_relaxed_madd(a, b, acc);
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
pub extern "C" fn matmul_fma(seed: i32) -> i32 {
    fill(seed);
    unsafe {
        matmul_fma_inner();
        let mut sum: f32 = 0.0;
        let mut i = 0;
        while i < N {
            sum += C[i * N + i];
            i += 1;
        }
        sum as i32
    }
}
