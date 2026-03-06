import {
  MOTLYNode,
  MOTLYPropertyValue,
  MOTLYRef,
  MOTLYValue,
  isRef,
  isEnvRef,
} from "../../interface/src/types";

export interface Mot {
  readonly exists: boolean;
  readonly valueType: "string" | "number" | "boolean" | "date" | "array" | undefined;
  readonly text: string | undefined;
  readonly number: number | undefined;
  readonly boolean: boolean | undefined;
  readonly date: Date | undefined;
  readonly values: Mot[] | undefined;
  readonly texts: string[] | undefined;
  readonly numbers: number[] | undefined;
  readonly booleans: boolean[] | undefined;
  readonly dates: Date[] | undefined;
  readonly keys: Iterable<string>;
  readonly entries: Iterable<[string, Mot]>;
  get(...props: string[]): Mot;
  has(...props: string[]): boolean;
}

export interface GetMotOptions {
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

// Navigate a ref to its final concrete MOTLYNode target.
// Returns undefined if the ref can't be resolved (missing path, cycle, etc.)
function navigateRef(
  ref: MOTLYRef,
  root: MOTLYNode,
  ancestors: MOTLYNode[],
  visiting: Set<MOTLYPropertyValue>,
): { target: MOTLYNode; ancestors: MOTLYNode[] } | undefined {
  let start: MOTLYNode;
  let startAncestors: MOTLYNode[];
  if (ref.linkUps === 0) {
    start = root;
    startAncestors = [];
  } else {
    const idx = ancestors.length - ref.linkUps;
    if (idx < 0 || idx >= ancestors.length) return undefined;
    start = ancestors[idx];
    startAncestors = ancestors.slice(0, idx);
  }

  let current: MOTLYPropertyValue = start;
  let navAncestors = startAncestors;
  let parent: MOTLYNode = start;

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
      parent = current as MOTLYNode;
      i--;
      continue;
    }
    const node = current as MOTLYNode;
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

  return { target: current as MOTLYNode, ancestors: navAncestors };
}

export function buildMot(root: MOTLYNode, options?: GetMotOptions): Mot {
  const env = options?.env;
  const cache = new Map<MOTLYNode, Mot>();

  // ancestors does NOT include `node` — it's the chain above.
  // Matches the convention in resolve.ts / validate.ts.
  function resolveNode(
    node: MOTLYNode,
    root: MOTLYNode,
    ancestors: MOTLYNode[],
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
        const childMot = resolvePropertyValue(pv, root, ancestors, node);
        if (childMot.exists) {
          properties.set(key, childMot);
        }
      }
    }

    return mot;
  }

  // ancestors = chain above parentNode (not including parentNode).
  // parentNode = the node that owns this property.
  function resolvePropertyValue(
    pv: MOTLYPropertyValue,
    root: MOTLYNode,
    ancestors: MOTLYNode[],
    parentNode: MOTLYNode,
  ): Mot {
    if (isRef(pv)) {
      // Refs resolve relative to ancestors (not including parentNode)
      const visiting = new Set<MOTLYPropertyValue>();
      visiting.add(pv);
      const nav = navigateRef(pv, root, ancestors, visiting);
      if (!nav) return undefinedMot;
      if (nav.target.deleted) return undefinedMot;
      return resolveNode(nav.target, root, nav.ancestors);
    }
    const node = pv as MOTLYNode;
    if (node.deleted) return undefinedMot;
    // Child nodes get parentNode pushed onto ancestors
    return resolveNode(node, root, [...ancestors, parentNode]);
  }

  function resolveEq(
    eq: MOTLYValue | undefined,
    root: MOTLYNode,
    ancestors: MOTLYNode[],
    parentNode: MOTLYNode,
  ): ResolvedValue {
    if (eq === undefined) return undefined;
    if (isEnvRef(eq)) {
      const val = env ? env[eq.env] : undefined;
      if (val === undefined) return undefined;
      return { type: "string", value: val };
    }
    if (Array.isArray(eq)) {
      // Array elements are children of parentNode.
      // Push parentNode onto ancestors, matching resolve.ts convention.
      const arrAncestors = [...ancestors, parentNode];
      const elements: Mot[] = [];
      for (const elem of eq) {
        elements.push(resolvePropertyValue(elem, root, arrAncestors, parentNode));
      }
      return { type: "array", value: elements };
    }
    if (typeof eq === "string") return { type: "string", value: eq };
    if (typeof eq === "number") return { type: "number", value: eq };
    if (typeof eq === "boolean") return { type: "boolean", value: eq };
    if (eq instanceof Date) return { type: "date", value: eq };
    return undefined;
  }

  // Initial call: root is in its own ancestor chain (matches resolve.ts)
  return resolveNode(root, root, [root]);
}
