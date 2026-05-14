// graphql-js validation benchmark driver, structured for StarlingMonkey
// AOT mode.
//
// StarlingMonkey AOT compiles wasi:cli/run as "preinit then exit" — the
// top-level script runs at componentize time and its post-init heap state
// is captured into the wasm. So if we put validate() at top-level, it runs
// at compile time and the runtime wasm just exits with no work.
//
// To get the JS class/interface dispatch happening AT RUNTIME (which is
// the whole point — we want a wasm full of call_indirect actually executing
// during the timed window), we register a `fetch` event listener. Top-level
// runs at preinit (schema build, query parse — these are setup costs we
// pay once). The listener fires for every incoming HTTP request, which
// the host invokes via `wasi:http/incoming-handler@0.2.10`.
//
// Benchmark protocol:
//   - Host instantiates the component once.
//   - Host issues N synthetic incoming-handler invocations (no actual HTTP
//     traffic — wasmtime's component-model API can call the export directly).
//   - Each invocation runs `validate(schema, document)` M times and reports
//     the elapsed time + result count via the response body.
//   - Host times each invocation; the steady-state median is what we report.

import {
  buildSchema,
  parse,
  validate,
} from "graphql";

const schemaSDL = `
  interface Node {
    id: ID!
  }

  interface Timestamped {
    createdAt: String!
    updatedAt: String
  }

  type User implements Node & Timestamped {
    id: ID!
    createdAt: String!
    updatedAt: String
    name: String!
    email: String
    posts: [Post!]!
    followers: [User!]!
    following: [User!]!
  }

  type Post implements Node & Timestamped {
    id: ID!
    createdAt: String!
    updatedAt: String
    author: User!
    title: String!
    body: String!
    comments: [Comment!]!
    tags: [Tag!]!
  }

  type Comment implements Node & Timestamped {
    id: ID!
    createdAt: String!
    updatedAt: String
    author: User!
    post: Post!
    body: String!
    replies: [Comment!]!
  }

  type Tag implements Node {
    id: ID!
    label: String!
    posts: [Post!]!
  }

  type Image implements Node {
    id: ID!
    url: String!
    width: Int
    height: Int
  }

  union SearchResult = User | Post | Comment | Tag | Image

  type Query {
    node(id: ID!): Node
    user(id: ID!): User
    post(id: ID!): Post
    search(query: String!, limit: Int): [SearchResult!]!
    timeline(userId: ID!, before: String, after: String, limit: Int): [Post!]!
  }
`;

const querySource = `
  query Feed($userId: ID!, $limit: Int!, $cursor: String) {
    user(id: $userId) {
      ...UserFields
      posts {
        ...PostFields
        comments {
          ...CommentFields
          replies {
            ...CommentFields
          }
        }
      }
      followers {
        id
        name
      }
    }
    timeline(userId: $userId, after: $cursor, limit: $limit) {
      ...PostFields
      comments {
        ...CommentFields
      }
    }
    search(query: "graphql", limit: 5) {
      ... on Node { id }
      ... on User { name email }
      ... on Post { title body }
      ... on Comment { body }
      ... on Tag { label posts { id title } }
      ... on Image { url width height }
    }
  }

  fragment UserFields on User {
    id
    createdAt
    updatedAt
    name
    email
  }

  fragment PostFields on Post {
    id
    createdAt
    updatedAt
    title
    body
    author {
      ...UserFields
    }
    tags {
      id
      label
    }
  }

  fragment CommentFields on Comment {
    id
    createdAt
    updatedAt
    body
    author {
      id
      name
    }
  }
`;

// Setup happens at preinit (componentize-time). The schema and parsed
// document are part of the post-init snapshot.
const schema = buildSchema(schemaSDL);
const document = parse(querySource);

// Per-request iteration count. Defaults to 200 but can be overridden via
// `?n=NNN` on the request URL — useful for tuning the benchmark window.
const DEFAULT_N = 200;

async function handle(event) {
  const url = new URL(event.request.url);
  const n = parseInt(url.searchParams.get("n") || String(DEFAULT_N), 10);

  let totalErrors = 0;
  for (let i = 0; i < n; i++) {
    const errors = validate(schema, document);
    totalErrors += errors.length;
  }

  const body = `graphql-validation: n=${n} totalErrors=${totalErrors}`;
  return new Response(body, {
    status: 200,
    headers: { "content-type": "text/plain" },
  });
}

addEventListener("fetch", (event) => {
  event.respondWith(handle(event));
});
