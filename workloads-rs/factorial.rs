// factorial(n) using wrapping i32 multiplication.
//
// fact(20) overflows i32 — what we benchmark is wrapping fact(n), which is
// still deterministic and exercises a tight call/multiply/branch loop. The
// host-side reference in `benchmark-core` performs the same wrapping math
// and the Pulley-emitted result must match exactly.

#![no_std]
#![no_main]

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    loop {}
}

#[unsafe(no_mangle)]
pub extern "C" fn factorial(n: i32) -> i32 {
    let mut acc: i32 = 1;
    let mut i: i32 = 2;
    while i <= n {
        acc = acc.wrapping_mul(i);
        i = i.wrapping_add(1);
    }
    acc
}
