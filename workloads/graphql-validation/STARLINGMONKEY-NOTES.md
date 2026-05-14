# StarlingMonkey evaluation — deferred, not adopted

## Context

When we first set out to build a JS-on-wasm benchmark workload that exercises
the same `call_indirect`-heavy dispatch shapes graphql-js's validation uses,
[StarlingMonkey](https://github.com/bytecodealliance/StarlingMonkey) was the
obvious choice: a SpiderMonkey-on-wasm runtime with weval-based AOT, run
under Bytecode Alliance maintenance, sharing tooling with wasmtime. We invested
~half a day getting the build pipeline working end-to-end before deciding to
defer.

## What worked

- `cmake -DWEVAL=ON` configure (~70 s), `cmake --build --target starling`
  (~5 min) downloaded prebuilt SpiderMonkey artifacts and produced
  `cmake-build-release/starling-raw.wasm` (10.8 MB) and
  `starling-ics.wevalcache` (1.2 MB).
- `componentize.sh driver.bundle.mjs -o graphql-validation.component.wasm`
  ran weval against the SpiderMonkey runtime + our 597 KB esbuild bundle of
  graphql-js validation, producing a 31 MB Component.
- The Component's `wasi:cli/run@0.2.10` entry point did execute under
  `wasmtime run -S http` — the validation work happens at runtime (not
  preinit-baked) once the driver was restructured to register a `fetch`
  event listener instead of running validation at module top level.

## Why we stopped

Three converging issues:

### 1. The output is a Component, not a core module

StarlingMonkey produces WASI-2 Components, not core wasm modules. Our
`benchmark-core` harness uses the core-module API
(`wasmtime::Module::from_binary` + `Linker<()>` + `Instance::new`). Running a
Component requires the `wasmtime::component::*` API + `wasmtime-wasi` host
implementations.

This adds ~MB of dependencies to the static lib for our watch / iOS app
targets, which is the whole reason we care about pure-interpreter
deployment in the first place. Ironic.

### 2. The import surface is enormous and not whittle-able by configuration

The 31 MB Component imports **81 host functions across 11 WASI 0.2
namespaces**:

| count | namespace |
|---:|---|
| 35 | `wasi:http/types@0.2.10` |
| 20 | `wasi_snapshot_preview1` |
|  9 | `wasi:io/streams@0.2.10` |
|  5 | `wasi:sockets/tcp@0.2.10` |
|  3 | `wasi:io/poll@0.2.10` |
|  3 | `wasi:clocks/monotonic-clock@0.2.10` |
|  2 | `wasi:random/random@0.2.10` |
|  1 | `wasi:sockets/tcp-create-socket@0.2.10` |
|  1 | `wasi:sockets/instance-network@0.2.10` |
|  1 | `wasi:http/outgoing-handler@0.2.10` |
|  1 | `wasi:cli/environment@0.2.10` |

We tried `cmake -DENABLE_BUILTIN_*=OFF` to drop the unused builtins. The
builtin dependency graph blocks meaningful whittling:

- `web_event` requires `web_abort` (the AbortSignal type).
- `web_url` requires `web_worker_location` + `web_crypto` + `web_blob`.
- `web_fetch_fetch_event` (which is our entry point — `addEventListener('fetch', ...)`)
  requires `web_fetch` + `web_performance` + `web_worker_location`.

Even with the safer disable set
(`web_streams`, `web_timers`, `web_form_data`, `web_structured_clone`,
`web_base64`, `web_file`, `wpt_support` — all OFF), the entry-point chain
still drags in `wasi:http/types` (35 imports on its own — Request/Response
resource types) and `wasi:http/outgoing-handler`. `cmake-build-minimal/`
holds the half-finished attempt.

### 3. Wizer + weval semantics fight against runtime measurement

When we put validation work at module top level (the natural JS pattern),
wizer ran it during componentize-time and snapshotted the post-validation
heap. The 200 validations during componentize were "Log: graphql-validation:
ran 200 validations" — at runtime the wasm just exited (or trapped on
`unreachable`) without redoing any work.

To get validation to run at runtime, the script had to register an event
listener. The `addEventListener('fetch', ...)` pattern works, but the
runtime invocation path then routes through `wasi:http/incoming-handler` —
which is what reintroduces all the `wasi:http` imports. There is no
"plain `_start` entry point that runs JS at runtime" path in
StarlingMonkey AOT mode.

## The pivot

Hand-written graphql-shape benchmarks in two parallel paths:

- **AssemblyScript** at `assemblyscript/` — full-fidelity classes/interfaces/
  closures, mirrors graphql-js's exact names and dispatch shapes. Compiles
  to a small core wasm with single-digit imports.
- **Porffor-compatible JS** at `porffor/` — same names + dispatch shapes,
  restructured to work around Porffor's closure-capture limits. See
  `PORFFOR-NOTES.md` for what Porffor can't do.

Both produce small core wasm modules (target: <200 KB compiled) with no
WASI dependencies, slot directly into our existing `benchmark-core`, and
preserve the dispatch patterns we want to optimize against.

## Artifacts retained for future reference

- `StarlingMonkey/cmake-build-release/` — the working build (WEVAL=ON, all
  builtins). Re-run `cmake --build` to rebuild.
- `StarlingMonkey/cmake-build-minimal/` — the half-finished disabled-builtins
  attempt. Currently fails to link (web_event needs web_abort etc.) but
  documents the dependency graph for anyone investigating later.
- `graphql-validation.component.wasm` (31 MB) — the working AOT'd Component.
  Useful if we ever want to compare the *real* graphql-js wasm shape against
  our hand-written approximation.
- `decomposed/graphql-validation-core.fat.wat` (480 MB) — the WAT export of
  the core module inside the Component (after `wasm-tools component
  unbundle`). Reference for verifying our hand-written port produces
  similar dispatch shapes.

## When to revisit

Two scenarios that would justify another pass at StarlingMonkey:

- **A `_start`-only entry point lands in StarlingMonkey** (issue worth
  filing upstream): a way to run JS at runtime without going through the
  fetch event handler / WASI HTTP. That eliminates the wasi:http import
  bloat.
- **wasmtime gains a way to run a Component as a core module** with stub
  WASI: less likely, but would let us treat the existing 31 MB Component as
  if it were a core module, no `wasmtime::component::*` integration needed
  on our end.

Until then, the hand-written benchmarks are the right call.
