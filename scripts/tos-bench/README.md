# tos-bench

Where should an interpreter keep its operand stack's height and top between tail-called
handlers, and what does each choice cost in registers? (watch report, "Registers for the value
stack"; tinywasm discussion #78)

`gen.py` writes `src/lib.rs` (the variants, `program` and `run`) and `src/main.rs` (the macOS
CLI). Every variant implements the same stack-machine handlers in up to
64 copies each:
- `c` const, `g` local.get, `s` local.set, `t` local.tee;
- `b` binop, `u` in-place update, `l` load, `w` store;
- `S` and `W`, the local.set and store that leave the stack empty (the parser knows the height).

They are dispatched like tinywasm's nightly tail-call dispatch: `become` through a table, with the
executor, the instruction slice, the instruction pointer and the next instruction as arguments.
The variants differ only in where the stack's height and top live:

| variant | height | top |
|---|---|---|
| `vec` | `Vec` length through the executor (tinywasm today) | in memory |
| `sp` | handler argument | in memory |
| `sp_wt` | handler argument | handler argument, written through; a pop reloads it |
| `sp_wt_nr` | handler argument | as `sp_wt`, but `S` and `W` skip the reload |
| `sp_wb`, `sp_wb_nr` | handler argument | handler argument, stored when a push covers it |
| `tos_flag` | handler argument | cached behind a validity flag (a branch per push and pop) |
| `vec_tos` | `Vec` length | handler argument |
| `sp_spill` | the ninth integer argument, so on the stack | in memory |
| `sp_wt_pn` | `sp_wt` with `extern "rust-preserve-none"` handlers | |
| `sp_wt_nr_ni` | `sp_wt_nr` without the Instruction argument (handlers reload it) | |
| `sp_wt_nr_sl` | `sp_wt_nr` with the stack's slice as two more arguments (`rust-preserve-none`) | |
| `sp_wt_nr_sl_ni` | `sp_wt_nr_sl` without the Instruction argument: eight, under the Rust ABI | |

The program is 64 random statements (277 dispatches) over 16 locals and a 1024-word heap; every
handler picks one of *m* copies of its kind. All variants print the same checksum.

    python3 gen.py
    RUSTFLAGS="-Z merge-functions=disabled" rustup run nightly-2026-07-05 cargo build --release
    ./target/release/tosb <variant> <copies 1..=64> <iterations> [runs] [seed] [statements]

Measuring on the E-cores (nothing else running):

    OUT=sweep.txt ./sweep.sh && python3 table.py sweep.txt       # VARIANTS=, MS=, REPS=
    WORK=<dir> ./pmu-retry.sh ds:vec:64 bottlenecks:sp_wt_nr:64  # mode:variant:copies
    python3 pmu_table.py <dir>/pmu.jsonl

`pmu.sh` needs `opts-m4-ds.json`, `opts-m4-bottlenecks.json` and `opts-m4-processing.json` in
`WORK` (see `scripts/run-m4-pmu-pass.sh`). It deletes each capture's kernel trace but never one that
another process holds open. `pmu-retry.sh` retries captures that come back empty, which happens
while another tool is recording.

Where the state lives in machine code on other targets (release, no LTO; `-C linker=/usr/bin/true`
keeps a foreign target's link from failing):

    RUSTFLAGS="-Z merge-functions=disabled -C linker=/usr/bin/true" CARGO_PROFILE_RELEASE_LTO=false \
      rustup run nightly-2026-07-05 cargo rustc --release --target x86_64-pc-windows-msvc \
      -Z build-std=std,panic_abort --bin tosb -- --emit asm -C llvm-args=-x86-asm-syntax=intel
    python3 asm_regs.py target/x86_64-pc-windows-msvc/release/deps/tosb.s

`abi_probe.py <work dir>` counts, per target and calling convention, how many integer and float
arguments arrive in registers.

## On the phones

`build-ios.sh` builds the iOS app with benchmark-core's `tos-model` feature into
`apps/build/DerivedData-tw-tosb`. The feature turns every variant into `tos-bench <variant> (m=<copies>)`
cases of 554,000 dispatches per call, which run under any engine:

    scripts/tos-bench/build-ios.sh
    REPS=5 WORKLOADS="tos-bench" UDID=<udid> DEVICE_NAME=<name> scripts/tinywasm-ab-iphone.sh <out> tosb
    python3 scripts/tinywasm_ab_summary.py <out> tosb --csv <out>.csv
    python3 scripts/tos-bench/device_table.py <out>.csv

On the iPhone 12, the CPU Counters come from `scripts/run-device-pmu.sh` with one row per case:

    UDID=00008101-000A044A3C28801E RUNTIMES_LIST=tinywasm CAPTURE_MS=6000 BENCH_TARGET_MS=8000 \
      WORKLOADS_LIST="tos-bench vec (m=64);tos-bench sp_wt_nr (m=64)" \
      MODES="bottleneck:discarded_sampling bottleneck:bottlenecks" scripts/run-device-pmu.sh <pmu-out>
    python3 scripts/tos-bench/device_pmu_table.py <pmu-out>/pmu.jsonl <out>.csv

A launch stalls if another tool brings its own app to the foreground on the phone. The launcher
then waits for its timeout, so end the stalled launch and rerun the cases it missed.
