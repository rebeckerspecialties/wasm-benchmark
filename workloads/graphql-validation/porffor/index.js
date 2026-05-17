// graphql-validation benchmark — Porffor-compatible JS
// Mirrors graphql-js's validation dispatch shape: visitor over AST,
// rules-as-class-instances dispatched indirectly through visitor[node.kind].
//
// Names + canonical Kind strings match graphql-js exactly so the
// call_indirect-shape is faithful. Closures-over-locals are forbidden by
// Porffor (PORFFOR-NOTES bugs 1, 2). Plain objects as string-keyed maps on
// class instances drop entries (PORFFOR-NOTES bug 3), so we use `new Map()`
// and `Array` for state on `this`.

// =====================================================================
// Kinds (canonical string values from graphql-js language/kinds.mjs)
// =====================================================================
var KIND_NAME = "Name";
var KIND_DOCUMENT = "Document";
var KIND_OPERATION_DEFINITION = "OperationDefinition";
var KIND_VARIABLE_DEFINITION = "VariableDefinition";
var KIND_SELECTION_SET = "SelectionSet";
var KIND_FIELD = "Field";
var KIND_ARGUMENT = "Argument";
var KIND_FRAGMENT_SPREAD = "FragmentSpread";
var KIND_INLINE_FRAGMENT = "InlineFragment";
var KIND_FRAGMENT_DEFINITION = "FragmentDefinition";
var KIND_VARIABLE = "Variable";
var KIND_NAMED_TYPE = "NamedType";
var KIND_LIST_TYPE = "ListType";
var KIND_NON_NULL_TYPE = "NonNullType";
var KIND_OBJECT_TYPE_DEFINITION = "ObjectTypeDefinition";
var KIND_INTERFACE_TYPE_DEFINITION = "InterfaceTypeDefinition";

// All kind values used by visitInParallel.
var ALL_KINDS = [
  "Name",
  "Document",
  "OperationDefinition",
  "VariableDefinition",
  "SelectionSet",
  "Field",
  "Argument",
  "FragmentSpread",
  "InlineFragment",
  "FragmentDefinition",
  "Variable",
  "NamedType",
  "ListType",
  "NonNullType",
  "ObjectTypeDefinition",
  "InterfaceTypeDefinition",
];

// QueryDocumentKeys subset: child-property names per kind, as a Map of
// kind -> Array<string>. We use a Map here too — to be safe against bug 3
// even though this is a module-level value (and therefore would probably be
// fine as an object literal).
var QueryDocumentKeys = new Map();
QueryDocumentKeys.set("Name", []);
QueryDocumentKeys.set("Document", ["definitions"]);
QueryDocumentKeys.set("OperationDefinition", ["name", "variableDefinitions", "selectionSet"]);
QueryDocumentKeys.set("VariableDefinition", ["variable", "type", "defaultValue"]);
QueryDocumentKeys.set("Variable", ["name"]);
QueryDocumentKeys.set("SelectionSet", ["selections"]);
QueryDocumentKeys.set("Field", ["alias", "name", "arguments", "selectionSet"]);
QueryDocumentKeys.set("Argument", ["name", "value"]);
QueryDocumentKeys.set("FragmentSpread", ["name"]);
QueryDocumentKeys.set("InlineFragment", ["typeCondition", "selectionSet"]);
QueryDocumentKeys.set("FragmentDefinition", ["name", "typeCondition", "selectionSet"]);
QueryDocumentKeys.set("NamedType", ["name"]);
QueryDocumentKeys.set("ListType", ["type"]);
QueryDocumentKeys.set("NonNullType", ["type"]);
QueryDocumentKeys.set("ObjectTypeDefinition", ["name", "interfaces", "fields"]);
QueryDocumentKeys.set("InterfaceTypeDefinition", ["name", "interfaces", "fields"]);

// BREAK sentinel — frozen empty object identity, like graphql-js.
var BREAK = Object.freeze({});

// =====================================================================
// GraphQLError
// =====================================================================
// =====================================================================
// Exception class hierarchy — mirrors graphql-js's `error/` directory:
//
//   Error (built-in)
//     ├── GraphQLError      — graphql-js/error/GraphQLError.mjs
//     └── NonErrorThrown    — graphql-js/jsutils/toError.mjs
//
// Plus factory helpers:
//   toError(value)                     — graphql-js/jsutils/toError.mjs
//   syntaxError(source, pos, descr)    — graphql-js/error/syntaxError.mjs
//   locatedError(raw, nodes, path)     — graphql-js/error/locatedError.mjs
//
// Real graphql-js's validate.mjs distinguishes between thrown values
// (e instanceof GraphQLError vs e === abortObj vs other) inside its
// top-level catch handler, so Porffor needs to emit one wasm tag per
// distinct exception type — which is why we keep both subclasses
// rather than collapsing GraphQLError back to a flat class.
// =====================================================================

class GraphQLError extends Error {
  constructor(message, options) {
    super(message);
    this.name = "GraphQLError";
    if (options !== undefined && options !== null) {
      this.nodes = options.nodes !== undefined ? options.nodes : null;
      this.path = options.path !== undefined ? options.path : null;
      this.originalError =
        options.originalError !== undefined ? options.originalError : null;
      this.extensions =
        options.extensions !== undefined ? options.extensions : null;
    } else {
      this.nodes = null;
      this.path = null;
      this.originalError = null;
      this.extensions = null;
    }
    // Execution-path fields populated by graphql-js's full ctor; the
    // validation path leaves these null but we set them explicitly so
    // JSON shape matches across runtimes that serialize the error.
    this.source = null;
    this.positions = null;
    this.locations = null;
  }
}

class NonErrorThrown extends Error {
  constructor(thrownValue) {
    super("Unexpected error value");
    this.name = "NonErrorThrown";
    this.thrownValue = thrownValue;
  }
}

// graphql-js/jsutils/toError.mjs — promote a raw thrown value to an
// Error instance so catch-side code can rely on a stable API.
function toError(thrownValue) {
  if (thrownValue instanceof Error) {
    return thrownValue;
  }
  return new NonErrorThrown(thrownValue);
}

// graphql-js/error/syntaxError.mjs — convenience GraphQLError ctor
// for parse-time syntax errors.
function syntaxError(source, position, description) {
  return new GraphQLError("Syntax Error: " + description, {
    nodes: null,
  });
}

// graphql-js/error/locatedError.mjs — wraps a downstream error with
// AST location info; preserves an already-located GraphQLError.
function locatedError(rawOriginalError, nodes, path) {
  var originalError = toError(rawOriginalError);
  if (originalError instanceof GraphQLError
      && originalError.path !== null
      && originalError.path !== undefined) {
    return originalError;
  }
  return new GraphQLError(originalError.message, {
    nodes: nodes,
    path: path,
    originalError: originalError,
  });
}

// =====================================================================
// Predicates
// =====================================================================
function isExecutableDefinitionNode(node) {
  return (
    node.kind === KIND_OPERATION_DEFINITION ||
    node.kind === KIND_FRAGMENT_DEFINITION
  );
}

function isCompositeTypeKind(kind) {
  return kind === "Object" || kind === "Interface" || kind === "Union";
}

// =====================================================================
// Schema (Maps everywhere, no plain-object class fields)
// =====================================================================
class TypeDef {
  constructor(name, kind) {
    this.name = name;
    this.kind = kind;            // "Object" | "Interface" | "Scalar" | "Union"
    this.fields = new Map();     // fieldName -> typeName
    this.interfaces = [];        // names
  }
  addField(fname, ftype) { this.fields.set(fname, ftype); }
  getFields() { return this.fields; }
  getInterfaces() { return this.interfaces; }
}

class Schema {
  constructor() {
    this.types = new Map();      // typeName -> TypeDef
    this.queryType = null;
  }
  addType(t) { this.types.set(t.name, t); }
  getType(name) { return this.types.get(name); }
  getTypeMap() { return this.types; }
  getQueryType() { return this.queryType; }
}

function buildSchema(_sdl) {
  // _sdl ignored; schema hand-coded. Mirrors the AS port's
  // assemblyscript/assembly/fixture.ts buildSchema, which is the canonical
  // shape (User/Post/Comment/Tag/Image with Node + Timestamped interfaces
  // and SearchResult union) shared with workloads/graphql-validation/driver.mjs.
  var s = new Schema();

  // Built-in scalars.
  s.addType(new TypeDef("ID", "Scalar"));
  s.addType(new TypeDef("String", "Scalar"));
  s.addType(new TypeDef("Int", "Scalar"));
  s.addType(new TypeDef("Float", "Scalar"));
  s.addType(new TypeDef("Boolean", "Scalar"));

  // interface Node { id: ID! }
  var Node = new TypeDef("Node", "Interface");
  Node.addField("id", "ID");
  s.addType(Node);

  // interface Timestamped { createdAt: String!, updatedAt: String }
  var Timestamped = new TypeDef("Timestamped", "Interface");
  Timestamped.addField("createdAt", "String");
  Timestamped.addField("updatedAt", "String");
  s.addType(Timestamped);

  // type User implements Node & Timestamped
  var User = new TypeDef("User", "Object");
  User.interfaces.push("Node");
  User.interfaces.push("Timestamped");
  User.addField("id", "ID");
  User.addField("createdAt", "String");
  User.addField("updatedAt", "String");
  User.addField("name", "String");
  User.addField("email", "String");
  User.addField("posts", "Post");
  User.addField("followers", "User");
  User.addField("following", "User");
  s.addType(User);

  // type Post implements Node & Timestamped
  var Post = new TypeDef("Post", "Object");
  Post.interfaces.push("Node");
  Post.interfaces.push("Timestamped");
  Post.addField("id", "ID");
  Post.addField("createdAt", "String");
  Post.addField("updatedAt", "String");
  Post.addField("author", "User");
  Post.addField("title", "String");
  Post.addField("body", "String");
  Post.addField("comments", "Comment");
  Post.addField("tags", "Tag");
  s.addType(Post);

  // type Comment implements Node & Timestamped
  var Comment = new TypeDef("Comment", "Object");
  Comment.interfaces.push("Node");
  Comment.interfaces.push("Timestamped");
  Comment.addField("id", "ID");
  Comment.addField("createdAt", "String");
  Comment.addField("updatedAt", "String");
  Comment.addField("author", "User");
  Comment.addField("post", "Post");
  Comment.addField("body", "String");
  Comment.addField("replies", "Comment");
  s.addType(Comment);

  // type Tag implements Node
  var Tag = new TypeDef("Tag", "Object");
  Tag.interfaces.push("Node");
  Tag.addField("id", "ID");
  Tag.addField("label", "String");
  Tag.addField("posts", "Post");
  s.addType(Tag);

  // type Image implements Node
  var Image = new TypeDef("Image", "Object");
  Image.interfaces.push("Node");
  Image.addField("id", "ID");
  Image.addField("url", "String");
  Image.addField("width", "Int");
  Image.addField("height", "Int");
  s.addType(Image);

  // union SearchResult = User | Post | Comment | Tag | Image
  // Modeled as a TypeDef with kind "Union"; the rule consults isCompositeTypeName
  // by name (string), so we only need its name to be in the type map.
  s.addType(new TypeDef("SearchResult", "Union"));

  // type Query
  var Query = new TypeDef("Query", "Object");
  Query.addField("node", "Node");
  Query.addField("user", "User");
  Query.addField("post", "Post");
  Query.addField("search", "SearchResult");
  Query.addField("timeline", "Post");
  s.addType(Query);

  s.queryType = Query;
  return s;
}

// =====================================================================
// Hand-built AST — fixture for the benchmark
// =====================================================================
function nameNode(value) {
  return { kind: KIND_NAME, value: value };
}
function namedType(name) {
  return { kind: KIND_NAMED_TYPE, name: nameNode(name) };
}
function fieldNode(name, sub) {
  var f = {
    kind: KIND_FIELD,
    alias: null,
    name: nameNode(name),
    arguments: [],
    selectionSet: null,
  };
  if (sub) f.selectionSet = sub;
  return f;
}
function fieldNodeWithArgs(name, args, sub) {
  var f = {
    kind: KIND_FIELD,
    alias: null,
    name: nameNode(name),
    arguments: args,
    selectionSet: null,
  };
  if (sub) f.selectionSet = sub;
  return f;
}
function selectionSet(selections) {
  return { kind: KIND_SELECTION_SET, selections: selections };
}
function fragmentSpread(name) {
  return { kind: KIND_FRAGMENT_SPREAD, name: nameNode(name) };
}
function inlineFragment(typeName, selections) {
  return {
    kind: KIND_INLINE_FRAGMENT,
    typeCondition: namedType(typeName),
    selectionSet: selectionSet(selections),
  };
}
function variableNode(name) {
  return { kind: KIND_VARIABLE, name: nameNode(name) };
}
function nonNullType(inner) {
  return { kind: KIND_NON_NULL_TYPE, type: inner };
}
function argumentNode(name, value) {
  return { kind: KIND_ARGUMENT, name: nameNode(name), value: value };
}
function variableDefinition(varName, typeRef) {
  return {
    kind: KIND_VARIABLE_DEFINITION,
    variable: variableNode(varName),
    type: typeRef,
    defaultValue: null,
  };
}
function stringValue(value) {
  return { kind: "StringValue", value: value };
}
function intValue(value) {
  return { kind: "IntValue", value: value };
}

function buildFixtureDocument() {
  // Mirror of the AS port's parse() in
  // assemblyscript/assembly/fixture.ts. The Feed query is the canonical
  // valid sub-tree; five intentional error sites are injected to exercise
  // the rule set:
  //   #1 — second `query Feed` (UniqueOperationNamesRule)
  //   #2 — inline fragment on __INVALID__ inside `search` (KnownTypeNamesRule)
  //   #3 — `nonExistentField` selection on `user(id: $userId)` (FieldsOnCorrectTypeRule)
  //   #4 — `fragment UnknownBits on DoesNotExistType` (KnownTypeNamesRule)
  //   #5 — top-level ObjectTypeDefinition (ExecutableDefinitionsRule)
  // Note on #2: AS's FragmentsOnCompositeTypesRule only fires when the
  // type condition resolves to a non-composite schema type. An unknown
  // name resolves to null and doesn't fire it; the rule below is gated
  // identically (see isCompositeTypeName).

  // Variable definitions: ($userId: ID!, $limit: Int!, $cursor: String)
  var userIdVar = variableDefinition("userId", nonNullType(namedType("ID")));
  var limitVar = variableDefinition("limit", nonNullType(namedType("Int")));
  var cursorVar = variableDefinition("cursor", namedType("String"));

  // user(id: $userId) { ...UserFields posts {…} followers {…} nonExistentField }
  var userPostsBlock = fieldNode("posts", selectionSet([
    fragmentSpread("PostFields"),
    fieldNode("comments", selectionSet([
      fragmentSpread("CommentFields"),
      fieldNode("replies", selectionSet([
        fragmentSpread("CommentFields"),
      ])),
    ])),
  ]));
  var userFollowers = fieldNode("followers", selectionSet([
    fieldNode("id"),
    fieldNode("name"),
  ]));
  var userQueryField = fieldNodeWithArgs(
    "user",
    [argumentNode("id", variableNode("userId"))],
    selectionSet([
      fragmentSpread("UserFields"),
      userPostsBlock,
      userFollowers,
      // Error site #3: nonExistentField on User.
      fieldNode("nonExistentField"),
    ])
  );

  // timeline(userId: $userId, after: $cursor, limit: $limit) {…}
  var timelineQueryField = fieldNodeWithArgs(
    "timeline",
    [
      argumentNode("userId", variableNode("userId")),
      argumentNode("after", variableNode("cursor")),
      argumentNode("limit", variableNode("limit")),
    ],
    selectionSet([
      fragmentSpread("PostFields"),
      fieldNode("comments", selectionSet([
        fragmentSpread("CommentFields"),
      ])),
    ])
  );

  // search(query: "graphql", limit: 5) {
  //   ... on Node, User, Post, Comment, Tag, Image
  //   ... on __INVALID__ {…}                     ← Error site #2
  // }
  var searchQueryField = fieldNodeWithArgs(
    "search",
    [
      argumentNode("query", stringValue("graphql")),
      argumentNode("limit", intValue("5")),
    ],
    selectionSet([
      inlineFragment("Node", [fieldNode("id")]),
      inlineFragment("User", [fieldNode("name"), fieldNode("email")]),
      inlineFragment("Post", [fieldNode("title"), fieldNode("body")]),
      inlineFragment("Comment", [fieldNode("body")]),
      inlineFragment("Tag", [
        fieldNode("label"),
        fieldNode("posts", selectionSet([fieldNode("id"), fieldNode("title")])),
      ]),
      inlineFragment("Image", [
        fieldNode("url"),
        fieldNode("width"),
        fieldNode("height"),
      ]),
      inlineFragment("__INVALID__", [fieldNode("id")]),
    ])
  );

  var operation = {
    kind: KIND_OPERATION_DEFINITION,
    operation: "query",
    name: nameNode("Feed"),
    variableDefinitions: [userIdVar, limitVar, cursorVar],
    selectionSet: selectionSet([userQueryField, timelineQueryField, searchQueryField]),
  };

  // Error site #1: a second operation named "Feed" → UniqueOperationNamesRule.
  var operationDup = {
    kind: KIND_OPERATION_DEFINITION,
    operation: "query",
    name: nameNode("Feed"),
    variableDefinitions: [],
    selectionSet: selectionSet([
      fieldNodeWithArgs("user", [argumentNode("id", stringValue("x"))], selectionSet([
        fieldNode("id"),
      ])),
    ]),
  };

  // fragment UserFields on User { id createdAt updatedAt name email }
  var userFieldsFrag = {
    kind: KIND_FRAGMENT_DEFINITION,
    name: nameNode("UserFields"),
    typeCondition: namedType("User"),
    selectionSet: selectionSet([
      fieldNode("id"),
      fieldNode("createdAt"),
      fieldNode("updatedAt"),
      fieldNode("name"),
      fieldNode("email"),
    ]),
  };

  // fragment PostFields on Post { id createdAt updatedAt title body author { …UserFields } tags { id label } }
  var postFieldsFrag = {
    kind: KIND_FRAGMENT_DEFINITION,
    name: nameNode("PostFields"),
    typeCondition: namedType("Post"),
    selectionSet: selectionSet([
      fieldNode("id"),
      fieldNode("createdAt"),
      fieldNode("updatedAt"),
      fieldNode("title"),
      fieldNode("body"),
      fieldNode("author", selectionSet([fragmentSpread("UserFields")])),
      fieldNode("tags", selectionSet([fieldNode("id"), fieldNode("label")])),
    ]),
  };

  // fragment CommentFields on Comment { id createdAt updatedAt body author { id name } }
  var commentFieldsFrag = {
    kind: KIND_FRAGMENT_DEFINITION,
    name: nameNode("CommentFields"),
    typeCondition: namedType("Comment"),
    selectionSet: selectionSet([
      fieldNode("id"),
      fieldNode("createdAt"),
      fieldNode("updatedAt"),
      fieldNode("body"),
      fieldNode("author", selectionSet([fieldNode("id"), fieldNode("name")])),
    ]),
  };

  // Error site #4: fragment on unknown type → KnownTypeNamesRule.
  var unknownFrag = {
    kind: KIND_FRAGMENT_DEFINITION,
    name: nameNode("UnknownBits"),
    typeCondition: namedType("DoesNotExistType"),
    selectionSet: selectionSet([fieldNode("id")]),
  };

  // Error site #5: a non-executable definition (ObjectTypeDefinition) at
  // document scope → ExecutableDefinitionsRule. Empty interfaces / fields
  // so no NamedType children would also fire KnownTypeNamesRule.
  var schemaLikeDef = {
    kind: KIND_OBJECT_TYPE_DEFINITION,
    name: nameNode("ExtraSchemaTypeDef"),
    interfaces: [],
    fields: [],
  };

  return {
    kind: KIND_DOCUMENT,
    definitions: [
      operation,
      operationDup,
      userFieldsFrag,
      postFieldsFrag,
      commentFieldsFrag,
      unknownFrag,
      schemaLikeDef,
    ],
  };
}

function parse(_source) {
  return buildFixtureDocument();
}

// =====================================================================
// ValidationContext
// =====================================================================
class ValidationContext {
  constructor(schema, ast, typeInfo, maxErrors, abortObj) {
    this._schema = schema;
    this._ast = ast;
    this._typeInfo = typeInfo;
    this._errors = [];
    this._typeStack = [];
    this._fieldDefStack = [];
    // graphql-js's validate.mjs caps the error list at 100 by default and
    // throws a sentinel object (`abortObj`) when the cap is hit, which the
    // top-level `validate()` catches to short-circuit the visitor pass.
    // We propagate the same two pieces of state into ValidationContext so
    // `reportError` can decide whether to throw the sentinel.
    this._maxErrors = maxErrors;
    this._abortObj = abortObj;
  }
  // Mirrors graphql-js's anonymous `error => { ... }` callback in
  // validate.mjs — once errors.length hits maxErrors, push a synthetic
  // GraphQLError telling the host the limit was reached and throw the
  // sentinel so the surrounding `validate()` try/catch can stop the
  // visitor early. The throw target is a frozen object reference,
  // intentionally NOT a GraphQLError, so the catch handler can do
  // identity comparison (`e !== abortObj`) to distinguish the abort
  // path from real errors that need to propagate.
  reportError(error) {
    if (this._errors.length >= this._maxErrors) {
      this._errors.push(new GraphQLError(
        "Too many validation errors, error limit reached. Validation aborted.",
        null,
      ));
      throw this._abortObj;
    }
    this._errors.push(error);
  }
  getErrors() { return this._errors; }
  getSchema() { return this._schema; }
  getDocument() { return this._ast; }
  getParentType() {
    var n = this._typeStack.length;
    if (n === 0) return null;
    return this._typeStack[n - 1];
  }
  getFieldDef() {
    var n = this._fieldDefStack.length;
    if (n === 0) return null;
    return this._fieldDefStack[n - 1];
  }
  pushType(t) { this._typeStack.push(t); }
  popType() { this._typeStack.pop(); }
  pushFieldDef(f) { this._fieldDefStack.push(f); }
  popFieldDef() { this._fieldDefStack.pop(); }
}

// =====================================================================
// Visitor primitives
// =====================================================================
// getEnterLeaveForKind — pulls the {enter, leave} pair for `kind` out of a
// visitor object. Mirrors graphql-js's three visitor permutations.
function getEnterLeaveForKind(visitor, kind) {
  var kv = visitor[kind];
  if (kv !== undefined && kv !== null && typeof kv === "object") {
    return kv;
  }
  if (typeof kv === "function") {
    return { enter: kv, leave: undefined };
  }
  return { enter: visitor.enter, leave: visitor.leave };
}

// MergedKindEntry — one per kind, holds the per-rule enter/leave function
// arrays plus shared `skipping`. The inner enter() loop is the hot
// call_indirect dispatch site; mirrors graphql-js's mergedEnterLeave.
class MergedKindEntry {
  constructor(visitors, enterList, leaveList, skipping) {
    this.visitors = visitors;
    this.enterList = enterList;
    this.leaveList = leaveList;
    this.skipping = skipping;
  }
  enter(node, key, parent, path, ancestors) {
    var visitors = this.visitors;
    var enterList = this.enterList;
    var skipping = this.skipping;
    for (var i = 0; i < visitors.length; i++) {
      if (skipping[i] === null) {
        var fn = enterList[i];
        if (fn !== undefined && fn !== null) {
          // call_indirect-shape: fn comes out of an array, called via .call.
          var result = fn.call(visitors[i], node, key, parent, path, ancestors);
          if (result === false) {
            skipping[i] = node;
          } else if (result === BREAK) {
            skipping[i] = BREAK;
          } else if (result !== undefined) {
            return result;
          }
        }
      }
    }
    return undefined;
  }
  leave(node, key, parent, path, ancestors) {
    var visitors = this.visitors;
    var leaveList = this.leaveList;
    var skipping = this.skipping;
    for (var i = 0; i < visitors.length; i++) {
      if (skipping[i] === null) {
        var fn = leaveList[i];
        if (fn !== undefined && fn !== null) {
          var result = fn.call(visitors[i], node, key, parent, path, ancestors);
          if (result === BREAK) {
            skipping[i] = BREAK;
          } else if (result !== undefined && result !== false) {
            return result;
          }
        }
      } else if (skipping[i] === node) {
        skipping[i] = null;
      }
    }
    return undefined;
  }
}

// visitInParallel — returns a Map of kind -> MergedKindEntry. We use a Map
// instead of a plain object since the merged visitor itself is referenced
// indirectly during traversal.
function visitInParallel(visitors) {
  var skipping = new Array(visitors.length);
  for (var i = 0; i < visitors.length; i++) skipping[i] = null;

  var merged = new Map();
  for (var k = 0; k < ALL_KINDS.length; k++) {
    var kind = ALL_KINDS[k];
    var hasVisitor = false;
    var enterList = new Array(visitors.length);
    var leaveList = new Array(visitors.length);
    for (var i = 0; i < visitors.length; i++) {
      var pair = getEnterLeaveForKind(visitors[i], kind);
      var enterFn = pair !== undefined && pair !== null ? pair.enter : undefined;
      var leaveFn = pair !== undefined && pair !== null ? pair.leave : undefined;
      if (enterFn !== undefined || leaveFn !== undefined) hasVisitor = true;
      enterList[i] = enterFn;
      leaveList[i] = leaveFn;
    }
    if (!hasVisitor) continue;
    merged.set(kind, new MergedKindEntry(visitors, enterList, leaveList, skipping));
  }
  return merged;
}

// visit — DFS over the AST using the merged visitor.
//
// We special-case type tracking for Field/OperationDefinition/Fragment so
// FieldsOnCorrectTypeRule's getParentType()/getFieldDef() returns useful
// values. Done inline here (rather than via a wrapping visitor) to avoid
// layering more class instances.
//
// The hot dispatch site is `entry.enter(node, ...)` — `entry` is a
// MergedKindEntry, and its .enter method loops over the per-rule function
// array, so the indirect call lands inside MergedKindEntry.enter. That's
// the pattern the call_indirect-shape benchmark targets.
function visit(root, merged, context) {
  walkSubtree(root, undefined, null, merged, context);
}

// Walk a child array. Each element is a node — recurse into walkSubtree.
function walkArrayChildren(arr, parent, merged, context) {
  for (var i = 0; i < arr.length; i++) {
    var child = arr[i];
    if (child === undefined || child === null) continue;
    if (typeof child !== "object") continue;
    if (child.kind === undefined) continue;
    walkSubtree(child, i, parent, merged, context);
  }
}

// Walk a single subtree. Uses real recursion (depth bounded by AST depth,
// fine for our fixture). Closures-free: passes `merged`/`context` as args.
function walkSubtree(node, key, parent, merged, context) {
  var entry = merged.get(node.kind);
  var skip = false;
  if (entry !== undefined) {
    var er = entry.enter(node, key, parent, [], []);
    if (er === BREAK) return;
    if (er === false) skip = true;
  }
  if (!skip) {
    enterNodeContext(node, context);
    var ks = QueryDocumentKeys.get(node.kind);
    if (ks !== undefined) {
      for (var i = 0; i < ks.length; i++) {
        var ckey = ks[i];
        var child = node[ckey];
        if (child === undefined || child === null) continue;
        if (Array.isArray(child)) {
          walkArrayChildren(child, node, merged, context);
        } else if (typeof child === "object" && child.kind !== undefined) {
          walkSubtree(child, ckey, node, merged, context);
        }
      }
    }
    leaveNodeContext(node, context);
  }
  if (entry !== undefined) {
    entry.leave(node, key, parent, [], []);
  }
}

// Type-tracking — pushes the "current parent type" for Field nodes so
// FieldsOnCorrectTypeRule sees a parent type. For OperationDefinition we
// push the schema's queryType.
function enterNodeContext(node, context) {
  var k = node.kind;
  if (k === KIND_OPERATION_DEFINITION) {
    var schema = context.getSchema();
    if (schema && schema.queryType) {
      context.pushType(schema.queryType);
    } else {
      context.pushType(null);
    }
    return;
  }
  if (k === KIND_FIELD) {
    var parent = context.getParentType();
    var fieldName = node.name.value;
    var fieldType = null;
    var fieldDef = null;
    if (parent && parent.fields) {
      var typeName = parent.fields.get(fieldName);
      if (typeName !== undefined) {
        fieldDef = { name: fieldName, typeName: typeName };
        var schema = context.getSchema();
        if (schema) {
          var resolved = schema.getType(typeName);
          if (resolved !== undefined) fieldType = resolved;
        }
      }
    }
    context.pushFieldDef(fieldDef);
    context.pushType(fieldType);
    return;
  }
  if (k === KIND_INLINE_FRAGMENT || k === KIND_FRAGMENT_DEFINITION) {
    var schema2 = context.getSchema();
    var typeName2 = node.typeCondition ? node.typeCondition.name.value : null;
    var resolved2 = null;
    if (schema2 && typeName2) {
      var t = schema2.getType(typeName2);
      if (t !== undefined) resolved2 = t;
    }
    context.pushType(resolved2);
    return;
  }
}

function leaveNodeContext(node, context) {
  var k = node.kind;
  if (k === KIND_OPERATION_DEFINITION) {
    context.popType();
    return;
  }
  if (k === KIND_FIELD) {
    context.popFieldDef();
    context.popType();
    return;
  }
  if (k === KIND_INLINE_FRAGMENT || k === KIND_FRAGMENT_DEFINITION) {
    context.popType();
    return;
  }
}

// =====================================================================
// Rules — class-shaped (not factory-returning-closure)
// =====================================================================

class ExecutableDefinitionsRule {
  constructor(context) { this.context = context; }
  Document(node) {
    for (var i = 0; i < node.definitions.length; i++) {
      var d = node.definitions[i];
      if (!isExecutableDefinitionNode(d)) {
        var defName;
        if (d.name && d.name.value) defName = '"' + d.name.value + '"';
        else defName = "schema";
        this.context.reportError(
          new GraphQLError("The " + defName + " definition is not executable.", { nodes: d })
        );
      }
    }
    return false;
  }
}

class UniqueOperationNamesRule {
  constructor(context) {
    this.context = context;
    this.knownOperationNames = new Map();
  }
  OperationDefinition(node) {
    var nameAst = node.name;
    if (nameAst) {
      var v = nameAst.value;
      if (this.knownOperationNames.has(v)) {
        this.context.reportError(
          new GraphQLError(
            'There can be only one operation named "' + v + '".',
            { nodes: [this.knownOperationNames.get(v), nameAst] }
          )
        );
      } else {
        this.knownOperationNames.set(v, nameAst);
      }
    }
    return false;
  }
  FragmentDefinition(_node) { return false; }
}

class KnownTypeNamesRule {
  constructor(context) {
    this.context = context;
    var schema = context.getSchema();
    var existing = schema ? schema.getTypeMap() : new Map();
    var defined = new Map();
    var doc = context.getDocument();
    for (var i = 0; i < doc.definitions.length; i++) {
      var d = doc.definitions[i];
      if (
        d.kind === KIND_OBJECT_TYPE_DEFINITION ||
        d.kind === KIND_INTERFACE_TYPE_DEFINITION
      ) {
        defined.set(d.name.value, true);
      }
    }
    this.existing = existing;
    this.defined = defined;
  }
  NamedType(node, _key, _parent, _path, _ancestors) {
    var typeName = node.name.value;
    if (!this.existing.has(typeName) && !this.defined.has(typeName)) {
      this.context.reportError(
        new GraphQLError('Unknown type "' + typeName + '".', { nodes: node })
      );
    }
  }
}

class FieldsOnCorrectTypeRule {
  constructor(context) { this.context = context; }
  Field(node) {
    // walkSubtree calls rule enters BEFORE enterNodeContext pushes the
    // field's own fieldDef onto the stack, so getFieldDef() is stale here.
    // Look the field up on the parent type directly to mirror graphql-js's
    // behavior (and the AS port).
    var type = this.context.getParentType();
    if (type && type.fields) {
      var fieldName = node.name.value;
      if (!type.fields.has(fieldName)) {
        this.context.reportError(
          new GraphQLError(
            'Cannot query field "' + fieldName + '" on type "' + type.name + '".',
            { nodes: node }
          )
        );
      }
    }
  }
}

class FragmentsOnCompositeTypesRule {
  constructor(context) { this.context = context; }
  InlineFragment(node) {
    var tc = node.typeCondition;
    if (tc) {
      // Align with AS port: typeFromAST returns null for unknown names, so
      // unknown types never fire this rule (they fire KnownTypeNamesRule
      // instead). Only types in the schema that aren't composite fire here.
      var schema = this.context.getSchema();
      var typeName = tc.name.value;
      var t = schema ? schema.getType(typeName) : undefined;
      if (t !== undefined && !isCompositeTypeKind(t.kind)) {
        this.context.reportError(
          new GraphQLError(
            'Fragment cannot condition on non composite type "' + typeName + '".',
            { nodes: tc }
          )
        );
      }
    }
  }
  FragmentDefinition(node) {
    var typeName = node.typeCondition.name.value;
    var schema = this.context.getSchema();
    var t = schema ? schema.getType(typeName) : undefined;
    if (t !== undefined && !isCompositeTypeKind(t.kind)) {
      this.context.reportError(
        new GraphQLError(
          'Fragment "' + node.name.value + '" cannot condition on non composite type "' + typeName + '".',
          { nodes: node.typeCondition }
        )
      );
    }
  }
}

// specifiedRules — same constant graphql-js exports.
var specifiedRules = [
  ExecutableDefinitionsRule,
  UniqueOperationNamesRule,
  KnownTypeNamesRule,
  FieldsOnCorrectTypeRule,
  FragmentsOnCompositeTypesRule,
];

// =====================================================================
// TypeInfo (placeholder)
// =====================================================================
class TypeInfo {
  constructor(schema) { this.schema = schema; }
}

// =====================================================================
// validate — orchestrator
//
// Mirrors graphql-js/validation/validate.mjs:
//
//   const errors = [];
//   const abortObj = Object.freeze({});
//   const context = new ValidationContext(..., (error) => {
//     if (errors.length >= maxErrors) {
//       errors.push(new GraphQLError('Too many ...'));
//       throw abortObj;
//     }
//     errors.push(error);
//   });
//   try {
//     visit(documentAST, ..., visitor);
//   } catch (e) {
//     if (e !== abortObj) throw e;
//   }
//   return errors;
//
// Two same-function exception flows:
//   1. abortObj escape — a reportError call deep inside the visitor
//      throws `abortObj`. The throw walks up through the visitor frame
//      tree (visit → enter → rule → context.reportError → throw) and
//      lands in validate's own catch handler. Spec exercise of cross-
//      function throw → same-function catch.
//   2. Rethrow — if any non-abortObj error escapes (e.g. a TypeError
//      from a bad cast), validate re-throws so the host sees it. Spec
//      exercise of `throw e` from a catch body.
//
// We hoist `abortObj` to module scope rather than declaring it inside
// validate() because Porffor can't capture function-local bindings in
// the closure passed to ValidationContext (see PORFFOR-NOTES.md bug 1).
// The semantics are identical — abortObj is a frozen sentinel object
// that's compared with identity (`e !== abortObj`).
// =====================================================================
var __validateAbortObj = Object.freeze({});

function validate(schema, documentAST, rules, options, typeInfo) {
  var actualRules = rules ? rules : specifiedRules;
  var actualTypeInfo = typeInfo ? typeInfo : new TypeInfo(schema);
  // graphql-js default. Allows the cap to be tuned per-call (we don't
  // expose that knob here but pass a value so ValidationContext has it).
  var maxErrors =
    options !== undefined && options !== null
      && options.maxErrors !== undefined && options.maxErrors !== null
      ? options.maxErrors
      : 100;

  var context = new ValidationContext(
    schema, documentAST, actualTypeInfo, maxErrors, __validateAbortObj);

  // Build rule visitors via `new` (NOT rules.map(rule => rule(context)) —
  // both `map` w/ closure and the factory pattern violate Porffor bug 1).
  var visitors = [];
  for (var i = 0; i < actualRules.length; i++) {
    visitors.push(new actualRules[i](context));
  }

  var merged = visitInParallel(visitors);

  try {
    visit(documentAST, merged, context);
  } catch (e) {
    // graphql-js: `if (e !== abortObj) throw e;`
    // Two states the catch can land in:
    //   - e === abortObj: the maxErrors cap fired, swallow and return
    //     the (now capped) errors list.
    //   - otherwise: an unexpected error escaped the visitor pipeline
    //     (probably a TypeError from a malformed schema/document);
    //     re-raise it so the host surfaces it as a fatal trap.
    if (e !== __validateAbortObj) {
      throw e;
    }
  }

  return context.getErrors();
}

// =====================================================================
// Entry point
// =====================================================================
var schema = buildSchema("");
var doc = parse("");
var errs = validate(schema, doc, specifiedRules, null, null);

console.log("validate: errors=" + errs.length);
