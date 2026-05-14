// CRC32 (IEEE 802.3 polynomial 0xEDB88320) over a 64 KiB deterministic input.
//
// Branch-heavy / table-lookup workload: the input is 64 KiB of pseudo-random
// bytes generated from an LCG so the wasm has no static-data dependency
// (and so the wasm file stays small). The host reference recomputes from the
// same LCG seed and asserts equality.

#![no_std]
#![no_main]

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    loop {}
}

const N: usize = 64 * 1024;
static mut INPUT: [u8; N] = [0; N];

const POLY: u32 = 0xEDB88320;

fn build_table() -> [u32; 256] {
    let mut table = [0u32; 256];
    let mut i = 0;
    while i < 256 {
        let mut c = i as u32;
        let mut j = 0;
        while j < 8 {
            c = if c & 1 != 0 { POLY ^ (c >> 1) } else { c >> 1 };
            j += 1;
        }
        table[i] = c;
        i += 1;
    }
    table
}

fn fill_input(seed: u32) {
    let mut s = seed;
    unsafe {
        let mut i = 0;
        while i < N {
            // Numerical Recipes LCG.
            s = s.wrapping_mul(1664525).wrapping_add(1013904223);
            INPUT[i] = (s >> 24) as u8;
            i += 1;
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn crc32(seed: i32) -> i32 {
    fill_input(seed as u32);
    let table = build_table();
    let mut crc: u32 = 0xFFFFFFFF;
    unsafe {
        let mut i = 0;
        while i < N {
            let b = INPUT[i] as u32;
            crc = (crc >> 8) ^ table[((crc ^ b) & 0xFF) as usize];
            i += 1;
        }
    }
    !crc as i32
}
