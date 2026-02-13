import {
  MOTLYValue,
  MOTLYError,
  MOTLYSchemaError,
  MOTLYValidationError,
} from "motly-ts-interface";
import { parse } from "./parser";
import { execute } from "./interpreter";
import { validateReferences, validateSchema } from "./validate";

/**
 * A stateful MOTLY parsing session.
 *
 * Pure TypeScript implementation — no WASM, no native dependencies.
 * API-compatible with the Rust `MotlySession`.
 */
export class MOTLYSession {
  private value: MOTLYValue = {};
  private schema: MOTLYValue | null = null;
  private disposed = false;

  /**
   * Parse MOTLY source and apply it to the session's value in place.
   * Returns only parse errors.
   */
  parse(source: string): MOTLYError[] {
    this.ensureAlive();
    try {
      const stmts = parse(source);
      this.value = execute(stmts, this.value);
      return [];
    } catch (e) {
      if (isMotlyError(e)) return [e];
      throw e;
    }
  }

  /**
   * Parse MOTLY source as a schema and store it in the session.
   * The schema is parsed fresh (not merged).
   */
  parseSchema(source: string): MOTLYError[] {
    this.ensureAlive();
    try {
      const stmts = parse(source);
      this.schema = execute(stmts, {});
      return [];
    } catch (e) {
      if (isMotlyError(e)) return [e];
      throw e;
    }
  }

  /**
   * Reset the session's value to empty, keeping the schema.
   */
  reset(): void {
    this.ensureAlive();
    this.value = {};
  }

  /**
   * Return a deep clone of the session's current value.
   */
  getValue(): MOTLYValue {
    this.ensureAlive();
    return deepClone(this.value);
  }

  /**
   * Validate the session's value against its stored schema.
   * Returns an empty array if no schema has been set.
   */
  validateSchema(): MOTLYSchemaError[] {
    this.ensureAlive();
    if (this.schema === null) return [];
    return validateSchema(this.value, this.schema);
  }

  /**
   * Validate that all `$`-references in the session's value resolve.
   */
  validateReferences(): MOTLYValidationError[] {
    this.ensureAlive();
    return validateReferences(this.value);
  }

  /**
   * No-op for pure TS (no native resources to free).
   * After calling `dispose()`, all other methods will throw.
   */
  dispose(): void {
    this.disposed = true;
  }

  private ensureAlive(): void {
    if (this.disposed) {
      throw new Error("MOTLYSession has been disposed");
    }
  }
}

function isMotlyError(e: unknown): e is MOTLYError {
  return (
    typeof e === "object" &&
    e !== null &&
    "code" in e &&
    "message" in e &&
    "begin" in e &&
    "end" in e
  );
}

function deepClone(value: MOTLYValue): MOTLYValue {
  const result: MOTLYValue = {};

  if (value.deleted) result.deleted = true;

  if (value.eq !== undefined) {
    if (value.eq instanceof Date) {
      result.eq = new Date(value.eq.getTime());
    } else if (Array.isArray(value.eq)) {
      result.eq = value.eq.map(cloneNode);
    } else {
      result.eq = value.eq;
    }
  }

  if (value.properties) {
    const props: Record<string, import("motly-ts-interface").MOTLYNode> = {};
    for (const key of Object.keys(value.properties)) {
      props[key] = cloneNode(value.properties[key]);
    }
    result.properties = props;
  }

  return result;
}

function cloneNode(node: import("motly-ts-interface").MOTLYNode): import("motly-ts-interface").MOTLYNode {
  if ("linkTo" in node) {
    return { linkTo: node.linkTo };
  }
  return deepClone(node);
}
