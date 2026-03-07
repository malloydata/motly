import {
  MOTLYNode,
  MOTLYPropertyValue,
  MOTLYRef,
  MOTLYRefSegment,
  MOTLYLocation,
  MOTLYSchemaError,
  MOTLYValidationError,
  isRef,
  formatRef,
} from "../../interface/src/types";

/** Push a schema error, attaching node location if available. */
function pushSchemaError(
  errors: MOTLYSchemaError[],
  code: string,
  message: string,
  path: string[],
  node?: MOTLYNode
): void {
  const err: MOTLYSchemaError = { code, message, path };
  if (node?.location) err.location = node.location;
  errors.push(err);
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
        const errMsg = checkLink(childPv, ancestors, root);
        if (errMsg !== null) {
          const err: MOTLYValidationError = {
            message: errMsg,
            path: [...path],
            code: "unresolved-reference",
          };
          if (node.location) err.location = node.location;
          errors.push(err);
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
      const errMsg = checkLink(elemPv, ancestors, root);
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
  link: MOTLYRef,
  ancestors: MOTLYNode[],
  root: MOTLYNode
): string | null {
  const linkStr = formatRef(link);

  let start: MOTLYNode;
  if (link.linkUps === 0) {
    start = root;
  } else {
    const idx = ancestors.length - link.linkUps;
    if (idx < 0 || idx >= ancestors.length) {
      return `Reference "${linkStr}" goes ${link.linkUps} level(s) up but only ${ancestors.length} ancestor(s) available`;
    }
    start = ancestors[idx];
  }

  return resolvePath(start, link.linkTo, linkStr);
}

function resolvePath(
  start: MOTLYNode,
  segments: MOTLYRefSegment[],
  linkStr: string
): string | null {
  let current: MOTLYNode | "terminal" = start;

  for (const seg of segments) {
    if (current === "terminal") {
      return `Reference "${linkStr}" could not be resolved: cannot follow path through a link`;
    }

    if (typeof seg === "string") {
      if (!current.properties) {
        return `Reference "${linkStr}" could not be resolved: property "${seg}" not found (node has no properties)`;
      }
      const childPv: MOTLYPropertyValue | undefined = current.properties[seg];
      if (childPv === undefined) {
        return `Reference "${linkStr}" could not be resolved: property "${seg}" not found`;
      }
      if (isRef(childPv)) {
        current = "terminal";
      } else {
        current = childPv;
      }
    } else {
      if (current.eq === undefined || !Array.isArray(current.eq)) {
        return `Reference "${linkStr}" could not be resolved: index [${seg}] used on non-array`;
      }
      if (seg >= current.eq.length) {
        return `Reference "${linkStr}" could not be resolved: index [${seg}] out of bounds (array length ${current.eq.length})`;
      }
      const elemPv: MOTLYPropertyValue = current.eq[seg];
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
//
// Implements the ALL-CAPS schema language defined in
// docs/schema_spec.md. Directives: TYPES, REQUIRED,
// OPTIONAL, ADDITIONAL, VALUE, ONEOF, ENUM, MATCHES, MIN, MAX,
// MIN_LENGTH, MAX_LENGTH, EXCLUSIVE, REQUIRES.

const MAX_VALIDATION_DEPTH = 64;

// Pre-loaded types: the validator seeds the namespace with these
// before reading user-defined types from the schema's TYPES block.
const PRELOADED_TYPES: Record<string, MOTLYNode> = {
  string:  { properties: { VALUE: { eq: "string" } } },
  number:  { properties: { VALUE: { eq: "number" } } },
  integer: { properties: { VALUE: { eq: "integer" } } },
  boolean: { properties: { VALUE: { eq: "boolean" } } },
  date:    { properties: { VALUE: { eq: "date" } } },
  flag:    { properties: { ADDITIONAL: { eq: "reject" } } },
  tag:     { properties: { ADDITIONAL: { eq: "accept" } } },
  any:     { properties: { ADDITIONAL: { eq: "accept" } } },
};

export function validateSchema(
  tag: MOTLYNode,
  schema: MOTLYNode
): MOTLYSchemaError[] {
  const errors: MOTLYSchemaError[] = [];
  const types = buildTypesMap(schema, errors);
  validateConstraint(tag, schema, types, [], errors, 0);
  return errors;
}

function buildTypesMap(
  schema: MOTLYNode,
  errors: MOTLYSchemaError[]
): Record<string, MOTLYNode> {
  const types: Record<string, MOTLYNode> = { ...PRELOADED_TYPES };
  const typesNode = getDirective(schema, "TYPES");
  if (typesNode?.properties) {
    for (const [name, pv] of Object.entries(typesNode.properties)) {
      if (isRef(pv)) continue;
      if (name in PRELOADED_TYPES) {
        errors.push({
          message: `Type "${name}" cannot shadow pre-loaded type`,
          path: ["TYPES", name],
          code: "invalid-schema",
        });
        continue;
      }
      types[name] = pv;
    }
  }
  return types;
}

/** Read a directive property from a constraint node. */
function getDirective(node: MOTLYNode, name: string): MOTLYNode | undefined {
  if (!node.properties) return undefined;
  const pv = node.properties[name];
  if (pv === undefined || isRef(pv)) return undefined;
  return pv;
}

// ── Core constraint validation ──────────────────────────────────

function validateConstraint(
  target: MOTLYNode,
  constraint: MOTLYNode,
  types: Record<string, MOTLYNode>,
  path: string[],
  errors: MOTLYSchemaError[],
  depth: number
): void {
  if (depth > MAX_VALIDATION_DEPTH) {
    pushSchemaError(errors, "invalid-schema", "Maximum validation depth exceeded (possible recursive type cycle)", [...path], target);
    return;
  }

  // ONEOF — union dispatch
  const oneOfNode = getDirective(constraint, "ONEOF");
  if (oneOfNode && Array.isArray(oneOfNode.eq)) {
    validateOneOfArray(target, oneOfNode.eq, types, path, errors, depth);
    return;
  }

  // VALUE — value slot constraint
  const valueNode = getDirective(constraint, "VALUE");
  if (valueNode) {
    validateValue(target, valueNode, types, path, errors, depth);
  }

  // Property structure (REQUIRED, OPTIONAL, ADDITIONAL, EXCLUSIVE, REQUIRES)
  validateProperties(target, constraint, types, path, errors, depth);
}

// ── Value slot validation ───────────────────────────────────────

function validateValue(
  target: MOTLYNode,
  valueNode: MOTLYNode,
  types: Record<string, MOTLYNode>,
  path: string[],
  errors: MOTLYSchemaError[],
  depth: number
): void {
  const valueType = typeof valueNode.eq === "string" ? valueNode.eq : undefined;
  if (!valueType) return;

  const eq = target.eq;
  switch (valueType) {
    case "string":
      if (typeof eq !== "string") {
        pushSchemaError(errors, "wrong-type", `Expected string, got ${describeValue(eq)}`, [...path], target);
        return;
      }
      validateStringRefinements(eq, valueNode, path, errors, target);
      break;

    case "number":
      if (typeof eq !== "number") {
        pushSchemaError(errors, "wrong-type", `Expected number, got ${describeValue(eq)}`, [...path], target);
        return;
      }
      validateNumberRefinements(eq, valueNode, path, errors, target);
      break;

    case "integer":
      if (typeof eq !== "number" || !Number.isInteger(eq)) {
        pushSchemaError(errors, "wrong-type", `Expected integer, got ${describeValue(eq)}`, [...path], target);
        return;
      }
      validateNumberRefinements(eq, valueNode, path, errors, target);
      break;

    case "boolean":
      if (typeof eq !== "boolean") {
        pushSchemaError(errors, "wrong-type", `Expected boolean, got ${describeValue(eq)}`, [...path], target);
        return;
      }
      validateEnumRefinement(eq, valueNode, path, errors, target);
      break;

    case "date":
      if (!(eq instanceof Date)) {
        pushSchemaError(errors, "wrong-type", `Expected date, got ${describeValue(eq)}`, [...path], target);
        return;
      }
      validateEnumRefinement(eq, valueNode, path, errors, target);
      break;

    default: {
      // User-defined value type — resolve its VALUE constraint
      const typeDef = types[valueType];
      if (!typeDef) {
        pushSchemaError(errors, "invalid-schema", `Unknown VALUE type "${valueType}"`, [...path], target);
        return;
      }
      const innerValue = getDirective(typeDef, "VALUE");
      if (!innerValue) {
        pushSchemaError(errors, "invalid-schema", `Type "${valueType}" cannot be used as a VALUE type (no VALUE constraint)`, [...path], target);
        return;
      }
      validateValue(target, innerValue, types, path, errors, depth + 1);
    }
  }
}

/** Describe a value for error messages. */
function describeValue(eq: unknown): string {
  if (eq === undefined) return "no value";
  if (eq instanceof Date) return "date";
  if (Array.isArray(eq)) return "array";
  return typeof eq;
}

// ── Refinements ─────────────────────────────────────────────────

function validateStringRefinements(
  value: string,
  valueNode: MOTLYNode,
  path: string[],
  errors: MOTLYSchemaError[],
  target: MOTLYNode
): void {
  validateEnumRefinement(value, valueNode, path, errors, target);

  const matchesNode = getDirective(valueNode, "MATCHES");
  if (matchesNode && typeof matchesNode.eq === "string") {
    try {
      const re = new RegExp(matchesNode.eq);
      if (!re.test(value)) {
        pushSchemaError(errors, "pattern-mismatch", `Value "${value}" does not match pattern "${matchesNode.eq}"`, [...path], target);
      }
    } catch (e) {
      pushSchemaError(errors, "invalid-schema", `Invalid regex pattern "${matchesNode.eq}": ${e}`, [...path], target);
    }
  }

  const minLen = getDirective(valueNode, "MIN_LENGTH");
  if (minLen && typeof minLen.eq === "number" && value.length < minLen.eq) {
    pushSchemaError(errors, "length-violation", `String length ${value.length} is less than minimum ${minLen.eq}`, [...path], target);
  }

  const maxLen = getDirective(valueNode, "MAX_LENGTH");
  if (maxLen && typeof maxLen.eq === "number" && value.length > maxLen.eq) {
    pushSchemaError(errors, "length-violation", `String length ${value.length} exceeds maximum ${maxLen.eq}`, [...path], target);
  }
}

function validateNumberRefinements(
  value: number,
  valueNode: MOTLYNode,
  path: string[],
  errors: MOTLYSchemaError[],
  target: MOTLYNode
): void {
  validateEnumRefinement(value, valueNode, path, errors, target);

  const min = getDirective(valueNode, "MIN");
  if (min && typeof min.eq === "number" && value < min.eq) {
    pushSchemaError(errors, "out-of-range", `Value ${value} is less than minimum ${min.eq}`, [...path], target);
  }

  const max = getDirective(valueNode, "MAX");
  if (max && typeof max.eq === "number" && value > max.eq) {
    pushSchemaError(errors, "out-of-range", `Value ${value} exceeds maximum ${max.eq}`, [...path], target);
  }
}

function validateEnumRefinement(
  value: string | number | boolean | Date,
  valueNode: MOTLYNode,
  path: string[],
  errors: MOTLYSchemaError[],
  target: MOTLYNode
): void {
  const enumNode = getDirective(valueNode, "ENUM");
  if (!enumNode || !Array.isArray(enumNode.eq)) return;

  const matches = enumNode.eq.some((a) => {
    if (isRef(a)) return false;
    const aeq = a.eq;
    if (aeq instanceof Date && value instanceof Date) {
      return aeq.getTime() === value.getTime();
    }
    return aeq === value;
  });

  if (!matches) {
    const allowed = enumNode.eq
      .filter((a): a is MOTLYNode => !isRef(a))
      .map((a) => String(a.eq));
    pushSchemaError(errors, "invalid-enum-value", `Value does not match any allowed enum value. Allowed: [${allowed.join(", ")}]`, [...path], target);
  }
}

// ── Property structure validation ───────────────────────────────

type AdditionalPolicy =
  | { kind: "reject" }
  | { kind: "accept" }
  | { kind: "type"; typeName: string }
  | { kind: "inline"; constraint: MOTLYNode };

function getAdditionalPolicy(constraint: MOTLYNode): AdditionalPolicy {
  if (!constraint.properties) return { kind: "reject" };
  const pv = constraint.properties["ADDITIONAL"];
  if (pv === undefined) return { kind: "reject" };
  if (isRef(pv)) return { kind: "reject" };

  if (typeof pv.eq === "string") {
    if (pv.eq === "reject") return { kind: "reject" };
    if (pv.eq === "accept") return { kind: "accept" };
    return { kind: "type", typeName: pv.eq };
  }

  // Inline constraint (has structural directives) or bare flag (accept)
  if (pv.properties) {
    const keys = Object.keys(pv.properties);
    if (keys.some((k) => k === "VALUE" || k === "REQUIRED" || k === "OPTIONAL" || k === "ADDITIONAL" || k === "ONEOF")) {
      return { kind: "inline", constraint: pv };
    }
  }

  return { kind: "accept" }; // bare ADDITIONAL = accept
}

function validateProperties(
  target: MOTLYNode,
  constraint: MOTLYNode,
  types: Record<string, MOTLYNode>,
  path: string[],
  errors: MOTLYSchemaError[],
  depth: number
): void {
  const required = getDirective(constraint, "REQUIRED")?.properties;
  const optional = getDirective(constraint, "OPTIONAL")?.properties;
  const additional = getAdditionalPolicy(constraint);
  const targetProps = target.properties;

  // Check required properties
  if (required) {
    for (const [key, propDefPv] of Object.entries(required)) {
      if (isRef(propDefPv)) continue;
      const propPath = [...path, key];
      const targetValue = targetProps?.[key];
      if (targetValue === undefined) {
        pushSchemaError(errors, "missing-required", `Missing required property "${key}"`, propPath, target);
      } else {
        validatePropertyValue(targetValue, propDefPv, types, propPath, errors, depth);
      }
    }
  }

  // Check optional properties that exist
  if (optional && targetProps) {
    for (const [key, propDefPv] of Object.entries(optional)) {
      if (isRef(propDefPv)) continue;
      const targetValue = targetProps[key];
      if (targetValue !== undefined) {
        validatePropertyValue(targetValue, propDefPv, types, [...path, key], errors, depth);
      }
    }
  }

  // Check unknown properties
  if (targetProps) {
    const knownKeys = new Set<string>();
    if (required) for (const k of Object.keys(required)) knownKeys.add(k);
    if (optional) for (const k of Object.keys(optional)) knownKeys.add(k);

    for (const key of Object.keys(targetProps)) {
      if (knownKeys.has(key)) continue;
      const propPath = [...path, key];
      switch (additional.kind) {
        case "reject": {
          const unknownNode = isRef(targetProps[key]) ? undefined : targetProps[key] as MOTLYNode;
          pushSchemaError(errors, "unknown-property", `Unknown property "${key}"`, propPath, unknownNode);
          break;
        }
        case "accept":
          break;
        case "type":
          validatePropertyValue(
            targetProps[key],
            { eq: additional.typeName },
            types,
            propPath,
            errors,
            depth
          );
          break;
        case "inline":
          if (isRef(targetProps[key])) {
            pushSchemaError(errors, "wrong-type", "Expected a value but found a link", propPath);
          } else {
            validateConstraint(
              targetProps[key] as MOTLYNode,
              additional.constraint,
              types,
              propPath,
              errors,
              depth + 1
            );
          }
          break;
      }
    }
  }

  // EXCLUSIVE group checks
  validateExclusiveGroups(required, optional, targetProps, path, errors);

  // REQUIRES dependency checks
  validateRequiresDeps(required, optional, targetProps, path, errors);
}

/**
 * Validate a target property value against a property definition.
 *
 * A property definition is either:
 *   - A type reference: eq is a string type name (e.g. { eq: "string" })
 *   - An inline constraint: no eq, has directive properties (VALUE, REQUIRED, etc.)
 */
function validatePropertyValue(
  targetPv: MOTLYPropertyValue,
  propDef: MOTLYNode,
  types: Record<string, MOTLYNode>,
  path: string[],
  errors: MOTLYSchemaError[],
  depth: number
): void {
  if (isRef(targetPv)) {
    pushSchemaError(errors, "wrong-type", "Expected a value but found a link", [...path]);
    return;
  }

  const typeName = typeof propDef.eq === "string" ? propDef.eq : undefined;
  if (typeName) {
    validateAgainstTypeName(targetPv, typeName, types, path, errors, depth);
    return;
  }

  // Inline constraint
  validateConstraint(targetPv, propDef, types, path, errors, depth + 1);
}

// ── Type resolution ─────────────────────────────────────────────

function validateAgainstTypeName(
  target: MOTLYNode,
  typeName: string,
  types: Record<string, MOTLYNode>,
  path: string[],
  errors: MOTLYSchemaError[],
  depth: number
): void {
  // Array type: "string[]", "TypeName[]"
  if (typeName.endsWith("[]")) {
    validateArrayType(target, typeName.slice(0, -2), types, path, errors, depth);
    return;
  }

  const typeDef = types[typeName];
  if (!typeDef) {
    pushSchemaError(errors, "invalid-schema", `Unknown type "${typeName}" in schema`, [...path], target);
    return;
  }

  // Union shorthand at TYPES level: TypeName = [TypeA, TypeB]
  if (Array.isArray(typeDef.eq)) {
    validateOneOfArray(target, typeDef.eq, types, path, errors, depth);
    return;
  }

  validateConstraint(target, typeDef, types, path, errors, depth + 1);
}

function validateArrayType(
  target: MOTLYNode,
  innerType: string,
  types: Record<string, MOTLYNode>,
  path: string[],
  errors: MOTLYSchemaError[],
  depth: number
): void {
  if (!Array.isArray(target.eq)) {
    pushSchemaError(errors, "wrong-type", `Expected ${innerType}[], got ${describeValue(target.eq)}`, [...path], target);
    return;
  }

  for (let i = 0; i < target.eq.length; i++) {
    const elemPath = [...path, `[${i}]`];
    const elemPv = target.eq[i];
    if (isRef(elemPv)) {
      pushSchemaError(errors, "wrong-type", `Expected ${innerType}, got reference`, elemPath);
    } else {
      validateAgainstTypeName(elemPv, innerType, types, elemPath, errors, depth);
    }
  }
}

// ── Union validation ────────────────────────────────────────────

function validateOneOfArray(
  target: MOTLYNode,
  typeRefs: MOTLYPropertyValue[],
  types: Record<string, MOTLYNode>,
  path: string[],
  errors: MOTLYSchemaError[],
  depth: number
): void {
  const typeNames: string[] = [];
  let bestErrors: MOTLYSchemaError[] | undefined;
  let bestBranch: string | undefined;

  for (const ref of typeRefs) {
    if (isRef(ref)) continue;
    const name = typeof ref.eq === "string" ? ref.eq : undefined;
    if (!name) continue;
    typeNames.push(name);

    const trialErrors: MOTLYSchemaError[] = [];
    validateAgainstTypeName(target, name, types, path, trialErrors, depth);
    if (trialErrors.length === 0) return; // matches this branch

    if (!bestErrors || trialErrors.length < bestErrors.length) {
      bestErrors = trialErrors;
      bestBranch = name;
    }
  }

  let msg = `Value does not match any type in oneOf: [${typeNames.join(", ")}]`;
  if (bestErrors && bestErrors.length > 0 && typeNames.length > 1) {
    const details = bestErrors.map((e) => e.message).join("; ");
    msg += `. Closest match "${bestBranch}": ${details}`;
  }

  pushSchemaError(errors, "wrong-type", msg, [...path], target);
}

// ── Metadata validation ─────────────────────────────────────────

function validateExclusiveGroups(
  required: Record<string, MOTLYPropertyValue> | undefined,
  optional: Record<string, MOTLYPropertyValue> | undefined,
  targetProps: Record<string, MOTLYPropertyValue> | undefined,
  path: string[],
  errors: MOTLYSchemaError[]
): void {
  if (!targetProps) return;

  const groups: Record<string, string[]> = {};

  function collect(propDefs: Record<string, MOTLYPropertyValue> | undefined) {
    if (!propDefs) return;
    for (const [key, pv] of Object.entries(propDefs)) {
      if (isRef(pv)) continue;
      const exclusive = getDirective(pv, "EXCLUSIVE");
      if (!exclusive) continue;

      let groupNames: string[];
      if (typeof exclusive.eq === "string") {
        groupNames = [exclusive.eq];
      } else if (Array.isArray(exclusive.eq)) {
        groupNames = exclusive.eq
          .filter((e): e is MOTLYNode => !isRef(e))
          .map((e) => String(e.eq));
      } else {
        continue;
      }

      for (const g of groupNames) {
        if (!groups[g]) groups[g] = [];
        groups[g].push(key);
      }
    }
  }

  collect(required);
  collect(optional);

  for (const [group, members] of Object.entries(groups)) {
    const present = members.filter((m) => targetProps[m] !== undefined);
    if (present.length > 1) {
      pushSchemaError(errors, "exclusive-violation", `Properties [${present.join(", ")}] are mutually exclusive (group "${group}")`, [...path]);
    }
  }
}

function validateRequiresDeps(
  required: Record<string, MOTLYPropertyValue> | undefined,
  optional: Record<string, MOTLYPropertyValue> | undefined,
  targetProps: Record<string, MOTLYPropertyValue> | undefined,
  path: string[],
  errors: MOTLYSchemaError[]
): void {
  if (!targetProps) return;

  function check(propDefs: Record<string, MOTLYPropertyValue> | undefined) {
    if (!propDefs) return;
    for (const [key, pv] of Object.entries(propDefs)) {
      if (isRef(pv)) continue;
      if (targetProps![key] === undefined) continue; // property not present
      const requires = getDirective(pv, "REQUIRES");
      if (!requires || !Array.isArray(requires.eq)) continue;

      for (const req of requires.eq) {
        if (isRef(req)) continue;
        const reqName = typeof req.eq === "string" ? req.eq : undefined;
        if (!reqName) continue;
        if (targetProps![reqName] === undefined) {
          pushSchemaError(errors, "requires-violation", `Property "${key}" requires "${reqName}" to be present`, [...path, key]);
        }
      }
    }
  }

  check(required);
  check(optional);
}
