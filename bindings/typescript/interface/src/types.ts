/** A MOTLY scalar: string, number, boolean, or Date. */
export type MOTLYScalar = string | number | boolean | Date;

/** A reference to another node in the MOTLY tree (e.g. `$^parent.name`). */
export interface MOTLYRef {
  linkTo: string;
}

/**
 * A value node in the MOTLY tree.
 *
 * - `eq` — the node's assigned value: a scalar, or an array of child nodes
 * - `properties` — named child nodes (the node's "tags")
 * - `deleted` — true if this node was explicitly deleted with `-name`
 */
export interface MOTLYValue {
  eq?: MOTLYScalar | MOTLYNode[];
  properties?: Record<string, MOTLYNode>;
  deleted?: boolean;
}

/**
 * A node in the MOTLY tree: either a {@link MOTLYValue} (with eq/properties)
 * or a {@link MOTLYRef} (a link to another node). Distinguish them by checking
 * for the `linkTo` property.
 */
export type MOTLYNode = MOTLYValue | MOTLYRef;

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

/** An error from reference validation. */
export interface MOTLYValidationError {
  /** Machine-readable error code (e.g. `"unresolved-reference"`). */
  code: string;
  /** Human-readable error message. */
  message: string;
  /** Path to the offending reference (e.g. `["spec", "ref"]`). */
  path: string[];
}
