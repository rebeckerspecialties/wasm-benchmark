// Minimal schema model mirroring the surface area of graphql-js's
// GraphQLSchema that the rules in this benchmark touch:
//   - getTypeMap()    — { [typeName]: GraphQLType }
//   - getType(name)
//   - getPossibleTypes(abstractType) for unions / interfaces
//   - GraphQLObjectType.getFields(), .getInterfaces()
//   - GraphQLInterfaceType.getFields()
//
// We don't model directives, scalars beyond a fixed set, etc. — only what
// the validation rules actually consult.

export class GraphQLField {
  name: string;
  typeName: string;

  constructor(name: string, typeName: string) {
    this.name = name;
    this.typeName = typeName;
  }
}

export class GraphQLType {
  name: string;
  // "Object" | "Interface" | "Union" | "Scalar"
  kind: string;
  fields: Map<string, GraphQLField>;
  interfaces: string[];   // Interface type names this type implements
  possibleTypes: string[]; // For Union / Interface: concrete types

  constructor(name: string, kind: string) {
    this.name = name;
    this.kind = kind;
    this.fields = new Map<string, GraphQLField>();
    this.interfaces = [];
    this.possibleTypes = [];
  }

  getFields(): Map<string, GraphQLField> {
    return this.fields;
  }

  getInterfaces(): string[] {
    return this.interfaces;
  }
}

export class GraphQLSchema {
  types: Map<string, GraphQLType>;
  queryTypeName: string;

  constructor() {
    this.types = new Map<string, GraphQLType>();
    this.queryTypeName = "Query";
  }

  getTypeMap(): Map<string, GraphQLType> {
    return this.types;
  }

  getType(name: string): GraphQLType | null {
    if (this.types.has(name)) return this.types.get(name);
    return null;
  }

  // Mirrors GraphQLSchema.getPossibleTypes(abstractType).
  getPossibleTypes(abstract_: GraphQLType): GraphQLType[] {
    const out: GraphQLType[] = [];
    const names = abstract_.possibleTypes;
    for (let i = 0; i < names.length; i++) {
      if (this.types.has(names[i])) out.push(this.types.get(names[i]));
    }
    return out;
  }

  // graphql-js: schema.getQueryType()
  getQueryType(): GraphQLType | null {
    if (this.types.has(this.queryTypeName)) return this.types.get(this.queryTypeName);
    return null;
  }
}

// Composite types are Object | Interface | Union — the rule
// FragmentsOnCompositeTypesRule consults this.
export function isCompositeType(t: GraphQLType): boolean {
  return t.kind == "Object" || t.kind == "Interface" || t.kind == "Union";
}

export function isAbstractType(t: GraphQLType): boolean {
  return t.kind == "Interface" || t.kind == "Union";
}

export function isObjectType(t: GraphQLType): boolean {
  return t.kind == "Object";
}

export function isInterfaceType(t: GraphQLType): boolean {
  return t.kind == "Interface";
}
