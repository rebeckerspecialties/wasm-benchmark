# AssemblyScript evaluation for graphql-shape benchmark

Companion to `PORFFOR-NOTES.md` (Option C). This documents what we hit
implementing the same graphql-js-shape benchmark in
[AssemblyScript](https://www.assemblyscript.org/) v0.28.17 (Option A).

## What worked

- Compile pipeline: `npx asc assembly/index.ts --target release --use abort=` →
  `build/release.wasm` (61 KB).
- **Zero imports.** `--use abort=` aliases AssemblyScript's only otherwise-needed
  host import (`env.abort`) to a no-op — bounds violations now trap directly
  via `unreachable`. No host glue required.
- All canonical graphql-js names preserved: `validate`, `parse`, `buildSchema`,
  `visit`, `visitInParallel`, `getEnterLeaveForKind`, `BREAK`,
  `ValidationContext.reportError`, `GraphQLError`. Rule classes
  (`ExecutableDefinitionsRule`, `UniqueOperationNamesRule`,
  `KnownTypeNamesRule`, `FieldsOnCorrectTypeRule`,
  `FragmentsOnCompositeTypesRule`) match too.
- Hot dispatch shape preserved in `MergedEnterLeaveDispatcher.dispatchEnter` /
  `.dispatchLeave`. The branch structure around the indirect call (`enterList[i]`
  → `if !== null` → call → `if result === false` / `=== BREAK` /
  `!== undefined`) matches graphql-js's `mergedEnterLeave.enter` exactly.

## Bug 1 (AS) — `AS100: Not implemented: Closures`

This is the headline limitation. AssemblyScript v0.28.17 cannot pass closures
as function references — exactly the pattern graphql-js uses for visitor
construction. The compiler explicitly rejects it with:

```
AS100: Not implemented: Closures
```

(I had earlier said AS supported closures since v0.20 — that's misremembered.
Closures over outer-scope variables work *within direct calls*, but **passing
a closure as a function reference, e.g. storing it in an array**, is what's
unimplemented.)

### Workarounds we used

1. **Module-global state instead of closure capture.** Rule constructors set
   `g_context`, `g_knownOperationNames`, etc. (file-level `let` bindings)
   instead of closing over `context`. Visitor methods read from those
   globals.

2. **Hand-coded shim functions per node kind** (42 of them, `enter_NAME`,
   `leave_NAME`, etc.). Each shim is a bare function (no closure) registered
   into a `Map<string, EnterFn>` for the merged dispatcher. The shim
   forwards to a `g_enter_dispatchers.get(kind)` call.

3. **`MergedEnterLeaveDispatcher` as a class with method-form dispatch**
   instead of an inline `{ enter(...), leave(...) }` object literal. This
   keeps the indirect-call shape (the loop body has the same `enterList[i]`
   call pattern) but moves the receiver bookkeeping onto class fields rather
   than closure captures.

4. **Drop the `apply(this, args)` form.** graphql-js uses
   `enterList[i].apply(visitors[i], args)` — AS function references support
   direct call only, no `apply` / `call` machinery. Validation rules don't
   actually use `this`, so we call `enterList[i](node)` directly. The
   indirect-call site and surrounding branches are preserved; only the
   `this` argument is dropped.

## Bug 2 (AS) — non-nullable element type traps on holey arrays

```ts
const skipping: Array<ASTNode> = new Array<ASTNode>(visitors.length);
skipping.fill(null);
// RangeError on the first read because AS's __get validates non-null.
```

Fix: type as `Array<ASTNode | null>` so the slot can semantically hold null.
Matches graphql-js's `null`-initialized `skipping` array.

## Consequence: `call_indirect` density is much lower than Porffor's

Even with the workarounds, AS's compiler has more freedom to lower dispatch
to direct calls because:

- The 42 shim functions are bare `function` declarations, statically
  resolvable from their use sites.
- `MergedEnterLeaveDispatcher` is a class with known type at every call
  site; the method dispatch can be devirtualized.
- Module-global state means the function references *stored in arrays* are
  module-level constants, which AS can sometimes inline through.

Result: **only 13 `call_indirect` instructions in the 61 KB output**, vs.
**98 in Porffor's 121 KB**. The Porffor version is the more graphql-js-shaped
megamorphic-dispatch benchmark, exactly because Porffor doesn't aggressively
devirtualize.

## When to prefer which

| | AssemblyScript | Porffor |
|---|---|---|
| Tight wasm size | ✓ 61 KB | 121 KB |
| Zero imports | ✓ | 1 (host print) |
| Many `call_indirect` | 13 | ✓ 98 |
| Idiomatic source | TypeScript | classes-not-closures |
| Compile speed | fast | fast |
| Extensible by others | ✓ wider toolchain | research-grade |

For our optimization work (memory.copy lowering, AOT IC seeding, opcode-swap
IC), the **Porffor wasm is the primary benchmark** because the
megamorphic-dispatch pattern is what we're targeting. The AS wasm is a useful
secondary baseline — same workload, "compiler-friendly" lowering — and its
results show how much the optimizer can already do statically when the source
isn't constrained by Porffor's closure issues.

## Validation-result divergence (open question)

- **AS:** `validate: errors=0` — query is valid against the schema
- **Porffor:** `validate: errors=14` — same fixture reportedly, 14 errors

We haven't tracked down the divergence yet. Possible causes:
- Slightly different fixture (one schema/query has a typo)
- Rule implementation differences (one is stricter)
- `KnownTypeNamesRule` traversal differences

Doesn't block benchmarking — both produce real validation work over the AST
and exercise the dispatch pipeline. Worth investigating if/when we add more
rules or want exact parity.
