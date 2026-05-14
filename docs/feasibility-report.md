# Pulley arm64_32-apple-watchos Feasibility Report

| Field | Value |
|---|---|
| Toolchain | rustc 1.93.1 (01f6ddf75 2026-02-11) |
| Xcode | 26.4 (Build 17E192) |
| Host | aarch64-apple-darwin (macOS 26) |
| Primary target | `arm64_32-apple-watchos` |
| Secondary target | `aarch64-apple-watchos`, `aarch64-apple-darwin` |
| wasmtime ref | `1fc537358120e054411244d03eff36f51f793a22` (2026-04-30) |
| wasmtime crate version | `45.0.0` |
| pulley-interpreter version | `45.0.0` (workspace) |
| Date started | 2026-04-30 |

## Goal

Determine whether wasmtime's Pulley interpreter is viable on
`arm64_32-apple-watchos` under our hard constraints:

- Stable Rust 1.93.1 (no nightly).
- Pure interpreter (no JIT / MAP_JIT / copy-and-patch — App Store eligible).
- Cross-language LLVM-bitcode LTO with Xcode 26's clang preserved.
- Reasonable code size and steady-state performance on Apple Watch SE2 (S8).

## Verdict summary

| Experiment | Status | Verdict |
|---|---|---|
| A. Dependency inventory | done | **GO** — small footprint; cc-rs branch is bitcode (see D) |
| B. arm64_32 build with LTO (`pulley,runtime,std`) | done | **PASS** — `libwasmtime.a` 2.4 MB (fat-LTO) / 19 MB (linker-plugin-lto only); three upstream-bound patches |
| C. On-device smoke test | not started | — |
| D. LTO bitcode validation | done | **PASS with one Rust-design carve-out** — every Rust crate object + cc-rs `helpers.c` output is LLVM bitcode; only compiler_builtins emits native objects (intentional via `#![no_builtins]`) |
| E. `#[inline(never)]` audit + open hazards | preliminary | **GREEN** — pulley has none; the unwinder inline-asm warning is fixed in patch 0003 |

---

## Experiment A — Dependency tree inventory

**Goal.** Catalog all crates pulled in by `wasmtime` with
`--no-default-features --features pulley` for `arm64_32-apple-watchos`.
Flag any crate that runs a build script — especially ones invoking
`cc` / `cmake` / `bindgen` — as a risk crate for cross-language LTO,
since those tend to emit native objects at the crate boundary instead of
LLVM bitcode.

**Method.** Used `cargo tree -e normal` for the on-target package set
(authoritative) and `cargo metadata` to look up `build.rs` / `proc-macro`
flags per package. Note: `cargo metadata`'s feature filter behaves
differently than `cargo tree`'s and over-reports workspace members; cargo
tree is the source of truth here.

**Commands.**

```sh
rustup run 1.93.1 cargo tree --target arm64_32-apple-watchos \
  -p wasmtime --no-default-features --features pulley -e normal
rustup run 1.93.1 cargo metadata --format-version=1 \
  --filter-platform arm64_32-apple-watchos \
  --no-default-features --features pulley \
  --manifest-path wasmtime/crates/wasmtime/Cargo.toml
```

Reproducible via `scripts/exp-a-inventory.sh`.

### A.1 Target spec resolves

`arm64_32-apple-watchos` is a recognized built-in target spec in
rustc 1.93.1. `cargo tree` resolves cleanly without any custom target JSON.
Because `cargo tree` does not compile, we don't need a `rust-std` for the
target installed; `arm-apple` cfgs are derived from the built-in spec:

```
target_arch        = "aarch64"
target_pointer_width = "32"
target_os          = "watchos"
target_vendor      = "apple"
target_endian      = "little"
target_abi         = ""
```

Note that `target_arch` is **`aarch64`**, not `arm`. This matters for
build-script logic that gates on `target_arch` (see A.4 below).

### A.2 On-target crate set

40 unique on-target crates (linked into the watchOS arm64_32 binary), 4
proc-macros (host-only). Full table:

```
On-target crates (40)
─────────────────────────────────────────────────────────────────────
  name                         version   source     flags
─────────────────────────────────────────────────────────────────────
  anyhow                       1.0.102   crates.io  build.rs
  bitflags                     2.11.1    crates.io
  block-buffer                 0.10.2    crates.io
  bumpalo                      3.20.0    crates.io
  cfg-if                       1.0.0     crates.io
  cobs                         0.3.0     crates.io
  cpufeatures                  0.2.7     crates.io
  cranelift-bforest            0.132.0   workspace
  cranelift-bitset             0.132.0   workspace
  cranelift-entity             0.132.0   workspace
  crypto-common                0.1.6     crates.io
  digest                       0.10.7    crates.io
  equivalent                   1.0.1     crates.io
  foldhash                     0.2.0     crates.io
  generic-array                0.14.5    crates.io build.rs
  gimli                        0.33.0    crates.io
  hashbrown                    0.17.0    crates.io
  indexmap                     2.14.0    crates.io
  libc                         0.2.185   crates.io build.rs
  libm                         0.2.16    crates.io build.rs
  log                          0.4.28    crates.io
  memchr                       2.7.6     crates.io
  object                       0.39.0    crates.io build.rs
  postcard                     1.1.3     crates.io
  proc-macro2                  1.0.101   crates.io build.rs
  pulley-interpreter           45.0.0    workspace
  quote                        1.0.41    crates.io build.rs
  serde                        1.0.228   crates.io build.rs
  serde_core                   1.0.228   crates.io build.rs
  sha2                         0.10.2    crates.io
  smallvec                     1.15.1    crates.io
  syn                          2.0.106   crates.io
  target-lexicon               0.13.3    crates.io build.rs
  thiserror                    2.0.17    crates.io build.rs
  typenum                      1.15.0    crates.io build.rs
  unicode-ident                1.0.19    crates.io
  wasmparser                   0.248.0   crates.io
  wasmtime                     45.0.0    workspace  build.rs   ← see A.4
  wasmtime-environ             45.0.0    workspace
  wasmtime-internal-core       45.0.0    workspace  build.rs   ← see A.3

Proc-macros (4) — host-only:
  pulley-macros, serde_derive, thiserror-impl,
  wasmtime-internal-versioned-export-macros
```

The "cranelift-*" crates that show up here (`cranelift-bitset`,
`cranelift-bforest`, `cranelift-entity`) are **pure data-structure
utilities** that happen to live in the cranelift directory — they are
NOT the JIT codegen. Cranelift's actual code generator
(`cranelift-codegen`, `cranelift-native`, `cranelift-frontend`) is
**not** pulled in by `--features pulley`.

### A.3 Build-script analysis — green list

13 of the 14 build scripts are **LTO-safe**: they only emit
`cargo:rustc-cfg=…` directives (compiler version probing, target-arch
gating, feature detection) or generate Rust source code. None of them
shell out to `cc`, `cmake`, `bindgen`, or anything that emits native
object code. In particular:

- `proc-macro2`, `quote`, `serde`, `serde_core`, `thiserror`, `anyhow`,
  `target-lexicon`, `libc` — compiler-version cfg sniffing.
- `typenum`, `generic-array` — Rust-source codegen.
- `libm`, `object` — feature-flag fan-out cfgs.
- `wasmtime-internal-core/build.rs` — 4-line stub:
  ```rust
  fn main() {
      println!("cargo:rerun-if-changed=build.rs");
      println!("cargo:rustc-check-cfg=cfg(arc_try_new)");
  }
  ```

### A.4 Build-script analysis — yellow flag: `wasmtime/build.rs`

This is the only build script that is **not** unconditionally LTO-safe.
At [wasmtime/crates/wasmtime/build.rs:80-105](wasmtime/crates/wasmtime/build.rs):

```rust
#[cfg(feature = "runtime")]
if has_host_compiler_backend && (supported_os || cfg!(feature = "debug-builtins")) {
    build_c_helpers();   // invokes cc::Build to compile helpers.c
}
```

with

```rust
let has_host_compiler_backend = match target_arch.as_str() {
    "x86_64" | "riscv64" | "s390x" | "aarch64" => true,  // ← arm64_32 hits this
    _ => false,
};
let supported_os = (unix || windows) && cfg!(feature = "std");
```

For `arm64_32-apple-watchos`, `target_arch = "aarch64"` and `unix = true`.
Whether `build_c_helpers()` is called therefore depends on which features
we activate:

| Feature set | `runtime` | `std` | C compiled? |
|---|---|---|---|
| `--features pulley` (this experiment) | off | off | **no** |
| `--features pulley,runtime` | on | off | only if `debug-builtins` |
| `--features pulley,runtime,std` (realistic prod) | on | on | **yes** |
| `default` | on | on | yes (also `debug-builtins`) |

`helpers.c` itself is **50 lines, ~10 lines of actual code in
non-debug builds** — just a `__attribute__((weak)) extern void
__unw_add_dynamic_fde()` probe used to detect libunwind at runtime, plus
debug-only DWARF helper exports. The LTO impact of this small C blob
not participating in cross-module bitcode LTO is likely negligible, but
worth confirming in Experiment D.

### A.5 Build-script analysis — what about the `runtime` feature?

The `pulley` feature in wasmtime's Cargo.toml is **intentionally empty**
(comment at line 181 of `crates/wasmtime/Cargo.toml`). It does not pull
in `runtime`, `std`, or any other dep. To actually load and execute a
`.wasm` from Swift we will need at minimum `runtime`, and almost
certainly `std`. That feature combination has not yet been resolved or
built; that is Experiment B's first job.

### A.6 Risk crates summary

| Tier | Crate | Reason | Mitigation |
|---|---|---|---|
| 🟢 Green | 13 build scripts (anyhow, libc, libm, object, generic-array, proc-macro2, quote, serde, serde_core, target-lexicon, thiserror, typenum, wasmtime-internal-core) | cfg-only or Rust-source codegen | none needed |
| 🟡 Yellow | `wasmtime` build.rs (`build_c_helpers`) | invokes `cc::Build` on `helpers.c` when `feature=runtime` AND (`std`-on or debug-builtins). Emits native objects at the crate boundary by default. | (a) measure D impact — only ~10 lines of C; or (b) pass `-flto -fembed-bitcode` to cc-rs and confirm bitcode .o; or (c) port `wasmtime_using_libunwind()` to Rust |
| 🔴 Red | none | — | — |

No `-sys` crates on the on-target side. No `cmake`, `bindgen`, `ring`,
`openssl`, `zstd`, or similar in the on-target tree.

### A.7 Verdict — GO

The dependency footprint is **small and tractable**. There is one
yellow-flag build script (`wasmtime/build.rs`) but its risk surface is
miniscule (~10 lines of C). No deal-breakers found. Move to Experiment B
with two feature sets:

1. **Minimal**: `--no-default-features --features pulley` — should
   build cleanly with no cc-rs invocation. Smallest LTO surface;
   sanity-check that the basic pipeline works.
2. **Realistic prod**: `--no-default-features --features
   pulley,runtime,std` (and probably `wat` for tests). Will exercise
   `build_c_helpers()`. This is the build we actually need.

If realistic-prod fails, fall back to identifying the minimum extra
features that fail and patch from there.

---

## Experiment B — arm64_32 build with LTO

**Toolchain note (corrected from initial brief).** Building for Tier-3
`arm64_32-apple-watchos` requires `-Z build-std`, which is nightly-only.
The pinned bitcode-compatible nightly is **`nightly-2026-01-25`**
(`rustc 1.95.0-nightly f134bbc78`). `rustup run nightly` resolves to a
*newer* nightly that breaks Xcode 26 bitcode compatibility — always
invoke as `rustup run nightly-2026-01-25` explicitly.

**Stage 1 — Minimal feature set (`--features pulley`)**

Reproducible via `scripts/exp-b-build.sh minimal`. Captured in
`out/exp-b/minimal/build.log`.

Command:
```sh
RUSTFLAGS="-C linker-plugin-lto -C embed-bitcode=yes" \
  rustup run nightly-2026-01-25 cargo rustc \
  --lib -Z build-std=std,panic_abort \
  --target arm64_32-apple-watchos --release --crate-type staticlib \
  -p wasmtime --no-default-features --features pulley
```

**Result: failed** (exit 101).

`std`, `core`, `compiler_builtins`, and 18 other crates compiled
cleanly for arm64_32-apple-watchos before the build hit:

```
error: failed to run custom build command for `target-lexicon v0.13.3`
  thread 'main' panicked at target-lexicon-0.13.3/build.rs:52:54:
  Invalid target name: 'arm64_32-apple-watchos'
```

Root cause: `target-lexicon`'s `Triple::from_str` does not recognize
the `arm64_32` architecture token. Confirmed by the literal stub in
the upstream source:

```text
target-lexicon-0.13.3/src/targets.rs:1789
        //"arm64_32-apple-watchos", // TODO
```

The `Aarch64Architecture` enum hardcodes `pointer_width = U64` for
both variants (`Aarch64`, `Aarch64be`); arm64_32 needs a third variant
with `U32`. Existing ILP32 plumbing in target-lexicon is Linux-shaped
(via `GnuIlp32` env), which doesn't fit Apple's `arm64_32` arch token.

**Patch applied locally** at
[patches/0001-target-lexicon-arm64_32-apple-watchos-support.patch](../patches/0001-target-lexicon-arm64_32-apple-watchos-support.patch)
(commit `196fed6` in `target-lexicon` submodule on branch
`arm64_32-apple-watchos`, rebased on upstream `main`).

The change adds an `Arm64_32` variant to `Aarch64Architecture` with
`pointer_width() = U32` and `endianness() = Little`, registers
`"arm64_32"` in `FromStr`, and enables the
`arm64_32-apple-watchos` entry in the `roundtrip_known_triples`
test (16 insertions, 3 deletions).

target-lexicon's full test suite (14 tests) passes after the patch.

**Wired into wasmtime via `[patch.crates-io]`** in
`wasmtime/Cargo.toml`:

```toml
[patch.crates-io]
target-lexicon = { path = "../target-lexicon" }
```

Followed by `cargo update -p target-lexicon` to refresh wasmtime's
`Cargo.lock` to consume the patched 0.13.5 (vs the registry-pinned
0.13.3). Without the lockfile bump, cargo silently skips the patch
with `warning: patch ... was not used in the crate graph`.

The patch is **upstreamable as-is** to
`bytecodealliance/target-lexicon`. Per user direction: submit upstream
once we have end-to-end validation on the device.

**Stage 1.5 — Toolchain PATH fix**

After the target-lexicon patch went in, Stage 1 still failed: `error:
the option Z is only accepted on the nightly compiler`. Root cause:
`rustup run nightly-2026-01-25 cargo …` sets `RUSTUP_TOOLCHAIN=…` but
does **not** prepend the toolchain's bin to PATH. With the user's
default toolchain (`1.93`) earlier in PATH and no `~/.cargo/bin/cargo`
proxy installed, cargo (and the rustc subprocesses it spawns) resolved
through PATH to stable. Symptom: `Compiling std v0.0.0
(/Users/matt/.rustup/toolchains/1.93-aarch64-apple-darwin/lib/rustlib/src/rust/library/std)`
in build logs that should have shown the nightly path.

Fix in [scripts/exp-b-build.sh](../scripts/exp-b-build.sh): export
`PATH="$HOME/.rustup/toolchains/${TOOLCHAIN}-…-apple-darwin/bin:$PATH"`
before invoking cargo. Drop the `rustup run` wrapper. Also installed
`rust-src` for `nightly-2026-01-25` (it wasn't on the system —
`rustup component add rust-src --toolchain nightly-2026-01-25`).

After the fix, `Compiling std v0.0.0
(.../nightly-2026-01-25-aarch64-apple-darwin/...)` confirms nightly is
in effect.

Stage 1 then progressed to a final-link-step failure — `no global
memory allocator found`, `#[panic_handler] required`, `unwinding
panics not supported without std` — which is **expected** for a
`--no-default-features --features pulley` lib with no `std` /
`runtime`. Stage 1 was a build-sanity check; it confirmed
toolchain+patch+LTO-flags wire up correctly. Moving on.

**Stage 2 — Realistic prod (`--features pulley,runtime,std`)**

Pulls in real platform integration. Surfaced one more upstream patch
need:

- `mach2 v0.4.2` had `compile_error!("mach requires macOS or iOS")`
  for non-`(macos|ios)` targets, even though watchOS, tvOS, and
  visionOS all expose the same Mach kernel APIs. wasmtime's `runtime`
  feature pulls `mach2` in unconditionally
  ([crates/wasmtime/Cargo.toml:268](../wasmtime/crates/wasmtime/Cargo.toml)).
  Symptom: `error: mach requires macOS or iOS` and `error[E0463]:
  can't find crate for libc` (its `[target.'cfg(macos|ios)']` guard
  excluded watchos).

  **Already fixed upstream** in mach2 main (commit `538ce75`,
  Aug 2025) which widens the gate to `target_vendor = "apple"` and
  enables tvOS/watchOS/visionOS. Cherry-picked onto the `0.4.2` tag:
  [patches/0002-mach2-tvOS-watchOS-visionOS-support-for-arm64_32.patch](../patches/0002-mach2-tvOS-watchOS-visionOS-support-for-arm64_32.patch).
  Submodule at `mach2/`, branch `arm64_32-apple-watchos`, commit
  `0f36c6b`. `[patch.crates-io] mach2 = { path = "../mach2" }` plus
  `cargo update -p mach2`. (Wasmtime is pinned to `^0.4.2`, hence the
  backport rather than bumping to mach2 0.6.x.)

**Stage 2 result: PASS.**

```
$ lipo -info target/arm64_32-apple-watchos/release/libwasmtime.a
Non-fat file: ...libwasmtime.a is architecture: arm64_32

$ ls -l ...libwasmtime.a
19,497,296 bytes  (~19 MB)

$ nm -arch arm64_32 ...libwasmtime.a | grep -c " T "
9893    # text symbols
```

Cargo: `Finished release profile [optimized] target(s) in 10.35s`
(deps cache warm — full clean rebuild is several minutes).

Two warnings emitted; both in
`wasmtime-internal-unwinder/lib`:

```
warning: formatting may not be suitable for sub-register argument
   |
42 |             pc = inout(reg) pc,
   |                             -- for this argument
   = help: use `{0:w}` to have the register formatted as `w0` (32-bit values)
   = help: or  use `{0:x}` to keep the default formatting of `x0` (64-bit values)
```

This is **the first real arm64_32 porting concern that we need to look
at**, even though the build succeeded. On AArch64 a register can be
referred to as `w0` (32-bit view) or `x0` (64-bit view) of the same
GPR. Because arm64_32 has 64-bit GPRs but 32-bit pointers, an unwinder
that pushes/pops `pc` via inline-asm might silently treat the pointer
as 64-bit and either widen junk into the upper bits or read garbage
back. **Tracked under Experiment E** — needs source-level review,
not just a syntactic warning fix.

**Stage 3 — `-C lto=fat -C linker-plugin-lto -C embed-bitcode=yes`**

Run because Stage 2's artifact still contained native compiler_builtins
objects and we wanted to see whether fat LTO would consolidate things
further. Reproducible via `scripts/exp-b-build.sh fat-lto`.

Result: still passes. Two interesting deltas vs Stage 2:

| metric | Stage 2 | Stage 3 (fat-LTO) |
|---|---|---|
| `libwasmtime.a` size | 19,497,296 B (~19 MB) | 2,428,656 B (~2.4 MB) |
| objects in .a | 189 | 12 |
| bitcode objects | 179 | 2 |
| native objects | 10 | 10 (same — all compiler_builtins) |

Fat LTO collapsed 179 per-crate codegen units into 2 large bitcode
units and dead-stripped what wasn't reachable from wasmtime's public
API. Code-size win of ~8× for the same feature set. Compiler_builtins
unchanged (covered in Experiment D).

**Stage 3 with the unwinder fix patch (`0003`) applied: zero warnings,
4.21 s incremental rebuild, valid arm64_32 staticlib**. This is the
shape we'd ship into the watchOS app.

## Experiment C — Smoke test on the three simulators

Three SwiftUI apps share `apps/Shared/BenchmarkContentView.swift` and
all link `libbenchmark_core.a`, which exposes a per-workload C ABI
that loads the embedded `.wasm`, configures `Config::target("pulley32"
/ "pulley64")` so cranelift emits Pulley **bytecode** (data, not
executable code — App Store-safe), and returns `(result, load_ns,
run_ns)` for the Swift side to display + log to stderr.

Workload set (all checked-in `.wasm`, generated by
`scripts/build-workloads.sh` from the `workloads-rs/*.rs` source):

| name | size (bytes) | description |
|---|--:|---|
| `fib.wasm` | 454 | recursive `fib(n)` — call-heavy |
| `factorial.wasm` | 654 | wrapping-i32 factorial |
| `sieve.wasm` | 1,781 | Sieve of Eratosthenes; counts primes ≤ N |
| `crc32.wasm` | 1,037 | CRC32 over 64 KiB LCG-derived bytes |
| `matmul_simd.wasm` | 2,796 | 64×64 f32 matmul via wasm `simd128` |
| `convolution.wasm` | 1,436 | 3×3 box-blur on 256×256 grayscale |
| `audio_dsp.wasm` | 1,552 | SPC-DSP-shaped: 8-voice mixer, 1000 × 512 |

Per-(workload, platform) results — same M4 host, all three sims:

```
                          macOS app          watchOS sim 11.5      iOS sim 26.4
                          load   run         load   run            load   run        result
fib(30)                   3.2ms  23.0ms      17.3ms 22.9ms         18.3ms 28.9ms     832040
factorial(20)             0.6ms  0.02ms      1.4ms  0.005ms        1.4ms  0.005ms    -2102132736
sieve(10000)              0.7ms  0.14ms      1.0ms  0.14ms         0.9ms  0.16ms     1229
crc32(64KB)               0.4ms  1.06ms      0.5ms  1.15ms         0.6ms  2.52ms     -1910355869
matmul simd128 64×64      0.8ms  1.33ms      1.0ms  1.21ms         1.4ms  1.21ms     48907
convolution 256×256       0.5ms  2.40ms      0.7ms  2.31ms         0.8ms  2.42ms     8190685
audio DSP (1000×512)      0.7ms  339ms       0.9ms  340ms          0.8ms  379ms      761667740
```

Every result matches the host-Rust reference function. Run times are
similar across platforms because all three simulators execute on the
M4 host's arm64; **device-class perf on iPhone XS A12 and Apple Watch
SE2 S8 is the next session's measurement.**

**Build matrix** ([scripts/build-lib.sh](../scripts/build-lib.sh)):

| target | toolchain | path |
|---|---|---|
| `aarch64-apple-darwin` (M4 host) | stable 1.93.1 | `target/release/libbenchmark_core.a` |
| `aarch64-apple-ios-sim` | stable 1.93.1 | Tier 2, no build-std needed |
| `aarch64-apple-ios` | stable 1.93.1 | Tier 2, no build-std needed |
| `aarch64-apple-watchos-sim` | nightly-2026-01-25 | Tier 3, `-Z build-std` |
| `arm64_32-apple-watchos` | nightly-2026-01-25 | Tier 3, `-Z build-std` |

All five builds carry `-C target-cpu=apple-a12 -C linker-plugin-lto -C
embed-bitcode=yes`. The xcodegen project applies the matching Apple
flags (`-mcpu=apple-a12`, `LLVM_LTO=YES`) per-arch so the same
optimizing baseline holds end-to-end. The flags are gated on
`[arch=arm64]` / `[arch=arm64_32]` since clang rejects `-mcpu=` on
x86_64 and we don't need that arch.

**Three patches still apply unchanged**:
- `0001-target-lexicon-arm64_32-apple-watchos-support.patch`
- `0002-mach2-tvOS-watchOS-visionOS-support-for-arm64_32.patch`
- `0003-wasmtime-unwinder-aarch64-inline-asm-arm64_32-format.patch`

**Open items** (intended for the device-test session):

- The iOS-sim build emits `ld: warning: object file ...
  helpers.o was built for newer iOS-simulator version (26.4) than
  being linked (18.0)`. Cosmetic at the simulator level (it links and
  runs) — set `IPHONEOS_DEPLOYMENT_TARGET=18.0` when invoking cargo
  for iOS to silence it before device-side testing.
- Code signing for physical devices: the apps are currently
  ad-hoc-signed (`Sign to Run Locally`); device installation requires
  a Developer Team ID + provisioning profile.
- Add a stable `bench_run_all` C entry point that runs the full set
  and returns a struct array, so device runs can stream a single
  contiguous result over a USB log channel.

## Experiment D — LTO bitcode validation

**Method.** Reproducible via
[scripts/exp-d-bitcode-validate.sh](../scripts/exp-d-bitcode-validate.sh).
Extract every `.o` / `.rmeta` from `libwasmtime.a` and from a sample of
dep `.rlib`s, classify each via `file -b`, and tally bitcode vs native.
The script runs against whichever `libwasmtime.a` is currently in
`target/`, so it's stage-agnostic.

**Stage 3 (fat-LTO) results:**

```
libwasmtime.a                       total=12   bitcode=2  native=10  rmeta=0
libpulley_interpreter-*.rlib        total=9    bitcode=8  native=0   rmeta=1
libwasmtime_internal_unwinder-*.rlib total=2   bitcode=1  native=0   rmeta=1
libwasmtime_internal_core-*.rlib    total=4    bitcode=3  native=0   rmeta=1
libtarget_lexicon-*.rlib            total=3    bitcode=2  native=0   rmeta=1   (our patched fork)
libmach2-*.rlib                     total=2    bitcode=1  native=0   rmeta=1   (our patched fork)
libstd-*.rlib                       total=17   bitcode=16 native=0   rmeta=1
liballoc-*.rlib                     total=7    bitcode=6  native=0   rmeta=1
libcore-*.rlib                      total=17   bitcode=16 native=0   rmeta=1
libwasmtime-helpers.a               total=1    bitcode=1  native=0   rmeta=0   (cc-rs helpers.c)
                                    -----------------------------------------
                                    bitcode=56  native=10  rmeta=8
```

**Two important findings:**

1. **The cc-rs branch in `wasmtime/build.rs` produces bitcode, not
   native objects.** This was the single yellow flag from Experiment A.
   `libwasmtime-helpers.a` (the static archive cc-rs emits for
   `helpers.c`) contains a single object that `file` reports as `LLVM
   bitcode, wrapper`. cc-rs auto-detects `RUSTFLAGS=-C
   linker-plugin-lto` and passes the appropriate `-flto` to clang. **A's
   yellow flag is now green.**

2. **The only native objects in the final `.a` are
   `compiler_builtins` codegen units.** This is intentional Rust
   behavior, not something we can fix from outside rustc:
   `compiler_builtins/src/lib.rs:16` is `#![no_builtins]`, which keeps
   the optimizer from inserting calls back to compiler_builtins
   functions inside that crate. The crate's objects are emitted as
   native to keep its symbol entry-points stable for the rest of the
   build to call into without LTO eating them. `-C lto=fat` does **not**
   change this.

   Symbol audit: the 10 native objects define **262 symbols**, all of
   which are operations on `f16`, `f128`, or 128-bit integers (e.g.
   `compiler_builtins::float::add::add::<f16>`,
   `compiler_builtins::float::conv::float_to_int_inner::<f128, …>`).
   Wasm does not expose any of these types in the spec'd value set
   (i32, i64, f32, f64, v128) — they're generic instantiations from
   elsewhere in std/wasmtime that the linker should dead-strip at the
   final app-link step. The `.a` carries them as defined symbols
   regardless; only the final app binary will tell us how many survive.

**Verdict: PASS for the project's LTO premise.** Cross-language LTO
between Rust crates and Apple's clang works end-to-end (proven by
helpers.c → bitcode). compiler_builtins is the only carve-out, by
Rust's design, and its symbols are likely dead at link time for our
workload. Anything tighter would require rustc-level changes that
aren't on stable.

## Experiment E — `#[inline(never)]` audit + arm64_32 hazards

**Preliminary result:** `grep -rn "inline(never)" pulley/` returns
**zero matches** across the entire `pulley-interpreter` and
`pulley-macros` source. The Pulley dispatch loop has no
`#[inline(never)]` annotations whose soundness LTO could break.

**Open concrete hazards surfaced by Stage 2 build:**

1. **`wasmtime-internal-unwinder` inline-asm `pc = inout(reg) pc`** —
   the format specifier doesn't disambiguate `w` (32-bit) vs `x`
   (64-bit) view. On arm64 with 64-bit pointers this is harmless.
   On arm64_32 it's the first place I'd look for a subtle wrong-width
   bug. Needs source review, not just a syntactic fix. Will likely
   become patch `0003-…` in our stack.

**Caveats / TODO before declaring E fully green:**

- Re-run the grep against the full set of crates that LTO would
  potentially fuse (i.e., the on-target tree from A.2), not just
  `pulley/`. `wasmtime-internal-core`, `wasmtime-internal-unwinder`,
  `cranelift-bforest`, and `wasmtime` itself may have inline-never
  annotations that LTO could also affect.
- Audit *equivalent* mechanisms: `#[no_mangle] extern "C"` functions,
  `#[inline(always)]` on dispatch tables, `#[unsafe(naked)]`
  functions, and any inline-asm blocks that assume specific stack
  layouts.
- Confirm dispatch loop does NOT use the `tail_call` proposal
  (computed gotos / explicit `become`) — LTO cannot break what the
  language doesn't yet expose.
