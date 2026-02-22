import {
  Statement,
  TagValue,
  ArrayElement,
  RefPathSegment,
} from "./ast";
import { MOTLYValue, MOTLYNode, MOTLYError, isRef } from "motly-ts-interface";
import { cloneValue } from "./clone";

/** Execute a list of parsed statements against an existing MOTLYValue. */
export function execute(statements: Statement[], root: MOTLYValue): MOTLYError[] {
  const errors: MOTLYError[] = [];
  for (const stmt of statements) {
    executeStatement(stmt, root, errors);
  }
  return errors;
}

function executeStatement(stmt: Statement, node: MOTLYValue, errors: MOTLYError[]): void {
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
 */
function executeSetEq(
  node: MOTLYValue,
  path: string[],
  value: TagValue,
  properties: Statement[] | null,
  errors: MOTLYError[]
): void {
  const [writeKey, parent] = buildAccessPath(node, path);
  const props = getOrCreateProperties(parent);

  // Get or create target (preserves existing node and its properties)
  let target = props[writeKey];
  if (target === undefined) {
    target = {};
    props[writeKey] = target;
  }

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
  node: MOTLYValue,
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
    let cloned: MOTLYValue;
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
    const result: MOTLYValue = {};
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
  node: MOTLYValue,
  path: string[],
  properties: Statement[],
  errors: MOTLYError[]
): void {
  const [writeKey, parent] = buildAccessPath(node, path);

  const result: MOTLYValue = {};

  // Always preserve the existing value
  const parentProps = getOrCreateProperties(parent);
  const existing = parentProps[writeKey];
  if (existing !== undefined) {
    result.eq = existing.eq;
  }

  for (const stmt of properties) {
    executeStatement(stmt, result, errors);
  }

  parentProps[writeKey] = result;
}

function executeUpdateProperties(
  node: MOTLYValue,
  path: string[],
  properties: Statement[],
  errors: MOTLYError[]
): void {
  const [writeKey, parent] = buildAccessPath(node, path);

  const props = getOrCreateProperties(parent);
  let target = props[writeKey];

  if (target === undefined) {
    target = {};
    props[writeKey] = target;
  }

  for (const stmt of properties) {
    executeStatement(stmt, target, errors);
  }
}

function executeDefine(
  node: MOTLYValue,
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
  node: MOTLYValue,
  path: string[]
): [string, MOTLYValue] {
  let current = node;

  for (let i = 0; i < path.length - 1; i++) {
    const segment = path[i];
    const props = getOrCreateProperties(current);

    let entry = props[segment];
    if (entry === undefined) {
      entry = {};
      props[segment] = entry;
    }

    current = entry;
  }

  return [path[path.length - 1], current];
}

/** Set the eq slot on a target node from a TagValue. */
function setEqSlot(target: MOTLYValue, value: TagValue, errors?: MOTLYError[]): void {
  if (value.kind === "array") {
    target.eq = resolveArray(value.elements, errors ?? []);
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
        target.eq = { linkTo: formatRefString(sv.ups, sv.path) };
        break;
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
function resolveArray(elements: ArrayElement[], errors: MOTLYError[]): MOTLYNode[] {
  return elements.map((el) => resolveArrayElement(el, errors));
}

function resolveArrayElement(el: ArrayElement, errors: MOTLYError[]): MOTLYNode {
  const node: MOTLYValue = {};

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
  root: MOTLYValue,
  stmtPath: string[],
  ups: number,
  refPath: RefPathSegment[]
): MOTLYValue {
  const refStr = formatRefString(ups, refPath);
  let start: MOTLYValue;

  if (ups === 0) {
    // Absolute reference: start at root
    start = root;
  } else {
    // Relative reference: go up from the current context.
    // stmtPath is the full write path (including the key being assigned to).
    // Current context = parent of write target = stmtPath[0..len-2].
    // Going up `ups` levels: stmtPath[0..len-2-ups].
    const contextLen = stmtPath.length - 1 - ups;
    if (contextLen < 0) {
      throw cloneError(`Clone reference ${refStr} goes ${ups} level(s) up but only ${stmtPath.length - 1} ancestor(s) available`);
    }
    start = root;
    for (let i = 0; i < contextLen; i++) {
      if (!start.properties || !start.properties[stmtPath[i]]) {
        throw cloneError(`Clone reference ${refStr} could not be resolved: path segment "${stmtPath[i]}" not found`);
      }
      start = start.properties[stmtPath[i]];
    }
  }

  // Follow refPath segments
  let current: MOTLYValue = start;
  for (const seg of refPath) {
    if (seg.kind === "name") {
      if (!current.properties || !current.properties[seg.name]) {
        throw cloneError(`Clone reference ${refStr} could not be resolved: property "${seg.name}" not found`);
      }
      current = current.properties[seg.name];
    } else {
      if (!current.eq || !Array.isArray(current.eq)) {
        throw cloneError(`Clone reference ${refStr} could not be resolved: index [${seg.index}] used on non-array`);
      }
      if (seg.index >= current.eq.length) {
        throw cloneError(`Clone reference ${refStr} could not be resolved: index [${seg.index}] out of bounds (array length ${current.eq.length})`);
      }
      current = current.eq[seg.index];
    }
  }

  return cloneValue(current);
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
  node: MOTLYValue,
  depth: number,
  errors: MOTLYError[]
): void {
  if (isRef(node.eq)) {
    const parsed = parseRefUps(node.eq.linkTo);
    if (parsed.ups > 0 && parsed.ups > depth) {
      const zero = { line: 0, column: 0, offset: 0 };
      errors.push({
        code: "clone-reference-out-of-scope",
        message: `Cloned reference "${node.eq.linkTo}" escapes the clone boundary (${parsed.ups} level(s) up from depth ${depth})`,
        begin: zero,
        end: zero,
      });
      delete node.eq;
    }
  }

  if (node.eq !== undefined && Array.isArray(node.eq)) {
    for (const elem of node.eq) {
      sanitizeClonedRefs(elem, depth + 1, errors);
    }
  }

  if (node.properties) {
    for (const key of Object.keys(node.properties)) {
      sanitizeClonedRefs(node.properties[key], depth + 1, errors);
    }
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

/** Get or create the properties object on a MOTLYValue. */
function getOrCreateProperties(
  node: MOTLYValue
): Record<string, MOTLYNode> {
  if (!node.properties) {
    node.properties = {};
  }
  return node.properties;
}
