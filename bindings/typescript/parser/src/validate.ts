import {
  MOTLYNode,
  MOTLYPropertyValue,
  MOTLYRef,
  MOTLYSchemaError,
  MOTLYValidationError,
  isRef,
  isEnvRef,
} from "../../interface/src/types";

function getEqString(node: MOTLYNode): string | undefined {
  return typeof node.eq === "string" ? node.eq : undefined;
}

function pvEqString(pv: MOTLYPropertyValue): string | undefined {
  if (isRef(pv)) return undefined;
  return getEqString(pv);
}

function extractSection(
  node: MOTLYNode,
  name: string
): Record<string, MOTLYPropertyValue> | undefined {
  if (!node.properties) return undefined;
  const pv = node.properties[name];
  if (pv === undefined || isRef(pv)) return undefined;
  return pv.properties;
}

// ── Reference Validation ────────────────────────────────────────

export function validateReferences(root: MOTLYNode): MOTLYValidationError[] {
  const errors: MOTLYValidationError[] = [];
  const path: string[] = [];
  const ancestors: MOTLYNode[] = [root];
  walkRefs(root, path, ancestors, root, errors);
  return errors;
}

function walkRefs(
  node: MOTLYNode,
  path: string[],
  ancestors: MOTLYNode[],
  root: MOTLYNode,
  errors: MOTLYValidationError[]
): void {
  // Check array elements in eq
  if (node.eq !== undefined && Array.isArray(node.eq)) {
    walkArrayRefs(node.eq, path, ancestors, node, root, errors);
  }

  if (node.properties) {
    for (const key of Object.keys(node.properties)) {
      const childPv = node.properties[key];
      path.push(key);

      if (isRef(childPv)) {
        // This property is a reference — check it
        const errMsg = checkLink(childPv.linkTo, ancestors, root);
        if (errMsg !== null) {
          errors.push({
            message: errMsg,
            path: [...path],
            code: "unresolved-reference",
          });
        }
      } else {
        // Recurse into child node
        ancestors.push(node);
        walkRefs(childPv, path, ancestors, root, errors);
        ancestors.pop();
      }

      path.pop();
    }
  }
}

function walkArrayRefs(
  arr: MOTLYPropertyValue[],
  path: string[],
  ancestors: MOTLYNode[],
  parentNode: MOTLYNode,
  root: MOTLYNode,
  errors: MOTLYValidationError[]
): void {
  for (let i = 0; i < arr.length; i++) {
    const elemPv = arr[i];
    const idxKey = `[${i}]`;
    path.push(idxKey);

    if (isRef(elemPv)) {
      const errMsg = checkLink(elemPv.linkTo, ancestors, root);
      if (errMsg !== null) {
        errors.push({
          message: errMsg,
          path: [...path],
          code: "unresolved-reference",
        });
      }
    } else {
      // Recurse into element node
      ancestors.push(parentNode);
      walkRefs(elemPv, path, ancestors, root, errors);
      ancestors.pop();
    }

    path.pop();
  }
}

function checkLink(
  linkTo: string,
  ancestors: MOTLYNode[],
  root: MOTLYNode
): string | null {
  const { ups, segments, error } = parseLinkString(linkTo);
  if (error !== null) return error;

  let start: MOTLYNode;
  if (ups === 0) {
    start = root;
  } else {
    const idx = ancestors.length - ups;
    if (idx < 0 || idx >= ancestors.length) {
      return `Reference "${linkTo}" goes ${ups} level(s) up but only ${ancestors.length} ancestor(s) available`;
    }
    start = ancestors[idx];
  }

  return resolvePath(start, segments, linkTo);
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
  start: MOTLYNode,
  segments: RefSeg[],
  linkStr: string
): string | null {
  let current: MOTLYNode | "terminal" = start;

  for (const seg of segments) {
    if (current === "terminal") {
      return `Reference "${linkStr}" could not be resolved: cannot follow path through a link`;
    }

    if (seg.kind === "name") {
      if (!current.properties) {
        return `Reference "${linkStr}" could not be resolved: property "${seg.name}" not found (node has no properties)`;
      }
      const childPv: MOTLYPropertyValue | undefined = current.properties[seg.name];
      if (childPv === undefined) {
        return `Reference "${linkStr}" could not be resolved: property "${seg.name}" not found`;
      }
      if (isRef(childPv)) {
        current = "terminal";
      } else {
        current = childPv;
      }
    } else {
      if (current.eq === undefined || !Array.isArray(current.eq)) {
        return `Reference "${linkStr}" could not be resolved: index [${seg.index}] used on non-array`;
      }
      if (seg.index >= current.eq.length) {
        return `Reference "${linkStr}" could not be resolved: index [${seg.index}] out of bounds (array length ${current.eq.length})`;
      }
      const elemPv: MOTLYPropertyValue = current.eq[seg.index];
      if (isRef(elemPv)) {
        current = "terminal";
      } else {
        current = elemPv;
      }
    }
  }

  return null;
}

// ── Schema Validation ───────────────────────────────────────────

export function validateSchema(
  tag: MOTLYNode,
  schema: MOTLYNode
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

function getAdditionalPolicy(schema: MOTLYNode): AdditionalPolicy {
  if (!schema.properties) return { kind: "reject" };
  const additionalPv = schema.properties["Additional"];
  if (additionalPv === undefined || isRef(additionalPv)) return { kind: "reject" };
  const eqStr = getEqString(additionalPv);
  if (eqStr !== undefined) {
    if (eqStr === "allow") return { kind: "allow" };
    if (eqStr === "reject") return { kind: "reject" };
    return { kind: "validateAs", typeName: eqStr };
  }
  return { kind: "allow" };
}

function validateNodeAgainstSchema(
  tag: MOTLYNode,
  schema: MOTLYNode,
  types: Record<string, MOTLYPropertyValue> | undefined,
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
      const tagValuePv = tagProps ? tagProps[key] : undefined;
      if (tagValuePv === undefined) {
        errors.push({
          message: `Missing required property "${key}"`,
          path: propPath,
          code: "missing-required",
        });
      } else {
        validateValueType(tagValuePv, required[key], types, propPath, errors);
      }
    }
  }

  // Check optional properties that exist
  if (optional && tagProps) {
    for (const key of Object.keys(optional)) {
      const tagValuePv = tagProps[key];
      if (tagValuePv !== undefined) {
        validateValueType(
          tagValuePv,
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
          const synthetic: MOTLYPropertyValue = makeTypeSpecNode(additional.typeName);
          validateValueType(tagProps[key], synthetic, types, propPath, errors);
          break;
        }
      }
    }
  }
}

function makeTypeSpecNode(typeName: string): MOTLYNode {
  return { eq: typeName };
}

function validateValueType(
  valuePv: MOTLYPropertyValue,
  typeSpecPv: MOTLYPropertyValue,
  types: Record<string, MOTLYPropertyValue> | undefined,
  path: string[],
  errors: MOTLYSchemaError[]
): void {
  // Skip ref type specs in schema
  if (isRef(typeSpecPv)) return;
  const specNode = typeSpecPv;

  // If value is a ref, generate appropriate "found a link" error
  if (isRef(valuePv)) {
    pushRefTypeError(specNode, path, errors);
    return;
  }

  // Value is a node
  const value = valuePv;

  validateNodeAgainstTypeSpec(value, specNode, types, path, errors);
}

function pushRefTypeError(
  specNode: MOTLYNode,
  path: string[],
  errors: MOTLYSchemaError[]
): void {
  // Check for enum
  if (specNode.properties) {
    const eqProp = specNode.properties["eq"];
    if (eqProp !== undefined && !isRef(eqProp)) {
      if (Array.isArray(eqProp.eq)) {
        errors.push({
          message: "Expected an enum value but found a link",
          path: [...path],
          code: "wrong-type",
        });
        return;
      }
    }
    if (specNode.properties["matches"] !== undefined) {
      errors.push({
        message: "Expected a value matching a pattern but found a link",
        path: [...path],
        code: "wrong-type",
      });
      return;
    }
  }

  const typeName = getEqString(specNode);
  if (typeName !== undefined) {
    errors.push({
      message: `Expected type "${typeName}" but found a link`,
      path: [...path],
      code: "wrong-type",
    });
  } else if (
    specNode.properties &&
    ("Required" in specNode.properties ||
      "Optional" in specNode.properties ||
      "Additional" in specNode.properties)
  ) {
    errors.push({
      message: "Expected a tag but found a link",
      path: [...path],
      code: "wrong-type",
    });
  }
}

function validateNodeAgainstTypeSpec(
  value: MOTLYNode,
  specNode: MOTLYNode,
  types: Record<string, MOTLYPropertyValue> | undefined,
  path: string[],
  errors: MOTLYSchemaError[]
): void {
  // Check for union type (oneOf)
  if (specNode.properties) {
    const oneOfPv = specNode.properties["oneOf"];
    if (oneOfPv !== undefined && !isRef(oneOfPv)) {
      validateUnion(value, oneOfPv, types, path, errors);
      return;
    }
  }

  // Check for enum (eq) or pattern (matches)
  if (specNode.properties) {
    const eqProp = specNode.properties["eq"];
    if (eqProp !== undefined && !isRef(eqProp)) {
      if (Array.isArray(eqProp.eq)) {
        validateEnum(value, eqProp.eq, path, errors);
        return;
      }
    }

    const matchesProp = specNode.properties["matches"];
    if (matchesProp !== undefined && !isRef(matchesProp)) {
      const baseType = getEqString(specNode);
      if (baseType !== undefined) {
        validateBaseType(value, baseType, types, path, errors);
      }
      validatePattern(value, matchesProp as MOTLYNode, path, errors);
      return;
    }
  }

  // Get the type name from the spec's eq value
  const typeName = getEqString(specNode);
  if (typeName === undefined) {
    // Nested schema (has Required/Optional/Additional)
    if (
      specNode.properties &&
      ("Required" in specNode.properties ||
        "Optional" in specNode.properties ||
        "Additional" in specNode.properties)
    ) {
      validateNodeAgainstSchema(value, specNode, types, path, errors);
    }
    return;
  }

  validateBaseType(value, typeName, types, path, errors);
}

function validateBaseType(
  value: MOTLYNode,
  typeName: string,
  types: Record<string, MOTLYPropertyValue> | undefined,
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
      break; // tag — node exists, always valid for a non-ref
    case "flag":
      break; // flag — presence-only, always valid for a non-ref
    case "any":
      break; // any — always valid
    default: {
      // Custom type
      if (types) {
        const typeDefPv = types[typeName];
        if (typeDefPv !== undefined) {
          if (isRef(typeDefPv)) {
            // Type definition is a ref — skip
          } else {
            validateNodeAgainstTypeSpec(value, typeDefPv, types, path, errors);
          }
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
  if (!(value.eq instanceof Date)) {
    errors.push({
      message: 'Expected type "date"',
      path: [...path],
      code: "wrong-type",
    });
  }
}

function validateArrayType(
  value: MOTLYNode,
  innerType: string,
  types: Record<string, MOTLYPropertyValue> | undefined,
  path: string[],
  errors: MOTLYSchemaError[]
): void {
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
    const elemPv = value.eq[i];
    if (isRef(elemPv)) {
      errors.push({
        message: `Expected type "${innerType}" but found a link`,
        path: elemPath,
        code: "wrong-type",
      });
    } else {
      validateBaseType(elemPv, innerType, types, elemPath, errors);
    }
  }
}

function validateEnum(
  value: MOTLYNode,
  allowed: MOTLYPropertyValue[],
  path: string[],
  errors: MOTLYSchemaError[]
): void {
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
    if (isRef(a)) return false;
    const aeq = a.eq;
    if (aeq instanceof Date && nodeEq instanceof Date) {
      return aeq.getTime() === nodeEq.getTime();
    }
    return aeq === nodeEq;
  });

  if (!matches) {
    const allowedStrs = allowed
      .filter((a) => !isRef(a))
      .map((a) => {
        const aeq = (a as MOTLYNode).eq;
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
  matchesNode: MOTLYNode,
  path: string[],
  errors: MOTLYSchemaError[]
): void {
  const pattern = getEqString(matchesNode);
  if (pattern === undefined) return;

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
  oneOfNode: MOTLYNode,
  types: Record<string, MOTLYPropertyValue> | undefined,
  path: string[],
  errors: MOTLYSchemaError[]
): void {
  if (!Array.isArray(oneOfNode.eq)) return;

  // We need to wrap value as a MOTLYPropertyValue for validate_value_type
  const valuePv: MOTLYPropertyValue = value;

  for (const typePv of oneOfNode.eq) {
    const typeName = pvEqString(typePv);
    if (typeName === undefined) continue;
    const trialErrors: MOTLYSchemaError[] = [];
    const synthetic: MOTLYPropertyValue = makeTypeSpecNode(typeName);
    validateValueType(valuePv, synthetic, types, path, trialErrors);
    if (trialErrors.length === 0) return;
  }

  const typeStrs = oneOfNode.eq
    .map((v) => pvEqString(v))
    .filter((s) => s !== undefined);
  errors.push({
    message: `Value does not match any type in oneOf: [${typeStrs.join(", ")}]`,
    path: [...path],
    code: "wrong-type",
  });
}
