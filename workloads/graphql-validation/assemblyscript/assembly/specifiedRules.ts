// Mirrors graphql-js's validation/specifiedRules.mjs `specifiedRules` array.
// Each entry is a rule constructor: `(context) => Visitor`.
// validate() does `rules.map(rule => rule(context))` to materialize them.

import { ValidationContext } from "./context";
import { Visitor } from "./visitor";

import { ExecutableDefinitionsRule } from "./rules/ExecutableDefinitionsRule";
import { UniqueOperationNamesRule } from "./rules/UniqueOperationNamesRule";
import { KnownTypeNamesRule } from "./rules/KnownTypeNamesRule";
import { FieldsOnCorrectTypeRule } from "./rules/FieldsOnCorrectTypeRule";
import { FragmentsOnCompositeTypesRule } from "./rules/FragmentsOnCompositeTypesRule";

export type ValidationRule = (context: ValidationContext) => Visitor;

export const specifiedRules: ValidationRule[] = [
  ExecutableDefinitionsRule,
  UniqueOperationNamesRule,
  KnownTypeNamesRule,
  FieldsOnCorrectTypeRule,
  FragmentsOnCompositeTypesRule,
];
