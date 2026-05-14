// Tail-recursive Fibonacci in accumulator-passing style.
//
// LLVM with `-C target-feature=+tail-call` lowers a `becomes` (explicit
// tail call) and an ordinary recursive call written in tail position
// into wasm `return_call`. Pulley's CLIF lowering then emits a
// PulleyShared `return_call_impl` instruction (cranelift/.../inst.isle).
// This is the workload that actually exercises that path.
//
// The non-tail `fib(30)` returns 832040 via O(2^n) calls — gets blown
// out of the stack quickly without tail-call elimination. This tail
// version handles n up to thousands without growing wasm stack at all.
// We compute fib(40) so the workload runtime is non-trivial in
// interpretation while staying small enough that a non-TCO version
// would still complete (sanity: fib(40) is the 40th iteration of an
// O(n) accumulator loop).

#![no_std]
#![no_main]

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    loop {}
}

#[inline(never)]
fn fib_acc(n: i32, a: i32, b: i32) -> i32 {
    if n == 0 {
        a
    } else {
        // Tail call. With `+tail-call`, rustc emits this as a wasm
        // `return_call` (or LLVM's `musttail` on suitable targets).
        fib_acc(n - 1, b, a.wrapping_add(b))
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn fib_tail(n: i32) -> i32 {
    fib_acc(n, 0, 1)
}
