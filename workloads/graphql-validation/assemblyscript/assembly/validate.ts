// Mirrors graphql-js's validation/validate.mjs `validate()`:
//
//   export function validate(schema, documentAST, rules = specifiedRules,
//                            options, typeInfo = new TypeInfo(schema)) {
//     …
//     const context = new ValidationContext(schema, documentAST, typeInfo, onError);
//     const visitor = visitInParallel(rules.map((rule) => rule(context)));
//     visit(documentAST, visitWithTypeInfo(typeInfo, visitor), QueryDocumentKeysToValidate);
//     return errors;
//   }
//
// Argument names match the JS signature: schema, documentAST, rules,
// options (placeholder), typeInfo (placeholder). The TypeInfo wrapper is
// inlined into ValidationContext + the visitor's parent-type tracking.

import {
  ASTNode,
  DocumentNode,
  FieldNode,
  FragmentDefinitionNode,
  InlineFragmentNode,
  OperationDefinitionNode,
  defaultVisitorKeys,
} from "./ast";
import { ValidationContext } from "./context";
import { GraphQLError } from "./error";
import { GraphQLField, GraphQLSchema, GraphQLType } from "./schema";
import { specifiedRules, ValidationRule } from "./specifiedRules";
import {
  EnterFn,
  EnterLeaveVisitor,
  LeaveFn,
  Visitor,
  VisitorResult,
  visit,
  visitInParallel,
} from "./visitor";
import { Kind, KIND_VALUES } from "./kinds";

// Placeholder for graphql-js options parameter. Kept for signature parity.
export class ValidateOptions {
  maxErrors: i32 = 100;
}

// Placeholder TypeInfo class to keep `validate(schema, doc, rules, options, typeInfo)`
// signature parity with graphql-js. Actual parent-type tracking is done
// inline by `visitWithTypeInfo` below via the ValidationContext.
export class TypeInfo {
  schema: GraphQLSchema;
  constructor(schema: GraphQLSchema) {
    this.schema = schema;
  }
}

// TypeInfoState — global state holder for the active visit's
// (parentType, fieldDef) stack. visitWithTypeInfo's enter/leave hooks
// need to push/pop through this state without closures, so we keep a
// module-global pointer that's set per `validate()` call.
class TypeInfoState {
  context: ValidationContext;
  parentTypeStack: Array<GraphQLType | null>;
  // We carry the inner Visitor reference so the wrapper enter/leave
  // hooks can forward to the inner enter/leave once they finish their
  // TypeInfo update.
  inner: Visitor;

  constructor(context: ValidationContext, inner: Visitor) {
    this.context = context;
    this.parentTypeStack = [];
    this.inner = inner;
  }
}

let g_typeInfoState: TypeInfoState | null = null;

// Helper to fetch the inner visitor's per-kind entry, or null if not registered.
function innerEntry(kind: string): EnterLeaveVisitor | null {
  const s = g_typeInfoState;
  if (s === null) return null;
  if (s.inner.perKind.has(kind)) return s.inner.perKind.get(kind);
  return null;
}

// --- Wrapper enter/leave hooks for kinds whose TypeInfo state changes ---
//
// Mirror graphql-js's TypeInfo.enter / .leave dispatch:
//   OperationDefinition: parent = schema.getQueryType() (or Mutation/Subscription)
//   FragmentDefinition:  parent = schema.getType(typeCondition.name.value)
//   InlineFragment:      parent = schema.getType(typeCondition.name.value) if typeCondition
//   Field:               parent = type-of-field-from-current-parent
// Each push the current parentType onto a stack on enter, pop on leave.

function tinfo_enter_OPERATION_DEFINITION(node: ASTNode): VisitorResult {
  const s = g_typeInfoState!;
  const op = changetype<OperationDefinitionNode>(node);
  let rootName: string = "Query";
  if (op.operation == "mutation") rootName = "Mutation";
  else if (op.operation == "subscription") rootName = "Subscription";
  const root = s.context.schema.getType(rootName);
  s.parentTypeStack.push(s.context.parentType);
  s.context.parentType = root;
  const inner = innerEntry(Kind.OPERATION_DEFINITION);
  if (inner !== null && inner.enter !== null) return inner.enter!(node);
  return VisitorResult.CONTINUE;
}
function tinfo_leave_OPERATION_DEFINITION(node: ASTNode): VisitorResult {
  const s = g_typeInfoState!;
  let r: VisitorResult = VisitorResult.CONTINUE;
  const inner = innerEntry(Kind.OPERATION_DEFINITION);
  if (inner !== null && inner.leave !== null) r = inner.leave!(node);
  s.context.parentType = s.parentTypeStack.pop();
  return r;
}

function tinfo_enter_FRAGMENT_DEFINITION(node: ASTNode): VisitorResult {
  const s = g_typeInfoState!;
  const frag = changetype<FragmentDefinitionNode>(node);
  const t = s.context.schema.getType(frag.typeCondition.name.value);
  s.parentTypeStack.push(s.context.parentType);
  s.context.parentType = t;
  const inner = innerEntry(Kind.FRAGMENT_DEFINITION);
  if (inner !== null && inner.enter !== null) return inner.enter!(node);
  return VisitorResult.CONTINUE;
}
function tinfo_leave_FRAGMENT_DEFINITION(node: ASTNode): VisitorResult {
  const s = g_typeInfoState!;
  let r: VisitorResult = VisitorResult.CONTINUE;
  const inner = innerEntry(Kind.FRAGMENT_DEFINITION);
  if (inner !== null && inner.leave !== null) r = inner.leave!(node);
  s.context.parentType = s.parentTypeStack.pop();
  return r;
}

function tinfo_enter_INLINE_FRAGMENT(node: ASTNode): VisitorResult {
  const s = g_typeInfoState!;
  const frag = changetype<InlineFragmentNode>(node);
  s.parentTypeStack.push(s.context.parentType);
  if (frag.typeCondition !== null) {
    s.context.parentType = s.context.schema.getType(frag.typeCondition!.name.value);
  }
  const inner = innerEntry(Kind.INLINE_FRAGMENT);
  if (inner !== null && inner.enter !== null) return inner.enter!(node);
  return VisitorResult.CONTINUE;
}
function tinfo_leave_INLINE_FRAGMENT(node: ASTNode): VisitorResult {
  const s = g_typeInfoState!;
  let r: VisitorResult = VisitorResult.CONTINUE;
  const inner = innerEntry(Kind.INLINE_FRAGMENT);
  if (inner !== null && inner.leave !== null) r = inner.leave!(node);
  s.context.parentType = s.parentTypeStack.pop();
  return r;
}

function tinfo_enter_FIELD(node: ASTNode): VisitorResult {
  const s = g_typeInfoState!;
  const field = changetype<FieldNode>(node);
  const parent = s.context.parentType;
  s.context.fieldDef = null;
  let nextParent: GraphQLType | null = null;
  if (parent !== null) {
    const fields = parent.getFields();
    if (fields.has(field.name.value)) {
      const def = fields.get(field.name.value);
      s.context.fieldDef = def;
      nextParent = s.context.schema.getType(def.typeName);
    }
  }
  s.parentTypeStack.push(s.context.parentType);
  // Run the inner rule with parentType still pointing at the type that
  // contains this field (graphql-js's TypeInfo updates `_parentTypeStack`
  // only on SelectionSet entry — the Field rule's getParentType() returns
  // the parent of the field, not the field's own resolved output type).
  // Then update parentType for descent into the field's children.
  const inner = innerEntry(Kind.FIELD);
  let result: VisitorResult = VisitorResult.CONTINUE;
  if (inner !== null && inner.enter !== null) result = inner.enter!(node);
  s.context.parentType = nextParent;
  return result;
}
function tinfo_leave_FIELD(node: ASTNode): VisitorResult {
  const s = g_typeInfoState!;
  let r: VisitorResult = VisitorResult.CONTINUE;
  const inner = innerEntry(Kind.FIELD);
  if (inner !== null && inner.leave !== null) r = inner.leave!(node);
  s.context.parentType = s.parentTypeStack.pop();
  s.context.fieldDef = null;
  return r;
}

// visitWithTypeInfo mirrors graphql-js's utilities/TypeInfo.visitWithTypeInfo:
// wraps a visitor so each kind's enter/leave updates ValidationContext's
// parentType / fieldDef. Returns a new Visitor whose per-kind table holds
// the wrapper functions for state-changing kinds and the inner functions
// for everything else.
export function visitWithTypeInfo(
  typeInfo: TypeInfo,
  context: ValidationContext,
  inner: Visitor,
): Visitor {
  // Stash global state for the wrapper hooks.
  g_typeInfoState = new TypeInfoState(context, inner);

  const wrapper = new Visitor();

  // Copy inner visitor's per-kind table — we'll override the few kinds
  // whose TypeInfo state changes (OperationDefinition, Field,
  // FragmentDefinition, InlineFragment).
  const innerKeys = inner.perKind.keys();
  for (let i = 0; i < innerKeys.length; i++) {
    const k = innerKeys[i];
    wrapper.perKind.set(k, inner.perKind.get(k));
  }

  wrapper.onEnterLeave(
    Kind.OPERATION_DEFINITION,
    tinfo_enter_OPERATION_DEFINITION,
    tinfo_leave_OPERATION_DEFINITION,
  );
  wrapper.onEnterLeave(
    Kind.FRAGMENT_DEFINITION,
    tinfo_enter_FRAGMENT_DEFINITION,
    tinfo_leave_FRAGMENT_DEFINITION,
  );
  wrapper.onEnterLeave(
    Kind.INLINE_FRAGMENT,
    tinfo_enter_INLINE_FRAGMENT,
    tinfo_leave_INLINE_FRAGMENT,
  );
  wrapper.onEnterLeave(
    Kind.FIELD,
    tinfo_enter_FIELD,
    tinfo_leave_FIELD,
  );

  return wrapper;
}

// Main entry. Argument names match graphql-js exactly:
// schema, documentAST, rules, options, typeInfo.
export function validate(
  schema: GraphQLSchema,
  documentAST: DocumentNode,
  rules: ValidationRule[] = specifiedRules,
  options: ValidateOptions | null = null,
  typeInfo: TypeInfo | null = null,
): GraphQLError[] {
  const ti = typeInfo !== null ? typeInfo : new TypeInfo(schema);
  const context = new ValidationContext(schema, documentAST);

  // rules.map(rule => rule(context)) — materializes each rule into a Visitor.
  const visitorList: Visitor[] = [];
  for (let i = 0; i < rules.length; i++) {
    visitorList.push(rules[i](context));
  }

  const visitor = visitInParallel(visitorList);
  const wrapped = visitWithTypeInfo(ti, context, visitor);

  visit(documentAST, wrapped, defaultVisitorKeys());

  return context.errors;
}
