// Mirrors graphql-js's language/visitor.mjs.
//
// The hot path is `visitInParallel`'s mergedEnterLeave.enter/leave: per
// visited node, walk an array of indirect calls (one per rule), with a
// `skipping[i]` predicate around each. That `enterList[i]?.()` indirect
// call is the structural feature optimization passes pattern-match against,
// so the surrounding branches mirror graphql-js exactly.
//
// AssemblyScript-specific deviations (with rationale):
//
//   - graphql-js dispatches via `enterList[i].apply(visitors[i], args)`.
//     AS function references are first-class but lack `Function.apply`,
//     so we use typed function pointers (EnterFn/LeaveFn) and call them
//     directly. Indirect-call shape is preserved.
//
//   - graphql-js writes the merged enter/leave as inline closures over
//     enterList/leaveList/skipping. AS's closure support is restrictive
//     for cross-function mutable captures, so we replace closures with a
//     `MergedEnterLeaveDispatcher` class that owns the per-kind table
//     and exposes `dispatchEnter(node)` / `dispatchLeave(node)` with
//     identical control flow.
//
//   - graphql-js's `BREAK` is a frozen sentinel object compared by ===.
//     We mirror that with a `BreakSentinel` ASTNode subclass.

import {
  ASTNode,
  VisitorKeys,
  defaultVisitorKeys,
} from "./ast";
import { KIND_VALUES } from "./kinds";

// Result codes mirror graphql-js's enter/leave return contract:
//   undefined -> CONTINUE      (most common: no opinion)
//   false     -> SKIP_SUBTREE  (don't descend; only meaningful from enter)
//   BREAK     -> ABORT         (stop the whole traversal)
// (Editing — returning a replacement node — isn't exercised by validation
// rules, so we omit it from the sum type.)
export const enum VisitorResult {
  CONTINUE = 0,
  SKIP_SUBTREE = 1,
  BREAK = 2,
}

// Sentinel preserved for fidelity with graphql-js's `export const BREAK`.
// Rules that want to abort the traversal `return BREAK` from an enter/leave.
export const BREAK: VisitorResult = VisitorResult.BREAK;

// Enter/Leave function types. Match graphql-js shape minus the parent /
// path / ancestors trailing args (validation rules in this benchmark
// don't consult them).
export type EnterFn = (node: ASTNode) => VisitorResult;
export type LeaveFn = (node: ASTNode) => VisitorResult;

// EnterLeaveVisitor — graphql-js's `{ enter?, leave? }` shape.
export class EnterLeaveVisitor {
  enter: EnterFn | null;
  leave: LeaveFn | null;

  constructor(enter: EnterFn | null = null, leave: LeaveFn | null = null) {
    this.enter = enter;
    this.leave = leave;
  }
}

// Visitor — AssemblyScript analogue of graphql-js's plain object visitor
// `{ Document(node) {…}, Field: { enter, leave }, … }`. We store one
// EnterLeaveVisitor per kind, plus optional generic enter/leave that fire
// when no kind-specific entry is registered (mirrors getEnterLeaveForKind's
// fallback path).
export class Visitor {
  perKind: Map<string, EnterLeaveVisitor>;
  enter: EnterFn | null;
  leave: LeaveFn | null;

  constructor() {
    this.perKind = new Map<string, EnterLeaveVisitor>();
    this.enter = null;
    this.leave = null;
  }

  // Register a kind-specific function that runs on enter only. Mirrors
  // graphql-js's `{ Kind(node) {…} }` shorthand (no leave).
  on(kind: string, fn: EnterFn): Visitor {
    this.perKind.set(kind, new EnterLeaveVisitor(fn, null));
    return this;
  }

  // Register full `{ enter, leave }` shape for a kind.
  onEnterLeave(kind: string, enter: EnterFn | null, leave: LeaveFn | null): Visitor {
    this.perKind.set(kind, new EnterLeaveVisitor(enter, leave));
    return this;
  }
}

// getEnterLeaveForKind mirrors graphql-js's homonymous helper.
// Returns the kind-specific entry if registered; otherwise the generic
// enter/leave fallback.
export function getEnterLeaveForKind(visitor: Visitor, kind: string): EnterLeaveVisitor {
  if (visitor.perKind.has(kind)) {
    return visitor.perKind.get(kind);
  }
  return new EnterLeaveVisitor(visitor.enter, visitor.leave);
}

// BreakSentinel: a unique ASTNode subclass used as the "BREAK" marker in
// the skipping[] array. Reference equality picks it out from real nodes.
export class BreakSentinel extends ASTNode {
  constructor() {
    super("__BREAK__");
  }
  getChildren(_key: string): ASTNode[] | null { return null; }
  getChild(_key: string): ASTNode | null { return null; }
}

// MergedEnterLeaveDispatcher — class form of graphql-js's
// `mergedEnterLeave` object inside visitInParallel. Holds the per-kind
// enter/leave tables for one kind, plus shared skipping[] and visitors[]
// references (passed in once at construction). dispatchEnter / dispatchLeave
// run the per-visitor loop with the canonical branch shape.
//
// The hot loop in dispatchEnter mirrors graphql-js exactly:
//
//     for (let i = 0; i < visitors.length; i++) {
//       if (skipping[i] === null) {
//         const result = enterList[i]?.apply(visitors[i], args);
//         if (result === false)            skipping[i] = node;
//         else if (result === BREAK)       skipping[i] = BREAK;
//         else if (result !== undefined)   return result;
//       }
//     }
class MergedEnterLeaveDispatcher {
  enterList: Array<EnterFn | null>;
  leaveList: Array<LeaveFn | null>;
  skipping: Array<ASTNode | null>;
  breakSentinel: ASTNode;

  constructor(
    enterList: Array<EnterFn | null>,
    leaveList: Array<LeaveFn | null>,
    skipping: Array<ASTNode | null>,
    breakSentinel: ASTNode,
  ) {
    this.enterList = enterList;
    this.leaveList = leaveList;
    this.skipping = skipping;
    this.breakSentinel = breakSentinel;
  }

  dispatchEnter(node: ASTNode): VisitorResult {
    const enterList = this.enterList;
    const skipping = this.skipping;
    for (let i = 0; i < enterList.length; i++) {
      // Skip predicate around the indirect call. Mirrors `skipping[i] === null`.
      if (skipping[i] === null) {
        const fn = enterList[i];
        // The structural `enterList[i]?.apply(...)` indirect call.
        let result: VisitorResult = VisitorResult.CONTINUE;
        if (fn !== null) {
          result = fn(node);
        }

        if (result == VisitorResult.SKIP_SUBTREE) {
          skipping[i] = node;
        } else if (result == VisitorResult.BREAK) {
          skipping[i] = this.breakSentinel;
        }
        // Editing branch (`result !== undefined && result !== BREAK && result !== false`)
        // is structurally present in graphql-js but unused by validation rules.
      }
    }
    return VisitorResult.CONTINUE;
  }

  dispatchLeave(node: ASTNode): VisitorResult {
    const leaveList = this.leaveList;
    const skipping = this.skipping;
    for (let i = 0; i < leaveList.length; i++) {
      if (skipping[i] === null) {
        const fn = leaveList[i];
        let result: VisitorResult = VisitorResult.CONTINUE;
        if (fn !== null) {
          result = fn(node);
        }

        if (result == VisitorResult.BREAK) {
          skipping[i] = this.breakSentinel;
        }
      } else if (skipping[i] === node) {
        // Reached the leave for the node that triggered SKIP — clear it.
        skipping[i] = null;
      }
    }
    return VisitorResult.CONTINUE;
  }
}

// We need to bind the dispatcher's methods as plain `EnterFn` /
// `LeaveFn` values for the Visitor's per-kind table. AS doesn't have
// `.bind`, so we do it via a small one-shot dispatcher table per kind:
// a singleton MergedEnterLeaveDispatcher pointer is stashed in module
// globals (`g_enter_dispatchers[kindIdx]`), and the EnterFn we register
// is a tiny shim that loads and calls dispatchEnter/dispatchLeave for
// the kind. AS supports static module-globals fine.
let g_enter_dispatchers: Map<string, MergedEnterLeaveDispatcher> = new Map<string, MergedEnterLeaveDispatcher>();

function dispatchEnterFor(kind: string, node: ASTNode): VisitorResult {
  const d = g_enter_dispatchers.get(kind);
  return d.dispatchEnter(node);
}
function dispatchLeaveFor(kind: string, node: ASTNode): VisitorResult {
  const d = g_enter_dispatchers.get(kind);
  return d.dispatchLeave(node);
}

// One shim per kind (so the EnterFn / LeaveFn refs are stable function pointers).
// We pre-generate them with a tiny code-gen-style approach: each shim closes
// over its kind name as a constant. AS can do this with literal arrow funcs.
class KindShim {
  enter: EnterFn;
  leave: LeaveFn;
  constructor(enter: EnterFn, leave: LeaveFn) {
    this.enter = enter;
    this.leave = leave;
  }
}

// We'll register the shim functions per-kind as plain top-level functions so
// they can be taken as function references. This avoids closures.
//
// Approach: visitInParallel installs each kind's MergedEnterLeaveDispatcher
// into g_enter_dispatchers under that kind name, then registers the EnterFn
// for that kind to call dispatchEnterFor(kind, node). This *would* be a
// closure over `kind`, but we can avoid that by using only-one set of
// merged dispatchers active at a time and a per-kind static shim.
//
// Concrete strategy: define one EnterFn per kind in code, hard-coding the
// kind constant. There are 42 kinds; below we generate them by hand.

// --- Per-kind enter/leave shims ---

import { Kind } from "./kinds";

function enter_NAME(node: ASTNode): VisitorResult { return dispatchEnterFor(Kind.NAME, node); }
function leave_NAME(node: ASTNode): VisitorResult { return dispatchLeaveFor(Kind.NAME, node); }
function enter_DOCUMENT(node: ASTNode): VisitorResult { return dispatchEnterFor(Kind.DOCUMENT, node); }
function leave_DOCUMENT(node: ASTNode): VisitorResult { return dispatchLeaveFor(Kind.DOCUMENT, node); }
function enter_OPERATION_DEFINITION(node: ASTNode): VisitorResult { return dispatchEnterFor(Kind.OPERATION_DEFINITION, node); }
function leave_OPERATION_DEFINITION(node: ASTNode): VisitorResult { return dispatchLeaveFor(Kind.OPERATION_DEFINITION, node); }
function enter_VARIABLE_DEFINITION(node: ASTNode): VisitorResult { return dispatchEnterFor(Kind.VARIABLE_DEFINITION, node); }
function leave_VARIABLE_DEFINITION(node: ASTNode): VisitorResult { return dispatchLeaveFor(Kind.VARIABLE_DEFINITION, node); }
function enter_SELECTION_SET(node: ASTNode): VisitorResult { return dispatchEnterFor(Kind.SELECTION_SET, node); }
function leave_SELECTION_SET(node: ASTNode): VisitorResult { return dispatchLeaveFor(Kind.SELECTION_SET, node); }
function enter_FIELD(node: ASTNode): VisitorResult { return dispatchEnterFor(Kind.FIELD, node); }
function leave_FIELD(node: ASTNode): VisitorResult { return dispatchLeaveFor(Kind.FIELD, node); }
function enter_ARGUMENT(node: ASTNode): VisitorResult { return dispatchEnterFor(Kind.ARGUMENT, node); }
function leave_ARGUMENT(node: ASTNode): VisitorResult { return dispatchLeaveFor(Kind.ARGUMENT, node); }
function enter_FRAGMENT_SPREAD(node: ASTNode): VisitorResult { return dispatchEnterFor(Kind.FRAGMENT_SPREAD, node); }
function leave_FRAGMENT_SPREAD(node: ASTNode): VisitorResult { return dispatchLeaveFor(Kind.FRAGMENT_SPREAD, node); }
function enter_INLINE_FRAGMENT(node: ASTNode): VisitorResult { return dispatchEnterFor(Kind.INLINE_FRAGMENT, node); }
function leave_INLINE_FRAGMENT(node: ASTNode): VisitorResult { return dispatchLeaveFor(Kind.INLINE_FRAGMENT, node); }
function enter_FRAGMENT_DEFINITION(node: ASTNode): VisitorResult { return dispatchEnterFor(Kind.FRAGMENT_DEFINITION, node); }
function leave_FRAGMENT_DEFINITION(node: ASTNode): VisitorResult { return dispatchLeaveFor(Kind.FRAGMENT_DEFINITION, node); }
function enter_VARIABLE(node: ASTNode): VisitorResult { return dispatchEnterFor(Kind.VARIABLE, node); }
function leave_VARIABLE(node: ASTNode): VisitorResult { return dispatchLeaveFor(Kind.VARIABLE, node); }
function enter_INT(node: ASTNode): VisitorResult { return dispatchEnterFor(Kind.INT, node); }
function leave_INT(node: ASTNode): VisitorResult { return dispatchLeaveFor(Kind.INT, node); }
function enter_FLOAT(node: ASTNode): VisitorResult { return dispatchEnterFor(Kind.FLOAT, node); }
function leave_FLOAT(node: ASTNode): VisitorResult { return dispatchLeaveFor(Kind.FLOAT, node); }
function enter_STRING(node: ASTNode): VisitorResult { return dispatchEnterFor(Kind.STRING, node); }
function leave_STRING(node: ASTNode): VisitorResult { return dispatchLeaveFor(Kind.STRING, node); }
function enter_BOOLEAN(node: ASTNode): VisitorResult { return dispatchEnterFor(Kind.BOOLEAN, node); }
function leave_BOOLEAN(node: ASTNode): VisitorResult { return dispatchLeaveFor(Kind.BOOLEAN, node); }
function enter_NULL(node: ASTNode): VisitorResult { return dispatchEnterFor(Kind.NULL, node); }
function leave_NULL(node: ASTNode): VisitorResult { return dispatchLeaveFor(Kind.NULL, node); }
function enter_ENUM(node: ASTNode): VisitorResult { return dispatchEnterFor(Kind.ENUM, node); }
function leave_ENUM(node: ASTNode): VisitorResult { return dispatchLeaveFor(Kind.ENUM, node); }
function enter_LIST(node: ASTNode): VisitorResult { return dispatchEnterFor(Kind.LIST, node); }
function leave_LIST(node: ASTNode): VisitorResult { return dispatchLeaveFor(Kind.LIST, node); }
function enter_OBJECT(node: ASTNode): VisitorResult { return dispatchEnterFor(Kind.OBJECT, node); }
function leave_OBJECT(node: ASTNode): VisitorResult { return dispatchLeaveFor(Kind.OBJECT, node); }
function enter_OBJECT_FIELD(node: ASTNode): VisitorResult { return dispatchEnterFor(Kind.OBJECT_FIELD, node); }
function leave_OBJECT_FIELD(node: ASTNode): VisitorResult { return dispatchLeaveFor(Kind.OBJECT_FIELD, node); }
function enter_DIRECTIVE(node: ASTNode): VisitorResult { return dispatchEnterFor(Kind.DIRECTIVE, node); }
function leave_DIRECTIVE(node: ASTNode): VisitorResult { return dispatchLeaveFor(Kind.DIRECTIVE, node); }
function enter_NAMED_TYPE(node: ASTNode): VisitorResult { return dispatchEnterFor(Kind.NAMED_TYPE, node); }
function leave_NAMED_TYPE(node: ASTNode): VisitorResult { return dispatchLeaveFor(Kind.NAMED_TYPE, node); }
function enter_LIST_TYPE(node: ASTNode): VisitorResult { return dispatchEnterFor(Kind.LIST_TYPE, node); }
function leave_LIST_TYPE(node: ASTNode): VisitorResult { return dispatchLeaveFor(Kind.LIST_TYPE, node); }
function enter_NON_NULL_TYPE(node: ASTNode): VisitorResult { return dispatchEnterFor(Kind.NON_NULL_TYPE, node); }
function leave_NON_NULL_TYPE(node: ASTNode): VisitorResult { return dispatchLeaveFor(Kind.NON_NULL_TYPE, node); }
function enter_SCHEMA_DEFINITION(node: ASTNode): VisitorResult { return dispatchEnterFor(Kind.SCHEMA_DEFINITION, node); }
function leave_SCHEMA_DEFINITION(node: ASTNode): VisitorResult { return dispatchLeaveFor(Kind.SCHEMA_DEFINITION, node); }
function enter_OPERATION_TYPE_DEFINITION(node: ASTNode): VisitorResult { return dispatchEnterFor(Kind.OPERATION_TYPE_DEFINITION, node); }
function leave_OPERATION_TYPE_DEFINITION(node: ASTNode): VisitorResult { return dispatchLeaveFor(Kind.OPERATION_TYPE_DEFINITION, node); }
function enter_SCALAR_TYPE_DEFINITION(node: ASTNode): VisitorResult { return dispatchEnterFor(Kind.SCALAR_TYPE_DEFINITION, node); }
function leave_SCALAR_TYPE_DEFINITION(node: ASTNode): VisitorResult { return dispatchLeaveFor(Kind.SCALAR_TYPE_DEFINITION, node); }
function enter_OBJECT_TYPE_DEFINITION(node: ASTNode): VisitorResult { return dispatchEnterFor(Kind.OBJECT_TYPE_DEFINITION, node); }
function leave_OBJECT_TYPE_DEFINITION(node: ASTNode): VisitorResult { return dispatchLeaveFor(Kind.OBJECT_TYPE_DEFINITION, node); }
function enter_FIELD_DEFINITION(node: ASTNode): VisitorResult { return dispatchEnterFor(Kind.FIELD_DEFINITION, node); }
function leave_FIELD_DEFINITION(node: ASTNode): VisitorResult { return dispatchLeaveFor(Kind.FIELD_DEFINITION, node); }
function enter_INPUT_VALUE_DEFINITION(node: ASTNode): VisitorResult { return dispatchEnterFor(Kind.INPUT_VALUE_DEFINITION, node); }
function leave_INPUT_VALUE_DEFINITION(node: ASTNode): VisitorResult { return dispatchLeaveFor(Kind.INPUT_VALUE_DEFINITION, node); }
function enter_INTERFACE_TYPE_DEFINITION(node: ASTNode): VisitorResult { return dispatchEnterFor(Kind.INTERFACE_TYPE_DEFINITION, node); }
function leave_INTERFACE_TYPE_DEFINITION(node: ASTNode): VisitorResult { return dispatchLeaveFor(Kind.INTERFACE_TYPE_DEFINITION, node); }
function enter_UNION_TYPE_DEFINITION(node: ASTNode): VisitorResult { return dispatchEnterFor(Kind.UNION_TYPE_DEFINITION, node); }
function leave_UNION_TYPE_DEFINITION(node: ASTNode): VisitorResult { return dispatchLeaveFor(Kind.UNION_TYPE_DEFINITION, node); }
function enter_ENUM_TYPE_DEFINITION(node: ASTNode): VisitorResult { return dispatchEnterFor(Kind.ENUM_TYPE_DEFINITION, node); }
function leave_ENUM_TYPE_DEFINITION(node: ASTNode): VisitorResult { return dispatchLeaveFor(Kind.ENUM_TYPE_DEFINITION, node); }
function enter_ENUM_VALUE_DEFINITION(node: ASTNode): VisitorResult { return dispatchEnterFor(Kind.ENUM_VALUE_DEFINITION, node); }
function leave_ENUM_VALUE_DEFINITION(node: ASTNode): VisitorResult { return dispatchLeaveFor(Kind.ENUM_VALUE_DEFINITION, node); }
function enter_INPUT_OBJECT_TYPE_DEFINITION(node: ASTNode): VisitorResult { return dispatchEnterFor(Kind.INPUT_OBJECT_TYPE_DEFINITION, node); }
function leave_INPUT_OBJECT_TYPE_DEFINITION(node: ASTNode): VisitorResult { return dispatchLeaveFor(Kind.INPUT_OBJECT_TYPE_DEFINITION, node); }
function enter_DIRECTIVE_DEFINITION(node: ASTNode): VisitorResult { return dispatchEnterFor(Kind.DIRECTIVE_DEFINITION, node); }
function leave_DIRECTIVE_DEFINITION(node: ASTNode): VisitorResult { return dispatchLeaveFor(Kind.DIRECTIVE_DEFINITION, node); }
function enter_SCHEMA_EXTENSION(node: ASTNode): VisitorResult { return dispatchEnterFor(Kind.SCHEMA_EXTENSION, node); }
function leave_SCHEMA_EXTENSION(node: ASTNode): VisitorResult { return dispatchLeaveFor(Kind.SCHEMA_EXTENSION, node); }
function enter_SCALAR_TYPE_EXTENSION(node: ASTNode): VisitorResult { return dispatchEnterFor(Kind.SCALAR_TYPE_EXTENSION, node); }
function leave_SCALAR_TYPE_EXTENSION(node: ASTNode): VisitorResult { return dispatchLeaveFor(Kind.SCALAR_TYPE_EXTENSION, node); }
function enter_OBJECT_TYPE_EXTENSION(node: ASTNode): VisitorResult { return dispatchEnterFor(Kind.OBJECT_TYPE_EXTENSION, node); }
function leave_OBJECT_TYPE_EXTENSION(node: ASTNode): VisitorResult { return dispatchLeaveFor(Kind.OBJECT_TYPE_EXTENSION, node); }
function enter_INTERFACE_TYPE_EXTENSION(node: ASTNode): VisitorResult { return dispatchEnterFor(Kind.INTERFACE_TYPE_EXTENSION, node); }
function leave_INTERFACE_TYPE_EXTENSION(node: ASTNode): VisitorResult { return dispatchLeaveFor(Kind.INTERFACE_TYPE_EXTENSION, node); }
function enter_UNION_TYPE_EXTENSION(node: ASTNode): VisitorResult { return dispatchEnterFor(Kind.UNION_TYPE_EXTENSION, node); }
function leave_UNION_TYPE_EXTENSION(node: ASTNode): VisitorResult { return dispatchLeaveFor(Kind.UNION_TYPE_EXTENSION, node); }
function enter_ENUM_TYPE_EXTENSION(node: ASTNode): VisitorResult { return dispatchEnterFor(Kind.ENUM_TYPE_EXTENSION, node); }
function leave_ENUM_TYPE_EXTENSION(node: ASTNode): VisitorResult { return dispatchLeaveFor(Kind.ENUM_TYPE_EXTENSION, node); }
function enter_INPUT_OBJECT_TYPE_EXTENSION(node: ASTNode): VisitorResult { return dispatchEnterFor(Kind.INPUT_OBJECT_TYPE_EXTENSION, node); }
function leave_INPUT_OBJECT_TYPE_EXTENSION(node: ASTNode): VisitorResult { return dispatchLeaveFor(Kind.INPUT_OBJECT_TYPE_EXTENSION, node); }

// kindShimFor returns the (enter, leave) shim function pair for a given kind.
// These shims forward to the active dispatcher in g_enter_dispatchers.
function kindShimFor(kind: string): KindShim {
  if (kind == Kind.NAME) return new KindShim(enter_NAME, leave_NAME);
  if (kind == Kind.DOCUMENT) return new KindShim(enter_DOCUMENT, leave_DOCUMENT);
  if (kind == Kind.OPERATION_DEFINITION) return new KindShim(enter_OPERATION_DEFINITION, leave_OPERATION_DEFINITION);
  if (kind == Kind.VARIABLE_DEFINITION) return new KindShim(enter_VARIABLE_DEFINITION, leave_VARIABLE_DEFINITION);
  if (kind == Kind.SELECTION_SET) return new KindShim(enter_SELECTION_SET, leave_SELECTION_SET);
  if (kind == Kind.FIELD) return new KindShim(enter_FIELD, leave_FIELD);
  if (kind == Kind.ARGUMENT) return new KindShim(enter_ARGUMENT, leave_ARGUMENT);
  if (kind == Kind.FRAGMENT_SPREAD) return new KindShim(enter_FRAGMENT_SPREAD, leave_FRAGMENT_SPREAD);
  if (kind == Kind.INLINE_FRAGMENT) return new KindShim(enter_INLINE_FRAGMENT, leave_INLINE_FRAGMENT);
  if (kind == Kind.FRAGMENT_DEFINITION) return new KindShim(enter_FRAGMENT_DEFINITION, leave_FRAGMENT_DEFINITION);
  if (kind == Kind.VARIABLE) return new KindShim(enter_VARIABLE, leave_VARIABLE);
  if (kind == Kind.INT) return new KindShim(enter_INT, leave_INT);
  if (kind == Kind.FLOAT) return new KindShim(enter_FLOAT, leave_FLOAT);
  if (kind == Kind.STRING) return new KindShim(enter_STRING, leave_STRING);
  if (kind == Kind.BOOLEAN) return new KindShim(enter_BOOLEAN, leave_BOOLEAN);
  if (kind == Kind.NULL) return new KindShim(enter_NULL, leave_NULL);
  if (kind == Kind.ENUM) return new KindShim(enter_ENUM, leave_ENUM);
  if (kind == Kind.LIST) return new KindShim(enter_LIST, leave_LIST);
  if (kind == Kind.OBJECT) return new KindShim(enter_OBJECT, leave_OBJECT);
  if (kind == Kind.OBJECT_FIELD) return new KindShim(enter_OBJECT_FIELD, leave_OBJECT_FIELD);
  if (kind == Kind.DIRECTIVE) return new KindShim(enter_DIRECTIVE, leave_DIRECTIVE);
  if (kind == Kind.NAMED_TYPE) return new KindShim(enter_NAMED_TYPE, leave_NAMED_TYPE);
  if (kind == Kind.LIST_TYPE) return new KindShim(enter_LIST_TYPE, leave_LIST_TYPE);
  if (kind == Kind.NON_NULL_TYPE) return new KindShim(enter_NON_NULL_TYPE, leave_NON_NULL_TYPE);
  if (kind == Kind.SCHEMA_DEFINITION) return new KindShim(enter_SCHEMA_DEFINITION, leave_SCHEMA_DEFINITION);
  if (kind == Kind.OPERATION_TYPE_DEFINITION) return new KindShim(enter_OPERATION_TYPE_DEFINITION, leave_OPERATION_TYPE_DEFINITION);
  if (kind == Kind.SCALAR_TYPE_DEFINITION) return new KindShim(enter_SCALAR_TYPE_DEFINITION, leave_SCALAR_TYPE_DEFINITION);
  if (kind == Kind.OBJECT_TYPE_DEFINITION) return new KindShim(enter_OBJECT_TYPE_DEFINITION, leave_OBJECT_TYPE_DEFINITION);
  if (kind == Kind.FIELD_DEFINITION) return new KindShim(enter_FIELD_DEFINITION, leave_FIELD_DEFINITION);
  if (kind == Kind.INPUT_VALUE_DEFINITION) return new KindShim(enter_INPUT_VALUE_DEFINITION, leave_INPUT_VALUE_DEFINITION);
  if (kind == Kind.INTERFACE_TYPE_DEFINITION) return new KindShim(enter_INTERFACE_TYPE_DEFINITION, leave_INTERFACE_TYPE_DEFINITION);
  if (kind == Kind.UNION_TYPE_DEFINITION) return new KindShim(enter_UNION_TYPE_DEFINITION, leave_UNION_TYPE_DEFINITION);
  if (kind == Kind.ENUM_TYPE_DEFINITION) return new KindShim(enter_ENUM_TYPE_DEFINITION, leave_ENUM_TYPE_DEFINITION);
  if (kind == Kind.ENUM_VALUE_DEFINITION) return new KindShim(enter_ENUM_VALUE_DEFINITION, leave_ENUM_VALUE_DEFINITION);
  if (kind == Kind.INPUT_OBJECT_TYPE_DEFINITION) return new KindShim(enter_INPUT_OBJECT_TYPE_DEFINITION, leave_INPUT_OBJECT_TYPE_DEFINITION);
  if (kind == Kind.DIRECTIVE_DEFINITION) return new KindShim(enter_DIRECTIVE_DEFINITION, leave_DIRECTIVE_DEFINITION);
  if (kind == Kind.SCHEMA_EXTENSION) return new KindShim(enter_SCHEMA_EXTENSION, leave_SCHEMA_EXTENSION);
  if (kind == Kind.SCALAR_TYPE_EXTENSION) return new KindShim(enter_SCALAR_TYPE_EXTENSION, leave_SCALAR_TYPE_EXTENSION);
  if (kind == Kind.OBJECT_TYPE_EXTENSION) return new KindShim(enter_OBJECT_TYPE_EXTENSION, leave_OBJECT_TYPE_EXTENSION);
  if (kind == Kind.INTERFACE_TYPE_EXTENSION) return new KindShim(enter_INTERFACE_TYPE_EXTENSION, leave_INTERFACE_TYPE_EXTENSION);
  if (kind == Kind.UNION_TYPE_EXTENSION) return new KindShim(enter_UNION_TYPE_EXTENSION, leave_UNION_TYPE_EXTENSION);
  if (kind == Kind.ENUM_TYPE_EXTENSION) return new KindShim(enter_ENUM_TYPE_EXTENSION, leave_ENUM_TYPE_EXTENSION);
  if (kind == Kind.INPUT_OBJECT_TYPE_EXTENSION) return new KindShim(enter_INPUT_OBJECT_TYPE_EXTENSION, leave_INPUT_OBJECT_TYPE_EXTENSION);
  return new KindShim(enter_NAME, leave_NAME); // unreachable; AS needs total returns
}

// visitInParallel mirrors graphql-js's homonymous function.
// For each kind, build per-visitor enter/leave tables and a merged
// dispatcher. The merged dispatcher's enter/leave run the indirect-call
// loop that's the optimization target.
export function visitInParallel(visitors: Visitor[]): Visitor {
  const breakSentinel: ASTNode = new BreakSentinel();
  // skipping[i] is null initially, set to a node when that visitor
  // returned SKIP, set to breakSentinel when BREAK.
  const skipping: Array<ASTNode | null> = new Array<ASTNode | null>(visitors.length);
  for (let i = 0; i < skipping.length; i++) skipping[i] = null;

  // Reset the global dispatcher table so a new parallel-visitor doesn't
  // inherit stale entries. Multiple `validate()` calls reuse this module
  // state safely because each `visitInParallel` reassigns it before use.
  g_enter_dispatchers = new Map<string, MergedEnterLeaveDispatcher>();

  const merged = new Visitor();

  for (let kindIdx = 0; kindIdx < KIND_VALUES.length; kindIdx++) {
    const kind = KIND_VALUES[kindIdx];
    let hasVisitor = false;
    const enterList = new Array<EnterFn | null>(visitors.length);
    const leaveList = new Array<LeaveFn | null>(visitors.length);

    for (let i = 0; i < visitors.length; i++) {
      const el = getEnterLeaveForKind(visitors[i], kind);
      const enter = el.enter;
      const leave = el.leave;
      if (!hasVisitor && (enter !== null || leave !== null)) hasVisitor = true;
      enterList[i] = enter;
      leaveList[i] = leave;
    }

    if (!hasVisitor) continue;

    const dispatcher = new MergedEnterLeaveDispatcher(enterList, leaveList, skipping, breakSentinel);
    g_enter_dispatchers.set(kind, dispatcher);
    const shim = kindShimFor(kind);
    merged.onEnterLeave(kind, shim.enter, shim.leave);
  }

  return merged;
}

// visit walks `root` depth-first. Mirrors graphql-js's visit() control
// flow but iterative-with-recursion: per-node enter dispatch via the
// kind-keyed table, per-key descent driven by `visitorKeys[node.kind]`,
// post-order leave call.
export function visit(root: ASTNode, visitor: Visitor, visitorKeys: VisitorKeys = defaultVisitorKeys()): void {
  const enterLeaveMap = new Map<string, EnterLeaveVisitor>();
  for (let kindIdx = 0; kindIdx < KIND_VALUES.length; kindIdx++) {
    const kind = KIND_VALUES[kindIdx];
    enterLeaveMap.set(kind, getEnterLeaveForKind(visitor, kind));
  }
  walkNode(root, enterLeaveMap, visitorKeys);
}

function walkNode(
  node: ASTNode,
  enterLeaveMap: Map<string, EnterLeaveVisitor>,
  visitorKeys: VisitorKeys,
): bool {
  let enterRes: VisitorResult = VisitorResult.CONTINUE;
  if (enterLeaveMap.has(node.kind)) {
    const el = enterLeaveMap.get(node.kind);
    if (el.enter !== null) {
      enterRes = el.enter!(node);
    }
  }
  if (enterRes == VisitorResult.BREAK) return false;
  if (enterRes == VisitorResult.SKIP_SUBTREE) {
    return runLeave(node, enterLeaveMap);
  }

  const keys = visitorKeys.get(node.kind);
  if (keys !== null) {
    for (let i = 0; i < keys.length; i++) {
      const key = keys[i];
      const single = node.getChild(key);
      if (single !== null) {
        if (!walkNode(single, enterLeaveMap, visitorKeys)) return false;
      }
      const list = node.getChildren(key);
      if (list !== null) {
        for (let j = 0; j < list.length; j++) {
          if (!walkNode(list[j], enterLeaveMap, visitorKeys)) return false;
        }
      }
    }
  }

  return runLeave(node, enterLeaveMap);
}

function runLeave(node: ASTNode, enterLeaveMap: Map<string, EnterLeaveVisitor>): bool {
  if (enterLeaveMap.has(node.kind)) {
    const el = enterLeaveMap.get(node.kind);
    if (el.leave !== null) {
      const r = el.leave!(node);
      if (r == VisitorResult.BREAK) return false;
    }
  }
  return true;
}
