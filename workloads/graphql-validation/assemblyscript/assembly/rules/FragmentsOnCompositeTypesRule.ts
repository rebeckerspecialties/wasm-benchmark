// Mirrors graphql-js's FragmentsOnCompositeTypesRule:
//
//   export function FragmentsOnCompositeTypesRule(context) {
//     return {
//       InlineFragment(node) {
//         const typeCondition = node.typeCondition;
//         if (typeCondition) {
//           const type = typeFromAST(context.getSchema(), typeCondition);
//           if (type && !isCompositeType(type)) {
//             context.reportError(…);
//           }
//         }
//       },
//       FragmentDefinition(node) {
//         const type = typeFromAST(context.getSchema(), node.typeCondition);
//         if (type && !isCompositeType(type)) {
//           context.reportError(…);
//         }
//       },
//     };
//   }

import {
  ASTNode,
  FragmentDefinitionNode,
  InlineFragmentNode,
  NamedTypeNode,
} from "../ast";
import { ValidationContext } from "../context";
import { GraphQLError } from "../error";
import { Kind } from "../kinds";
import { GraphQLSchema, GraphQLType, isCompositeType } from "../schema";
import { Visitor, VisitorResult } from "../visitor";

let g_context: ValidationContext | null = null;

// typeFromAST mirrors graphql-js's utilities/typeFromAST: resolves a type
// reference AST node to a schema type. We only handle NamedType (the case
// validation rules consult); ListType / NonNullType wrap NamedType.
function typeFromAST(schema: GraphQLSchema, ref: NamedTypeNode): GraphQLType | null {
  if (ref.kind == Kind.NAMED_TYPE) {
    return schema.getType(ref.name.value);
  }
  return null;
}

function fragmentsOnComposite_InlineFragment(node: ASTNode): VisitorResult {
  const ctx = g_context!;
  const frag = changetype<InlineFragmentNode>(node);
  const typeCondition = frag.typeCondition;
  if (typeCondition !== null) {
    const type = typeFromAST(ctx.getSchema(), typeCondition);
    if (type !== null && !isCompositeType(type)) {
      ctx.reportError(
        new GraphQLError(
          "Fragment cannot condition on non composite type \"" + typeCondition.name.value + "\".",
          [typeCondition],
        ),
      );
    }
  }
  return VisitorResult.CONTINUE;
}

function fragmentsOnComposite_FragmentDefinition(node: ASTNode): VisitorResult {
  const ctx = g_context!;
  const frag = changetype<FragmentDefinitionNode>(node);
  const type = typeFromAST(ctx.getSchema(), frag.typeCondition);
  if (type !== null && !isCompositeType(type)) {
    ctx.reportError(
      new GraphQLError(
        "Fragment \"" + frag.name.value + "\" cannot condition on non composite type \"" + frag.typeCondition.name.value + "\".",
        [frag.typeCondition],
      ),
    );
  }
  return VisitorResult.CONTINUE;
}

export function FragmentsOnCompositeTypesRule(context: ValidationContext): Visitor {
  g_context = context;
  return new Visitor()
    .on(Kind.INLINE_FRAGMENT, fragmentsOnComposite_InlineFragment)
    .on(Kind.FRAGMENT_DEFINITION, fragmentsOnComposite_FragmentDefinition);
}
