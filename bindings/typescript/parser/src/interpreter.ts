import {
  Statement,
  TagValue,
  ArrayElement,
  RefPathSegment,
} from "./ast";
import { MOTLYNode, MOTLYPropertyValue, MOTLYError, isRef } from "../../interface/src/types";
import { cloneNode } from "./clone";

/** Execute a list of parsed statements against an existing MOTLYNode. */
export function execute(statements: Statement[], root: MOTLYNode): MOTLYError[] {
  const errors: MOTLYError[] = [];
  for (const stmt of statements) {
    executeStatement(stmt, root, errors);
  }
  return errors;
}

function executeStatement(stmt: Statement, node: MOTLYNode, errors: MOTLYError[]): void {
  switch (stmt.kind) {
    case "setEq":
      executeSetEq(node, stmt.path, stmt.value, stmt.properties, errors);
      break;
    case "assignBoth":
      executeAssignBoth(node, stmt.path, stmt.value, stmt.properties, errors);
      break;
    case "replaceProperties":
      executeReplaceProperties(node, stmt.path, stmt.properties, errors);
      break;
    case "updateProperties":
      executeUpdateProperties(node, stmt.path, stmt.properties, errors);
      break;
    case "define":
      executeDefine(node, stmt.path, stmt.deleted);
      break;
    case "clearAll":
      delete node.eq;
      node.properties = {};
      break;
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
  node: MOTLYNode,
  path: string[],
  value: TagValue,
  properties: Statement[] | null,
  errors: MOTLYError[]
): void {
  // Special case: reference value → insert as MOTLYRef
  if (value.kind === "scalar" && value.value.kind === "reference") {
    const refStr = formatRefString(value.value.ups, value.value.path);
    if (properties !== null) {
      const zero = { line: 0, column: 0, offset: 0 };
      errors.push({
        code: "ref-with-properties",
        message: "Cannot add properties to a reference. Did you mean := (clone)?",
        begin: zero,
        end: zero,
      });
    }
    const [writeKey, parent] = buildAccessPath(node, path);
    getOrCreateProperties(parent)[writeKey] = { linkTo: refStr };
    return;
  }

  const [writeKey, parent] = buildAccessPath(node, path);
  const props = getOrCreateProperties(parent);

  // Get or create target (preserves existing node and its properties)
  let targetPv = props[writeKey];
  if (targetPv === undefined) {
    targetPv = {};
    props[writeKey] = targetPv;
  }

  // If it was a ref, convert to empty node
  const target = ensureNode(props, writeKey);

  // Set the value slot
  setEqSlot(target, value);

  // If properties block present, MERGE them
  if (properties !== null) {
    for (const s of properties) {
      executeStatement(s, target, errors);
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
  node: MOTLYNode,
  path: string[],
  value: TagValue,
  properties: Statement[] | null,
  errors: MOTLYError[]
): void {
  if (
    value.kind === "scalar" &&
    value.value.kind === "reference"
  ) {
    // CLONE semantics: resolve + deep copy the target
    let cloned: MOTLYNode;
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
        executeStatement(s, cloned, errors);
      }
    }
    const [writeKey, parent] = buildAccessPath(node, path);
    getOrCreateProperties(parent)[writeKey] = cloned;
  } else {
    // Literal value: create fresh node (replaces everything)
    const result: MOTLYNode = {};
    setEqSlot(result, value);
    if (properties !== null) {
      for (const s of properties) {
        executeStatement(s, result, errors);
      }
    }
    const [writeKey, parent] = buildAccessPath(node, path);
    getOrCreateProperties(parent)[writeKey] = result;
  }
}

/**
 * `name: { props }` — preserve existing value, replace properties.
 */
function executeReplaceProperties(
  node: MOTLYNode,
  path: string[],
  properties: Statement[],
  errors: MOTLYError[]
): void {
  const [writeKey, parent] = buildAccessPath(node, path);

  const result: MOTLYNode = {};

  // Always preserve the existing value (if it's a node, not a ref)
  const parentProps = getOrCreateProperties(parent);
  const existing = parentProps[writeKey];
  if (existing !== undefined && !isRef(existing)) {
    result.eq = existing.eq;
  }

  for (const stmt of properties) {
    executeStatement(stmt, result, errors);
  }

  parentProps[writeKey] = result;
}

function executeUpdateProperties(
  node: MOTLYNode,
  path: string[],
  properties: Statement[],
  errors: MOTLYError[]
): void {
  const [writeKey, parent] = buildAccessPath(node, path);

  const props = getOrCreateProperties(parent);

  // Get or create the target node (merging semantics - preserves existing)
  if (props[writeKey] === undefined) {
    props[writeKey] = {};
  }

  const target = ensureNode(props, writeKey);

  for (const stmt of properties) {
    executeStatement(stmt, target, errors);
  }
}

function executeDefine(
  node: MOTLYNode,
  path: string[],
  deleted: boolean
): void {
  const [writeKey, parent] = buildAccessPath(node, path);
  const props = getOrCreateProperties(parent);
  if (deleted) {
    props[writeKey] = { deleted: true };
  } else {
    // Get-or-create: if node already exists, leave it alone
    if (props[writeKey] === undefined) {
      props[writeKey] = {};
    }
  }
}

/** Navigate to the parent of the final path segment, creating intermediate nodes. */
function buildAccessPath(
  node: MOTLYNode,
  path: string[]
): [string, MOTLYNode] {
  let current = node;

  for (let i = 0; i < path.length - 1; i++) {
    const segment = path[i];
    const props = getOrCreateProperties(current);

    if (props[segment] === undefined) {
      props[segment] = {};
    }

    current = ensureNode(props, segment);
  }

  return [path[path.length - 1], current];
}

/** Set the eq slot on a target node from a TagValue. */
function setEqSlot(target: MOTLYNode, value: TagValue): void {
  if (value.kind === "array") {
    target.eq = resolveArray(value.elements, []);
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

/** Resolve an array of AST elements to MOTLYPropertyValues. */
function resolveArray(elements: ArrayElement[], errors: MOTLYError[]): MOTLYPropertyValue[] {
  return elements.map((el) => resolveArrayElement(el, errors));
}

function resolveArrayElement(el: ArrayElement, errors: MOTLYError[]): MOTLYPropertyValue {
  // Check if the element value is a reference → becomes MOTLYRef
  if (el.value !== null && el.value.kind === "scalar" && el.value.value.kind === "reference") {
    const refStr = formatRefString(el.value.value.ups, el.value.value.path);
    if (el.properties !== null) {
      const zero = { line: 0, column: 0, offset: 0 };
      errors.push({
        code: "ref-with-properties",
        message: "Cannot add properties to a reference. Did you mean := (clone)?",
        begin: zero,
        end: zero,
      });
    }
    return { linkTo: refStr };
  }

  const node: MOTLYNode = {};

  if (el.value !== null) {
    setEqSlot(node, el.value);
  }

  if (el.properties !== null) {
    for (const stmt of el.properties) {
      executeStatement(stmt, node, errors);
    }
  }

  return node;
}

/** Format a reference path back to its string form: `$^^name[0].sub` */
function formatRefString(ups: number, path: RefPathSegment[]): string {
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
  root: MOTLYNode,
  stmtPath: string[],
  ups: number,
  refPath: RefPathSegment[]
): MOTLYNode {
  const refStr = formatRefString(ups, refPath);
  let start: MOTLYNode;

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
  let current: MOTLYNode = start;
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
  node: MOTLYNode,
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

/** Sanitize a single property value within a cloned subtree (in an array context). */
function sanitizeClonedPv(
  arr: MOTLYPropertyValue[],
  index: number,
  depth: number,
  errors: MOTLYError[]
): void {
  const pv = arr[index];
  if (isRef(pv)) {
    const parsed = parseRefUps(pv.linkTo);
    if (parsed.ups > 0 && parsed.ups > depth) {
      const zero = { line: 0, column: 0, offset: 0 };
      errors.push({
        code: "clone-reference-out-of-scope",
        message: `Cloned reference "${pv.linkTo}" escapes the clone boundary (${parsed.ups} level(s) up from depth ${depth})`,
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

/** Sanitize a single property value within a cloned subtree (in a properties context). */
function sanitizeClonedPvInProps(
  props: Record<string, MOTLYPropertyValue>,
  key: string,
  depth: number,
  errors: MOTLYError[]
): void {
  const pv = props[key];
  if (isRef(pv)) {
    const parsed = parseRefUps(pv.linkTo);
    if (parsed.ups > 0 && parsed.ups > depth) {
      const zero = { line: 0, column: 0, offset: 0 };
      errors.push({
        code: "clone-reference-out-of-scope",
        message: `Cloned reference "${pv.linkTo}" escapes the clone boundary (${parsed.ups} level(s) up from depth ${depth})`,
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

/** Extract the ups count from a linkTo string like "$^^name". */
function parseRefUps(linkTo: string): { ups: number } {
  let i = 0;
  if (i < linkTo.length && linkTo[i] === "$") i++;
  let ups = 0;
  while (i < linkTo.length && linkTo[i] === "^") {
    ups++;
    i++;
  }
  return { ups };
}

/** Get or create the properties object on a MOTLYNode. */
function getOrCreateProperties(
  node: MOTLYNode
): Record<string, MOTLYPropertyValue> {
  if (!node.properties) {
    node.properties = {};
  }
  return node.properties;
}

/**
 * Ensure the property value at props[key] is a MOTLYNode (not a MOTLYRef).
 * If it's a ref, replace it with an empty node.
 * Returns a mutable reference to the node.
 */
function ensureNode(
  props: Record<string, MOTLYPropertyValue>,
  key: string
): MOTLYNode {
  const pv = props[key];
  if (isRef(pv)) {
    const node: MOTLYNode = {};
    props[key] = node;
    return node;
  }
  return pv as MOTLYNode;
}
