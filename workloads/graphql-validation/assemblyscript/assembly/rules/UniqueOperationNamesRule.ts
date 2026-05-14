// Mirrors graphql-js's UniqueOperationNamesRule:
//
//   export function UniqueOperationNamesRule(context) {
//     const knownOperationNames = Object.create(null);
//     return {
//       OperationDefinition(node) {
//         const operationName = node.name;
//         if (operationName) {
//           if (knownOperationNames[operationName.value]) {
//             context.reportError(…);
//           } else {
//             knownOperationNames[operationName.value] = operationName;
//           }
//         }
//         return false;
//       },
//       FragmentDefinition: () => false,
//     };
//   }
//
// AS closure restriction → context + knownOperationNames live in module
// globals. Functionally identical to the closure form.

import {
  ASTNode,
  NameNode,
  OperationDefinitionNode,
} from "../ast";
import { ValidationContext } from "../context";
import { GraphQLError } from "../error";
import { Kind } from "../kinds";
import { Visitor, VisitorResult } from "../visitor";

let g_context: ValidationContext | null = null;
let g_knownOperationNames: Map<string, NameNode> | null = null;

function uniqueOperationNames_OperationDefinition(node: ASTNode): VisitorResult {
  const ctx = g_context!;
  const known = g_knownOperationNames!;
  const op = changetype<OperationDefinitionNode>(node);
  const operationName = op.name;
  if (operationName !== null) {
    if (known.has(operationName.value)) {
      ctx.reportError(
        new GraphQLError(
          "There can be only one operation named \"" + operationName.value + "\".",
          [operationName],
        ),
      );
    } else {
      known.set(operationName.value, operationName);
    }
  }
  return VisitorResult.SKIP_SUBTREE;
}

function uniqueOperationNames_FragmentDefinition(_node: ASTNode): VisitorResult {
  return VisitorResult.SKIP_SUBTREE;
}

export function UniqueOperationNamesRule(context: ValidationContext): Visitor {
  g_context = context;
  g_knownOperationNames = new Map<string, NameNode>();
  return new Visitor()
    .on(Kind.OPERATION_DEFINITION, uniqueOperationNames_OperationDefinition)
    .on(Kind.FRAGMENT_DEFINITION, uniqueOperationNames_FragmentDefinition);
}
