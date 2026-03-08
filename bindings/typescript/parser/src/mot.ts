import {
  MOTLYNode,
  MOTLYDataNode,
  MOTLYRef,
  MOTLYValue,
  isRef,
  isEnvRef,
} from "../../interface/src/types";

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
 * const port = config.get("server", "port").number; // number | undefined
 * ```
 */
export interface Mot {
  /** `true` for any real node (including flags with no value). `false` only for the Undefined Mot. */
  readonly exists: boolean;

  /** The type of the value slot, or `undefined` if the node has no value (flag or Undefined Mot). */
  readonly valueType: "string" | "number" | "boolean" | "date" | "array" | undefined;

  /** The string value, or `undefined` if the value is not a string. */
  readonly text: string | undefined;
  /** The numeric value, or `undefined` if the value is not a number. */
  readonly number: number | undefined;
  /** The boolean value, or `undefined` if the value is not a boolean. */
  readonly boolean: boolean | undefined;
  /** The date value, or `undefined` if the value is not a date. */
  readonly date: Date | undefined;

  /** The array elements as Mots, or `undefined` if the value is not an array. */
  readonly values: Mot[] | undefined;
  /** All array elements as strings, or `undefined` if any element is not a string. */
  readonly texts: string[] | undefined;
  /** All array elements as numbers, or `undefined` if any element is not a number. */
  readonly numbers: number[] | undefined;
  /** All array elements as booleans, or `undefined` if any element is not a boolean. */
  readonly booleans: boolean[] | undefined;
  /** All array elements as Dates, or `undefined` if any element is not a date. */
  readonly dates: Date[] | undefined;

  /** The property names. Empty for nodes with no properties and for the Undefined Mot. */
  readonly keys: Iterable<string>;
  /** The `[name, Mot]` pairs for all properties. */
  readonly entries: Iterable<[string, Mot]>;

  /**
   * Walk into properties by name. Returns the Mot at the end of the path.
   * If any step does not exist, returns the Undefined Mot.
   *
   * ```ts
   * config.get("server", "port")       // equivalent to
   * config.get("server").get("port")
   * ```
   */
  get(...props: string[]): Mot;

  /**
   * Returns `true` if the full property path exists.
   * Equivalent to `.get(...props).exists`.
   */
  has(...props: string[]): boolean;
}

/**
 * Options for {@link MOTLYSession.getMot}.
 */
export interface GetMotOptions {
  /** Environment variable map for resolving `@env.NAME` references. */
  env?: Record<string, string | undefined>;
}

const EMPTY_ITER: Iterable<never> = {
  [Symbol.iterator]: () => ({
    next: () => ({ done: true as const, value: undefined as never }),
  }),
};

const undefinedMot: Mot = {
  exists: false,
  valueType: undefined,
  text: undefined,
  number: undefined,
  boolean: undefined,
  date: undefined,
  values: undefined,
  texts: undefined,
  numbers: undefined,
  booleans: undefined,
  dates: undefined,
  keys: EMPTY_ITER as Iterable<string>,
  entries: EMPTY_ITER as Iterable<[string, Mot]>,
  get() {
    return undefinedMot;
  },
  has() {
    return false;
  },
};

type ResolvedValue =
  | { type: "string"; value: string }
  | { type: "number"; value: number }
  | { type: "boolean"; value: boolean }
  | { type: "date"; value: Date }
  | { type: "array"; value: Mot[] }
  | undefined;

function makeMot(
  resolvedValue: ResolvedValue,
  properties: Map<string, Mot>,
): Mot {
  const mot: Mot = {
    exists: true,
    get valueType() {
      return resolvedValue?.type;
    },
    get text() {
      return resolvedValue?.type === "string" ? resolvedValue.value : undefined;
    },
    get number() {
      return resolvedValue?.type === "number" ? resolvedValue.value : undefined;
    },
    get boolean() {
      return resolvedValue?.type === "boolean"
        ? resolvedValue.value
        : undefined;
    },
    get date() {
      return resolvedValue?.type === "date" ? resolvedValue.value : undefined;
    },
    get values() {
      return resolvedValue?.type === "array" ? resolvedValue.value : undefined;
    },
    // TODO: cache convenience array accessors if perf becomes a concern
    get texts() {
      if (resolvedValue?.type !== "array") return undefined;
      const result: string[] = [];
      for (const m of resolvedValue.value) {
        if (m.text === undefined) return undefined;
        result.push(m.text);
      }
      return result;
    },
    get numbers() {
      if (resolvedValue?.type !== "array") return undefined;
      const result: number[] = [];
      for (const m of resolvedValue.value) {
        if (m.number === undefined) return undefined;
        result.push(m.number);
      }
      return result;
    },
    get booleans() {
      if (resolvedValue?.type !== "array") return undefined;
      const result: boolean[] = [];
      for (const m of resolvedValue.value) {
        if (m.boolean === undefined) return undefined;
        result.push(m.boolean);
      }
      return result;
    },
    get dates() {
      if (resolvedValue?.type !== "array") return undefined;
      const result: Date[] = [];
      for (const m of resolvedValue.value) {
        if (m.date === undefined) return undefined;
        result.push(m.date);
      }
      return result;
    },
    get keys() {
      return [...properties.keys()];
    },
    get entries() {
      return [...properties.entries()];
    },
    get(...props: string[]) {
      let current: Mot = mot;
      for (const p of props) {
        if (current === mot) {
          current = properties.get(p) ?? undefinedMot;
        } else {
          current = current.get(p);
        }
        if (!current.exists) return undefinedMot;
      }
      return current;
    },
    has(...props: string[]) {
      return mot.get(...props).exists;
    },
  };
  return mot;
}

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
  const cache = new Map<MOTLYDataNode, Mot>();

  // ancestors does NOT include `node` — it's the chain above.
  // Matches the convention in validate.ts.
  function resolveNode(
    node: MOTLYDataNode,
    root: MOTLYDataNode,
    ancestors: MOTLYDataNode[],
  ): Mot {
    if (node.deleted) return undefinedMot;
    if (cache.has(node)) return cache.get(node)!;

    const properties = new Map<string, Mot>();
    // For eq resolution, array elements are children of node, so push node
    const resolvedValue = resolveEq(node.eq, root, ancestors, node);

    const mot = makeMot(resolvedValue, properties);
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
      if (!nav) return undefinedMot;
      if (nav.target.deleted) return undefinedMot;
      return resolveNode(nav.target, root, nav.ancestors);
    }
    const node = pv;
    if (node.deleted) return undefinedMot;
    // Child nodes get parentNode pushed onto ancestors
    return resolveNode(node, root, [...ancestors, parentNode]);
  }

  function resolveEq(
    eq: MOTLYValue | undefined,
    root: MOTLYDataNode,
    ancestors: MOTLYDataNode[],
    parentNode: MOTLYDataNode,
  ): ResolvedValue {
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
