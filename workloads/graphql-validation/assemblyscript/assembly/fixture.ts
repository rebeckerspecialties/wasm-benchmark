// Hardcoded benchmark fixture. Mirrors the schema + query in
// `workloads/graphql-validation/driver.mjs` — same node count and shape,
// hand-built directly to avoid bundling a parser into the wasm.
//
// `buildSchema` and `parse` are exposed as zero-arg constructors that
// return the schema / document built from the in-memory data. The names
// match graphql-js's top-level API (`buildSchema(sdl)`, `parse(source)`)
// — the arguments are dropped because we don't carry SDL text through.

import {
  ArgumentNode,
  ASTNode,
  DocumentNode,
  FieldDefinitionNode,
  FieldNode,
  FragmentDefinitionNode,
  FragmentSpreadNode,
  InlineFragmentNode,
  IntValueNode,
  InterfaceTypeDefinitionNode,
  ListTypeNode,
  NameNode,
  NamedTypeNode,
  NonNullTypeNode,
  ObjectTypeDefinitionNode,
  OperationDefinitionNode,
  SelectionSetNode,
  StringValueNode,
  UnionTypeDefinitionNode,
  VariableDefinitionNode,
  VariableNode,
} from "./ast";
import { GraphQLField, GraphQLSchema, GraphQLType } from "./schema";

// Build a GraphQLSchema matching driver.mjs's SDL.
// Types: Node (interface), Timestamped (interface), User, Post, Comment,
// Tag, Image, SearchResult (union), Query.
// Plus standard scalars: ID, String, Int, Boolean.
export function buildSchema(_sdl: string = ""): GraphQLSchema {
  const s = new GraphQLSchema();

  // Scalars (built-in).
  const scalars = ["ID", "String", "Int", "Float", "Boolean"];
  for (let i = 0; i < scalars.length; i++) {
    s.types.set(scalars[i], new GraphQLType(scalars[i], "Scalar"));
  }

  // interface Node { id: ID! }
  const node = new GraphQLType("Node", "Interface");
  node.fields.set("id", new GraphQLField("id", "ID"));
  node.possibleTypes = ["User", "Post", "Comment", "Tag", "Image"];
  s.types.set("Node", node);

  // interface Timestamped { createdAt: String!, updatedAt: String }
  const ts = new GraphQLType("Timestamped", "Interface");
  ts.fields.set("createdAt", new GraphQLField("createdAt", "String"));
  ts.fields.set("updatedAt", new GraphQLField("updatedAt", "String"));
  ts.possibleTypes = ["User", "Post", "Comment"];
  s.types.set("Timestamped", ts);

  // type User implements Node & Timestamped
  const user = new GraphQLType("User", "Object");
  user.interfaces = ["Node", "Timestamped"];
  user.fields.set("id", new GraphQLField("id", "ID"));
  user.fields.set("createdAt", new GraphQLField("createdAt", "String"));
  user.fields.set("updatedAt", new GraphQLField("updatedAt", "String"));
  user.fields.set("name", new GraphQLField("name", "String"));
  user.fields.set("email", new GraphQLField("email", "String"));
  user.fields.set("posts", new GraphQLField("posts", "Post"));
  user.fields.set("followers", new GraphQLField("followers", "User"));
  user.fields.set("following", new GraphQLField("following", "User"));
  s.types.set("User", user);

  // type Post implements Node & Timestamped
  const post = new GraphQLType("Post", "Object");
  post.interfaces = ["Node", "Timestamped"];
  post.fields.set("id", new GraphQLField("id", "ID"));
  post.fields.set("createdAt", new GraphQLField("createdAt", "String"));
  post.fields.set("updatedAt", new GraphQLField("updatedAt", "String"));
  post.fields.set("author", new GraphQLField("author", "User"));
  post.fields.set("title", new GraphQLField("title", "String"));
  post.fields.set("body", new GraphQLField("body", "String"));
  post.fields.set("comments", new GraphQLField("comments", "Comment"));
  post.fields.set("tags", new GraphQLField("tags", "Tag"));
  s.types.set("Post", post);

  // type Comment implements Node & Timestamped
  const comment = new GraphQLType("Comment", "Object");
  comment.interfaces = ["Node", "Timestamped"];
  comment.fields.set("id", new GraphQLField("id", "ID"));
  comment.fields.set("createdAt", new GraphQLField("createdAt", "String"));
  comment.fields.set("updatedAt", new GraphQLField("updatedAt", "String"));
  comment.fields.set("author", new GraphQLField("author", "User"));
  comment.fields.set("post", new GraphQLField("post", "Post"));
  comment.fields.set("body", new GraphQLField("body", "String"));
  comment.fields.set("replies", new GraphQLField("replies", "Comment"));
  s.types.set("Comment", comment);

  // type Tag implements Node
  const tag = new GraphQLType("Tag", "Object");
  tag.interfaces = ["Node"];
  tag.fields.set("id", new GraphQLField("id", "ID"));
  tag.fields.set("label", new GraphQLField("label", "String"));
  tag.fields.set("posts", new GraphQLField("posts", "Post"));
  s.types.set("Tag", tag);

  // type Image implements Node
  const image = new GraphQLType("Image", "Object");
  image.interfaces = ["Node"];
  image.fields.set("id", new GraphQLField("id", "ID"));
  image.fields.set("url", new GraphQLField("url", "String"));
  image.fields.set("width", new GraphQLField("width", "Int"));
  image.fields.set("height", new GraphQLField("height", "Int"));
  s.types.set("Image", image);

  // union SearchResult = User | Post | Comment | Tag | Image
  const search = new GraphQLType("SearchResult", "Union");
  search.possibleTypes = ["User", "Post", "Comment", "Tag", "Image"];
  s.types.set("SearchResult", search);

  // type Query
  const query = new GraphQLType("Query", "Object");
  query.fields.set("node", new GraphQLField("node", "Node"));
  query.fields.set("user", new GraphQLField("user", "User"));
  query.fields.set("post", new GraphQLField("post", "Post"));
  query.fields.set("search", new GraphQLField("search", "SearchResult"));
  query.fields.set("timeline", new GraphQLField("timeline", "Post"));
  s.types.set("Query", query);

  return s;
}

// Helpers. Match graphql-js node-builder names where possible.

function n(name: string): NameNode { return new NameNode(name); }
function nt(name: string): NamedTypeNode { return new NamedTypeNode(n(name)); }
function nn(inner: ASTNode): NonNullTypeNode { return new NonNullTypeNode(inner); }
function lt(inner: ASTNode): ListTypeNode { return new ListTypeNode(inner); }

function field(name: string, args: ArgumentNode[], sel: SelectionSetNode | null): FieldNode {
  return new FieldNode(n(name), args, sel);
}

function arg(name: string, value: ASTNode): ArgumentNode {
  return new ArgumentNode(n(name), value);
}

function vref(name: string): VariableNode {
  return new VariableNode(n(name));
}

function fragSpread(name: string): FragmentSpreadNode {
  return new FragmentSpreadNode(n(name));
}

function inlineFrag(typeName: string, sels: ASTNode[]): InlineFragmentNode {
  return new InlineFragmentNode(nt(typeName), new SelectionSetNode(sels));
}

function selSet(items: ASTNode[]): SelectionSetNode {
  return new SelectionSetNode(items);
}

// `parse` mirrors graphql-js's top-level `parse(source)` — same argument
// name, but ignored (we hand-build the AST instead of parsing).
export function parse(_source: string = ""): DocumentNode {
  // query Feed($userId: ID!, $limit: Int!, $cursor: String) { … }
  const userIdVar = new VariableDefinitionNode(vref("userId"), nn(nt("ID")));
  const limitVar = new VariableDefinitionNode(vref("limit"), nn(nt("Int")));
  const cursorVar = new VariableDefinitionNode(vref("cursor"), nt("String"));

  // Inner bits used multiple times.
  const userPostsBlock = field("posts", [], selSet([
    fragSpread("PostFields"),
    field("comments", [], selSet([
      fragSpread("CommentFields"),
      field("replies", [], selSet([
        fragSpread("CommentFields"),
      ])),
    ])),
  ]));
  const userFollowers = field("followers", [], selSet([
    field("id", [], null),
    field("name", [], null),
  ]));

  // Error site #3: nonExistentField on User → FieldsOnCorrectTypeRule.
  const userQueryField = field("user", [arg("id", vref("userId"))], selSet([
    fragSpread("UserFields"),
    userPostsBlock,
    userFollowers,
    field("nonExistentField", [], null),
  ]));

  const timelineQueryField = field("timeline", [
    arg("userId", vref("userId")),
    arg("after", vref("cursor")),
    arg("limit", vref("limit")),
  ], selSet([
    fragSpread("PostFields"),
    field("comments", [], selSet([
      fragSpread("CommentFields"),
    ])),
  ]));

  // Error site #2: inline fragment on __INVALID__ (non-existent type)
  // → KnownTypeNamesRule fires for the unknown NamedType. AS's
  // FragmentsOnCompositeTypesRule only fires when typeFromAST resolves to
  // a non-composite type; an unknown name resolves to null and doesn't
  // fire. Aligned in Porffor by gating on typeMap presence.
  const searchQueryField = field("search", [
    arg("query", new StringValueNode("graphql")),
    arg("limit", new IntValueNode("5")),
  ], selSet([
    inlineFrag("Node", [field("id", [], null)]),
    inlineFrag("User", [field("name", [], null), field("email", [], null)]),
    inlineFrag("Post", [field("title", [], null), field("body", [], null)]),
    inlineFrag("Comment", [field("body", [], null)]),
    inlineFrag("Tag", [
      field("label", [], null),
      field("posts", [], selSet([
        field("id", [], null),
        field("title", [], null),
      ])),
    ]),
    inlineFrag("Image", [
      field("url", [], null),
      field("width", [], null),
      field("height", [], null),
    ]),
    inlineFrag("__INVALID__", [field("id", [], null)]),
  ]));

  const opSelections: ASTNode[] = [userQueryField, timelineQueryField, searchQueryField];
  const opSel = selSet(opSelections);

  const op = new OperationDefinitionNode(
    "query",
    n("Feed"),
    [userIdVar, limitVar, cursorVar],
    opSel,
  );

  // fragment UserFields on User { id createdAt updatedAt name email }
  const userFields = new FragmentDefinitionNode(
    n("UserFields"),
    nt("User"),
    selSet([
      field("id", [], null),
      field("createdAt", [], null),
      field("updatedAt", [], null),
      field("name", [], null),
      field("email", [], null),
    ]),
  );

  // fragment PostFields on Post { id createdAt updatedAt title body author { ...UserFields } tags { id label } }
  const postFields = new FragmentDefinitionNode(
    n("PostFields"),
    nt("Post"),
    selSet([
      field("id", [], null),
      field("createdAt", [], null),
      field("updatedAt", [], null),
      field("title", [], null),
      field("body", [], null),
      field("author", [], selSet([fragSpread("UserFields")])),
      field("tags", [], selSet([
        field("id", [], null),
        field("label", [], null),
      ])),
    ]),
  );

  // fragment CommentFields on Comment { id createdAt updatedAt body author { id name } }
  const commentFields = new FragmentDefinitionNode(
    n("CommentFields"),
    nt("Comment"),
    selSet([
      field("id", [], null),
      field("createdAt", [], null),
      field("updatedAt", [], null),
      field("body", [], null),
      field("author", [], selSet([
        field("id", [], null),
        field("name", [], null),
      ])),
    ]),
  );

  // Error site #1: a SECOND `query Feed` operation → UniqueOperationNamesRule.
  const opDup = new OperationDefinitionNode(
    "query",
    n("Feed"),
    [],
    selSet([
      field("user", [arg("id", new StringValueNode("x"))], selSet([
        field("id", [], null),
      ])),
    ]),
  );

  // Error site #4: fragment on unknown type → KnownTypeNamesRule fires on
  // the typeCondition's NamedType.
  const unknownFrag = new FragmentDefinitionNode(
    n("UnknownBits"),
    nt("DoesNotExistType"),
    selSet([field("id", [], null)]),
  );

  // Error site #5: a non-executable definition (ObjectTypeDefinition) at
  // document scope → ExecutableDefinitionsRule. Empty interfaces / fields
  // so no NamedType children exist that would trip KnownTypeNamesRule.
  const schemaLikeDef = new ObjectTypeDefinitionNode(
    n("ExtraSchemaTypeDef"),
    [],
    [],
  );

  const defs: ASTNode[] = [
    op,
    opDup,
    userFields,
    postFields,
    commentFields,
    unknownFrag,
    schemaLikeDef,
  ];
  return new DocumentNode(defs);
}
