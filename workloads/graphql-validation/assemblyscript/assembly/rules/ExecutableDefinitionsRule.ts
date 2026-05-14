// Mirrors graphql-js's ExecutableDefinitionsRule:
//
//   export function ExecutableDefinitionsRule(context) {
//     return {
//       Document(node) {
//         for (const definition of node.definitions) {
//           if (!isExecutableDefinitionNode(definition)) {
//             context.reportError(new GraphQLError(…));
//           }
//         }
//         return false;
//       },
//     };
//   }
//
// AssemblyScript closure restriction workaround: per-rule context lives
// in a module global, and the visitor entry is a plain function that
// reads the global. Functionally equivalent to graphql-js's closure.

import {
  ASTNode,
  DocumentNode,
  isExecutableDefinitionNode,
} from "../ast";
import { ValidationContext } from "../context";
import { GraphQLError } from "../error";
import { Kind } from "../kinds";
import { Visitor, VisitorResult } from "../visitor";

let g_context: ValidationContext | null = null;

function executableDefinitions_Document(node: ASTNode): VisitorResult {
  const ctx = g_context!;
  const doc = changetype<DocumentNode>(node);
  const defs = doc.definitions;
  for (let i = 0; i < defs.length; i++) {
    const definition = defs[i];
    if (!isExecutableDefinitionNode(definition)) {
      ctx.reportError(
        new GraphQLError("The definition is not executable.", [definition]),
      );
    }
  }
  // graphql-js returns `false` to skip subtree — definitions are visited
  // by their own kind-specific visitors elsewhere.
  return VisitorResult.SKIP_SUBTREE;
}

export function ExecutableDefinitionsRule(context: ValidationContext): Visitor {
  g_context = context;
  return new Visitor().on(Kind.DOCUMENT, executableDefinitions_Document);
}
