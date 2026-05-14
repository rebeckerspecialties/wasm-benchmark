// `call_indirect` workload — dispatches through a function-pointer table to
// exercise the wasm `call_indirect` instruction (and Pulley's per-site
// indirect-call cache when `Config::cache_call_indirects(true)` is set).
//
// 16 leaf ops plus an LCG-driven dispatch index. Per iteration we pick an
// op index from the LCG state, call it through a `static` function-pointer
// table, and feed the result back into the LCG. Return a checksum.

#![no_std]
#![no_main]

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    loop {}
}

type Op = fn(i32, i32) -> i32;

#[inline(never)]
fn op_add(a: i32, b: i32) -> i32 { a.wrapping_add(b) }
#[inline(never)]
fn op_sub(a: i32, b: i32) -> i32 { a.wrapping_sub(b) }
#[inline(never)]
fn op_mul(a: i32, b: i32) -> i32 { a.wrapping_mul(b) }
#[inline(never)]
fn op_xor(a: i32, b: i32) -> i32 { a ^ b }
#[inline(never)]
fn op_and(a: i32, b: i32) -> i32 { a & b }
#[inline(never)]
fn op_or(a: i32, b: i32) -> i32 { a | b }
#[inline(never)]
fn op_shl(a: i32, b: i32) -> i32 { a.wrapping_shl((b & 31) as u32) }
#[inline(never)]
fn op_shr(a: i32, b: i32) -> i32 { (a as u32).wrapping_shr((b & 31) as u32) as i32 }
#[inline(never)]
fn op_rotl(a: i32, b: i32) -> i32 { (a as u32).rotate_left((b & 31) as u32) as i32 }
#[inline(never)]
fn op_rotr(a: i32, b: i32) -> i32 { (a as u32).rotate_right((b & 31) as u32) as i32 }
#[inline(never)]
fn op_min(a: i32, b: i32) -> i32 { if a < b { a } else { b } }
#[inline(never)]
fn op_max(a: i32, b: i32) -> i32 { if a > b { a } else { b } }
#[inline(never)]
fn op_clz(a: i32, _b: i32) -> i32 { (a as u32).leading_zeros() as i32 }
#[inline(never)]
fn op_ctz(a: i32, _b: i32) -> i32 { (a as u32).trailing_zeros() as i32 }
#[inline(never)]
fn op_popcount(a: i32, _b: i32) -> i32 { (a as u32).count_ones() as i32 }
#[inline(never)]
fn op_negxor(a: i32, b: i32) -> i32 { (!a) ^ b }

static OPS: [Op; 16] = [
    op_add, op_sub, op_mul, op_xor,
    op_and, op_or, op_shl, op_shr,
    op_rotl, op_rotr, op_min, op_max,
    op_clz, op_ctz, op_popcount, op_negxor,
];

const ITERS: usize = 200_000;

#[unsafe(no_mangle)]
pub extern "C" fn call_indirect(seed: i32) -> i32 {
    let mut acc: i32 = seed;
    let mut s: u32 = seed as u32;
    let mut i = 0;
    while i < ITERS {
        s = s.wrapping_mul(1664525).wrapping_add(1013904223);
        let idx = (s >> 28) as usize; // 4-bit index → table of 16
        let op = OPS[idx];
        // wasm: call_indirect through the function pointer.
        acc = op(acc, s as i32);
        i += 1;
    }
    acc
}
