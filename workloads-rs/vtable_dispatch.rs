// `vtable_dispatch` workload — exercises C++-style vtable dispatch
// patterns through `call_indirect`. Modeled after the StarlingMonkey
// `CryptoAlgorithm` hierarchy (pure-virtual base + ~6 concrete
// algorithm subclasses), the realistic Wasm-from-C++ dispatch shape
// that JS/Lua inline caches were designed to optimize.
//
// We use a hand-rolled C-style vtable struct rather than Rust's
// `dyn Trait` so LTO can't devirtualize — every dispatch goes through
// a function pointer in the wasm function table, lowered to
// `call_indirect`. The `pick_alg` factory is `#[inline(never)]` so
// the optimizer can't propagate the concrete vtable through the loop.
//
// Four entry points sweep the IC's polymorphism dimension:
//   - `vtable_mono`   — single algorithm, 100 % monomorphic at every
//                       site. Maximum IC win expected.
//   - `vtable_bi`     — alternating two algorithms (i & 1). 1-way IC
//                       thrashes; 2-way IC would catch this.
//   - `vtable_poly4`  — rotating four. 1-way IC always misses.
//   - `vtable_poly6`  — all six in rotation. Worst case for caching.

#![no_std]
#![no_main]

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    loop {}
}

#[repr(C)]
struct Algorithm {
    digest_step: fn(&Algorithm, u32, u32) -> u32,
    mix: fn(&Algorithm, u32, u32) -> u32,
    finalize: fn(&Algorithm, u32) -> u32,
}

// --- AES-GCM-flavored vtable ---
#[inline(never)]
fn aes_digest_step(_: &Algorithm, state: u32, input: u32) -> u32 {
    state.wrapping_add(input.wrapping_mul(0x9e3779b1))
}
#[inline(never)]
fn aes_mix(_: &Algorithm, a: u32, b: u32) -> u32 {
    a.rotate_left(5) ^ b
}
#[inline(never)]
fn aes_finalize(_: &Algorithm, s: u32) -> u32 {
    s ^ (s >> 16)
}
static AES_GCM: Algorithm = Algorithm {
    digest_step: aes_digest_step,
    mix: aes_mix,
    finalize: aes_finalize,
};

// --- SHA-256-flavored vtable ---
#[inline(never)]
fn sha_digest_step(_: &Algorithm, state: u32, input: u32) -> u32 {
    let e = state.wrapping_add(input);
    e.rotate_right(17).wrapping_add(e)
}
#[inline(never)]
fn sha_mix(_: &Algorithm, a: u32, b: u32) -> u32 {
    a.wrapping_add(b.wrapping_mul(0x9e3779b9))
}
#[inline(never)]
fn sha_finalize(_: &Algorithm, s: u32) -> u32 {
    s.rotate_left(13).wrapping_mul(0x85ebca6b)
}
static SHA256: Algorithm = Algorithm {
    digest_step: sha_digest_step,
    mix: sha_mix,
    finalize: sha_finalize,
};

// --- HMAC-flavored vtable ---
#[inline(never)]
fn hmac_digest_step(_: &Algorithm, state: u32, input: u32) -> u32 {
    state.wrapping_mul(input.wrapping_add(1)).wrapping_add(0xdeadbeef)
}
#[inline(never)]
fn hmac_mix(_: &Algorithm, a: u32, b: u32) -> u32 {
    (a ^ b).wrapping_add(a.rotate_left(7) ^ b.rotate_right(25))
}
#[inline(never)]
fn hmac_finalize(_: &Algorithm, s: u32) -> u32 {
    s.rotate_left(11) ^ s.rotate_right(19)
}
static HMAC: Algorithm = Algorithm {
    digest_step: hmac_digest_step,
    mix: hmac_mix,
    finalize: hmac_finalize,
};

// --- ECDSA-flavored vtable ---
#[inline(never)]
fn ecdsa_digest_step(_: &Algorithm, state: u32, input: u32) -> u32 {
    state ^ input.rotate_left(13)
}
#[inline(never)]
fn ecdsa_mix(_: &Algorithm, a: u32, b: u32) -> u32 {
    a.wrapping_add(b).wrapping_mul(0xc2b2ae35)
}
#[inline(never)]
fn ecdsa_finalize(_: &Algorithm, s: u32) -> u32 {
    s.wrapping_mul(0xcc9e2d51)
}
static ECDSA: Algorithm = Algorithm {
    digest_step: ecdsa_digest_step,
    mix: ecdsa_mix,
    finalize: ecdsa_finalize,
};

// --- Ed25519-flavored vtable ---
#[inline(never)]
fn ed_digest_step(_: &Algorithm, state: u32, input: u32) -> u32 {
    state.rotate_right(7).wrapping_add(input ^ 0xa5a5a5a5)
}
#[inline(never)]
fn ed_mix(_: &Algorithm, a: u32, b: u32) -> u32 {
    a.wrapping_mul(b).wrapping_add(b.rotate_left(11))
}
#[inline(never)]
fn ed_finalize(_: &Algorithm, s: u32) -> u32 {
    s ^ s.wrapping_mul(0x9e3779b9).rotate_right(15)
}
static ED25519: Algorithm = Algorithm {
    digest_step: ed_digest_step,
    mix: ed_mix,
    finalize: ed_finalize,
};

// --- RSA-PSS-flavored vtable ---
#[inline(never)]
fn rsa_digest_step(_: &Algorithm, state: u32, input: u32) -> u32 {
    state.wrapping_add(input).rotate_left(3).wrapping_mul(0x1b873593)
}
#[inline(never)]
fn rsa_mix(_: &Algorithm, a: u32, b: u32) -> u32 {
    (a.wrapping_add(b)) ^ (a.rotate_right(13).wrapping_mul(b | 1))
}
#[inline(never)]
fn rsa_finalize(_: &Algorithm, s: u32) -> u32 {
    s.rotate_left(5).wrapping_sub(s.rotate_right(11))
}
static RSA_PSS: Algorithm = Algorithm {
    digest_step: rsa_digest_step,
    mix: rsa_mix,
    finalize: rsa_finalize,
};

// Hide concrete vtable selection from the optimizer. Without
// `#[inline(never)]` LTO would constant-propagate the vtable pointer
// through callers, defeating the benchmark.
#[inline(never)]
fn pick_alg(idx: u32) -> &'static Algorithm {
    match idx % 6 {
        0 => &AES_GCM,
        1 => &SHA256,
        2 => &HMAC,
        3 => &ECDSA,
        4 => &ED25519,
        _ => &RSA_PSS,
    }
}

const ITERS: u32 = 200_000;

// 100% monomorphic — same vtable through every dispatch.
#[unsafe(no_mangle)]
pub extern "C" fn vtable_mono(seed: i32) -> i32 {
    let alg = pick_alg(seed as u32);
    let mut state: u32 = seed as u32;
    let mut i: u32 = 0;
    while i < ITERS {
        state = (alg.digest_step)(alg, state, i);
        state = (alg.mix)(alg, state, i.wrapping_mul(31));
        i = i.wrapping_add(1);
    }
    (alg.finalize)(alg, state) as i32
}

// Bimodal — alternates between two distinct vtables (i & 1).
#[unsafe(no_mangle)]
pub extern "C" fn vtable_bi(seed: i32) -> i32 {
    let a = pick_alg(seed as u32);
    let b = pick_alg(seed.wrapping_add(1) as u32);
    let mut state: u32 = seed as u32;
    let mut i: u32 = 0;
    while i < ITERS {
        let alg = if (i & 1) == 0 { a } else { b };
        state = (alg.digest_step)(alg, state, i);
        state = (alg.mix)(alg, state, i.wrapping_mul(31));
        i = i.wrapping_add(1);
    }
    state as i32
}

// 4-way polymorphic rotation.
#[unsafe(no_mangle)]
pub extern "C" fn vtable_poly4(seed: i32) -> i32 {
    let algs: [&'static Algorithm; 4] = [
        pick_alg(seed as u32),
        pick_alg(seed.wrapping_add(1) as u32),
        pick_alg(seed.wrapping_add(2) as u32),
        pick_alg(seed.wrapping_add(3) as u32),
    ];
    let mut state: u32 = seed as u32;
    let mut i: u32 = 0;
    while i < ITERS {
        let alg = algs[(i & 3) as usize];
        state = (alg.digest_step)(alg, state, i);
        state = (alg.mix)(alg, state, i.wrapping_mul(31));
        i = i.wrapping_add(1);
    }
    state as i32
}

// 6-way polymorphic — every concrete algorithm in rotation.
#[unsafe(no_mangle)]
pub extern "C" fn vtable_poly6(seed: i32) -> i32 {
    let algs: [&'static Algorithm; 6] = [
        pick_alg(seed as u32),
        pick_alg(seed.wrapping_add(1) as u32),
        pick_alg(seed.wrapping_add(2) as u32),
        pick_alg(seed.wrapping_add(3) as u32),
        pick_alg(seed.wrapping_add(4) as u32),
        pick_alg(seed.wrapping_add(5) as u32),
    ];
    let mut state: u32 = seed as u32;
    let mut i: u32 = 0;
    while i < ITERS {
        let alg = algs[(i % 6) as usize];
        state = (alg.digest_step)(alg, state, i);
        state = (alg.mix)(alg, state, i.wrapping_mul(31));
        i = i.wrapping_add(1);
    }
    state as i32
}
