# patches/ — upstream-PR-prep patch stack

Each patch in this directory is the same content as a commit on
one of our forks, exported via `git format-patch -k -1
--no-signature <sha>` for clean upstream submission. **You don't
need to apply these to build** — the submodule URLs in
`../.gitmodules` already point to the fork branches where these
patches are committed.

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
