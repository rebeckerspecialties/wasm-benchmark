// Mirrors graphql-js's FieldsOnCorrectTypeRule:
//
//   export function FieldsOnCorrectTypeRule(context) {
//     return {
//       Field(node) {
//         const type = context.getParentType();
//         if (type) {
//           const fieldDef = context.getFieldDef();
//           if (!fieldDef) {
//             …context.reportError(…);
//           }
//         }
//       },
//     };
//   }

import { ASTNode, FieldNode } from "../ast";
import { ValidationContext } from "../context";
import { GraphQLError } from "../error";
import { Kind } from "../kinds";
import { Visitor, VisitorResult } from "../visitor";

let g_context: ValidationContext | null = null;

function fieldsOnCorrectType_Field(node: ASTNode): VisitorResult {
  const ctx = g_context!;
  const field = changetype<FieldNode>(node);
  const type = ctx.getParentType();
  if (type !== null) {
    const fieldDef = ctx.getFieldDef();
    if (fieldDef === null) {
      const fieldName = field.name.value;
      ctx.reportError(
        new GraphQLError(
          "Cannot query field \"" + fieldName + "\" on type \"" + type.name + "\".",
          [node],
        ),
      );
    }
  }
  return VisitorResult.CONTINUE;
}

export function FieldsOnCorrectTypeRule(context: ValidationContext): Visitor {
  g_context = context;
  return new Visitor().on(Kind.FIELD, fieldsOnCorrectType_Field);
}
