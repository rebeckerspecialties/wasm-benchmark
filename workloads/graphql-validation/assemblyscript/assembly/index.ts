// Top-level entry. Two exports:
//
//   _start()           — WASI-style entry that runs the validation
//                        pipeline once, suitable for `wasmtime run`.
//
//   validate_once(_)   — i32 -> i32 entry suitable for the
//                        benchmark-core harness (which calls a typed
//                        i32->i32 export with no host imports). Returns
//                        the number of validation errors found.
//
// The shape of the call is what's interesting: rules.map(rule => rule(ctx))
// → visitInParallel(...) → visit(...). Inside, the per-node dispatch
// takes the form `enterList[i]?.apply(...)` — that's the indirect-call
// shape we want optimization passes to hit.

import { buildSchema, parse } from "./fixture";
import { validate } from "./validate";

// Module-scope storage for the most recent error count. _start has a
// `void` return but `wasmtime run --invoke` lets us inspect a getter.
let lastErrorCount: i32 = 0;

// validate_once: harness entry. Argument is unused (matches the i32->i32
// shape benchmark-core expects). Returns error count.
export function validate_once(_arg: i32): i32 {
  const schema = buildSchema();
  const documentAST = parse();
  const errors = validate(schema, documentAST);
  lastErrorCount = errors.length;
  return errors.length;
}

// _start: WASI-style entry. AssemblyScript exports the start function
// when --exportStart=_start is set in asconfig. We just call validate_once
// and stash the result — verification with `wasmtime run` confirms it
// runs without trapping.
export function _start(): void {
  validate_once(0);
}

// getLastErrorCount: convenience getter so `wasmtime run --invoke` can
// confirm the validate result without needing host log bindings.
export function getLastErrorCount(): i32 {
  return lastErrorCount;
}
