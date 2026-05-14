// 3×3 box-blur convolution on a 256×256 grayscale u8 image.
//
// Memory-heavy mixed workload. Output: i32 sum of all output pixels mod
// (1 << 30) — small enough that overflow wrap is irrelevant on i32 here.

#![no_std]
#![no_main]

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    loop {}
}

const W: usize = 256;
const H: usize = 256;
static mut IN_BUF: [u8; W * H] = [0; W * H];
static mut OUT_BUF: [u8; W * H] = [0; W * H];

fn fill(seed: u32) {
    let mut s = seed;
    unsafe {
        let mut i = 0;
        while i < W * H {
            s = s.wrapping_mul(1664525).wrapping_add(1013904223);
            IN_BUF[i] = (s >> 24) as u8;
            i += 1;
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn convolve(seed: i32) -> i32 {
    fill(seed as u32);
    unsafe {
        let mut y = 1;
        while y < H - 1 {
            let mut x = 1;
            while x < W - 1 {
                let mut acc: u32 = 0;
                let mut dy: i32 = -1;
                while dy <= 1 {
                    let mut dx: i32 = -1;
                    while dx <= 1 {
                        let yi = (y as i32 + dy) as usize;
                        let xi = (x as i32 + dx) as usize;
                        acc += IN_BUF[yi * W + xi] as u32;
                        dx += 1;
                    }
                    dy += 1;
                }
                OUT_BUF[y * W + x] = (acc / 9) as u8;
                x += 1;
            }
            y += 1;
        }
        let mut sum: u32 = 0;
        let mut i = 0;
        while i < W * H {
            sum = sum.wrapping_add(OUT_BUF[i] as u32);
            i += 1;
        }
        (sum & ((1u32 << 30) - 1)) as i32
    }
}
