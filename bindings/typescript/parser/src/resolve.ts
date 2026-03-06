import {
  MOTLYNode,
  MOTLYPropertyValue,
  MOTLYRef,
  MOTLYRefSegment,
  MOTLYValue,
  isRef,
  isEnvRef,
  formatRef,
} from "../../interface/src/types";

export interface ResolveOptions {
  env?: Record<string, string | undefined>;
}

export function resolveTree(
  root: MOTLYNode,
  options?: ResolveOptions,
): unknown {
  const env = options?.env;
  return resolveNode(root, root, [root], new Set<MOTLYPropertyValue>(), env);
}

function resolveNode(
  node: MOTLYNode,
  root: MOTLYNode,
  ancestors: MOTLYNode[],
  visiting: Set<MOTLYPropertyValue>,
  env: Record<string, string | undefined> | undefined,
): unknown {
  const hasEq = node.eq !== undefined;
  const hasProps = node.properties !== undefined && Object.keys(node.properties).length > 0;

  if (!hasEq && !hasProps) return {};

  // ancestors does NOT include `node` — it's the chain above us.
  // When resolving refs at this level, they see `ancestors` (matching validateReferences).
  // When recursing into child nodes, we push `node` onto ancestors.

  if (hasEq && !hasProps) {
    return resolveEq(node.eq!, root, ancestors, node, visiting, env);
  }

  const obj: Record<string, unknown> = {};
  if (hasEq) {
    obj["="] = resolveEq(node.eq!, root, ancestors, node, visiting, env);
  }
  for (const [key, pv] of Object.entries(node.properties!)) {
    const resolved = resolvePropertyValue(pv, root, ancestors, node, visiting, env);
    if (resolved !== DELETED) {
      obj[key] = resolved;
    }
  }
  return obj;
}

const DELETED = Symbol("deleted");

function resolvePropertyValue(
  pv: MOTLYPropertyValue,
  root: MOTLYNode,
  ancestors: MOTLYNode[],
  parentNode: MOTLYNode,
  visiting: Set<MOTLYPropertyValue>,
  env: Record<string, string | undefined> | undefined,
): unknown | typeof DELETED {
  if (isRef(pv)) {
    // Refs resolve relative to ancestors (not including parentNode)
    return resolveRef(pv, root, ancestors, visiting, env);
  }
  const node = pv as MOTLYNode;
  if (node.deleted) return DELETED;
  // Child nodes get parentNode pushed onto their ancestors
  return resolveNode(node, root, [...ancestors, parentNode], visiting, env);
}

function resolveEq(
  eq: MOTLYValue,
  root: MOTLYNode,
  ancestors: MOTLYNode[],
  parentNode: MOTLYNode,
  visiting: Set<MOTLYPropertyValue>,
  env: Record<string, string | undefined> | undefined,
): unknown {
  if (isEnvRef(eq)) {
    return env ? env[eq.env] : undefined;
  }
  if (Array.isArray(eq)) {
    // Array elements are children of parentNode, matching walkArrayRefs which pushes parentNode
    const arrAncestors = [...ancestors, parentNode];
    return eq.map((elem) => resolvePropertyValue(elem, root, arrAncestors, parentNode, visiting, env));
  }
  return eq;
}

interface NavResult {
  target: MOTLYPropertyValue;
  /** Ancestors of the node containing `target` (not including that node). */
  ancestors: MOTLYNode[];
}

function navigateSegments(
  start: MOTLYNode,
  segments: MOTLYRefSegment[],
  startAncestors: MOTLYNode[],
): NavResult | undefined {
  let current: MOTLYPropertyValue = start;
  // ancestors tracks the chain *above* the node containing `current`
  // (matching validate's convention: the containing node is NOT in ancestors)
  let ancestors = startAncestors;
  let parent: MOTLYNode = start;
  for (let i = 0; i < segments.length; i++) {
    if (isRef(current)) {
      return { target: current, ancestors };
    }
    const node = current as MOTLYNode;
    const seg = segments[i];
    if (typeof seg === "string") {
      if (!node.properties || !(seg in node.properties)) return undefined;
      // For intermediate segments, push the previous containing node onto ancestors.
      // For the first segment, `start` is the containing node — don't push it.
      if (i > 0) {
        ancestors = [...ancestors, parent];
      }
      parent = node;
      current = node.properties[seg];
    } else {
      if (!node.eq || !Array.isArray(node.eq)) return undefined;
      if (seg >= node.eq.length) return undefined;
      if (i > 0) {
        ancestors = [...ancestors, parent];
      }
      parent = node;
      current = node.eq[seg];
    }
  }
  return { target: current, ancestors };
}

function resolveRef(
  link: MOTLYRef,
  root: MOTLYNode,
  ancestors: MOTLYNode[],
  visiting: Set<MOTLYPropertyValue>,
  env: Record<string, string | undefined> | undefined,
): unknown {
  const refDisplay = formatRef(link);

  let start: MOTLYNode;
  let startAncestors: MOTLYNode[];
  if (link.linkUps === 0) {
    start = root;
    startAncestors = [];
  } else {
    const idx = ancestors.length - link.linkUps;
    if (idx < 0 || idx >= ancestors.length) {
      throw new Error(`Unresolved reference: ${refDisplay} (goes ${link.linkUps} level(s) up but only ${ancestors.length} ancestor(s) available)`);
    }
    start = ancestors[idx];
    startAncestors = ancestors.slice(0, idx);
  }

  const nav = navigateSegments(start, link.linkTo, startAncestors);
  if (nav === undefined) {
    throw new Error(`Unresolved reference: ${refDisplay}`);
  }

  // Cycle detection by object identity
  if (visiting.has(nav.target)) {
    throw new Error(`Circular reference detected: ${refDisplay}`);
  }
  visiting.add(nav.target);
  try {
    if (isRef(nav.target)) {
      // Chained ref — resolve from the target's position
      return resolveRef(nav.target, root, nav.ancestors, visiting, env);
    }
    const node = nav.target as MOTLYNode;
    if (node.deleted) {
      throw new Error(`Unresolved reference: ${refDisplay} (target is deleted)`);
    }
    return resolveNode(node, root, nav.ancestors, visiting, env);
  } finally {
    visiting.delete(nav.target);
  }
}
