import {
  MOTLYNode,
  MOTLYDataNode,
  MOTLYError,
  MOTLYParseResult,
  MOTLYSessionOptions,
  MOTLYSchemaError,
  MOTLYValidationError,
} from "../../interface/src/types";
import { parse } from "./parser";
import { execute, ExecContext } from "./interpreter";
import { validateReferences, validateSchema } from "./validate";
import { cloneNode } from "./clone";
import { Mot, GetMotOptions, buildMot } from "./mot";

/**
 * A stateful MOTLY parsing session.
 *
 * Pure TypeScript implementation — no WASM, no native dependencies.
 * API-compatible with the Rust `MotlySession`.
 */
export class MOTLYSession {
  private value: MOTLYDataNode = {};
  private schema: MOTLYDataNode | null = null;
  private disposed = false;
  private nextParseId = 0;
  private options: MOTLYSessionOptions;

  constructor(options?: MOTLYSessionOptions) {
    this.options = options ?? {};
  }

  /**
   * Parse MOTLY source and apply it to the session's value in place.
   * Returns the assigned parseId and any parse/execution errors.
   */
  parse(source: string): MOTLYParseResult {
    this.ensureAlive();
    const ctx = this.makeContext();
    try {
      const stmts = parse(source);
      const errors = execute(stmts, this.value, ctx);
      return { parseId: ctx.parseId, errors };
    } catch (e) {
      if (isMotlyError(e)) return { parseId: ctx.parseId, errors: [e] };
      throw e;
    }
  }

  /**
   * Parse MOTLY source as a schema and store it in the session.
   * The schema is parsed fresh (not merged).
   */
  parseSchema(source: string): MOTLYParseResult {
    this.ensureAlive();
    const ctx = this.makeContext();
    try {
      const stmts = parse(source);
      const fresh: MOTLYDataNode = {};
      const errors = execute(stmts, fresh, ctx);
      this.schema = fresh;
      return { parseId: ctx.parseId, errors };
    } catch (e) {
      if (isMotlyError(e)) return { parseId: ctx.parseId, errors: [e] };
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
  getValue(): MOTLYDataNode {
    this.ensureAlive();
    return cloneNode(this.value);
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
   * Return a resolved Mot view of the current value.
   * Follows references, resolves env refs, and omits deleted nodes.
   *
   * Pass a {@link MotFactory} via `options.factory` to control what
   * objects are created (e.g., Tags with read tracking). Without a
   * factory, returns plain Mot instances.
   */
  getMot<M extends Mot = Mot>(options?: GetMotOptions<M>): M {
    this.ensureAlive();
    const tree = this.getValue();
    return buildMot(tree, options as GetMotOptions) as M;
  }

  /**
   * No-op for pure TS (no native resources to free).
   * After calling `dispose()`, all other methods will throw.
   */
  dispose(): void {
    this.disposed = true;
  }

  private makeContext(): ExecContext {
    return {
      parseId: this.nextParseId++,
      options: {
        disableReferences: this.options.disableReferences,
      },
    };
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
