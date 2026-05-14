# Porffor evaluation for graphql-js validation benchmark

## Summary

Tried using [Porffor](https://github.com/CanadaHonk/porffor) v0.61.13 to AOT-compile
graphql-js's validation path to a small core wasm module (no embedded JS engine,
no WASI-2 component scaffolding). **Hit a fundamental closure-capture
limitation** that blocks any non-trivial JS, including the esbuild bundle of
`import { buildSchema, parse, validate } from "graphql"`.

## Bug: closures don't capture function parameters or function-local bindings

Porffor's closure support (the `closures` pref, on by default) handles
**module-level** `var`/`let`/`const` captured by inner functions, but does
**not** handle:

1. Function parameters captured by inner functions
2. Function-local `var`/`let`/`const` captured by inner functions

This blocks essentially every higher-order-function pattern in real JS:
factories, currying, partial application, visitors, event handlers,
`Array.prototype.map(fn => ...)` where `fn` references the outer scope, etc.

### Minimal repros

```js
// Repro 1 — function parameter captured by inner arrow
var f = (z) => () => z;
console.log(f(99)());
// Expected: 99
// Actual:   ReferenceError: z is not defined
```

```js
// Repro 2 — function-local let captured by inner declared function
function host() {
  let local = 30;
  function inner() { return local; }
  return inner;
}
console.log(host()());
// Expected: 30
// Actual:   ReferenceError: local is not defined
```

```js
// Repro 3 — esbuild's commonjs polyfill (the immediate failure on the
// graphql-js bundle):
var __commonJS = (cb, mod) => function __require() {
  return mod || (cb[Object.getOwnPropertyNames(cb)[0]])((mod = { exports: {} }).exports, mod), mod.exports;
};
// Whenever the returned __require runs:
// ReferenceError: cb is not defined (in __require)
```

```js
// Repro 4 — workaround attempt: hoist parameter to function-local. Doesn't
// help; function-locals also fail to capture.
var f = (z) => {
  const w = z;
  return () => w;
};
console.log(f(99)());
// Expected: 99
// Actual:   ReferenceError: w is not defined
```

### What works (control)

```js
// Module-level let/const/var captured by inner: WORKS
let counter = 0;
var increment = () => { counter++; return counter; };
console.log(increment()); // 1
console.log(increment()); // 2
```

### Bug 2 — `this` capture inside method's arrow callback **silently produces zero**

```js
// Repro 5 — silent correctness bug, no error
class Mapper {
  constructor(m) { this.m = m; }
  apply(arr) { return arr.map(x => x * this.m); }
}
console.log(new Mapper(7).apply([1,2,3]).join(","));
// Expected: 7,14,21
// Actual:   0,0,0
```

A pure-imperative version of the same method with no arrow callback works
correctly:

```js
class Mapper {
  constructor(m) { this.m = m; }
  apply(arr) {
    var result = new Array(arr.length);
    for (var i = 0; i < arr.length; i++) result[i] = arr[i] * this.m;
    return result;
  }
}
console.log(new Mapper(7).apply([1,2,3]).join(",")); // 7,14,21 — correct
```

This second bug is arguably worse than the first because it produces wrong
output instead of an error. Any code using `Array#map`/`#filter`/etc. with a
callback that needs `this` from the enclosing method gets silent corruption.

### What still works for our shape

After triangulating, the patterns we need for graphql-js-style validation
*can* be expressed in Porffor without closures:

- ✓ Functions as first-class values stored in arrays, called by index
- ✓ Object property → function dispatch (`visitor[node.kind]` style)
- ✓ Classes with constructor + methods using `this` *imperatively* (no arrow
  callbacks inside methods)
- ✓ Imperative for-loops everywhere (no `Array#map`/`#filter`/`#reduce`)

So Option C (a Porffor-compatible benchmark mirroring graphql-js's dispatch
shape) IS viable, just stylistically un-idiomatic. State has to be threaded
explicitly via constructor → `this`, and every "callback-receiving" idiom has
to be unrolled to a for-loop. The actual `call_indirect` shapes (function
arrays, dynamic property → function → call) are preserved.

### Bug 3 — class field that holds a plain object loses dynamic-key entries

```js
// Repro 6 — write-then-read on a class instance's plain-object field, where
// either side uses a dynamic key, silently misses.
class Schema { constructor() { this.types = {}; } }
class Foo { constructor(name) { this.name = name; } }

var s = new Schema();
var t = new Foo("Person");

s.types[t.name] = t;
// Expected: both reads return t
console.log(s.types["Person"]); // undefined  ← BUG
console.log(s.types[t.name]);    // undefined  ← BUG
// Same code on a non-class-field (var types = {} at module scope) works.
// Same code with this.types being a Map works.
```

Workarounds:
- Use `new Map()` for any string-keyed lookup table stored as a class field.
- Or hoist the map to module scope and reference it from within the class.

This combines badly with bug 1 (no closures over locals), so module-level
`var foo = new Map()` is often the only working pattern when the rule needs
contextual lookup data.

### Pinpoint by source

`compiler/semantic.js` (the closure-handling pass, gated behind
`Prefs.closures = true` by default at `compiler/prefs.js:1`). Multiple TODOs in
the codebase confirm this is known incomplete:

- `compiler/builtins/function.ts:28` — *"todo: no good way to bind without
  dynamic functions or closure yet, just return function"*
- `compiler/builtins/json.ts:281` — *"todo: not globals when closures work
  well"*
- `compiler/precompile.js:60` — runtime precompile passes `--no-closures` (so
  Porffor's own runtime explicitly avoids depending on closures)

## What this means for our benchmark

graphql-js relies on closure capture of function parameters and function-local
variables in the validation path: every visitor function, every rule
constructor (`function ExistingFragmentNamesRule(context) { return { Fragment(node) { ... } } }`),
every middleware-style helper. The first thing the esbuild bundle does is run
`__commonJS`, which fails on closure capture before we ever reach graphql code.

There is no obvious workaround at the JS source level — neither hoisting to
locals, nor restructuring as IIFE-returning-arrow, nor explicitly assigning
through declared variables, gets past the closure barrier. A complete fix
requires Porffor's `semantic.js` pass to track and synthesize captures across
all function boundaries, not just module boundaries.

## Suggested path forward

**Pivot to AssemblyScript** for our JS-on-wasm benchmark workload. Trade-offs:

- AssemblyScript is a strict TypeScript subset → graphql-js itself probably
  won't compile (it relies on JS-specific dynamic typing, `Object.create`,
  prototype manipulation that AssemblyScript rejects).
- We hand-write a "graphql-validation-shape" benchmark in AssemblyScript that
  exercises the same dispatch patterns (interface inheritance, visitor over
  AST, `instanceof` checks). Less authentic than real graphql-js, but gives us
  a small core wasm module full of `call_indirect` to benchmark optimizations
  against.

**Keep this directory as the actionable-issue draft** — copy the four repros
above into a Porffor issue when ready. The fix is well-scoped (semantic.js
needs to handle non-module scopes) and the maintainer is responsive; it may
land on a future Porffor release and unlock the more ambitious graphql-js path.

## Reproduction

Tested on:
- Porffor 0.61.13 (`git clone` of `main` at this date)
- Node v24.14.1
- macOS arm64

```sh
git clone https://github.com/CanadaHonk/porffor.git
cd porffor
npm install
echo 'var f = (z) => () => z; console.log(f(99)());' | ./porf -
# ReferenceError: z is not defined
```
