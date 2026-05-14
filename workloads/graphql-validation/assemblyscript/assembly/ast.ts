// AST node hierarchy mirroring graphql-js's language/ast.mjs.
//
// graphql-js relies on JS's structural property access (`node[key]`) for
// the visitor's child traversal. AssemblyScript is statically typed, so we
// expose a single `getChildren(key)` helper per node that returns the
// child array for a given key (or `null`). Visitor traversal still drives
// dispatch via `node.kind`, identical to graphql-js.

import { Kind } from "./kinds";

// QueryDocumentKeys mirrors graphql-js's table of "which child keys to
// traverse for each kind". The visitor uses this to know what to recurse
// into at each node. Lookup keyed on node.kind.
export class VisitorKeys {
  // For each kind, a list of child-property names the visitor walks into.
  private map: Map<string, string[]> = new Map<string, string[]>();

  set(kind: string, keys: string[]): void {
    this.map.set(kind, keys);
  }

  get(kind: string): string[] | null {
    if (this.map.has(kind)) return this.map.get(kind);
    return null;
  }
}

// Default visitor keys, matching graphql-js's QueryDocumentKeys for the
// kinds we exercise.
export function defaultVisitorKeys(): VisitorKeys {
  const keys = new VisitorKeys();
  keys.set(Kind.NAME, []);
  keys.set(Kind.DOCUMENT, ["definitions"]);
  keys.set(Kind.OPERATION_DEFINITION, [
    "name",
    "variableDefinitions",
    "directives",
    "selectionSet",
  ]);
  keys.set(Kind.VARIABLE_DEFINITION, ["variable", "type", "defaultValue", "directives"]);
  keys.set(Kind.VARIABLE, ["name"]);
  keys.set(Kind.SELECTION_SET, ["selections"]);
  keys.set(Kind.FIELD, ["alias", "name", "arguments", "directives", "selectionSet"]);
  keys.set(Kind.ARGUMENT, ["name", "value"]);
  keys.set(Kind.FRAGMENT_SPREAD, ["name", "directives"]);
  keys.set(Kind.INLINE_FRAGMENT, ["typeCondition", "directives", "selectionSet"]);
  keys.set(Kind.FRAGMENT_DEFINITION, [
    "name",
    "variableDefinitions",
    "typeCondition",
    "directives",
    "selectionSet",
  ]);
  keys.set(Kind.INT, []);
  keys.set(Kind.FLOAT, []);
  keys.set(Kind.STRING, []);
  keys.set(Kind.BOOLEAN, []);
  keys.set(Kind.NULL, []);
  keys.set(Kind.ENUM, []);
  keys.set(Kind.LIST, ["values"]);
  keys.set(Kind.OBJECT, ["fields"]);
  keys.set(Kind.OBJECT_FIELD, ["name", "value"]);
  keys.set(Kind.DIRECTIVE, ["name", "arguments"]);
  keys.set(Kind.NAMED_TYPE, ["name"]);
  keys.set(Kind.LIST_TYPE, ["type"]);
  keys.set(Kind.NON_NULL_TYPE, ["type"]);
  keys.set(Kind.OBJECT_TYPE_DEFINITION, [
    "name",
    "interfaces",
    "directives",
    "fields",
  ]);
  keys.set(Kind.INTERFACE_TYPE_DEFINITION, [
    "name",
    "interfaces",
    "directives",
    "fields",
  ]);
  keys.set(Kind.UNION_TYPE_DEFINITION, ["name", "directives", "types"]);
  keys.set(Kind.FIELD_DEFINITION, ["name", "arguments", "type", "directives"]);
  keys.set(Kind.INPUT_VALUE_DEFINITION, ["name", "type", "defaultValue", "directives"]);
  return keys;
}

// Base node. Every node has a `kind` discriminator string. Subclasses
// add typed fields. Mirrors graphql-js's ASTNode union.
export abstract class ASTNode {
  kind: string;

  constructor(kind: string) {
    this.kind = kind;
  }

  // Dispatched by the visitor: returns the array of child nodes for the
  // named key (or `null` when the key is not present / not an array).
  // graphql-js does `node[key]`; we centralize the resolution per type.
  abstract getChildren(key: string): ASTNode[] | null;

  // Some keys yield a single optional child rather than an array. The
  // visitor handles both shapes — this returns the single-child shape.
  // null when absent.
  abstract getChild(key: string): ASTNode | null;
}

// `name` field is canonically a NameNode in graphql-js.
export class NameNode extends ASTNode {
  value: string;

  constructor(value: string) {
    super(Kind.NAME);
    this.value = value;
  }

  getChildren(key: string): ASTNode[] | null { return null; }
  getChild(key: string): ASTNode | null { return null; }
}

export class VariableNode extends ASTNode {
  name: NameNode;

  constructor(name: NameNode) {
    super(Kind.VARIABLE);
    this.name = name;
  }

  getChildren(key: string): ASTNode[] | null { return null; }
  getChild(key: string): ASTNode | null {
    if (key == "name") return this.name;
    return null;
  }
}

export class NamedTypeNode extends ASTNode {
  name: NameNode;

  constructor(name: NameNode) {
    super(Kind.NAMED_TYPE);
    this.name = name;
  }

  getChildren(key: string): ASTNode[] | null { return null; }
  getChild(key: string): ASTNode | null {
    if (key == "name") return this.name;
    return null;
  }
}

export class ListTypeNode extends ASTNode {
  type: ASTNode;

  constructor(type: ASTNode) {
    super(Kind.LIST_TYPE);
    this.type = type;
  }

  getChildren(key: string): ASTNode[] | null { return null; }
  getChild(key: string): ASTNode | null {
    if (key == "type") return this.type;
    return null;
  }
}

export class NonNullTypeNode extends ASTNode {
  type: ASTNode;

  constructor(type: ASTNode) {
    super(Kind.NON_NULL_TYPE);
    this.type = type;
  }

  getChildren(key: string): ASTNode[] | null { return null; }
  getChild(key: string): ASTNode | null {
    if (key == "type") return this.type;
    return null;
  }
}

export class ArgumentNode extends ASTNode {
  name: NameNode;
  value: ASTNode;

  constructor(name: NameNode, value: ASTNode) {
    super(Kind.ARGUMENT);
    this.name = name;
    this.value = value;
  }

  getChildren(key: string): ASTNode[] | null { return null; }
  getChild(key: string): ASTNode | null {
    if (key == "name") return this.name;
    if (key == "value") return this.value;
    return null;
  }
}

export class StringValueNode extends ASTNode {
  value: string;

  constructor(value: string) {
    super(Kind.STRING);
    this.value = value;
  }

  getChildren(key: string): ASTNode[] | null { return null; }
  getChild(key: string): ASTNode | null { return null; }
}

export class IntValueNode extends ASTNode {
  value: string;

  constructor(value: string) {
    super(Kind.INT);
    this.value = value;
  }

  getChildren(key: string): ASTNode[] | null { return null; }
  getChild(key: string): ASTNode | null { return null; }
}

// Field selection. Mirrors graphql-js's FieldNode.
export class FieldNode extends ASTNode {
  alias: NameNode | null;
  name: NameNode;
  arguments_: ArgumentNode[];
  selectionSet: SelectionSetNode | null;

  constructor(
    name: NameNode,
    args: ArgumentNode[],
    selectionSet: SelectionSetNode | null,
    alias: NameNode | null = null,
  ) {
    super(Kind.FIELD);
    this.name = name;
    this.arguments_ = args;
    this.selectionSet = selectionSet;
    this.alias = alias;
  }

  getChildren(key: string): ASTNode[] | null {
    if (key == "arguments") {
      const out: ASTNode[] = [];
      for (let i = 0; i < this.arguments_.length; i++) out.push(this.arguments_[i]);
      return out;
    }
    return null;
  }
  getChild(key: string): ASTNode | null {
    if (key == "alias") return this.alias;
    if (key == "name") return this.name;
    if (key == "selectionSet") return this.selectionSet;
    return null;
  }
}

// Selection set: list of FieldNode | FragmentSpreadNode | InlineFragmentNode.
export class SelectionSetNode extends ASTNode {
  selections: ASTNode[];

  constructor(selections: ASTNode[]) {
    super(Kind.SELECTION_SET);
    this.selections = selections;
  }

  getChildren(key: string): ASTNode[] | null {
    if (key == "selections") return this.selections;
    return null;
  }
  getChild(key: string): ASTNode | null { return null; }
}

export class FragmentSpreadNode extends ASTNode {
  name: NameNode;

  constructor(name: NameNode) {
    super(Kind.FRAGMENT_SPREAD);
    this.name = name;
  }

  getChildren(key: string): ASTNode[] | null { return null; }
  getChild(key: string): ASTNode | null {
    if (key == "name") return this.name;
    return null;
  }
}

export class InlineFragmentNode extends ASTNode {
  typeCondition: NamedTypeNode | null;
  selectionSet: SelectionSetNode;

  constructor(typeCondition: NamedTypeNode | null, selectionSet: SelectionSetNode) {
    super(Kind.INLINE_FRAGMENT);
    this.typeCondition = typeCondition;
    this.selectionSet = selectionSet;
  }

  getChildren(key: string): ASTNode[] | null { return null; }
  getChild(key: string): ASTNode | null {
    if (key == "typeCondition") return this.typeCondition;
    if (key == "selectionSet") return this.selectionSet;
    return null;
  }
}

export class VariableDefinitionNode extends ASTNode {
  variable: VariableNode;
  type: ASTNode;

  constructor(variable: VariableNode, type: ASTNode) {
    super(Kind.VARIABLE_DEFINITION);
    this.variable = variable;
    this.type = type;
  }

  getChildren(key: string): ASTNode[] | null { return null; }
  getChild(key: string): ASTNode | null {
    if (key == "variable") return this.variable;
    if (key == "type") return this.type;
    return null;
  }
}

export class OperationDefinitionNode extends ASTNode {
  operation: string; // "query" | "mutation" | "subscription"
  name: NameNode | null;
  variableDefinitions: VariableDefinitionNode[];
  selectionSet: SelectionSetNode;

  constructor(
    operation: string,
    name: NameNode | null,
    variableDefinitions: VariableDefinitionNode[],
    selectionSet: SelectionSetNode,
  ) {
    super(Kind.OPERATION_DEFINITION);
    this.operation = operation;
    this.name = name;
    this.variableDefinitions = variableDefinitions;
    this.selectionSet = selectionSet;
  }

  getChildren(key: string): ASTNode[] | null {
    if (key == "variableDefinitions") {
      const out: ASTNode[] = [];
      for (let i = 0; i < this.variableDefinitions.length; i++) {
        out.push(this.variableDefinitions[i]);
      }
      return out;
    }
    return null;
  }
  getChild(key: string): ASTNode | null {
    if (key == "name") return this.name;
    if (key == "selectionSet") return this.selectionSet;
    return null;
  }
}

export class FragmentDefinitionNode extends ASTNode {
  name: NameNode;
  typeCondition: NamedTypeNode;
  selectionSet: SelectionSetNode;

  constructor(name: NameNode, typeCondition: NamedTypeNode, selectionSet: SelectionSetNode) {
    super(Kind.FRAGMENT_DEFINITION);
    this.name = name;
    this.typeCondition = typeCondition;
    this.selectionSet = selectionSet;
  }

  getChildren(key: string): ASTNode[] | null { return null; }
  getChild(key: string): ASTNode | null {
    if (key == "name") return this.name;
    if (key == "typeCondition") return this.typeCondition;
    if (key == "selectionSet") return this.selectionSet;
    return null;
  }
}

// Type-system definitions (schema-side). Used in the SDL fixture.

export class FieldDefinitionNode extends ASTNode {
  name: NameNode;
  arguments_: ASTNode[];
  type: ASTNode;

  constructor(name: NameNode, args: ASTNode[], type: ASTNode) {
    super(Kind.FIELD_DEFINITION);
    this.name = name;
    this.arguments_ = args;
    this.type = type;
  }

  getChildren(key: string): ASTNode[] | null {
    if (key == "arguments") return this.arguments_;
    return null;
  }
  getChild(key: string): ASTNode | null {
    if (key == "name") return this.name;
    if (key == "type") return this.type;
    return null;
  }
}

export class ObjectTypeDefinitionNode extends ASTNode {
  name: NameNode;
  interfaces: NamedTypeNode[];
  fields: FieldDefinitionNode[];

  constructor(name: NameNode, interfaces: NamedTypeNode[], fields: FieldDefinitionNode[]) {
    super(Kind.OBJECT_TYPE_DEFINITION);
    this.name = name;
    this.interfaces = interfaces;
    this.fields = fields;
  }

  getChildren(key: string): ASTNode[] | null {
    if (key == "interfaces") {
      const out: ASTNode[] = [];
      for (let i = 0; i < this.interfaces.length; i++) out.push(this.interfaces[i]);
      return out;
    }
    if (key == "fields") {
      const out: ASTNode[] = [];
      for (let i = 0; i < this.fields.length; i++) out.push(this.fields[i]);
      return out;
    }
    return null;
  }
  getChild(key: string): ASTNode | null {
    if (key == "name") return this.name;
    return null;
  }
}

export class InterfaceTypeDefinitionNode extends ASTNode {
  name: NameNode;
  interfaces: NamedTypeNode[];
  fields: FieldDefinitionNode[];

  constructor(name: NameNode, interfaces: NamedTypeNode[], fields: FieldDefinitionNode[]) {
    super(Kind.INTERFACE_TYPE_DEFINITION);
    this.name = name;
    this.interfaces = interfaces;
    this.fields = fields;
  }

  getChildren(key: string): ASTNode[] | null {
    if (key == "interfaces") {
      const out: ASTNode[] = [];
      for (let i = 0; i < this.interfaces.length; i++) out.push(this.interfaces[i]);
      return out;
    }
    if (key == "fields") {
      const out: ASTNode[] = [];
      for (let i = 0; i < this.fields.length; i++) out.push(this.fields[i]);
      return out;
    }
    return null;
  }
  getChild(key: string): ASTNode | null {
    if (key == "name") return this.name;
    return null;
  }
}

export class UnionTypeDefinitionNode extends ASTNode {
  name: NameNode;
  types: NamedTypeNode[];

  constructor(name: NameNode, types: NamedTypeNode[]) {
    super(Kind.UNION_TYPE_DEFINITION);
    this.name = name;
    this.types = types;
  }

  getChildren(key: string): ASTNode[] | null {
    if (key == "types") {
      const out: ASTNode[] = [];
      for (let i = 0; i < this.types.length; i++) out.push(this.types[i]);
      return out;
    }
    return null;
  }
  getChild(key: string): ASTNode | null {
    if (key == "name") return this.name;
    return null;
  }
}

// Document is the top-level AST node — list of definition nodes.
export class DocumentNode extends ASTNode {
  definitions: ASTNode[];

  constructor(definitions: ASTNode[]) {
    super(Kind.DOCUMENT);
    this.definitions = definitions;
  }

  getChildren(key: string): ASTNode[] | null {
    if (key == "definitions") return this.definitions;
    return null;
  }
  getChild(key: string): ASTNode | null { return null; }
}

// Mirrors graphql-js's predicate. We treat any kind with a `name` & body
// as a definition; for the simplified node set, the executable kinds are
// OperationDefinition and FragmentDefinition.
export function isExecutableDefinitionNode(node: ASTNode): boolean {
  const k = node.kind;
  return k == Kind.OPERATION_DEFINITION || k == Kind.FRAGMENT_DEFINITION;
}

export function isTypeDefinitionNode(node: ASTNode): boolean {
  const k = node.kind;
  return (
    k == Kind.OBJECT_TYPE_DEFINITION ||
    k == Kind.INTERFACE_TYPE_DEFINITION ||
    k == Kind.UNION_TYPE_DEFINITION ||
    k == Kind.SCALAR_TYPE_DEFINITION ||
    k == Kind.ENUM_TYPE_DEFINITION ||
    k == Kind.INPUT_OBJECT_TYPE_DEFINITION
  );
}
