/** A source location attached to a node, relative to a specific parse() call. */
export interface MOTLYLocation {
  /** Which parse() call produced this node (0-based, auto-incrementing per session). */
  parseId: number;
  /** Start of the defining region (0-based line, column, and byte offset). */
  begin: { line: number; column: number; offset: number };
  /** End of the defining region (0-based, exclusive). */
  end: { line: number; column: number; offset: number };
}

/** The result of a parse() call. */
export interface MOTLYParseResult {
  /** The parse ID assigned to this call (for mapping locations back to sources). */
  parseId: number;
  /** Any parse or execution errors encountered. */
  errors: MOTLYError[];
}

/** A MOTLY scalar: string, number, boolean, or Date. */
export type MOTLYScalar = string | number | boolean | Date;

/** A segment in a reference path: a property name (string) or an array index (number). */
export type MOTLYRefSegment = string | number;

/** A reference to another node in the MOTLY tree (e.g. `$^.parent.name`). */
export interface MOTLYRef {
  linkTo: MOTLYRefSegment[];
  linkUps: number;
}

/** An environment variable reference (e.g. `@env.API_KEY`). */
export interface MOTLYEnvRef {
  env: string;
}

/** What goes to the right of = (the eq slot). */
export type MOTLYValue = MOTLYScalar | MOTLYEnvRef | MOTLYNode[];

/**
 * A concrete node in the MOTLY tree.
 *
 * - `eq` — the node's assigned value: a scalar, an env ref ({@link MOTLYEnvRef}),
 *   or an array of nodes
 * - `properties` — named child nodes
 * - `deleted` — true if this node was explicitly deleted with `-name`
 */
export interface MOTLYDataNode {
  eq?: MOTLYValue;
  properties?: Record<string, MOTLYNode>;
  deleted?: boolean;
  /** Source location of this node's first appearance (set by the interpreter). */
  location?: MOTLYLocation;
}

/**
 * A node in the MOTLY tree: either a concrete data node or a link reference.
 *
 * A `MOTLYRef` means "this IS that other node" — no own value, no own properties.
 * A `MOTLYDataNode` is a full node with optional eq, properties, and deleted flag.
 */
export type MOTLYNode = MOTLYDataNode | MOTLYRef;

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
  /** Source location of the offending node (if available). */
  location?: MOTLYLocation;
}

/** Format a MOTLYRef for display (e.g. `$^^.parent.name`). */
export function formatRef(link: MOTLYRef): string {
  let s = "$";
  for (let i = 0; i < link.linkUps; i++) s += "^";
  let first = true;
  for (const seg of link.linkTo) {
    if (typeof seg === "string") {
      if (!first || link.linkUps > 0) s += ".";
      s += seg;
      first = false;
    } else {
      s += `[${seg}]`;
    }
  }
  return s;
}

/**
 * Create an empty property bag.
 *
 * Property names come from untrusted source text, so a bag must have no
 * prototype: a MOTLY property named `__proto__` or `toString` is an ordinary
 * entry, and on a plain `{}` it would instead read from or write to
 * `Object.prototype`. Every property bag in a MOTLY tree is built here.
 */
export function emptyProperties(): Record<string, MOTLYNode> {
  return Object.create(null) as Record<string, MOTLYNode>;
}

/**
 * Look up a name in a property bag, ignoring anything inherited. A bag handed
 * in by a caller may be a plain object, where `toString` answers a lookup
 * without being a property of the node.
 */
export function getProperty<T>(
  props: Record<string, T> | undefined,
  name: string
): T | undefined {
  if (props === undefined) return undefined;
  return Object.prototype.hasOwnProperty.call(props, name) ? props[name] : undefined;
}

/** Type guard: is this node a link reference? */
export function isRef(node: MOTLYNode | undefined): node is MOTLYRef {
  return typeof node === "object" && node !== null && "linkTo" in node && "linkUps" in node && !Array.isArray(node) && !(node instanceof Date);
}

/** Type guard: is this node a data node (not a ref)? */
export function isDataNode(node: MOTLYNode | undefined): node is MOTLYDataNode {
  return typeof node === "object" && node !== null && !isRef(node);
}

/** Type guard: is this eq value an env reference? */
export function isEnvRef(eq: MOTLYDataNode["eq"]): eq is MOTLYEnvRef {
  return typeof eq === "object" && eq !== null && "env" in eq && !Array.isArray(eq) && !(eq instanceof Date);
}

/** Type guard: is this caught value a MOTLYError? */
export function isMotlyError(e: unknown): e is MOTLYError {
  return (
    typeof e === "object" && e !== null &&
    "code" in e && "message" in e && "begin" in e && "end" in e
  );
}

/** Options for creating a MOTLYSession. */
export interface MOTLYSessionOptions {
  /** When true, `$`-references produce errors. `:= $ref` (clone) is always allowed. */
  disableReferences?: boolean;
}

/** An error from reference validation. */
export interface MOTLYValidationError {
  /** Machine-readable error code (e.g. `"unresolved-reference"`). */
  code: string;
  /** Human-readable error message. */
  message: string;
  /** Path to the offending reference (e.g. `["spec", "ref"]`). */
  path: string[];
  /** Source location of the offending reference (if available). */
  location?: MOTLYLocation;
}
