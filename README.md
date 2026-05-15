# wasm-benchmark

WebAssembly interpreter benchmark harness targeting Apple Silicon
deployment platforms — Apple Watch (arm64_32-apple-watchos), iPhone
(aarch64-apple-ios), and Mac (aarch64-apple-darwin). Compares
[wasmtime](https://github.com/bytecodealliance/wasmtime)'s **Pulley**
interpreter against [WAMR](https://github.com/bytecodealliance/wasm-micro-runtime)
on dispatch-heavy workloads (synthetic `call_indirect`, real-world
xmrsplayer tracker player, sqlite3 speedtest1, graphql-validation
in two ports). The harness drives the
[per-table-mutability optimization stack on the wasmtime fork](https://github.com/rebeckerspecialties/wasmtime/pull/2).

App-Store constraint: **pure-interpreter only**, no JIT / MAP_JIT /
copy-and-patch. Pulley is the only legal wasmtime runtime in that
space.

## Quickstart

```sh
# Clone with submodules
git clone --recurse-submodules https://github.com/<your-fork>/wasm-benchmark.git
cd wasm-benchmark

# First-time setup: rustup targets, optional wasmtime working clone
./scripts/setup.sh

# Build .wasm workloads (writes workloads/*.wasm)
./scripts/build-workloads.sh

# Build benchmark-core static lib for a platform
./scripts/build-lib.sh macos        # M-series host
./scripts/build-lib.sh ios          # iPhone (aarch64-apple-ios)
./scripts/build-lib.sh watchos      # arm64_32-apple-watchos
./scripts/build-lib.sh all          # everything

# Run the M4 host CLI (E-core pinned)
cargo build --release --bin run_dispatch_workloads
taskpolicy -b ./target/release/run_dispatch_workloads
```

iOS / watchOS app builds via `xcodebuild` from `apps/`. See
[AGENTS.md](AGENTS.md) for full build/deploy/measurement procedures.

## Repo layout

```
apps/                    SwiftUI app — iOS / watchOS / macOS targets
crates/benchmark-core/   Rust library — Pulley + WAMR adapters, harness
docs/                    project docs (feasibility report, dispatch
                         modes, IC investigation results, ...)
patches/                 upstream-PR-prep patches for each dependency
scripts/                 build, setup, PMU analysis
workloads-rs/            standalone Rust sources for *.wasm workloads
workloads-rs-cargo/      cargo-based wasm workload (xmrsplayer-bench)
workloads/               pre-built *.wasm files (checked in)
# --- submodules ---
mach2/                   pinned to fork's arm64_32-apple-watchos branch
target-lexicon/          pinned to fork's arm64_32-apple-watchos branch
wasm-micro-runtime/      pinned to upstream main
porffor/                 pinned to upstream main (JS→wasm AOT)
sightglass/              pinned to upstream main (sqlite3.wasm source)
# --- not tracked, set up via setup.sh ---
wasmtime/                gitignored — working clone with active PR
                         branches; see AGENTS.md → wasmtime section
```

## Where to read more

- **[AGENTS.md](AGENTS.md)** — deep agent context: toolchain pinning,
  build commands, workload registration pattern, measurement
  methodology, xctrace gotchas, QoS env var, the Pulley dispatch
  loop mode selection.
- **[docs/feasibility-report.md](docs/feasibility-report.md)** —
  original arm64_32-apple-watchos feasibility analysis.
- **[docs/ic-investigation-results.md](docs/ic-investigation-results.md)** —
  N=10 cross-platform IC measurement closeout. The IC investigation
  is closed; pulley super-op fusion is the next direction (see
  PR #2's description).
- **[docs/opcode-fusion-band-brif.md](docs/opcode-fusion-band-brif.md)** —
  Phase 1 of the opcode-fusion track: `xband_s8 + br_if` fused into
  one Pulley dispatch at call_indirect lazy-init sites. **Measurement
  closed out 2026-05-14 — hypothesis falsified, wallclock flat,
  Discarded +7.87 % regression on iPhone 12 PMU. Next step: skip to
  proposal (2) `funcref_load_dispatch`.**
- **[docs/archived-ic-branches.md](docs/archived-ic-branches.md)** —
  SHAs for the deleted `pulley-call-indirect-ic*` branches, in case
  a future 2-way-IC or poisoning variant wants that baseline.
- **[patches/README.md](patches/README.md)** — patch-stack workflow
  for the wasmtime + mach2 + target-lexicon upstream PRs.

## Current upstream-PR state

- **wasmtime PR #2 on the fork** (`table-mutability-tracking` branch):
  per-table mutability + four `call_indirect` dispatch elisions.
  11 commits, 2227 + 16 tests pass. See
  https://github.com/rebeckerspecialties/wasmtime/pull/2
- **wasmtime upstream PR #13259**: unwinder arm64_32 inline-asm
  format fix. Stacked under PR #2.
- **target-lexicon + mach2** patches in `patches/`, branches on
  the fork. Both have rebeckerspecialties forks with a single
  arm64_32-apple-watchos branch each.

## License

Dual-licensed under MIT or Apache-2.0 at your option — same conventions
as the `bytecodealliance/*` and `gfx-rs/wgpu` projects. See
[LICENSE-MIT](LICENSE-MIT) and [LICENSE-APACHE](LICENSE-APACHE).

Unless you explicitly state otherwise, any contribution intentionally
submitted for inclusion in the work by you, as defined in the
Apache-2.0 license, shall be dual licensed as above, without any
additional terms or conditions.
