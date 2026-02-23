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

/** What goes to the right of = (the eq slot). */
export type MOTLYValue = MOTLYScalar | MOTLYEnvRef | MOTLYPropertyValue[];

/**
 * A node in the MOTLY tree.
 *
 * - `eq` — the node's assigned value: a scalar, an env ref ({@link MOTLYEnvRef}),
 *   or an array of property values
 * - `properties` — named child property values
 * - `deleted` — true if this node was explicitly deleted with `-name`
 */
export interface MOTLYNode {
  eq?: MOTLYValue;
  properties?: Record<string, MOTLYPropertyValue>;
  deleted?: boolean;
}

/**
 * What a property or array element leads to: either a node or a link reference.
 *
 * A `MOTLYRef` means "this IS that other node" — no own value, no own properties.
 * A `MOTLYNode` is a full node with optional eq, properties, and deleted flag.
 */
export type MOTLYPropertyValue = MOTLYNode | MOTLYRef;

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

/** Type guard: is this property value a link reference? */
export function isRef(pv: MOTLYPropertyValue | undefined): pv is MOTLYRef {
  return typeof pv === "object" && pv !== null && "linkTo" in pv && !Array.isArray(pv) && !(pv instanceof Date);
}

/** Type guard: is this eq value an env reference? */
export function isEnvRef(eq: MOTLYNode["eq"]): eq is MOTLYEnvRef {
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
