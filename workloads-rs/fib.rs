// Recursive Fibonacci. Tight call/branch loop; small static surface.

#![no_std]
#![no_main]

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    loop {}
}

#[unsafe(no_mangle)]
pub extern "C" fn fib(n: i32) -> i32 {
    if n < 2 {
        n
    } else {
        fib(n - 1).wrapping_add(fib(n - 2))
    }
}
