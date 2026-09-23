// Relaxed-SIMD hot paths: an int8 dot-product kernel and an FMA kernel.
//
// `relaxed_dot`: 64×64 int8 matrix product with K = 256, the inner loop of
// quantized inference. Each 16-byte chunk goes through one
// `i32x4.relaxed_dot_i8x16_i7x16_add_s`. The right-hand operand is kept in
// 0..=127, the range where the instruction's result is fully specified
// (and the i16 pair sums cannot saturate), so every runtime must produce the
// same checksum.
//
// `relaxed_madd`: degree-8 polynomial evaluation (Horner's rule) over 16K
// points with `f32x4.relaxed_madd`, then `i32x4.relaxed_trunc_f32x4_s`.
// Points and coefficients are small integers, so every intermediate is an
// exact integer below 2^24: fused and unfused multiply-add round the same
// way and the truncation stays in range, which makes the result
// deterministic on every implementation choice the relaxed ops allow.

#![no_std]
#![no_main]

use core::arch::wasm32::*;

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    loop {}
}

const M: usize = 64;
const K: usize = 256;

#[repr(align(16))]
struct Mat<const L: usize>([i8; L]);

static mut A: Mat<{ M * K }> = Mat([0; M * K]);
/// B stored transposed (row j = column j of B), so both operands of each
/// dot product are contiguous.
static mut BT: Mat<{ M * K }> = Mat([0; M * K]);
static mut DOT_SEED: i32 = i32::MIN;

fn lcg(s: &mut u32) -> u32 {
    *s = s.wrapping_mul(1664525).wrapping_add(1013904223);
    *s
}

fn fill_dot(seed: i32) {
    unsafe {
        if DOT_SEED == seed {
            return;
        }
        let mut s = seed as u32 ^ 0x2545_F491;
        let mut i = 0;
        while i < M * K {
            A.0[i] = (lcg(&mut s) >> 24) as u8 as i8; // -128..=127
            BT.0[i] = ((lcg(&mut s) >> 24) & 0x7F) as i8; // 0..=127
            i += 1;
        }
        DOT_SEED = seed;
    }
}

#[target_feature(enable = "simd128,relaxed-simd")]
unsafe fn dot_kernel() -> i32 {
    unsafe {
        let a = &raw const A.0 as *const i8;
        let b = &raw const BT.0 as *const i8;
        let mut sum: i32 = 0;
        let mut i = 0;
        while i < M {
            let mut j = 0;
            while j < M {
                let mut acc = i32x4_splat(0);
                let mut k = 0;
                while k < K {
                    let va = v128_load(a.add(i * K + k) as *const v128);
                    let vb = v128_load(b.add(j * K + k) as *const v128);
                    acc = i32x4_relaxed_dot_i8x16_i7x16_add(va, vb, acc);
                    k += 16;
                }
                let c = i32x4_extract_lane::<0>(acc)
                    .wrapping_add(i32x4_extract_lane::<1>(acc))
                    .wrapping_add(i32x4_extract_lane::<2>(acc))
                    .wrapping_add(i32x4_extract_lane::<3>(acc));
                sum = sum.rotate_left(3).wrapping_add(c);
                j += 1;
            }
            i += 1;
        }
        sum
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn relaxed_dot(seed: i32) -> i32 {
    fill_dot(seed);
    unsafe { dot_kernel() }
}

const NPTS: usize = 16 * 1024;
const DEG: usize = 8;

#[repr(align(16))]
struct Pts([f32; NPTS]);

static mut X: Pts = Pts([0.0; NPTS]);
static mut COEF: [f32; DEG + 1] = [0.0; DEG + 1];
static mut MADD_SEED: i32 = i32::MIN;

fn fill_madd(seed: i32) {
    unsafe {
        if MADD_SEED == seed {
            return;
        }
        let mut s = seed as u32 ^ 0x9E37_79B9;
        let mut i = 0;
        while i < NPTS {
            X.0[i] = ((lcg(&mut s) >> 24) % 7) as f32 - 3.0; // -3..=3
            i += 1;
        }
        let mut c = 0;
        while c <= DEG {
            COEF[c] = ((lcg(&mut s) >> 24) % 9) as f32 - 4.0; // -4..=4
            c += 1;
        }
        MADD_SEED = seed;
    }
}

#[target_feature(enable = "simd128,relaxed-simd")]
unsafe fn madd_kernel() -> i32 {
    unsafe {
        let x = &raw const X.0 as *const f32;
        let mut coef = [f32x4_splat(0.0); DEG + 1];
        let mut c = 0;
        while c <= DEG {
            coef[c] = f32x4_splat(COEF[c]);
            c += 1;
        }
        let mut acc = i32x4_splat(0);
        let mut i = 0;
        while i < NPTS {
            let vx = v128_load(x.add(i) as *const v128);
            // Horner: p = ((c8·x + c7)·x + c6)·x + … + c0
            let mut p = coef[DEG];
            let mut d = DEG;
            while d > 0 {
                d -= 1;
                p = f32x4_relaxed_madd(p, vx, coef[d]);
            }
            acc = i32x4_add(i32x4_mul(acc, i32x4_splat(31)), i32x4_relaxed_trunc_f32x4(p));
            i += 4;
        }
        i32x4_extract_lane::<0>(acc)
            ^ i32x4_extract_lane::<1>(acc).rotate_left(8)
            ^ i32x4_extract_lane::<2>(acc).rotate_left(16)
            ^ i32x4_extract_lane::<3>(acc).rotate_left(24)
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn relaxed_madd(seed: i32) -> i32 {
    fill_madd(seed);
    unsafe { madd_kernel() }
}
