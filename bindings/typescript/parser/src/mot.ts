import {
  MOTLYNode,
  MOTLYDataNode,
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
 * const port = config.numeric("server", "port");      // equivalent shorthand
 * ```
 */
export interface Mot {
  /** `true` for any real node (including flags with no value). `false` only for the Undefined Mot. */
  readonly exists: boolean;

  /**
   * The type of the value slot, or `undefined` if the node has no value.
   * If path segments are provided, navigates first via {@link get}.
   */
  valueType(...path: MotPath): "string" | "number" | "boolean" | "date" | "array" | undefined;

  /**
   * The string value, or `undefined` if the value is not a string.
   * If path segments are provided, navigates first via {@link get}.
   */
  text(...path: MotPath): string | undefined;

  /**
   * The numeric value, or `undefined` if the value is not a number.
   * If path segments are provided, navigates first via {@link get}.
   */
  numeric(...path: MotPath): number | undefined;

  /**
   * The boolean value, or `undefined` if the value is not a boolean.
   * If path segments are provided, navigates first via {@link get}.
   */
  boolean(...path: MotPath): boolean | undefined;

  /**
   * The date value, or `undefined` if the value is not a date.
   * If path segments are provided, navigates first via {@link get}.
   */
  date(...path: MotPath): Date | undefined;

  /**
   * The array elements as Mots, or `undefined` if the value is not an array.
   * If path segments are provided, navigates first via {@link get}.
   */
  values(...path: MotPath): Mot[] | undefined;

  /**
   * All array elements as strings, or `undefined` if any element is not a string.
   * If path segments are provided, navigates first via {@link get}.
   */
  texts(...path: MotPath): string[] | undefined;

  /**
   * All array elements as numbers, or `undefined` if any element is not a number.
   * If path segments are provided, navigates first via {@link get}.
   */
  numerics(...path: MotPath): number[] | undefined;

  /**
   * All array elements as booleans, or `undefined` if any element is not a boolean.
   * If path segments are provided, navigates first via {@link get}.
   */
  booleans(...path: MotPath): boolean[] | undefined;

  /**
   * All array elements as Dates, or `undefined` if any element is not a date.
   * If path segments are provided, navigates first via {@link get}.
   */
  dates(...path: MotPath): Date[] | undefined;

  /** The property names. Empty for nodes with no properties and for the Undefined Mot. */
  readonly keys: Iterable<string>;

  /** The `[name, Mot]` pairs for all properties. */
  readonly entries: Iterable<[string, Mot]>;

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
  get(...path: MotPath): Mot;

  /**
   * Returns `true` if the full path exists.
   * Equivalent to `.get(...path).exists`.
   */
  has(...path: MotPath): boolean;
}

/**
 * A resolved value in a Mot node. References have been followed, env vars
 * substituted, and deletions consumed. Used by {@link MotFactory} to
 * communicate the resolved value to custom Mot implementations.
 */
export type MotResolvedValue =
  | { type: "string"; value: string }
  | { type: "number"; value: number }
  | { type: "boolean"; value: boolean }
  | { type: "date"; value: Date }
  | { type: "array"; value: Mot[] }
  | undefined;

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

const EMPTY_ITER: Iterable<never> = {
  [Symbol.iterator]: () => ({
    next: () => ({ done: true as const, value: undefined as never }),
  }),
};

const undefinedMot: Mot = {
  exists: false,
  valueType() { return undefined; },
  text() { return undefined; },
  numeric() { return undefined; },
  boolean() { return undefined; },
  date() { return undefined; },
  values() { return undefined; },
  texts() { return undefined; },
  numerics() { return undefined; },
  booleans() { return undefined; },
  dates() { return undefined; },
  keys: EMPTY_ITER as Iterable<string>,
  entries: EMPTY_ITER as Iterable<[string, Mot]>,
  get() { return undefinedMot; },
  has() { return false; },
};

function makeMot(
  resolvedValue: MotResolvedValue,
  properties: Map<string, Mot>,
): Mot {
  const mot: Mot = {
    exists: true,

    valueType(...path: MotPath) {
      if (path.length > 0) return mot.get(...path).valueType();
      return resolvedValue?.type;
    },

    text(...path: MotPath) {
      if (path.length > 0) return mot.get(...path).text();
      return resolvedValue?.type === "string" ? resolvedValue.value : undefined;
    },

    numeric(...path: MotPath) {
      if (path.length > 0) return mot.get(...path).numeric();
      return resolvedValue?.type === "number" ? resolvedValue.value : undefined;
    },

    boolean(...path: MotPath) {
      if (path.length > 0) return mot.get(...path).boolean();
      return resolvedValue?.type === "boolean"
        ? resolvedValue.value
        : undefined;
    },

    date(...path: MotPath) {
      if (path.length > 0) return mot.get(...path).date();
      return resolvedValue?.type === "date" ? resolvedValue.value : undefined;
    },

    values(...path: MotPath) {
      if (path.length > 0) return mot.get(...path).values();
      return resolvedValue?.type === "array" ? resolvedValue.value : undefined;
    },

    texts(...path: MotPath) {
      if (path.length > 0) return mot.get(...path).texts();
      if (resolvedValue?.type !== "array") return undefined;
      const result: string[] = [];
      for (const m of resolvedValue.value) {
        const t = m.text();
        if (t === undefined) return undefined;
        result.push(t);
      }
      return result;
    },

    numerics(...path: MotPath) {
      if (path.length > 0) return mot.get(...path).numerics();
      if (resolvedValue?.type !== "array") return undefined;
      const result: number[] = [];
      for (const m of resolvedValue.value) {
        const n = m.numeric();
        if (n === undefined) return undefined;
        result.push(n);
      }
      return result;
    },

    booleans(...path: MotPath) {
      if (path.length > 0) return mot.get(...path).booleans();
      if (resolvedValue?.type !== "array") return undefined;
      const result: boolean[] = [];
      for (const m of resolvedValue.value) {
        const b = m.boolean();
        if (b === undefined) return undefined;
        result.push(b);
      }
      return result;
    },

    dates(...path: MotPath) {
      if (path.length > 0) return mot.get(...path).dates();
      if (resolvedValue?.type !== "array") return undefined;
      const result: Date[] = [];
      for (const m of resolvedValue.value) {
        const d = m.date();
        if (d === undefined) return undefined;
        result.push(d);
      }
      return result;
    },

    get keys() {
      return [...properties.keys()];
    },

    get entries() {
      return [...properties.entries()];
    },

    get(...path: MotPath) {
      let current: Mot = mot;
      for (const seg of path) {
        if (typeof seg === "number") {
          const arr = current.values();
          if (!arr || !Number.isInteger(seg) || seg < 0 || seg >= arr.length) return undefinedMot;
          current = arr[seg];
        } else {
          if (current === mot) {
            current = properties.get(seg) ?? undefinedMot;
          } else {
            current = current.get(seg);
          }
        }
        if (!current.exists) return undefinedMot;
      }
      return current;
    },

    has(...path: MotPath) {
      return mot.get(...path).exists;
    },
  };
  return mot;
}

const defaultFactory: MotFactory = {
  createMot: makeMot,
  undefinedMot,
};

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
      return resolveNode(nav.target, root, nav.ancestors);
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
