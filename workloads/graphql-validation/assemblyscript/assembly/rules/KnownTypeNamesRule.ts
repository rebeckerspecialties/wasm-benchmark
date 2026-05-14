// Mirrors graphql-js's KnownTypeNamesRule:
//
//   export function KnownTypeNamesRule(context) {
//     const schema = context.getSchema();
//     const existingTypesMap = schema ? schema.getTypeMap() : Object.create(null);
//     const definedTypes = Object.create(null);
//     for (const def of context.getDocument().definitions) {
//       if (isTypeDefinitionNode(def)) {
//         definedTypes[def.name.value] = true;
//       }
//     }
//     return {
//       NamedType(node, _1, parent, _2, ancestors) {
//         const typeName = node.name.value;
//         if (!existingTypesMap[typeName] && !definedTypes[typeName]) {
//           context.reportError(…);
//         }
//       },
//     };
//   }

import {
  ASTNode,
  InterfaceTypeDefinitionNode,
  NamedTypeNode,
  ObjectTypeDefinitionNode,
  UnionTypeDefinitionNode,
  isTypeDefinitionNode,
} from "../ast";
import { ValidationContext } from "../context";
import { GraphQLError } from "../error";
import { Kind } from "../kinds";
import { GraphQLType } from "../schema";
import { Visitor, VisitorResult } from "../visitor";

let g_context: ValidationContext | null = null;
let g_existingTypesMap: Map<string, GraphQLType> | null = null;
let g_definedTypes: Map<string, bool> | null = null;

function knownTypeNames_NamedType(node: ASTNode): VisitorResult {
  const ctx = g_context!;
  const existing = g_existingTypesMap!;
  const defined = g_definedTypes!;
  const nt = changetype<NamedTypeNode>(node);
  const typeName = nt.name.value;
  if (!existing.has(typeName) && !defined.has(typeName)) {
    ctx.reportError(
      new GraphQLError("Unknown type \"" + typeName + "\".", [node]),
    );
  }
  return VisitorResult.CONTINUE;
}

export function KnownTypeNamesRule(context: ValidationContext): Visitor {
  g_context = context;
  g_existingTypesMap = context.getSchema().getTypeMap();
  const defined = new Map<string, bool>();

  const defs = context.getDocument().definitions;
  for (let i = 0; i < defs.length; i++) {
    const def = defs[i];
    if (isTypeDefinitionNode(def)) {
      const k = def.kind;
      let name: string = "";
      if (k == Kind.OBJECT_TYPE_DEFINITION) {
        name = changetype<ObjectTypeDefinitionNode>(def).name.value;
      } else if (k == Kind.INTERFACE_TYPE_DEFINITION) {
        name = changetype<InterfaceTypeDefinitionNode>(def).name.value;
      } else if (k == Kind.UNION_TYPE_DEFINITION) {
        name = changetype<UnionTypeDefinitionNode>(def).name.value;
      }
      if (name.length > 0) defined.set(name, true);
    }
  }
  g_definedTypes = defined;

  return new Visitor().on(Kind.NAMED_TYPE, knownTypeNames_NamedType);
}
