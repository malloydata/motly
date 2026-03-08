import {
  Statement,
  TagValue,
  ArrayElement,
  RefPathSegment,
  Span,
} from "./ast";
import { MOTLYNode, MOTLYDataNode, MOTLYRef, MOTLYError, MOTLYLocation, isRef, formatRef } from "../../interface/src/types";
import { cloneNode } from "./clone";

/** Execute a list of parsed statements against an existing MOTLYDataNode. */
export function execute(statements: Statement[], root: MOTLYDataNode, parseId: number): MOTLYError[] {
  const errors: MOTLYError[] = [];
  for (const stmt of statements) {
    executeStatement(stmt, root, errors, parseId);
  }
  return errors;
}

function executeStatement(stmt: Statement, node: MOTLYDataNode, errors: MOTLYError[], parseId: number): void {
  switch (stmt.kind) {
    case "setEq":
      executeSetEq(node, stmt.path, stmt.value, stmt.properties, errors, parseId, stmt.span);
      break;
    case "assignBoth":
      executeAssignBoth(node, stmt.path, stmt.value, stmt.properties, errors, parseId, stmt.span);
      break;
    case "replaceProperties":
      executeReplaceProperties(node, stmt.path, stmt.properties, errors, parseId, stmt.span);
      break;
    case "updateProperties":
      executeUpdateProperties(node, stmt.path, stmt.properties, errors, parseId, stmt.span);
      break;
    case "define":
      executeDefine(node, stmt.path, stmt.deleted, parseId, stmt.span);
      break;
    case "clearAll":
      delete node.eq;
      node.properties = {};
      break;
  }
}

/** Build a MOTLYLocation from a parseId and span. */
function makeLocation(parseId: number, span: Span): MOTLYLocation {
  return { parseId, begin: span.begin, end: span.end };
}

/** Set location on a node only if it doesn't already have one (first-appearance rule). */
function setFirstLocation(node: MOTLYDataNode, parseId: number, span: Span): void {
  if (!node.location) {
    node.location = makeLocation(parseId, span);
  }
}

/**
 * `name = value` — set eq, preserve existing properties.
 * `name = value { props }` — set eq, then merge properties.
 *
 * Special case: `name = $ref` inserts a MOTLYRef directly.
 * `name = $ref { props }` produces a non-fatal error (ref created, props ignored).
 */
function executeSetEq(
  node: MOTLYDataNode,
  path: string[],
  value: TagValue,
  properties: Statement[] | null,
  errors: MOTLYError[],
  parseId: number,
  span: Span
): void {
  // Special case: reference value → insert as MOTLYRef
  if (value.kind === "scalar" && value.value.kind === "reference") {
    if (properties !== null) {
      const zero = { line: 0, column: 0, offset: 0 };
      errors.push({
        code: "ref-with-properties",
        message: "Cannot add properties to a reference. Did you mean := (clone)?",
        begin: zero,
        end: zero,
      });
    }
    const [writeKey, parent] = buildAccessPath(node, path, parseId, span);
    getOrCreateProperties(parent)[writeKey] = makeRef(value.value.ups, value.value.path);
    return;
  }

  const [writeKey, parent] = buildAccessPath(node, path, parseId, span);
  const props = getOrCreateProperties(parent);

  // Get or create target (preserves existing node and its properties)
  let targetPv = props[writeKey];
  if (targetPv === undefined) {
    targetPv = {};
    props[writeKey] = targetPv;
  }

  // If it was a ref, convert to empty node
  const target = ensureDataNode(props, writeKey);

  // Set location on first appearance
  setFirstLocation(target, parseId, span);

  // Set the value slot
  setEqSlot(target, value, parseId);

  // If properties block present, MERGE them
  if (properties !== null) {
    for (const s of properties) {
      executeStatement(s, target, errors, parseId);
    }
  }
}

/**
 * `name := value` — assign value + clear properties.
 * `name := value { props }` — assign value + replace properties.
 * `name := $ref` — clone the referenced subtree.
 * `name := $ref { props }` — clone + replace properties.
 */
function executeAssignBoth(
  node: MOTLYDataNode,
  path: string[],
  value: TagValue,
  properties: Statement[] | null,
  errors: MOTLYError[],
  parseId: number,
  span: Span
): void {
  if (
    value.kind === "scalar" &&
    value.value.kind === "reference"
  ) {
    // CLONE semantics: resolve + deep copy the target
    let cloned: MOTLYDataNode;
    try {
      cloned = resolveAndClone(
        node,
        path,
        value.value.ups,
        value.value.path
      );
    } catch (err) {
      if (err && typeof err === "object" && "code" in err) {
        errors.push(err as MOTLYError);
      }
      return;
    }
    // Check for relative references that escape the clone boundary
    sanitizeClonedRefs(cloned, 0, errors);
    if (properties !== null) {
      cloned.properties = {};
      for (const s of properties) {
        executeStatement(s, cloned, errors, parseId);
      }
    }
    // := always sets a new location (it's a full replacement)
    cloned.location = makeLocation(parseId, span);
    const [writeKey, parent] = buildAccessPath(node, path, parseId, span);
    getOrCreateProperties(parent)[writeKey] = cloned;
  } else {
    // Literal value: create fresh node (replaces everything)
    const result: MOTLYDataNode = {};
    // := always sets a new location
    result.location = makeLocation(parseId, span);
    setEqSlot(result, value, parseId);
    if (properties !== null) {
      for (const s of properties) {
        executeStatement(s, result, errors, parseId);
      }
    }
    const [writeKey, parent] = buildAccessPath(node, path, parseId, span);
    getOrCreateProperties(parent)[writeKey] = result;
  }
}

/**
 * `name: { props }` — preserve existing value, replace properties.
 */
function executeReplaceProperties(
  node: MOTLYDataNode,
  path: string[],
  properties: Statement[],
  errors: MOTLYError[],
  parseId: number,
  span: Span
): void {
  const [writeKey, parent] = buildAccessPath(node, path, parseId, span);

  const result: MOTLYDataNode = {};

  // Always preserve the existing value (if it's a node, not a ref)
  const parentProps = getOrCreateProperties(parent);
  const existing = parentProps[writeKey];
  if (existing !== undefined && !isRef(existing)) {
    result.eq = existing.eq;
    // Preserve the existing location (first-appearance rule)
    if (existing.location) {
      result.location = existing.location;
    }
  }

  // If no existing location, this is the first appearance
  if (!result.location) {
    result.location = makeLocation(parseId, span);
  }

  for (const stmt of properties) {
    executeStatement(stmt, result, errors, parseId);
  }

  parentProps[writeKey] = result;
}

function executeUpdateProperties(
  node: MOTLYDataNode,
  path: string[],
  properties: Statement[],
  errors: MOTLYError[],
  parseId: number,
  span: Span
): void {
  const [writeKey, parent] = buildAccessPath(node, path, parseId, span);

  const props = getOrCreateProperties(parent);

  // Get or create the target node (merging semantics - preserves existing)
  if (props[writeKey] === undefined) {
    props[writeKey] = {};
  }

  const target = ensureDataNode(props, writeKey);

  // Set location on first appearance
  setFirstLocation(target, parseId, span);

  for (const stmt of properties) {
    executeStatement(stmt, target, errors, parseId);
  }
}

function executeDefine(
  node: MOTLYDataNode,
  path: string[],
  deleted: boolean,
  parseId: number,
  span: Span
): void {
  const [writeKey, parent] = buildAccessPath(node, path, parseId, span);
  const props = getOrCreateProperties(parent);
  if (deleted) {
    const delNode: MOTLYDataNode = { deleted: true };
    delNode.location = makeLocation(parseId, span);
    props[writeKey] = delNode;
  } else {
    // Get-or-create: if node already exists, leave it alone
    if (props[writeKey] === undefined) {
      const newNode: MOTLYDataNode = {};
      newNode.location = makeLocation(parseId, span);
      props[writeKey] = newNode;
    }
  }
}

/** Navigate to the parent of the final path segment, creating intermediate nodes. */
function buildAccessPath(
  node: MOTLYDataNode,
  path: string[],
  parseId: number,
  span: Span
): [string, MOTLYDataNode] {
  let current = node;

  for (let i = 0; i < path.length - 1; i++) {
    const segment = path[i];
    const props = getOrCreateProperties(current);

    if (props[segment] === undefined) {
      const intermediate: MOTLYDataNode = {};
      intermediate.location = makeLocation(parseId, span);
      props[segment] = intermediate;
    }

    current = ensureDataNode(props, segment);
    // Set location on intermediate nodes (first-appearance)
    setFirstLocation(current, parseId, span);
  }

  return [path[path.length - 1], current];
}

/** Set the eq slot on a target node from a TagValue. */
function setEqSlot(target: MOTLYDataNode, value: TagValue, parseId: number): void {
  if (value.kind === "array") {
    target.eq = resolveArray(value.elements, [], parseId);
  } else {
    const sv = value.value;
    switch (sv.kind) {
      case "string":
        target.eq = sv.value;
        break;
      case "number":
        target.eq = sv.value;
        break;
      case "boolean":
        target.eq = sv.value;
        break;
      case "date":
        target.eq = new Date(sv.value);
        break;
      case "reference":
        // References are handled by the caller — should not reach here
        throw new Error("References should be handled before calling setEqSlot");
      case "env":
        target.eq = { env: sv.name };
        break;
      case "none":
        delete target.eq;
        break;
    }
  }
}

/** Resolve an array of AST elements to MOTLYNodes. */
function resolveArray(elements: ArrayElement[], errors: MOTLYError[], parseId: number): MOTLYNode[] {
  return elements.map((el) => resolveArrayElement(el, errors, parseId));
}

function resolveArrayElement(el: ArrayElement, errors: MOTLYError[], parseId: number): MOTLYNode {
  // Check if the element value is a reference → becomes MOTLYRef
  if (el.value !== null && el.value.kind === "scalar" && el.value.value.kind === "reference") {
    if (el.properties !== null) {
      const zero = { line: 0, column: 0, offset: 0 };
      errors.push({
        code: "ref-with-properties",
        message: "Cannot add properties to a reference. Did you mean := (clone)?",
        begin: zero,
        end: zero,
      });
    }
    return makeRef(el.value.value.ups, el.value.value.path);
  }

  const node: MOTLYDataNode = {};
  node.location = makeLocation(parseId, el.span);

  if (el.value !== null) {
    setEqSlot(node, el.value, parseId);
  }

  if (el.properties !== null) {
    for (const stmt of el.properties) {
      executeStatement(stmt, node, errors, parseId);
    }
  }

  return node;
}

/** Build a structured MOTLYRef from parsed AST reference data. */
function makeRef(ups: number, path: RefPathSegment[]): MOTLYRef {
  return {
    linkTo: path.map((seg) => seg.kind === "name" ? seg.name : seg.index),
    linkUps: ups,
  };
}

/** Format ups + AST path for error messages (used before ref is constructed). */
function formatRefPath(ups: number, path: RefPathSegment[]): string {
  let s = "$";
  for (let i = 0; i < ups; i++) s += "^";
  let first = true;
  for (const seg of path) {
    if (seg.kind === "name") {
      if (!first) s += ".";
      s += seg.name;
      first = false;
    } else {
      s += `[${seg.index}]`;
    }
  }
  return s;
}

/** Resolve a reference path in the tree and return a deep clone. */
function resolveAndClone(
  root: MOTLYDataNode,
  stmtPath: string[],
  ups: number,
  refPath: RefPathSegment[]
): MOTLYDataNode {
  const refStr = formatRefPath(ups, refPath);
  let start: MOTLYDataNode;

  if (ups === 0) {
    // Absolute reference: start at root
    start = root;
  } else {
    const contextLen = stmtPath.length - 1 - ups;
    if (contextLen < 0) {
      throw cloneError(`Clone reference ${refStr} goes ${ups} level(s) up but only ${stmtPath.length - 1} ancestor(s) available`);
    }
    start = root;
    for (let i = 0; i < contextLen; i++) {
      if (!start.properties) {
        throw cloneError(`Clone reference ${refStr} could not be resolved: path segment "${stmtPath[i]}" not found`);
      }
      const pv = start.properties[stmtPath[i]];
      if (pv === undefined) {
        throw cloneError(`Clone reference ${refStr} could not be resolved: path segment "${stmtPath[i]}" not found`);
      }
      if (isRef(pv)) {
        throw cloneError(`Clone reference ${refStr} could not be resolved: path segment "${stmtPath[i]}" is a link reference`);
      }
      start = pv;
    }
  }

  // Follow refPath segments
  let current: MOTLYDataNode = start;
  for (const seg of refPath) {
    if (seg.kind === "name") {
      if (!current.properties) {
        throw cloneError(`Clone reference ${refStr} could not be resolved: property "${seg.name}" not found`);
      }
      const pv = current.properties[seg.name];
      if (pv === undefined) {
        throw cloneError(`Clone reference ${refStr} could not be resolved: property "${seg.name}" not found`);
      }
      if (isRef(pv)) {
        throw cloneError(`Clone reference ${refStr} could not be resolved: property "${seg.name}" is a link reference`);
      }
      current = pv;
    } else {
      if (!current.eq || !Array.isArray(current.eq)) {
        throw cloneError(`Clone reference ${refStr} could not be resolved: index [${seg.index}] used on non-array`);
      }
      if (seg.index >= current.eq.length) {
        throw cloneError(`Clone reference ${refStr} could not be resolved: index [${seg.index}] out of bounds (array length ${current.eq.length})`);
      }
      const elemPv = current.eq[seg.index];
      if (isRef(elemPv)) {
        throw cloneError(`Clone reference ${refStr} could not be resolved: index [${seg.index}] is a link reference`);
      }
      current = elemPv;
    }
  }

  return cloneNode(current);
}

function cloneError(message: string): MOTLYError {
  const zero = { line: 0, column: 0, offset: 0 };
  return { code: "unresolved-clone-reference", message, begin: zero, end: zero };
}

/**
 * Walk a cloned subtree and null out any relative (^) references that
 * escape the clone boundary. A reference at depth D with N ups escapes
 * if N > D. Absolute references (ups=0) are left alone.
 */
function sanitizeClonedRefs(
  node: MOTLYDataNode,
  depth: number,
  errors: MOTLYError[]
): void {
  // Check array elements
  if (node.eq !== undefined && Array.isArray(node.eq)) {
    for (let i = 0; i < node.eq.length; i++) {
      sanitizeClonedPv(node.eq, i, depth + 1, errors);
    }
  }

  // Check properties
  if (node.properties) {
    for (const key of Object.keys(node.properties)) {
      sanitizeClonedPvInProps(node.properties, key, depth + 1, errors);
    }
  }
}

/** Sanitize a single node within a cloned subtree (in an array context). */
function sanitizeClonedPv(
  arr: MOTLYNode[],
  index: number,
  depth: number,
  errors: MOTLYError[]
): void {
  const pv = arr[index];
  if (isRef(pv)) {
    if (pv.linkUps > 0 && pv.linkUps > depth) {
      const zero = { line: 0, column: 0, offset: 0 };
      errors.push({
        code: "clone-reference-out-of-scope",
        message: `Cloned reference "${formatRef(pv)}" escapes the clone boundary (${pv.linkUps} level(s) up from depth ${depth})`,
        begin: zero,
        end: zero,
      });
      // Convert to empty node
      arr[index] = {};
    }
  } else {
    sanitizeClonedRefs(pv, depth, errors);
  }
}

/** Sanitize a single node within a cloned subtree (in a properties context). */
function sanitizeClonedPvInProps(
  props: Record<string, MOTLYNode>,
  key: string,
  depth: number,
  errors: MOTLYError[]
): void {
  const pv = props[key];
  if (isRef(pv)) {
    if (pv.linkUps > 0 && pv.linkUps > depth) {
      const zero = { line: 0, column: 0, offset: 0 };
      errors.push({
        code: "clone-reference-out-of-scope",
        message: `Cloned reference "${formatRef(pv)}" escapes the clone boundary (${pv.linkUps} level(s) up from depth ${depth})`,
        begin: zero,
        end: zero,
      });
      // Convert to empty node
      props[key] = {};
    }
  } else {
    sanitizeClonedRefs(pv, depth, errors);
  }
}

/** Get or create the properties object on a MOTLYDataNode. */
function getOrCreateProperties(
  node: MOTLYDataNode
): Record<string, MOTLYNode> {
  if (!node.properties) {
    node.properties = {};
  }
  return node.properties;
}

/**
 * Ensure the node at props[key] is a MOTLYDataNode (not a MOTLYRef).
 * If it's a ref, replace it with an empty node.
 * Returns a mutable reference to the data node.
 */
function ensureDataNode(
  props: Record<string, MOTLYNode>,
  key: string
): MOTLYDataNode {
  const pv = props[key];
  if (isRef(pv)) {
    const node: MOTLYDataNode = {};
    props[key] = node;
    return node;
  }
  return pv;
}
