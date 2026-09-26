# patches/ — upstream-PR-prep patch stack

Two kinds of patch live here.

**Runtime series, applied at build time.** Each `scripts/build-<runtime>.sh`
resets its submodule to the pinned upstream gitlink and applies
`patches/<runtime>/*.patch` in order through
`scripts/apply_patch_series.sh` (idempotent: already-applied patches are
skipped). Current series (2026-09-22):

| series | base | patches | what |
|---|---|---|---|
| `wasm-micro-runtime/` | upstream `main` `b70d708d` | 0001-0012 | 0001-0010 relaxed SIMD (fork PR #3, upstream #4950), 0011-0012 opt-in PROT_NONE linear-memory reservation (fork PR #4). The fork branches are the authoritative copies. The fast-interp legacy-EH series (fork PRs #1 and #2) was retired on 2026-09-26: exnref supersedes legacy EH. |
| `wasmedge/` | 0.17.2-rc.3 | 0001-0003, 0006-0028 (26) | Apple-mobile guarded-memory fallbacks, interpreter super-instructions, arm64_32 fixes. 0004 retired earlier; 0005 retired in the 0.17.2-rc.3 rebase (upstream). Each rebased patch records its conflict resolution in its message. |
| `zwasm/` | v2.7.0 | 0001-0002 | 0001 compiles the JIT out of the C API when `-Dengine=interp`; 0002 restores the arm64_32-apple-watchos ILP32 static-lib build. The earlier arm64_32 patch landed as zwasm#98. |
| `wasmz/` | v0.1.4 | 0002 | arm64_32-apple-watchos support, reworked for v0.1.4. The Zig 0.16 port (0001) landed as wasmz#3. |
| wasm3 | v0.9.0 | — | the v128-as-opaque-slot patch landed as wasm3#559. |

A patch is never dropped silently: when one lands upstream, the commit
that retires it says where.

**Fork-branch exports, not applied by any build.** The wasmtime,
target-lexicon and mach2 changes are commits on our fork branches (the
submodule URLs in `../.gitmodules` point there); the files below are
`git format-patch -k -1 --no-signature <sha>` exports of 2026-05
branches for review and upstream submission. The wasmtime submodule now
pins `pulley-bench-stack-v49` (v49.0.0 plus the soundness-fixed split of
PR #2 and phase 4); fusion phases 1–3 below are not on it.

These files exist so:
- The change is reviewable as a standalone diff alongside this repo's
  history, without having to follow a submodule pointer.
- We can quickly send an upstream PR by rebasing the patch onto each
  project's `main` and pushing the result.

## Patches

### `0001-target-lexicon-arm64_32-apple-watchos-support.patch`

Adds `Arm64_32` variant to `target_lexicon::Aarch64Architecture` so
`Triple::from_str("arm64_32-apple-watchos")` resolves correctly.
Upstream: `bytecodealliance/target-lexicon`. Fork branch:
`rebeckerspecialties/target-lexicon@arm64_32-apple-watchos`.

### `0002-mach2-tvOS-watchOS-visionOS-support-for-arm64_32.patch`

Mach2 syscall bindings for the Apple non-iOS-non-macOS family
(tvOS / watchOS / visionOS), needed for wasmtime's task-info / time
queries on Apple Watch. Note: wasmtime's bump to `mach2 = "0.6"`
covers what we need; this fork patch is no longer required for the
production build but stays available for older mach2 consumers.
Upstream: `JohnTitor/mach2`. Fork branch:
`rebeckerspecialties/mach2@arm64_32-apple-watchos`.

### `0003-wasmtime-unwinder-aarch64-inline-asm-arm64_32-format.patch`

`wasmtime/crates/wasmtime/src/runtime/vm/sys/unix/unwind/aarch64.rs`
inline-asm fix: register-bearing locals are typed `u64` on aarch64
but pointer-sized (`usize`) on arm64_32-apple-watchos, breaking
LLVM bitcode emission. The patch unconditionally types the locals
as `u64` to match the inline-asm constraint. **Currently open as
upstream PR #13259** (`unwinder-arm64_32-asm-format` branch on the
wasmtime fork). PR #2's `table-mutability-tracking` branch is
stacked on top of it.

### `pulley-fusion-xband-brif/` (3 commits)

Phase 1 of the opcode-fusion track from PR #2's description: fuses
`band -2 + brif` into a single Pulley dispatch at the call_indirect
lazy-init site when the table is eagerly initialized. See
[docs/opcode-fusion-band-brif.md](../docs/opcode-fusion-band-brif.md)
for the full design + soundness + test coverage writeup.

These three patches stack on top of `table-mutability-tracking` on
the wasmtime fork. To apply:

```sh
cd wasmtime
git checkout -b claude/pulley-fusion-xband-brif table-mutability-tracking
git am ../patches/pulley-fusion-xband-brif/*.patch
git push -u origin claude/pulley-fusion-xband-brif    # needs gh auth
```

| # | subject |
|---|---------|
| 1 | `pulley: add xband_s8 + br_if fused dispatch ops` |
| 2 | `cranelift: Lower::sink_pure_inst — absorb pure ALU ops into terminators` |
| 3 | `cranelift/pulley: fuse band+brif at call_indirect lazy-init site` |

This branch was prepared in a cloud sandbox without push creds for
the wasmtime fork; the patches are the canonical hand-off. PMU /
wallclock measurement on iPhone 12 is the next step — see the doc.

### `pulley-fusion-funcref-dispatch/` (5 commits)

Phase 2 of the opcode-fusion track: fuses `brif + xload64 + xload64`
at the call_indirect lazy-init brif site into a single Pulley
`xfuncref_dispatch_*` op (the preceding `xband_s8 v, -2` stays as
a separate op in phase 2). Stacks on top of phase 1's branch tip.
See [docs/opcode-fusion-funcref-dispatch.md](../docs/opcode-fusion-funcref-dispatch.md).

### `pulley-fusion-band-funcref-dispatch/` (1 commit)

Phase 3: absorbs the preceding standalone `xband_s8 v, -2` into the
phase-2 `xfuncref_dispatch_*` op, emitting one
`xband_funcref_dispatch_*` Pulley op that covers the entire mask-
and-load tail. Dispatch tail at the call_indirect lazy-init site
goes from 5 ops (baseline) to **2** ops. See
[docs/opcode-fusion-band-funcref-dispatch.md](../docs/opcode-fusion-band-funcref-dispatch.md).

### `pulley-fusion-call-indirect-args/` (3 commits)

Phase 4 (commits 1–2): mirrors `Inst::Call`'s `call{1,2,3,4}`
arg-bundling for `Inst::IndirectCall`. Adds Pulley opcodes
`call_indirect{1,2,3,4}` that combine `xmov xN, argN` ABI fixups
with the indirect call into one dispatch. Cranelift side adds a
new `PulleyCallIndirect { target, args }` payload (mirror of
`PulleyCall`) so the first 0–4 integer ABI args bypass regalloc's
`reg_fixed_use` mechanism and are moved by the call opcode at
call time. Dispatch tail shrinks from phase-3's 2 ops to 1 fused
op (`xband_funcref_dispatch_*` + `call_indirect1`) per
call_indirect lazy-init site. See
[docs/four-way-baseline-phase3-phase4-wamr.md](../docs/four-way-baseline-phase3-phase4-wamr.md).

Commit 3 (correctness fix): trap on null in the 8 fused
funcref-dispatch handlers (phase 2 + phase 3 ops) instead of
falling through to the lazy-init `null_block`. Phase 2/3's load
absorption removed the loads from `continuation_block`, so the
slow path's `null_block → lazy_init → jump continuation` would
land in a continuation block with no loads — `call_indirect`
would observe uninitialized `dst_code`/`dst_vmctx`. The fusion is
gated on `is_eagerly_initialized_funcref_table` so the slow path
is unreachable in correct code; trapping defends against future
predicate unsoundness by failing closed.

| # | subject |
|---|---------|
| 1 | `pulley: add call_indirect{1,2,3,4} fused indirect-call ops` |
| 2 | `cranelift/pulley: pass first 4 indirect-call args via call_indirectN` |
| 3 | `pulley: trap on null in 8 fused funcref-dispatch handlers` |

iPhone 12 A14 Icestorm wallclock vs phase 3 (N=10, vtable suite):
**vtable_poly4 −8.94 %, vtable_bi −6.71 %, vtable_poly6 −3.72 %**.
iPhone XS A12 Tempest recovers phase-3's call_indirect regression
(−4.77 % vs phase 3, back to baseline parity).

## Workflow — sending an upstream PR

```sh
# 1. Update your fork's branch to the latest upstream + patch:
cd target-lexicon
git fetch origin main
git rebase origin/main arm64_32-apple-watchos
git push fork arm64_32-apple-watchos --force-with-lease

# 2. Open / update the upstream PR from that branch.

# 3. Regenerate this patch file in this repo:
cd ../  # back to wasm-benchmark root
git -C target-lexicon format-patch -1 --no-signature \
  --stdout arm64_32-apple-watchos \
  > patches/0001-target-lexicon-arm64_32-apple-watchos-support.patch
```

If a patch ever fails to apply against a refreshed upstream main:
the fork branch is the authoritative copy; re-export from there
after rebasing the fork.
