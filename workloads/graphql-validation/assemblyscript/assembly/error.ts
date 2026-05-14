// Mirrors graphql-js's GraphQLError — minimal shape (message + nodes).
// Validation rules construct GraphQLError instances and pass them to
// context.reportError().
import { ASTNode } from "./ast";

export class GraphQLError {
  message: string;
  nodes: ASTNode[];

  constructor(message: string, nodes: ASTNode[] = []) {
    this.message = message;
    this.nodes = nodes;
  }
}
