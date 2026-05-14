// Sieve of Eratosthenes. Caller passes N; we count primes ≤ N.
//
// For N = 10000 the answer is 1229. The function exercises wasm linear
// memory writes, conditional branches, and integer math.

#![no_std]
#![no_main]

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    loop {}
}

// Static buffer in wasm linear memory; sized for the brief's N=10000.
const N_MAX: usize = 10_000;
static mut SIEVE: [u8; N_MAX + 1] = [0; N_MAX + 1];

#[unsafe(no_mangle)]
pub extern "C" fn sieve(n: i32) -> i32 {
    let n = n as usize;
    if n > N_MAX {
        return -1;
    }
    unsafe {
        // Reset.
        let mut i = 0;
        while i <= n {
            SIEVE[i] = 0;
            i += 1;
        }
        let mut p: usize = 2;
        while p * p <= n {
            if SIEVE[p] == 0 {
                let mut m = p * p;
                while m <= n {
                    SIEVE[m] = 1;
                    m += p;
                }
            }
            p += 1;
        }
        let mut count = 0i32;
        let mut i = 2usize;
        while i <= n {
            if SIEVE[i] == 0 {
                count += 1;
            }
            i += 1;
        }
        count
    }
}
