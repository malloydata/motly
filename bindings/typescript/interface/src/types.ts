/** A MOTLY scalar: string, number, boolean, or Date. */
export type MOTLYScalar = string | number | boolean | Date;

/** A reference to another node in the MOTLY tree (e.g. `$^parent.name`). */
export interface MOTLYRef {
  linkTo: string;
}

/** An environment variable reference (e.g. `@env.API_KEY`). */
export interface MOTLYEnvRef {
  env: string;
}

/**
 * A value node in the MOTLY tree.
 *
 * - `eq` — the node's assigned value: a scalar, a reference ({@link MOTLYRef}),
 *   or an array of child nodes
 * - `properties` — named child nodes (the node's "tags")
 * - `deleted` — true if this node was explicitly deleted with `-name`
 */
export interface MOTLYValue {
  eq?: MOTLYScalar | MOTLYRef | MOTLYEnvRef | MOTLYNode[];
  properties?: Record<string, MOTLYNode>;
  deleted?: boolean;
}

/**
 * A node in the MOTLY tree. Every node is a {@link MOTLYValue}.
 * References are represented as `eq: { linkTo: "..." }` inside a value node.
 */
export type MOTLYNode = MOTLYValue;

/** A parse error with source location span. */
export interface MOTLYError {
  /** Machine-readable error code (e.g. `"tag-parse-syntax-error"`). */
  code: string;
  /** Human-readable error message. */
  message: string;
  /** Start of the offending region (0-based line, column, and byte offset). */
  begin: { line: number; column: number; offset: number };
  /** End of the offending region (0-based, exclusive). */
  end: { line: number; column: number; offset: number };
}

/** An error from schema validation. */
export interface MOTLYSchemaError {
  /** Machine-readable error code (e.g. `"missing-required"`, `"wrong-type"`). */
  code: string;
  /** Human-readable error message. */
  message: string;
  /** Path to the offending node (e.g. `["metadata", "name"]`). */
  path: string[];
}

/** Type guard: is this eq value a link reference? */
export function isRef(eq: MOTLYValue["eq"]): eq is MOTLYRef {
  return typeof eq === "object" && eq !== null && "linkTo" in eq && !Array.isArray(eq) && !(eq instanceof Date);
}

/** Type guard: is this eq value an env reference? */
export function isEnvRef(eq: MOTLYValue["eq"]): eq is MOTLYEnvRef {
  return typeof eq === "object" && eq !== null && "env" in eq && !Array.isArray(eq) && !(eq instanceof Date);
}

/** An error from reference validation. */
export interface MOTLYValidationError {
  /** Machine-readable error code (e.g. `"unresolved-reference"`). */
  code: string;
  /** Human-readable error message. */
  message: string;
  /** Path to the offending reference (e.g. `["spec", "ref"]`). */
  path: string[];
}
