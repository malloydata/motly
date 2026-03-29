import {
  MOTLYNode,
  MOTLYDataNode,
  MOTLYError,
  MOTLYParseResult,
  MOTLYSessionOptions,
  MOTLYSchemaError,
} from "../../interface/src/types";
import { Statement } from "./ast";
import { parse } from "./parser";
import { ExecContext, flatten, chunk, topoSort, executeChunked, Transformer } from "./interpreter";
import { validateReferences, validateSchema } from "./validate";
import { cloneNode } from "./clone";
import { Mot, GetMotOptions, buildMot } from "./mot";

/**
 * A write-only MOTLY parsing session that accumulates input.
 *
 * Call `parse()` to accumulate statements, then `finish()` to interpret
 * everything and get an immutable `MOTLYResult`. The session is spent
 * after `finish()` — create a new one to start over.
 */
export class MOTLYSession {
  private accumulated: { stmts: Statement[]; parseId: number }[] = [];
  private nextParseId = 0;
  private options: MOTLYSessionOptions;
  private finished = false;
  private disposed = false;

  constructor(options?: MOTLYSessionOptions) {
    this.options = options ?? {};
  }

  /**
   * Parse MOTLY source and accumulate statements.
   * Returns only syntax errors — semantic errors are deferred to `finish()`.
   */
  parse(source: string): MOTLYParseResult {
    this.ensureAlive();
    if (this.finished) {
      throw new Error("MOTLYSession is spent after finish() — create a new session");
    }
    const parseId = this.nextParseId++;
    try {
      const stmts = parse(source);
      this.accumulated.push({ stmts, parseId });
      return { parseId, errors: [] };
    } catch (e) {
      if (isMotlyError(e)) return { parseId, errors: [e] };
      throw e;
    }
  }

  /**
   * Interpret all accumulated statements, resolve references, and return
   * an immutable result. The session is spent after this call.
   */
  finish(): MOTLYResult {
    this.ensureAlive();
    if (this.finished) {
      throw new Error("finish() has already been called on this session");
    }
    this.finished = true;

    const allErrors: MOTLYError[] = [];
    const root: MOTLYDataNode = {};

    // Phase 1: Flatten all accumulated statements into transformers
    const allTransformers: Transformer[] = [];
    for (const { stmts, parseId } of this.accumulated) {
      const ctx: ExecContext = {
        parseId,
        options: { disableReferences: this.options.disableReferences },
      };
      allTransformers.push(...flatten(stmts, ctx));
    }

    // Phase 2: Chunk — find forward references, build splits + deps
    const { splits, deps } = chunk(allTransformers);

    // Phase 3: Topo-sort — dependency-respecting execution order
    // Cycles among chunks are rare (the chunker treats one direction of a
    // mutual clone as backward). Clone cycles are detected post-execution
    // by replaceCircularCloneErrors; link cycles are caught by validateReferences.
    const { order } = topoSort(deps);

    // Phase 4: Execute chunks in sorted order
    const execErrors = executeChunked(allTransformers, splits, order, root, this.options);
    allErrors.push(...execErrors);

    // Validate references (unless disabled)
    if (!this.options.disableReferences) {
      const refErrors = validateReferences(root);
      for (const re of refErrors) {
        allErrors.push({
          code: re.code,
          message: re.message,
          begin: { line: 0, column: 0, offset: 0 },
          end: { line: 0, column: 0, offset: 0 },
        });
      }
    }

    return new MOTLYResult(root, allErrors);
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

/**
 * Immutable result from `MOTLYSession.finish()`.
 * All the heavy lifting (interpretation, reference resolution) has already happened.
 */
export class MOTLYResult {
  readonly errors: MOTLYError[];
  private value: MOTLYDataNode;

  /** @internal */
  constructor(value: MOTLYDataNode, errors: MOTLYError[]) {
    this.value = value;
    this.errors = errors;
  }

  /** Return a deep clone of the interpreted tree. */
  getValue(): MOTLYDataNode {
    return cloneNode(this.value);
  }

  /**
   * Return a resolved Mot view of the tree.
   * Follows references lazily on read. Unresolved refs become Undefined Mot.
   */
  getMot<M extends Mot = Mot>(options?: GetMotOptions<M>): M {
    const tree = this.getValue();
    return buildMot(tree, options as GetMotOptions) as M;
  }
}

/**
 * A parsed MOTLY schema, independent of any session.
 *
 * Schemas use TYPES for reuse, not $-references. Link refs (`= $ref`)
 * produce errors; clones (`:= $ref`) are allowed for backward references.
 */
export class MOTLYSchema {
  private tree: MOTLYDataNode;

  private constructor(tree: MOTLYDataNode) {
    this.tree = tree;
  }

  /**
   * Parse MOTLY source as a schema.
   * Returns the schema and any parse/interpretation errors.
   */
  static parse(source: string): { schema: MOTLYSchema; errors: MOTLYError[] } {
    const options = { disableReferences: true };
    const ctx: ExecContext = { parseId: 0, options };
    try {
      const stmts = parse(source);
      const root: MOTLYDataNode = {};
      const transformers = flatten(stmts, ctx);
      const { splits, deps } = chunk(transformers);
      const { order } = topoSort(deps);
      const errors = executeChunked(transformers, splits, order, root, options);
      return { schema: new MOTLYSchema(root), errors };
    } catch (e) {
      if (isMotlyError(e)) {
        return { schema: new MOTLYSchema({}), errors: [e] };
      }
      throw e;
    }
  }

  /** Validate a MOTLY data tree against this schema. */
  validate(tree: MOTLYDataNode): MOTLYSchemaError[] {
    return validateSchema(tree, this.tree);
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
