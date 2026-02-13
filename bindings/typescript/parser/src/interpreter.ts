import {
  Statement,
  ScalarValue,
  TagValue,
  ArrayElement,
  RefPathSegment,
} from "./ast";
import { MOTLYValue, MOTLYNode, MOTLYRef } from "motly-ts-interface";

/** Execute a list of parsed statements against an existing MOTLYValue. */
export function execute(statements: Statement[], root: MOTLYValue): MOTLYValue {
  for (const stmt of statements) {
    executeStatement(stmt, root);
  }
  return root;
}

function executeStatement(stmt: Statement, node: MOTLYValue): void {
  switch (stmt.kind) {
    case "setEq":
      executeSetEq(
        node,
        stmt.path,
        stmt.value,
        stmt.properties,
        stmt.preserveProperties
      );
      break;
    case "replaceProperties":
      executeReplaceProperties(
        node,
        stmt.path,
        stmt.properties,
        stmt.preserveValue
      );
      break;
    case "updateProperties":
      executeUpdateProperties(node, stmt.path, stmt.properties);
      break;
    case "define":
      executeDefine(node, stmt.path, stmt.deleted);
      break;
    case "clearAll":
      node.properties = {};
      break;
  }
}

function executeSetEq(
  node: MOTLYValue,
  path: string[],
  value: TagValue,
  properties: Statement[] | null,
  preserveProperties: boolean
): void {
  // Reference without properties → produce a link
  if (
    value.kind === "scalar" &&
    value.value.kind === "reference" &&
    properties === null &&
    !preserveProperties
  ) {
    const [writeKey, parent] = buildAccessPath(node, path);
    const props = getOrCreateProperties(parent);
    props[writeKey] = {
      linkTo: formatRefString(value.value.ups, value.value.path),
    };

    return;
  }

  const [writeKey, parent] = buildAccessPath(node, path);

  if (properties !== null) {
    // name = value { new_properties } - set value and replace properties
    const result = createValueNode(value);
    for (const s of properties) {
      executeStatement(s, result);
    }
    const props = getOrCreateProperties(parent);
    props[writeKey] = result;

  } else if (preserveProperties) {
    // name = value { ... } - update value, preserve existing properties
    const props = getOrCreateProperties(parent);
    const existing = props[writeKey];

    if (existing !== undefined && !isRef(existing)) {
      const result = createValueNode(value);
      if (existing.properties) {
        result.properties = existing.properties;
      }
      props[writeKey] = result;
    } else {
      props[writeKey] = createValueNode(value);
    }

  } else {
    // name = value - simple assignment
    const props = getOrCreateProperties(parent);
    props[writeKey] = createValueNode(value);

  }
}

function executeReplaceProperties(
  node: MOTLYValue,
  path: string[],
  properties: Statement[],
  preserveValue: boolean
): void {
  const [writeKey, parent] = buildAccessPath(node, path);

  const result: MOTLYValue = {};

  if (preserveValue) {
    const props = getOrCreateProperties(parent);
    const existing = props[writeKey];
    if (existing !== undefined && !isRef(existing)) {
      result.eq = existing.eq;
    }
  }

  for (const stmt of properties) {
    executeStatement(stmt, result);
  }

  const props = getOrCreateProperties(parent);
  props[writeKey] = result;
}

function executeUpdateProperties(
  node: MOTLYValue,
  path: string[],
  properties: Statement[]
): void {
  const [writeKey, parent] = buildAccessPath(node, path);

  const props = getOrCreateProperties(parent);
  let target = props[writeKey];

  if (target === undefined) {
    target = {};
    props[writeKey] = target;

  }

  if (isRef(target)) {
    const newNode: MOTLYValue = {};
    for (const stmt of properties) {
      executeStatement(stmt, newNode);
    }
    props[writeKey] = newNode;

  } else {
    for (const stmt of properties) {
      executeStatement(stmt, target);
    }
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
    props[writeKey] = {};
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

    if (isRef(entry)) {
      entry = {};
      props[segment] = entry;
    }

    current = entry as MOTLYValue;
  }

  return [path[path.length - 1], current];
}

/** Convert an AST TagValue to a MOTLYValue. */
function createValueNode(value: TagValue): MOTLYValue {
  if (value.kind === "array") {
    return { eq: resolveArray(value.elements) };
  }
  const sv = value.value;
  switch (sv.kind) {
    case "string":
      return { eq: sv.value };
    case "number":
      return { eq: sv.value };
    case "boolean":
      return { eq: sv.value };
    case "date":
      return { eq: new Date(sv.value) };
    case "reference":
      // Should not be reached for simple ref assignments (handled above).
      return {};
  }
}

/** Resolve an array of AST elements to MOTLYNodes. */
function resolveArray(elements: ArrayElement[]): MOTLYNode[] {
  return elements.map(resolveArrayElement);
}

function resolveArrayElement(el: ArrayElement): MOTLYNode {
  // Reference without properties becomes a link
  if (
    el.value !== null &&
    el.value.kind === "scalar" &&
    el.value.value.kind === "reference" &&
    el.properties === null
  ) {
    return {
      linkTo: formatRefString(el.value.value.ups, el.value.value.path),
    };
  }

  const node: MOTLYValue = {};

  if (el.value !== null) {
    if (el.value.kind === "array") {
      node.eq = resolveArray(el.value.elements);
    } else {
      const sv = el.value.value;
      switch (sv.kind) {
        case "string":
          node.eq = sv.value;
          break;
        case "number":
          node.eq = sv.value;
          break;
        case "boolean":
          node.eq = sv.value;
          break;
        case "date":
          node.eq = new Date(sv.value);
          break;
        case "reference":
          // Reference with properties: ignore the reference value
          break;
      }
    }
  }

  if (el.properties !== null) {
    for (const stmt of el.properties) {
      executeStatement(stmt, node);
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

/** Check if a MOTLYNode is a MOTLYRef. */
function isRef(node: MOTLYNode): node is MOTLYRef {
  return "linkTo" in node;
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

