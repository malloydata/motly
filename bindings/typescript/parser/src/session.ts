import {
  MOTLYValue,
  MOTLYError,
  MOTLYSchemaError,
  MOTLYValidationError,
} from "motly-ts-interface";
import { parse } from "./parser";
import { execute } from "./interpreter";
import { validateReferences, validateSchema } from "./validate";
import { cloneValue } from "./clone";

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
      const errors = execute(stmts, this.value);
      return errors;
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
      const fresh: MOTLYValue = {};
      const errors = execute(stmts, fresh);
      this.schema = fresh;
      return errors;
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
    return cloneValue(this.value);
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

