// Mirrors graphql-js's language/kinds.mjs.
// Kept as a string-typed namespace so visitor dispatch keys are identical
// to the canonical graphql-js Kind values (e.g. node.kind === "Document").
export namespace Kind {
  export const NAME: string = "Name";
  export const DOCUMENT: string = "Document";
  export const OPERATION_DEFINITION: string = "OperationDefinition";
  export const VARIABLE_DEFINITION: string = "VariableDefinition";
  export const SELECTION_SET: string = "SelectionSet";
  export const FIELD: string = "Field";
  export const ARGUMENT: string = "Argument";
  export const FRAGMENT_SPREAD: string = "FragmentSpread";
  export const INLINE_FRAGMENT: string = "InlineFragment";
  export const FRAGMENT_DEFINITION: string = "FragmentDefinition";
  export const VARIABLE: string = "Variable";
  export const INT: string = "IntValue";
  export const FLOAT: string = "FloatValue";
  export const STRING: string = "StringValue";
  export const BOOLEAN: string = "BooleanValue";
  export const NULL: string = "NullValue";
  export const ENUM: string = "EnumValue";
  export const LIST: string = "ListValue";
  export const OBJECT: string = "ObjectValue";
  export const OBJECT_FIELD: string = "ObjectField";
  export const DIRECTIVE: string = "Directive";
  export const NAMED_TYPE: string = "NamedType";
  export const LIST_TYPE: string = "ListType";
  export const NON_NULL_TYPE: string = "NonNullType";
  export const SCHEMA_DEFINITION: string = "SchemaDefinition";
  export const OPERATION_TYPE_DEFINITION: string = "OperationTypeDefinition";
  export const SCALAR_TYPE_DEFINITION: string = "ScalarTypeDefinition";
  export const OBJECT_TYPE_DEFINITION: string = "ObjectTypeDefinition";
  export const FIELD_DEFINITION: string = "FieldDefinition";
  export const INPUT_VALUE_DEFINITION: string = "InputValueDefinition";
  export const INTERFACE_TYPE_DEFINITION: string = "InterfaceTypeDefinition";
  export const UNION_TYPE_DEFINITION: string = "UnionTypeDefinition";
  export const ENUM_TYPE_DEFINITION: string = "EnumTypeDefinition";
  export const ENUM_VALUE_DEFINITION: string = "EnumValueDefinition";
  export const INPUT_OBJECT_TYPE_DEFINITION: string = "InputObjectTypeDefinition";
  export const DIRECTIVE_DEFINITION: string = "DirectiveDefinition";
  export const SCHEMA_EXTENSION: string = "SchemaExtension";
  export const SCALAR_TYPE_EXTENSION: string = "ScalarTypeExtension";
  export const OBJECT_TYPE_EXTENSION: string = "ObjectTypeExtension";
  export const INTERFACE_TYPE_EXTENSION: string = "InterfaceTypeExtension";
  export const UNION_TYPE_EXTENSION: string = "UnionTypeExtension";
  export const ENUM_TYPE_EXTENSION: string = "EnumTypeExtension";
  export const INPUT_OBJECT_TYPE_EXTENSION: string = "InputObjectTypeExtension";
}

// All kind values, used by visit/visitInParallel to enumerate the dispatch
// table once per visit. Mirrors `Object.values(Kind)` in graphql-js.
export const KIND_VALUES: string[] = [
  Kind.NAME,
  Kind.DOCUMENT,
  Kind.OPERATION_DEFINITION,
  Kind.VARIABLE_DEFINITION,
  Kind.SELECTION_SET,
  Kind.FIELD,
  Kind.ARGUMENT,
  Kind.FRAGMENT_SPREAD,
  Kind.INLINE_FRAGMENT,
  Kind.FRAGMENT_DEFINITION,
  Kind.VARIABLE,
  Kind.INT,
  Kind.FLOAT,
  Kind.STRING,
  Kind.BOOLEAN,
  Kind.NULL,
  Kind.ENUM,
  Kind.LIST,
  Kind.OBJECT,
  Kind.OBJECT_FIELD,
  Kind.DIRECTIVE,
  Kind.NAMED_TYPE,
  Kind.LIST_TYPE,
  Kind.NON_NULL_TYPE,
  Kind.SCHEMA_DEFINITION,
  Kind.OPERATION_TYPE_DEFINITION,
  Kind.SCALAR_TYPE_DEFINITION,
  Kind.OBJECT_TYPE_DEFINITION,
  Kind.FIELD_DEFINITION,
  Kind.INPUT_VALUE_DEFINITION,
  Kind.INTERFACE_TYPE_DEFINITION,
  Kind.UNION_TYPE_DEFINITION,
  Kind.ENUM_TYPE_DEFINITION,
  Kind.ENUM_VALUE_DEFINITION,
  Kind.INPUT_OBJECT_TYPE_DEFINITION,
  Kind.DIRECTIVE_DEFINITION,
  Kind.SCHEMA_EXTENSION,
  Kind.SCALAR_TYPE_EXTENSION,
  Kind.OBJECT_TYPE_EXTENSION,
  Kind.INTERFACE_TYPE_EXTENSION,
  Kind.UNION_TYPE_EXTENSION,
  Kind.ENUM_TYPE_EXTENSION,
  Kind.INPUT_OBJECT_TYPE_EXTENSION,
];
