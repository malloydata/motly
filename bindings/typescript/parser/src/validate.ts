import {
  MOTLYValue,
  MOTLYNode,
  MOTLYRef,
  MOTLYSchemaError,
  MOTLYValidationError,
  isRef,
  isEnvRef,
} from "../../interface/src/types";

function getEqString(node: MOTLYValue): string | undefined {
  return typeof node.eq === "string" ? node.eq : undefined;
}

function valueEqString(node: MOTLYNode): string | undefined {
  if (isRef(node.eq) || isEnvRef(node.eq)) return undefined;
  return getEqString(node);
}

function extractSection(
  node: MOTLYValue,
  name: string
): Record<string, MOTLYNode> | undefined {
  if (!node.properties) return undefined;
  const section = node.properties[name];
  if (section === undefined || isRef(section.eq) || isEnvRef(section.eq)) return undefined;
  return section.properties;
}

// ── Reference Validation ────────────────────────────────────────

export function validateReferences(root: MOTLYValue): MOTLYValidationError[] {
  const errors: MOTLYValidationError[] = [];
  const path: string[] = [];
  const ancestors: MOTLYValue[] = [root];
  walkRefs(root, path, ancestors, root, errors);
  return errors;
}

function walkRefs(
  node: MOTLYValue,
  path: string[],
  ancestors: MOTLYValue[],
  root: MOTLYValue,
  errors: MOTLYValidationError[]
): void {
  // Check array elements in eq
  if (node.eq !== undefined && Array.isArray(node.eq)) {
    walkArrayRefs(node.eq, path, ancestors, node, root, errors);
  }

  if (node.properties) {
    for (const key of Object.keys(node.properties)) {
      const child = node.properties[key];
      path.push(key);

      // Check if child's eq is a reference (checked at property level
      // to maintain correct ancestor depth for reference resolution)
      if (isRef(child.eq)) {
        const errMsg = checkLink(child.eq, ancestors, root);
        if (errMsg !== null) {
          errors.push({
            message: errMsg,
            path: [...path],
            code: "unresolved-reference",
          });
        }
      }

      // Recurse into child
      ancestors.push(node);
      walkRefs(child, path, ancestors, root, errors);
      ancestors.pop();

      path.pop();
    }
  }
}

function walkArrayRefs(
  arr: MOTLYNode[],
  path: string[],
  ancestors: MOTLYValue[],
  parentNode: MOTLYValue,
  root: MOTLYValue,
  errors: MOTLYValidationError[]
): void {
  for (let i = 0; i < arr.length; i++) {
    const elem = arr[i];
    const idxKey = `[${i}]`;
    path.push(idxKey);

    // Check if element's eq is a reference
    if (isRef(elem.eq)) {
      const errMsg = checkLink(elem.eq, ancestors, root);
      if (errMsg !== null) {
        errors.push({
          message: errMsg,
          path: [...path],
          code: "unresolved-reference",
        });
      }
    }

    // Recurse into element
    ancestors.push(parentNode);
    walkRefs(elem, path, ancestors, root, errors);
    ancestors.pop();

    path.pop();
  }
}

function checkLink(
  link: MOTLYRef,
  ancestors: MOTLYValue[],
  root: MOTLYValue
): string | null {
  const { ups, segments, error } = parseLinkString(link.linkTo);
  if (error !== null) return error;

  let start: MOTLYValue;
  if (ups === 0) {
    start = root;
  } else {
    const idx = ancestors.length - ups;
    if (idx < 0 || idx >= ancestors.length) {
      return `Reference "${link.linkTo}" goes ${ups} level(s) up but only ${ancestors.length} ancestor(s) available`;
    }
    start = ancestors[idx];
  }

  return resolvePath(start, segments, link.linkTo);
}

type RefSeg = { kind: "name"; name: string } | { kind: "index"; index: number };

function parseLinkString(s: string): { ups: number; segments: RefSeg[]; error: string | null } {
  let i = 0;
  if (i < s.length && s[i] === "$") i++;

  let ups = 0;
  while (i < s.length && s[i] === "^") {
    ups++;
    i++;
  }

  const segments: RefSeg[] = [];
  let nameBuf = "";

  while (i < s.length) {
    const ch = s[i];
    if (ch === ".") {
      if (nameBuf.length > 0) {
        segments.push({ kind: "name", name: nameBuf });
        nameBuf = "";
      }
      i++;
    } else if (ch === "[") {
      if (nameBuf.length > 0) {
        segments.push({ kind: "name", name: nameBuf });
        nameBuf = "";
      }
      i++;
      let idxBuf = "";
      while (i < s.length && s[i] !== "]") {
        idxBuf += s[i];
        i++;
      }
      if (i < s.length) i++; // skip ']'
      const idx = parseInt(idxBuf, 10);
      if (isNaN(idx) || idx < 0) {
        return { ups, segments, error: `Reference "${s}" has invalid array index [${idxBuf}]` };
      }
      segments.push({ kind: "index", index: idx });
    } else {
      nameBuf += ch;
      i++;
    }
  }
  if (nameBuf.length > 0) {
    segments.push({ kind: "name", name: nameBuf });
  }

  return { ups, segments, error: null };
}

function resolvePath(
  start: MOTLYValue,
  segments: RefSeg[],
  linkStr: string
): string | null {
  let current: MOTLYValue = start;

  for (const seg of segments) {
    if (seg.kind === "name") {
      if (!current.properties) {
        return `Reference "${linkStr}" could not be resolved: property "${seg.name}" not found (node has no properties)`;
      }
      const child: MOTLYNode | undefined = current.properties[seg.name];
      if (child === undefined) {
        return `Reference "${linkStr}" could not be resolved: property "${seg.name}" not found`;
      }
      current = child;
    } else {
      if (current.eq === undefined || !Array.isArray(current.eq)) {
        return `Reference "${linkStr}" could not be resolved: index [${seg.index}] used on non-array`;
      }
      if (seg.index >= current.eq.length) {
        return `Reference "${linkStr}" could not be resolved: index [${seg.index}] out of bounds (array length ${current.eq.length})`;
      }
      current = current.eq[seg.index];
    }
  }

  return null;
}

// ── Schema Validation ───────────────────────────────────────────

export function validateSchema(
  tag: MOTLYValue,
  schema: MOTLYValue
): MOTLYSchemaError[] {
  const errors: MOTLYSchemaError[] = [];
  const types = extractSection(schema, "Types");
  validateNodeAgainstSchema(tag, schema, types, [], errors);
  return errors;
}

type AdditionalPolicy =
  | { kind: "reject" }
  | { kind: "allow" }
  | { kind: "validateAs"; typeName: string };

function getAdditionalPolicy(schema: MOTLYValue): AdditionalPolicy {
  if (!schema.properties) return { kind: "reject" };
  const additional = schema.properties["Additional"];
  if (additional === undefined) return { kind: "reject" };
  if (isRef(additional.eq)) return { kind: "reject" };
  const eqStr = getEqString(additional);
  if (eqStr !== undefined) {
    if (eqStr === "allow") return { kind: "allow" };
    if (eqStr === "reject") return { kind: "reject" };
    return { kind: "validateAs", typeName: eqStr };
  }
  return { kind: "allow" };
}

function validateNodeAgainstSchema(
  tag: MOTLYValue,
  schema: MOTLYValue,
  types: Record<string, MOTLYNode> | undefined,
  path: string[],
  errors: MOTLYSchemaError[]
): void {
  const required = extractSection(schema, "Required");
  const optional = extractSection(schema, "Optional");
  const additional = getAdditionalPolicy(schema);

  const tagProps = tag.properties;

  // Check required properties
  if (required) {
    for (const key of Object.keys(required)) {
      const propPath = [...path, key];
      const tagValue = tagProps ? tagProps[key] : undefined;
      if (tagValue === undefined) {
        errors.push({
          message: `Missing required property "${key}"`,
          path: propPath,
          code: "missing-required",
        });
      } else {
        validateValueType(tagValue, required[key], types, propPath, errors);
      }
    }
  }

  // Check optional properties that exist
  if (optional && tagProps) {
    for (const key of Object.keys(optional)) {
      const tagValue = tagProps[key];
      if (tagValue !== undefined) {
        validateValueType(
          tagValue,
          optional[key],
          types,
          [...path, key],
          errors
        );
      }
    }
  }

  // Check for unknown properties
  if (tagProps) {
    const knownKeys = new Set<string>();
    if (required) for (const k of Object.keys(required)) knownKeys.add(k);
    if (optional) for (const k of Object.keys(optional)) knownKeys.add(k);

    for (const key of Object.keys(tagProps)) {
      if (knownKeys.has(key)) continue;
      const propPath = [...path, key];
      switch (additional.kind) {
        case "reject":
          errors.push({
            message: `Unknown property "${key}"`,
            path: propPath,
            code: "unknown-property",
          });
          break;
        case "allow":
          break;
        case "validateAs": {
          const synthetic = makeTypeSpecNode(additional.typeName);
          validateValueType(tagProps[key], synthetic, types, propPath, errors);
          break;
        }
      }
    }
  }
}

function makeTypeSpecNode(typeName: string): MOTLYValue {
  return { eq: typeName };
}

function validateValueType(
  value: MOTLYNode,
  typeSpec: MOTLYNode,
  types: Record<string, MOTLYNode> | undefined,
  path: string[],
  errors: MOTLYSchemaError[]
): void {
  if (isRef(typeSpec.eq)) return;

  // Check for union type (oneOf)
  if (typeSpec.properties) {
    const oneOf = typeSpec.properties["oneOf"];
    if (oneOf !== undefined && !isRef(oneOf.eq)) {
      validateUnion(value, oneOf, types, path, errors);
      return;
    }
  }

  // Check for enum (eq) or pattern (matches)
  if (typeSpec.properties) {
    const eqProp = typeSpec.properties["eq"];
    if (eqProp !== undefined && !isRef(eqProp.eq)) {
      if (Array.isArray(eqProp.eq)) {
        validateEnum(value, eqProp.eq, path, errors);
        return;
      }
    }

    const matchesProp = typeSpec.properties["matches"];
    if (matchesProp !== undefined && !isRef(matchesProp.eq)) {
      const baseType = getEqString(typeSpec);
      if (baseType !== undefined) {
        validateBaseType(value, baseType, types, path, errors);
      }
      validatePattern(value, matchesProp, path, errors);
      return;
    }
  }

  // Get the type name from the spec's eq value
  const typeName = getEqString(typeSpec);
  if (typeName === undefined) {
    // Nested schema (has Required/Optional/Additional)
    if (
      typeSpec.properties &&
      ("Required" in typeSpec.properties ||
        "Optional" in typeSpec.properties ||
        "Additional" in typeSpec.properties)
    ) {
      if (isRef(value.eq)) {
        errors.push({
          message: "Expected a tag but found a link",
          path: [...path],
          code: "wrong-type",
        });
      } else {
        validateNodeAgainstSchema(value, typeSpec, types, path, errors);
      }
    }
    return;
  }

  validateBaseType(value, typeName, types, path, errors);
}

function validateBaseType(
  value: MOTLYNode,
  typeName: string,
  types: Record<string, MOTLYNode> | undefined,
  path: string[],
  errors: MOTLYSchemaError[]
): void {
  // Array types: "string[]", "number[]", etc.
  if (typeName.endsWith("[]")) {
    const innerType = typeName.slice(0, -2);
    validateArrayType(value, innerType, types, path, errors);
    return;
  }

  switch (typeName) {
    case "string":
      validateTypeString(value, path, errors);
      break;
    case "number":
      validateTypeNumber(value, path, errors);
      break;
    case "boolean":
      validateTypeBoolean(value, path, errors);
      break;
    case "date":
      validateTypeDate(value, path, errors);
      break;
    case "tag":
      validateTypeTag(value, path, errors);
      break;
    case "flag":
      validateTypeFlag(value, path, errors);
      break;
    case "any":
      break;
    default: {
      // Custom type
      if (types) {
        const typeDef = types[typeName];
        if (typeDef !== undefined) {
          validateValueType(value, typeDef, types, path, errors);
        } else {
          errors.push({
            message: `Unknown type "${typeName}" in schema`,
            path: [...path],
            code: "invalid-schema",
          });
        }
      } else {
        errors.push({
          message: `Unknown type "${typeName}" (no Types section in schema)`,
          path: [...path],
          code: "invalid-schema",
        });
      }
    }
  }
}

function validateTypeString(
  value: MOTLYNode,
  path: string[],
  errors: MOTLYSchemaError[]
): void {
  if (isRef(value.eq)) {
    errors.push({
      message: 'Expected type "string" but found a link',
      path: [...path],
      code: "wrong-type",
    });
    return;
  }
  if (typeof value.eq !== "string") {
    errors.push({
      message: 'Expected type "string"',
      path: [...path],
      code: "wrong-type",
    });
  }
}

function validateTypeNumber(
  value: MOTLYNode,
  path: string[],
  errors: MOTLYSchemaError[]
): void {
  if (isRef(value.eq)) {
    errors.push({
      message: 'Expected type "number" but found a link',
      path: [...path],
      code: "wrong-type",
    });
    return;
  }
  if (typeof value.eq !== "number") {
    errors.push({
      message: 'Expected type "number"',
      path: [...path],
      code: "wrong-type",
    });
  }
}

function validateTypeBoolean(
  value: MOTLYNode,
  path: string[],
  errors: MOTLYSchemaError[]
): void {
  if (isRef(value.eq)) {
    errors.push({
      message: 'Expected type "boolean" but found a link',
      path: [...path],
      code: "wrong-type",
    });
    return;
  }
  if (typeof value.eq !== "boolean") {
    errors.push({
      message: 'Expected type "boolean"',
      path: [...path],
      code: "wrong-type",
    });
  }
}

function validateTypeDate(
  value: MOTLYNode,
  path: string[],
  errors: MOTLYSchemaError[]
): void {
  if (isRef(value.eq)) {
    errors.push({
      message: 'Expected type "date" but found a link',
      path: [...path],
      code: "wrong-type",
    });
    return;
  }
  if (!(value.eq instanceof Date)) {
    errors.push({
      message: 'Expected type "date"',
      path: [...path],
      code: "wrong-type",
    });
  }
}

function validateTypeTag(
  value: MOTLYNode,
  path: string[],
  errors: MOTLYSchemaError[]
): void {
  if (isRef(value.eq)) {
    errors.push({
      message: 'Expected type "tag" but found a link',
      path: [...path],
      code: "wrong-type",
    });
  }
}

function validateTypeFlag(
  value: MOTLYNode,
  path: string[],
  errors: MOTLYSchemaError[]
): void {
  if (isRef(value.eq)) {
    errors.push({
      message: 'Expected type "flag" but found a link',
      path: [...path],
      code: "wrong-type",
    });
  }
}

function validateArrayType(
  value: MOTLYNode,
  innerType: string,
  types: Record<string, MOTLYNode> | undefined,
  path: string[],
  errors: MOTLYSchemaError[]
): void {
  if (isRef(value.eq)) {
    errors.push({
      message: `Expected type "${innerType}[]" but found a link`,
      path: [...path],
      code: "wrong-type",
    });
    return;
  }

  if (!Array.isArray(value.eq)) {
    errors.push({
      message: `Expected type "${innerType}[]" but value is not an array`,
      path: [...path],
      code: "wrong-type",
    });
    return;
  }

  for (let i = 0; i < value.eq.length; i++) {
    const elemPath = [...path, `[${i}]`];
    validateBaseType(value.eq[i], innerType, types, elemPath, errors);
  }
}

function validateEnum(
  value: MOTLYNode,
  allowed: MOTLYNode[],
  path: string[],
  errors: MOTLYSchemaError[]
): void {
  if (isRef(value.eq)) {
    errors.push({
      message: "Expected an enum value but found a link",
      path: [...path],
      code: "wrong-type",
    });
    return;
  }

  const nodeEq = value.eq;
  if (
    nodeEq === undefined ||
    (typeof nodeEq !== "string" &&
      typeof nodeEq !== "number" &&
      typeof nodeEq !== "boolean" &&
      !(nodeEq instanceof Date))
  ) {
    errors.push({
      message: "Expected an enum value",
      path: [...path],
      code: "invalid-enum-value",
    });
    return;
  }

  const matches = allowed.some((a) => {
    if (isRef(a.eq)) return false;
    const aeq = a.eq;
    if (aeq instanceof Date && nodeEq instanceof Date) {
      return aeq.getTime() === nodeEq.getTime();
    }
    return aeq === nodeEq;
  });

  if (!matches) {
    const allowedStrs = allowed
      .filter((a) => !isRef(a.eq))
      .map((a) => {
        const aeq = a.eq;
        return JSON.stringify(String(aeq));
      });
    errors.push({
      message: `Value does not match any allowed enum value. Allowed: [${allowedStrs.join(", ")}]`,
      path: [...path],
      code: "invalid-enum-value",
    });
  }
}

function validatePattern(
  value: MOTLYNode,
  matchesNode: MOTLYValue,
  path: string[],
  errors: MOTLYSchemaError[]
): void {
  const pattern = getEqString(matchesNode);
  if (pattern === undefined) return;

  if (isRef(value.eq)) {
    errors.push({
      message: "Expected a value matching a pattern but found a link",
      path: [...path],
      code: "wrong-type",
    });
    return;
  }

  if (typeof value.eq !== "string") {
    errors.push({
      message: `Expected a string matching pattern "${pattern}"`,
      path: [...path],
      code: "pattern-mismatch",
    });
    return;
  }

  try {
    const re = new RegExp(pattern);
    if (!re.test(value.eq)) {
      errors.push({
        message: `Value "${value.eq}" does not match pattern "${pattern}"`,
        path: [...path],
        code: "pattern-mismatch",
      });
    }
  } catch (e) {
    errors.push({
      message: `Invalid regex pattern "${pattern}": ${e}`,
      path: [...path],
      code: "invalid-schema",
    });
  }
}

function validateUnion(
  value: MOTLYNode,
  oneOfNode: MOTLYValue,
  types: Record<string, MOTLYNode> | undefined,
  path: string[],
  errors: MOTLYSchemaError[]
): void {
  if (!Array.isArray(oneOfNode.eq)) return;

  for (const typeVal of oneOfNode.eq) {
    const typeName = valueEqString(typeVal);
    if (typeName === undefined) continue;
    const trialErrors: MOTLYSchemaError[] = [];
    const synthetic = makeTypeSpecNode(typeName);
    validateValueType(value, synthetic, types, path, trialErrors);
    if (trialErrors.length === 0) return;
  }

  const typeStrs = oneOfNode.eq
    .map((v) => valueEqString(v))
    .filter((s) => s !== undefined);
  errors.push({
    message: `Value does not match any type in oneOf: [${typeStrs.join(", ")}]`,
    path: [...path],
    code: "wrong-type",
  });
}
