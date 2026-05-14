//! `xmrsplayer-bench` — call_indirect-shaped audio workload.
//!
//! Embeds `unreal.s3m` (a real-world Scream Tracker 3 module) at
//! compile time, parses it via the `xmrs` crate, and on each
//! invocation renders **one audio buffer** of 1024 stereo frames
//! (= 2048 i16s, ≈ 23 ms of audio at 44 100 Hz) through `xmrsplayer`
//! to a null sound driver — i.e., the produced i16 samples are
//! XOR-folded into a checksum returned to the host so the optimizer
//! can't drop the work, but the audio itself is dropped on the floor.
//! Player state is global and persists across calls; the song loops
//! indefinitely (xmrsplayer's `max_loop_count = 0` default), so the
//! benchmark harness can iterate as many times as fits in its
//! wallclock window without exhausting the song's material.
//!
//! ## Why one buffer per call
//!
//! The previous version of this harness rendered 15 seconds of audio
//! per call. On weak E-cores (iPhone XS Tempest, SE2 Avalanche) per-
//! call wallclock was ~18-21 s — `pick_iters` collapsed to 1 iteration
//! per launch, which means `min == median == p99` and the per-platform
//! noise floor was just "did this single 21 s call get unlucky." The
//! 15 s was also long enough that parse + module-init overhead amounted
//! to several percent of the measurement, mixing two costs into one
//! wallclock number.
//!
//! Switching to one-buffer-per-call (1024 frames ≈ 23 ms of audio):
//!
//! - The synthesis hot path runs hundreds-to-thousands of times per
//!   launch on the M4 P-core, dozens of times on watch/iPhone E-cores
//!   (depending on `BENCH_TARGET_MS`). `pick_iters` lands at a
//!   meaningful iter count and the harness reports a real
//!   min/median/p99 distribution again.
//! - Parse + module-init runs **once** per launch (lazy `init()` on
//!   first call), so it's amortized across every measured iter and
//!   gets billed against `load_ns`, not `run_ns`. The per-iter number
//!   is just the synthesis-loop cost — exactly the dispatch-shaped
//!   work this branch's `call_indirect` elisions are aimed at.
//! - One buffer ≈ 23 ms of audio matches CoreAudio's typical
//!   per-callback budget on Apple platforms, so per-iter wallclock
//!   directly answers the production-relevant question "can this
//!   runtime sustain real-time playback on this core."
//!
//! ## Public surface
//!
//! Single C-ABI export `play_buffer(seed: i32) -> i32`. `seed` is
//! ignored (the workload is fully deterministic; the parameter exists
//! only so the export's type matches the `(i32) -> i32` shape
//! `benchmark-core::run_workload` drives every workload through).
//! Returns a running 32-bit checksum of the i16 samples produced so
//! far across the whole module's lifetime, reinterpreted as `i32`.
//! The checksum is monotonic-ish (deterministic per iter index), so
//! comparing the *last* call's return value across runs of the same
//! length is enough to validate result-correctness — the harness's
//! `result` field already does this.

#![no_std]
#![no_main]

extern crate alloc;

use alloc::boxed::Box;
use core::cell::UnsafeCell;
use core::panic::PanicInfo;

use xmrs::module::Module;
use xmrsplayer::xmrsplayer::XmrsPlayer;

// dlmalloc is the standard pure-Rust allocator for wasm32-unknown-
// unknown. xmrsplayer/xmrs allocate Vecs internally; without an
// allocator the linker would fail looking for `__rust_alloc`.
#[global_allocator]
static ALLOCATOR: dlmalloc::GlobalDlmalloc = dlmalloc::GlobalDlmalloc;

// Panic = abort. cdylib + panic=abort means we don't link the unwinder
// and rustc warns if there's no panic handler at all, so this is the
// minimum viable handler for a no_std wasm cdylib.
#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    core::arch::wasm32::unreachable()
}

// Embed the S3M fixture at compile time. Path is relative to this file.
const UNREAL_S3M: &[u8] = include_bytes!("unreal.s3m");

const SAMPLE_RATE: u32 = 44_100;

/// Stereo frames rendered per `play_buffer` call. 1024 frames at
/// 44.1 kHz is ~23.2 ms of audio, which matches typical CoreAudio
/// per-callback buffer sizes on Apple platforms (so the benchmark's
/// per-iter cost mirrors a production audio thread's per-callback
/// cost) and lands `pick_iters` at a useful iter count on every
/// device class we measure on.
const FRAMES_PER_CALL: u32 = 1024;
/// Interleaved i16 count per call (left, right, left, right, ...).
const SAMPLES_PER_CALL: u32 = FRAMES_PER_CALL * 2;

/// Persistent player state. Lazily constructed on the first call to
/// `play_buffer` and kept alive for the lifetime of the wasm
/// instance. The synthesis cursor advances with each call, so call N
/// continues exactly where call N-1 left off; the song loops
/// indefinitely (`xmrsplayer`'s default `max_loop_count = 0`), so the
/// harness can iterate however many times its wallclock budget
/// allows without exhausting the song's material.
///
/// Pulley wasm is single-threaded, so an `UnsafeCell` is sufficient
/// — no locks, no `Mutex`, no atomic state. Wrapped in a struct so
/// `unsafe impl Sync` is scoped tightly.
struct PlayerState {
    /// Lifetime: leaked `Box<XmrsPlayer<'static>>` after first init,
    /// `None` until then. The static reference inside the player
    /// points at a leaked `Box<Module>`; both leak by design — the
    /// wasm process is bounded by instance teardown which reclaims
    /// the whole linear memory.
    player: Option<&'static mut XmrsPlayer<'static>>,
    /// Running XOR checksum across every sample produced so far. Folds
    /// in the per-call sample index (rotated) so reordering would be
    /// detected, and is reduced to `i32` on return so the wasm-to-host
    /// signature stays in the `(i32) -> i32` shape every other
    /// workload uses.
    checksum: u32,
    /// Total i16 samples produced across all calls. Used as the
    /// rotation amount in the checksum-fold above.
    produced_total: u64,
}

struct PlayerCell(UnsafeCell<PlayerState>);

// SAFETY: wasm32-unknown-unknown is single-threaded; there is no way
// to access this static from two threads simultaneously. The harness
// always calls `play_buffer` on a single store, single instance.
unsafe impl Sync for PlayerCell {}

static PLAYER: PlayerCell = PlayerCell(UnsafeCell::new(PlayerState {
    player: None,
    checksum: 0,
    produced_total: 0,
}));

/// Render one audio buffer's worth of samples (`SAMPLES_PER_CALL` i16s)
/// through `xmrsplayer` and fold them into a running checksum.
///
/// On the first call, parse the embedded S3M and construct the
/// `XmrsPlayer` (one-time cost, billed by the harness against
/// `load_ns`). On every subsequent call, just advance the synthesis
/// cursor by `SAMPLES_PER_CALL` and return the updated checksum. The
/// checksum is purely a sink that prevents the whole-program
/// optimizer from concluding the loop has no observable effect; we
/// never inspect its value other than returning it.
///
/// `_seed` is ignored — the workload is fully deterministic. The
/// parameter exists only so the export's type matches the `(i32) ->
/// i32` shape `benchmark-core::run_workload` expects.
#[unsafe(no_mangle)]
pub extern "C" fn play_buffer(_seed: i32) -> i32 {
    // SAFETY: single-threaded wasm; only one mutable borrow of the
    // cell exists at a time (this function isn't reentrant —
    // xmrsplayer's `next()` doesn't call back into the host).
    let state = unsafe { &mut *PLAYER.0.get() };

    if state.player.is_none() {
        // Parse the embedded S3M. xmrs's `Module::load_s3m` is
        // fallible (truncated / malformed inputs return a
        // `DecodeError`); the fixture is checked-in and known-good,
        // so error-on-failure is acceptable here.
        let module = match Module::load_s3m(UNREAL_S3M) {
            Ok(m) => Box::new(m),
            Err(_) => return i32::MIN,
        };

        // `XmrsPlayer<'a>` borrows the `Module`; leak both into
        // `'static` so the player can live in a static cell. The
        // wasm instance only ever runs one harness session per
        // instantiation, so the leaks are bounded by instance
        // teardown reclaiming the whole linear memory.
        let module_static: &'static Module = Box::leak(module);
        let player = Box::new(XmrsPlayer::new(module_static, SAMPLE_RATE, 0));
        state.player = Some(Box::leak(player));
    }

    // SAFETY: we just constructed it above if it was None.
    let player = state.player.as_deref_mut().unwrap();

    // Pull `SAMPLES_PER_CALL` samples (one buffer's worth) and fold
    // each into the running checksum. `xmrsplayer` defaults to
    // `max_loop_count = 0` which means "loop forever," so `next()`
    // never returns None across the lifetime of a benchmark launch.
    // We still match on it for safety: if a future xmrsplayer change
    // alters the default, hitting None would silently produce
    // wrong-length output and we'd want to surface that as a stuck
    // checksum rather than a crash.
    for _ in 0..SAMPLES_PER_CALL {
        let sample = match player.next() {
            Some(s) => s,
            None => break,
        };
        // Sign-extend i16 → i32 → u32 before XOR so negative samples
        // don't fold the same as their absolute values. Rotate by the
        // running sample index so re-ordering would change the
        // checksum.
        let rot = (state.produced_total & 31) as u32;
        state.checksum ^= ((sample as i32) as u32).rotate_left(rot);
        state.produced_total = state.produced_total.wrapping_add(1);
    }

    // Fold the running total in too so the returned value reflects
    // "how many samples we've produced over the lifetime of the
    // instance." This makes silent regressions (player stops yielding
    // early, harness changes per-call sample budget without us
    // noticing) visible as a checksum drift across runs.
    let count_lo = state.produced_total as u32;
    (state.checksum ^ count_lo) as i32
}
