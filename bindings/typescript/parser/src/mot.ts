import {
  MOTLYDataNode,
  MOTLYNode,
  MOTLYRef,
  MOTLYValue,
  isRef,
  isEnvRef,
} from "../../interface/src/types";

/**
 * A path segment for navigating a Mot tree: a property name (string)
 * or an array index (number).
 */
export type MotPath = (string | number)[];

/**
 * A resolved value in a Mot node. References have been followed, env vars
 * substituted, and deletions consumed. Used by {@link MotFactory} to
 * communicate the resolved value to custom Mot implementations.
 */
export type MotResolvedValue<M extends Mot = Mot> =
  | { type: "string"; value: string }
  | { type: "number"; value: number }
  | { type: "boolean"; value: boolean }
  | { type: "date"; value: Date }
  | { type: "array"; value: M[] }
  | undefined;

/**
 * Reference data from the parse tree: how many levels up, and the path
 * segments to follow. Passed to {@link MotFactory.createRefMot} so
 * implementations can preserve reference structure for serialization.
 */
export interface MotRefData {
  /** Number of `^` ups (0 = absolute from root). */
  readonly linkUps: number;
  /** Path segments: property names (string) or array indices (number). */
  readonly linkTo: readonly (string | number)[];
}

/**
 * Factory for creating Mot instances. Pass via {@link GetMotOptions.factory}
 * to control what objects {@link buildMot} produces (e.g., Tags with read
 * tracking).
 *
 * The factory's {@link createMot} receives a resolved value and a mutable
 * properties Map. The Map is empty at creation time and populated afterward —
 * implementations must read from it lazily (not copy at construction time).
 *
 * **Note on array types**: When `M extends Mot`, array elements in
 * `MotResolvedValue` are typed as `Mot[]` but are `M` instances at runtime.
 * Factory implementations should cast if needed: `value.value as M[]`.
 */
export interface MotFactory<M extends Mot = Mot> {
  /** Create a resolved Mot from a value and a lazily-populated properties Map. */
  createMot(value: MotResolvedValue, properties: Map<string, M>): M;
  /** Create a reference Mot that delegates all reads to the resolved target. */
  createRefMot(ref: MotRefData, target: M): M;
  /** The singleton representing a missing/nonexistent node. */
  undefinedMot: M;
}

/**
 * Options for {@link buildMot} and {@link MOTLYSession.getMot}.
 */
export interface GetMotOptions<M extends Mot = Mot> {
  /** Environment variable map for resolving `@env.NAME` references. */
  env?: Record<string, string | undefined>;
  /** Factory for creating custom Mot implementations (e.g., Tags with read tracking). */
  factory?: MotFactory<M>;
}

// ---------------------------------------------------------------------------
// Abstract base class
// ---------------------------------------------------------------------------

/**
 * A resolved, read-only view of a MOTLY node.
 *
 * Every `Mot` has two independent aspects: a **value** (scalar, array, or
 * nothing) and **properties** (named child Mots). References have been
 * followed, environment variables substituted, and deletions consumed.
 *
 * Navigation with {@link get} never returns `undefined` — missing paths
 * produce the **Undefined Mot**, a singleton where `exists` is `false`
 * and all accessors return `undefined`. This enables safe chaining:
 *
 * ```ts
 * const port = config.get("server", "port").numeric(); // number | undefined
 * const port = config.numeric("server", "port");       // equivalent shorthand
 * ```
 *
 * Mot is an abstract class with three concrete subclasses:
 * - {@link MotValue} — a concrete node with a value and/or properties.
 * - `MotRef` — a reference that delegates all reads to a resolved target.
 * - `MotUndefined` — the singleton representing a missing node.
 *
 * Shared logic (path dispatch, array extraction, `has`) lives in the
 * abstract base. Subclasses implement only the leaf-level behaviour via
 * protected abstract methods.
 */
export abstract class Mot {
  /** `true` for any real node (including flags with no value). `false` only for the Undefined Mot. */
  abstract readonly exists: boolean;

  /** `true` if this Mot is a reference to another node (delegates reads to target). */
  abstract readonly isRef: boolean;

  /** Leaf: string value at this node, or undefined. */
  protected abstract _text(): string | undefined;
  /** Leaf: numeric value at this node, or undefined. */
  protected abstract _numeric(): number | undefined;
  /** Leaf: boolean value at this node, or undefined. */
  protected abstract _boolean(): boolean | undefined;
  /** Leaf: date value at this node, or undefined. */
  protected abstract _date(): Date | undefined;
  /** Leaf: array elements at this node, or undefined. */
  protected abstract _values(): Mot[] | undefined;
  /** Leaf: value type at this node, or undefined. */
  protected abstract _valueType(): "string" | "number" | "boolean" | "date" | "array" | undefined;

  /**
   * Navigate by property names and/or array indices. Returns the Mot at the
   * end of the path. String segments navigate properties; number segments
   * index into array values. If any step does not exist, returns the
   * Undefined Mot.
   *
   * ```ts
   * config.get("server", "port")       // equivalent to
   * config.get("server").get("port")
   * ```
   */
  abstract get(...path: MotPath): Mot;

  /** The property names. Empty for nodes with no properties and for the Undefined Mot. */
  abstract get keys(): Iterable<string>;

  /** The `[name, Mot]` pairs for all properties. */
  abstract get entries(): Iterable<[string, Mot]>;

  // --- Shared concrete methods (path dispatch + leaf call) ---

  /**
   * The string value, or `undefined` if the value is not a string.
   * If path segments are provided, navigates first via {@link get}.
   */
  text(...path: MotPath): string | undefined {
    return this.get(...path)._text();
  }

  /**
   * The numeric value, or `undefined` if the value is not a number.
   * If path segments are provided, navigates first via {@link get}.
   */
  numeric(...path: MotPath): number | undefined {
    return this.get(...path)._numeric();
  }

  /**
   * The boolean value, or `undefined` if the value is not a boolean.
   * If path segments are provided, navigates first via {@link get}.
   */
  boolean(...path: MotPath): boolean | undefined {
    return this.get(...path)._boolean();
  }

  /**
   * The date value, or `undefined` if the value is not a date.
   * If path segments are provided, navigates first via {@link get}.
   */
  date(...path: MotPath): Date | undefined {
    return this.get(...path)._date();
  }

  /**
   * The type of the value slot, or `undefined` if the node has no value.
   * If path segments are provided, navigates first via {@link get}.
   */
  valueType(...path: MotPath): "string" | "number" | "boolean" | "date" | "array" | undefined {
    return this.get(...path)._valueType();
  }

  /**
   * The array elements as Mots, or `undefined` if the value is not an array.
   * If path segments are provided, navigates first via {@link get}.
   */
  values(...path: MotPath): Mot[] | undefined {
    return this.get(...path)._values();
  }

  /**
   * All array elements as strings, or `undefined` if any element is not a string.
   * If path segments are provided, navigates first via {@link get}.
   */
  texts(...path: MotPath): string[] | undefined {
    const vals = this.get(...path)._values();
    if (!vals) return undefined;
    const result: string[] = [];
    for (const m of vals) {
      const t = m._text();
      if (t === undefined) return undefined;
      result.push(t);
    }
    return result;
  }

  /**
   * All array elements as numbers, or `undefined` if any element is not a number.
   * If path segments are provided, navigates first via {@link get}.
   */
  numerics(...path: MotPath): number[] | undefined {
    const vals = this.get(...path)._values();
    if (!vals) return undefined;
    const result: number[] = [];
    for (const m of vals) {
      const n = m._numeric();
      if (n === undefined) return undefined;
      result.push(n);
    }
    return result;
  }

  /**
   * All array elements as booleans, or `undefined` if any element is not a boolean.
   * If path segments are provided, navigates first via {@link get}.
   */
  booleans(...path: MotPath): boolean[] | undefined {
    const vals = this.get(...path)._values();
    if (!vals) return undefined;
    const result: boolean[] = [];
    for (const m of vals) {
      const b = m._boolean();
      if (b === undefined) return undefined;
      result.push(b);
    }
    return result;
  }

  /**
   * All array elements as Dates, or `undefined` if any element is not a date.
   * If path segments are provided, navigates first via {@link get}.
   */
  dates(...path: MotPath): Date[] | undefined {
    const vals = this.get(...path)._values();
    if (!vals) return undefined;
    const result: Date[] = [];
    for (const m of vals) {
      const d = m._date();
      if (d === undefined) return undefined;
      result.push(d);
    }
    return result;
  }

  /**
   * Returns `true` if the full path exists.
   * Equivalent to `.get(...path).exists`.
   */
  has(...path: MotPath): boolean {
    return this.get(...path).exists;
  }
}

// ---------------------------------------------------------------------------
// Concrete value node
// ---------------------------------------------------------------------------

/**
 * A concrete Mot node with a resolved value and/or properties.
 * Exported so that Tag (and similar wrappers) can extend it.
 */
export class MotValue<M extends Mot = Mot> extends Mot {
  readonly exists: boolean;
  readonly isRef = false;
  protected readonly _val: MotResolvedValue<M>;
  protected readonly _props: Map<string, M>;

  constructor(
    value: MotResolvedValue<M>,
    properties: Map<string, M>,
    exists = true,
  ) {
    super();
    this._val = value;
    this._props = properties;
    this.exists = exists;
  }

  protected _text() {
    return this._val?.type === "string" ? this._val.value : undefined;
  }

  protected _numeric() {
    return this._val?.type === "number" ? this._val.value : undefined;
  }

  protected _boolean() {
    return this._val?.type === "boolean" ? this._val.value : undefined;
  }

  protected _date() {
    return this._val?.type === "date" ? this._val.value : undefined;
  }

  protected _values() {
    return this._val?.type === "array" ? this._val.value : undefined;
  }

  protected _valueType() {
    return this._val?.type;
  }

  get keys(): Iterable<string> {
    return this._props.keys();
  }

  get entries(): Iterable<[string, Mot]> {
    return this._props.entries();
  }

  get(...path: MotPath): Mot {
    // eslint-disable-next-line @typescript-eslint/no-this-alias
    let current: Mot = this;
    for (const seg of path) {
      if (typeof seg === "number") {
        const arr = current.values();
        if (!arr || !Number.isInteger(seg) || seg < 0 || seg >= arr.length) return undefinedMot;
        current = arr[seg];
      } else if (current === this) {
        current = this._props.get(seg) ?? undefinedMot;
      } else {
        current = current.get(seg);
      }
      if (!current.exists) return undefinedMot;
    }
    return current;
  }
}

// ---------------------------------------------------------------------------
// Reference node (delegates all reads to resolved target)
// ---------------------------------------------------------------------------

class MotRef extends Mot {
  readonly exists = true;
  readonly isRef = true;
  private readonly _target: Mot;

  constructor(_ref: MotRefData, target: Mot) {
    super();
    this._target = target;
  }

  // Leaf methods delegate through public API (no protected access issues)
  protected _text() { return this._target.text(); }
  protected _numeric() { return this._target.numeric(); }
  protected _boolean() { return this._target.boolean(); }
  protected _date() { return this._target.date(); }
  protected _values() { return this._target.values(); }
  protected _valueType() { return this._target.valueType(); }

  get keys() { return this._target.keys; }
  get entries() { return this._target.entries; }

  get(...path: MotPath): Mot {
    return this._target.get(...path);
  }
}

// ---------------------------------------------------------------------------
// Undefined singleton
// ---------------------------------------------------------------------------

const EMPTY_ITER: Iterable<never> = {
  [Symbol.iterator]: () => ({
    next: () => ({ done: true as const, value: undefined as never }),
  }),
};

class MotUndefined extends Mot {
  readonly exists = false;
  readonly isRef = false;

  protected _text() { return undefined; }
  protected _numeric() { return undefined; }
  protected _boolean() { return undefined; }
  protected _date() { return undefined; }
  protected _values() { return undefined; }
  protected _valueType() { return undefined; }

  get keys(): Iterable<string> { return EMPTY_ITER as Iterable<string>; }
  get entries(): Iterable<[string, Mot]> { return EMPTY_ITER as Iterable<[string, Mot]>; }

  get(..._path: MotPath): Mot { return this; }
}

const undefinedMot: Mot = new MotUndefined();

// ---------------------------------------------------------------------------
// Default factory
// ---------------------------------------------------------------------------

const defaultFactory: MotFactory = {
  createMot(value: MotResolvedValue, properties: Map<string, Mot>): Mot {
    return new MotValue(value, properties);
  },
  createRefMot(ref: MotRefData, target: Mot): Mot {
    return new MotRef(ref, target);
  },
  undefinedMot,
};

// ---------------------------------------------------------------------------
// buildMot — resolves the parse tree into a Mot tree
// ---------------------------------------------------------------------------

// Navigate a ref to its final concrete MOTLYDataNode target.
// Returns undefined if the ref can't be resolved (missing path, cycle, etc.)
function navigateRef(
  ref: MOTLYRef,
  root: MOTLYDataNode,
  ancestors: MOTLYDataNode[],
  visiting: Set<MOTLYNode>,
): { target: MOTLYDataNode; ancestors: MOTLYDataNode[] } | undefined {
  let start: MOTLYDataNode;
  let startAncestors: MOTLYDataNode[];
  if (ref.linkUps === 0) {
    start = root;
    startAncestors = [];
  } else {
    const idx = ancestors.length - ref.linkUps;
    if (idx < 0 || idx >= ancestors.length) return undefined;
    start = ancestors[idx];
    startAncestors = ancestors.slice(0, idx);
  }

  let current: MOTLYNode = start;
  let navAncestors = startAncestors;
  let parent: MOTLYDataNode = start;

  for (let i = 0; i < ref.linkTo.length; i++) {
    // If we hit a ref mid-navigation, follow it
    if (isRef(current)) {
      if (visiting.has(current)) return undefined;
      visiting.add(current);
      const resolved = navigateRef(current, root, navAncestors, visiting);
      visiting.delete(current);
      if (!resolved) return undefined;
      current = resolved.target;
      navAncestors = resolved.ancestors;
      parent = current;
      i--;
      continue;
    }
    const node: MOTLYDataNode = current;
    const seg = ref.linkTo[i];
    if (typeof seg === "string") {
      if (!node.properties || !(seg in node.properties)) return undefined;
      if (i > 0) navAncestors = [...navAncestors, parent];
      parent = node;
      current = node.properties[seg];
    } else {
      if (!node.eq || !Array.isArray(node.eq)) return undefined;
      if (seg >= node.eq.length) return undefined;
      if (i > 0) navAncestors = [...navAncestors, parent];
      parent = node;
      current = node.eq[seg];
    }
  }

  // Final target might itself be a ref — follow it
  if (isRef(current)) {
    if (visiting.has(current)) return undefined;
    visiting.add(current);
    const resolved = navigateRef(current, root, navAncestors, visiting);
    visiting.delete(current);
    return resolved;
  }

  return { target: current, ancestors: navAncestors };
}

export function buildMot(root: MOTLYDataNode, options?: GetMotOptions): Mot {
  const env = options?.env;
  const factory = (options?.factory ?? defaultFactory) as MotFactory;
  const undef = factory.undefinedMot;
  const cache = new Map<MOTLYDataNode, Mot>();

  // ancestors does NOT include `node` — it's the chain above.
  // Matches the convention in validate.ts.
  function resolveNode(
    node: MOTLYDataNode,
    root: MOTLYDataNode,
    ancestors: MOTLYDataNode[],
  ): Mot {
    if (node.deleted) return undef;
    if (cache.has(node)) return cache.get(node)!;

    const properties = new Map<string, Mot>();
    // For eq resolution, array elements are children of node, so push node
    const resolvedValue = resolveEq(node.eq, root, ancestors, node);

    const mot = factory.createMot(resolvedValue, properties);
    cache.set(node, mot);

    if (node.properties) {
      for (const [key, pv] of Object.entries(node.properties)) {
        // Refs in properties see `ancestors` (not including node).
        // Child nodes get [...ancestors, node].
        const childMot = resolveMotlyNode(pv, root, ancestors, node);
        if (childMot.exists) {
          properties.set(key, childMot);
        }
      }
    }

    return mot;
  }

  // ancestors = chain above parentNode (not including parentNode).
  // parentNode = the node that owns this property.
  function resolveMotlyNode(
    pv: MOTLYNode,
    root: MOTLYDataNode,
    ancestors: MOTLYDataNode[],
    parentNode: MOTLYDataNode,
  ): Mot {
    if (isRef(pv)) {
      // Refs resolve relative to ancestors (not including parentNode)
      const visiting = new Set<MOTLYNode>();
      visiting.add(pv);
      const nav = navigateRef(pv, root, ancestors, visiting);
      if (!nav) return undef;
      if (nav.target.deleted) return undef;
      const targetMot = resolveNode(nav.target, root, nav.ancestors);
      return factory.createRefMot(pv, targetMot);
    }
    const node = pv;
    if (node.deleted) return undef;
    // Child nodes get parentNode pushed onto ancestors
    return resolveNode(node, root, [...ancestors, parentNode]);
  }

  function resolveEq(
    eq: MOTLYValue | undefined,
    root: MOTLYDataNode,
    ancestors: MOTLYDataNode[],
    parentNode: MOTLYDataNode,
  ): MotResolvedValue {
    if (eq === undefined) return undefined;
    if (isEnvRef(eq)) {
      const val = env ? env[eq.env] : undefined;
      if (val === undefined) return undefined;
      return { type: "string", value: val };
    }
    if (Array.isArray(eq)) {
      // Array elements are children of parentNode.
      // Push parentNode onto ancestors, matching validate.ts convention.
      const arrAncestors = [...ancestors, parentNode];
      const elements: Mot[] = [];
      for (const elem of eq) {
        elements.push(resolveMotlyNode(elem, root, arrAncestors, parentNode));
      }
      return { type: "array", value: elements };
    }
    if (typeof eq === "string") return { type: "string", value: eq };
    if (typeof eq === "number") return { type: "number", value: eq };
    if (typeof eq === "boolean") return { type: "boolean", value: eq };
    if (eq instanceof Date) return { type: "date", value: eq };
    return undefined;
  }

  // Initial call: root is in its own ancestor chain (matches validate.ts)
  return resolveNode(root, root, [root]);
}
