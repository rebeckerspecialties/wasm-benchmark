// ValidationContext mirrors graphql-js's
// validation/ValidationContext.mjs (the slice the bundled rules use):
//   - reportError(error)
//   - getDocument()
//   - getSchema()
//   - getFragment(name)
//   - getParentType() / getFieldDef() — TypeInfo-driven, set externally
//     by the visitor as it descends. We mirror the property surface only.

import { ASTNode, DocumentNode, FragmentDefinitionNode } from "./ast";
import { Kind } from "./kinds";
import { GraphQLError } from "./error";
import { GraphQLSchema, GraphQLType, GraphQLField } from "./schema";

// ErrorReporter mirrors graphql-js's `(error) => { errors.push(error); }`
// callback. AssemblyScript supports closures, but for the benchmark we
// keep this as a plain class and have validate() inspect context.errors.
export class ValidationContext {
  schema: GraphQLSchema;
  documentAST: DocumentNode;
  errors: GraphQLError[];
  fragments: Map<string, FragmentDefinitionNode> | null;
  // TypeInfo-driven fields. The visitor pushes/pops parent type and
  // field def as it walks the AST, so rules see the type at the current
  // node. graphql-js uses TypeInfo + visitWithTypeInfo for this.
  parentType: GraphQLType | null;
  fieldDef: GraphQLField | null;

  constructor(schema: GraphQLSchema, documentAST: DocumentNode) {
    this.schema = schema;
    this.documentAST = documentAST;
    this.errors = [];
    this.fragments = null;
    this.parentType = null;
    this.fieldDef = null;
  }

  reportError(error: GraphQLError): void {
    this.errors.push(error);
  }

  getDocument(): DocumentNode {
    return this.documentAST;
  }

  getSchema(): GraphQLSchema {
    return this.schema;
  }

  // Lazy-built fragments map keyed on fragment name.
  getFragment(name: string): FragmentDefinitionNode | null {
    let frags = this.fragments;
    if (frags === null) {
      frags = new Map<string, FragmentDefinitionNode>();
      const defs = this.documentAST.definitions;
      for (let i = 0; i < defs.length; i++) {
        const d = defs[i];
        if (d.kind == Kind.FRAGMENT_DEFINITION) {
          const fd = changetype<FragmentDefinitionNode>(d);
          frags.set(fd.name.value, fd);
        }
      }
      this.fragments = frags;
    }
    if (frags.has(name)) return frags.get(name);
    return null;
  }

  getParentType(): GraphQLType | null {
    return this.parentType;
  }

  getFieldDef(): GraphQLField | null {
    return this.fieldDef;
  }
}
