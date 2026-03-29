import {
  Statement,
  TagValue,
  ArrayElement,
  RefPathSegment,
  Span,
} from "./ast";
import { MOTLYNode, MOTLYDataNode, MOTLYRef, MOTLYError, MOTLYLocation, MOTLYSessionOptions, isRef, formatRef } from "../../interface/src/types";
import { cloneNode } from "./clone";

/** Per-parse execution context, combining the parse ID with session options. */
export interface ExecContext {
  parseId: number;
  options: MOTLYSessionOptions;
}

// ─── Transformer types (Phase 1 output) ─────────────────────────────

export interface Transformer {
  path: string[];
  op: TransformerOp;
  span: Span;
  parseId: number;
}

export type TransformerOp =
  | { kind: "setValue"; value: TagValue }
  | { kind: "assignValue"; value: TagValue }
  | { kind: "clearProperties" }
  | { kind: "clearAll" }
  | { kind: "define" }
  | { kind: "delete" }
  | { kind: "link"; ups: number; refPath: RefPathSegment[] }
  | { kind: "clone"; ups: number; refPath: RefPathSegment[] };

// ─── Phase 1: Flatten ────────────────────────────────────────────────

/** Walk the AST depth-first, accumulating absolute paths. Emit one Transformer per leaf operation. Pure — no tree mutation. */
export function flatten(
  statements: Statement[],
  ctx: ExecContext,
): Transformer[] {
  const out: Transformer[] = [];
  flattenStmts(statements, [], ctx.parseId, out);
  return out;
}

function flattenStmts(
  stmts: Statement[],
  parentPath: string[],
  parseId: number,
  out: Transformer[],
): void {
  for (const stmt of stmts) {
    flattenStmt(stmt, parentPath, parseId, out);
  }
}

function flattenStmt(
  stmt: Statement,
  parentPath: string[],
  parseId: number,
  out: Transformer[],
): void {
  switch (stmt.kind) {
    case "setEq": {
      const path = [...parentPath, ...stmt.path];
      if (stmt.value.kind === "scalar" && stmt.value.value.kind === "reference") {
        const ref = stmt.value.value;
        out.push({ path, op: { kind: "link", ups: ref.ups, refPath: ref.path }, span: stmt.span, parseId });
      } else {
        out.push({ path, op: { kind: "setValue", value: stmt.value }, span: stmt.span, parseId });
        if (stmt.properties !== null) {
          flattenStmts(stmt.properties, path, parseId, out);
        }
      }
      break;
    }

    case "assignBoth": {
      const path = [...parentPath, ...stmt.path];
      if (stmt.value.kind === "scalar" && stmt.value.value.kind === "reference") {
        const ref = stmt.value.value;
        out.push({ path, op: { kind: "clone", ups: ref.ups, refPath: ref.path }, span: stmt.span, parseId });
        if (stmt.properties !== null) {
          out.push({ path, op: { kind: "clearProperties" }, span: stmt.span, parseId });
          flattenStmts(stmt.properties, path, parseId, out);
        }
      } else {
        out.push({ path, op: { kind: "assignValue", value: stmt.value }, span: stmt.span, parseId });
        if (stmt.properties !== null) {
          flattenStmts(stmt.properties, path, parseId, out);
        }
      }
      break;
    }

    case "replaceProperties": {
      const path = [...parentPath, ...stmt.path];
      out.push({ path, op: { kind: "clearProperties" }, span: stmt.span, parseId });
      flattenStmts(stmt.properties, path, parseId, out);
      break;
    }

    case "updateProperties": {
      const path = [...parentPath, ...stmt.path];
      out.push({ path, op: { kind: "define" }, span: stmt.span, parseId });
      flattenStmts(stmt.properties, path, parseId, out);
      break;
    }

    case "define": {
      const path = [...parentPath, ...stmt.path];
      out.push({ path, op: { kind: stmt.deleted ? "delete" : "define" }, span: stmt.span, parseId });
      break;
    }

    case "clearAll": {
      out.push({ path: [...parentPath], op: { kind: "clearAll" }, span: stmt.span, parseId });
      break;
    }
  }
}

// ─── Phase 2: Chunk ──────────────────────────────────────────────────

export interface ChunkResult {
  splits: number[];
  deps: number[][];
}

/**
 * Scan transformers for forward references, produce chunk boundaries and
 * a dependency graph. Pure — no tree mutation.
 *
 * A forward reference is a link/clone whose target path has not been
 * written by any preceding transformer. Each forward ref becomes a
 * singleton chunk; a cut is also placed at the last write to the
 * target so that the dependency graph can order chunks correctly.
 */
export function chunk(transformers: Transformer[]): ChunkResult {
  const n = transformers.length;
  if (n === 0) {
    return { splits: [0], deps: [] };
  }

  // Last-write index per serialized path
  const lastWrite = new Map<string, number>();
  for (let i = 0; i < n; i++) {
    lastWrite.set(serializePath(transformers[i].path), i);
  }

  // Scan for forward references, collect cut points
  const cutSet = new Set<number>([0, n]);
  const written = new Set<string>();
  const forwardRefs: { refIdx: number; targetKey: string }[] = [];

  for (let i = 0; i < n; i++) {
    const t = transformers[i];
    const key = serializePath(t.path);

    if (t.op.kind === "link" || t.op.kind === "clone") {
      const targetKey = resolveRefTargetKey(t.path, t.op.ups, t.op.refPath);
      if (!hasWrittenToTarget(written, targetKey)) {
        forwardRefs.push({ refIdx: i, targetKey });
        cutSet.add(i);       // before ref
        cutSet.add(i + 1);   // after ref (singleton)
        const targetIdx = findLastWriteToTarget(lastWrite, targetKey);
        if (targetIdx > i) {
          cutSet.add(targetIdx); // at last write to target
        }
      }
    }

    written.add(key);
  }

  const splits = Array.from(cutSet).sort((a, b) => a - b);
  const numChunks = splits.length - 1;

  // Build dependency graph
  const depSets: Set<number>[] = Array.from({ length: numChunks }, () => new Set());

  for (const { refIdx, targetKey } of forwardRefs) {
    const refChunk = chunkOf(splits, refIdx);

    // Reference edge: ref chunk depends on chunk containing last write to target
    const targetIdx = findLastWriteToTarget(lastWrite, targetKey);
    if (targetIdx !== -1) {
      const targetChunk = chunkOf(splits, targetIdx);
      if (targetChunk !== refChunk) {
        depSets[refChunk].add(targetChunk);
      }
    }

    // Write-after-reference edges: later chunks writing to the ref's
    // output path depend on the ref chunk
    const refPathKey = serializePath(transformers[refIdx].path);
    for (let i = refIdx + 1; i < n; i++) {
      const wKey = serializePath(transformers[i].path);
      if (wKey === refPathKey || wKey.startsWith(refPathKey + "\0")) {
        const writerChunk = chunkOf(splits, i);
        if (writerChunk !== refChunk) {
          depSets[writerChunk].add(refChunk);
        }
      }
    }
  }

  const deps = depSets.map(s => Array.from(s));
  return { splits, deps };
}

/** Join path segments with "\0" for use as a map key. Null byte avoids collisions with dots in backtick-quoted property names. */
function serializePath(path: string[]): string {
  return path.join("\0");
}

/** True if the target path or any descendant has been written. */
function hasWrittenToTarget(written: Set<string>, targetKey: string): boolean {
  for (const key of written) {
    if (key === targetKey || key.startsWith(targetKey + "\0")) {
      return true;
    }
  }
  return false;
}

/**
 * Find the maximum transformer index that writes to the target,
 * any descendant of the target, or any ancestor of the target.
 * Returns -1 if no match.
 */
function findLastWriteToTarget(lastWrite: Map<string, number>, targetKey: string): number {
  let maxIdx = -1;
  for (const [key, idx] of lastWrite) {
    if (key === targetKey || key.startsWith(targetKey + "\0") || targetKey.startsWith(key + "\0")) {
      maxIdx = Math.max(maxIdx, idx);
    }
  }
  return maxIdx;
}

/** Convert a reference's ups + refPath into an absolute serialized path. */
function resolveRefTargetKey(stmtPath: string[], ups: number, refPath: RefPathSegment[]): string {
  const parts: string[] = [];
  if (ups > 0) {
    const contextLen = Math.max(0, stmtPath.length - 1 - ups);
    for (let i = 0; i < contextLen; i++) {
      parts.push(stmtPath[i]);
    }
  }
  for (const seg of refPath) {
    if (seg.kind === "name") {
      parts.push(seg.name);
    } else {
      parts.push(`[${seg.index}]`);
    }
  }
  return parts.join("\0");
}

/** Binary search: find the chunk index containing transformer at idx. */
function chunkOf(splits: number[], idx: number): number {
  let lo = 0, hi = splits.length - 2;
  while (lo < hi) {
    const mid = (lo + hi + 1) >> 1;
    if (splits[mid] <= idx) lo = mid;
    else hi = mid - 1;
  }
  return lo;
}

// ─── Phase 3: Topo-sort ──────────────────────────────────────────────

export interface TopoSortResult {
  order: number[];
  cycles: number[];
}

/**
 * Kahn's algorithm. Returns chunk indices in dependency-respecting order.
 * Uses a FIFO queue so independent chunks preserve their original
 * (source) order — this is critical for correctness.
 *
 * Any vertices left unprocessed after the algorithm are involved in
 * cycles and returned in the `cycles` array.
 */
export function topoSort(deps: number[][]): TopoSortResult {
  const n = deps.length;
  const indegree = new Array<number>(n).fill(0);

  // Build reverse adjacency (dependents) and compute indegrees
  const dependents: number[][] = Array.from({ length: n }, () => []);
  for (let i = 0; i < n; i++) {
    for (const dep of deps[i]) {
      dependents[dep].push(i);
      indegree[i]++;
    }
  }

  // Seed queue with indegree-0 vertices in index order
  const queue: number[] = [];
  for (let i = 0; i < n; i++) {
    if (indegree[i] === 0) queue.push(i);
  }

  const order: number[] = [];
  let head = 0;
  while (head < queue.length) {
    const v = queue[head++];
    order.push(v);
    for (const w of dependents[v]) {
      if (--indegree[w] === 0) queue.push(w);
    }
  }

  // Anything not in order is in a cycle
  const cycles: number[] = [];
  if (order.length < n) {
    const inOrder = new Set(order);
    for (let i = 0; i < n; i++) {
      if (!inOrder.has(i)) cycles.push(i);
    }
  }

  return { order, cycles };
}

// ─── Phase 4: Execute ────────────────────────────────────────────────

/** A clone that failed to resolve, tracked for circular dependency detection. */
interface FailedClone {
  sourcePath: string;
  targetPath: string;
  errorIndex: number;
}

/**
 * Apply transformers to the tree in chunk-sorted order.
 * All dependencies are satisfied by the time each chunk executes.
 */
export function executeChunked(
  transformers: Transformer[],
  splits: number[],
  order: number[],
  root: MOTLYDataNode,
  options: MOTLYSessionOptions,
): MOTLYError[] {
  const errors: MOTLYError[] = [];
  const failedClones: FailedClone[] = [];
  for (const chunkIdx of order) {
    const start = splits[chunkIdx];
    const end = splits[chunkIdx + 1];
    for (let i = start; i < end; i++) {
      applyTransformer(transformers[i], root, options, errors, failedClones);
    }
  }
  replaceCircularCloneErrors(errors, failedClones);
  return errors;
}

/**
 * Detect circular dependencies among failed clones and replace their
 * unresolved-clone-reference errors with a single circular-reference error.
 *
 * A cycle exists when failed clone A targets B and failed clone B
 * (directly or transitively) targets A.
 */
function replaceCircularCloneErrors(errors: MOTLYError[], failedClones: FailedClone[]): void {
  if (failedClones.length < 2) return;

  // Build a map: sourcePath → FailedClone
  const bySource = new Map<string, FailedClone>();
  for (const fc of failedClones) {
    bySource.set(fc.sourcePath, fc);
  }

  // Find cycles: follow source→target chains
  const inCycle = new Set<number>(); // error indices involved in cycles
  const visited = new Set<string>();

  for (const fc of failedClones) {
    if (visited.has(fc.sourcePath)) continue;

    const chain: FailedClone[] = [];
    const chainSet = new Set<string>();
    let current: FailedClone | undefined = fc;

    while (current && !chainSet.has(current.sourcePath)) {
      if (visited.has(current.sourcePath)) break;
      chain.push(current);
      chainSet.add(current.sourcePath);
      current = bySource.get(current.targetPath);
    }

    if (current && chainSet.has(current.sourcePath)) {
      // Found a cycle — collect members starting from where the cycle begins
      const cycleStart = current.sourcePath;
      const cycleMembers: FailedClone[] = [];
      let collecting = false;
      for (const member of chain) {
        if (member.sourcePath === cycleStart) collecting = true;
        if (collecting) cycleMembers.push(member);
      }

      // Build descriptive message: a → $b → $a
      const displayPath = (p: string) => p.replace(/\0/g, ".");
      const parts = cycleMembers.map(m => displayPath(m.sourcePath));
      const refs = cycleMembers.map(m => `$${displayPath(m.targetPath)}`);
      let desc = parts[0];
      for (let i = 0; i < refs.length; i++) {
        desc += ` clones ${refs[i]}`;
        if (i < refs.length - 1) desc += `, ${parts[i + 1]}`;
      }

      for (const member of cycleMembers) {
        inCycle.add(member.errorIndex);
      }

      // Replace the first cycle member's error with the circular-reference error
      const firstIdx = cycleMembers[0].errorIndex;
      const zero = { line: 0, column: 0, offset: 0 };
      errors[firstIdx] = {
        code: "circular-reference",
        message: `Circular clone dependency: ${desc}`,
        begin: zero,
        end: zero,
      };
    }

    for (const member of chain) {
      visited.add(member.sourcePath);
    }
  }

  // Remove the other cycle errors (iterate backward to preserve indices)
  const toRemove = Array.from(inCycle).sort((a, b) => b - a);
  for (const idx of toRemove) {
    if (errors[idx].code !== "circular-reference") {
      errors.splice(idx, 1);
    }
  }
}

function applyTransformer(
  t: Transformer,
  root: MOTLYDataNode,
  options: MOTLYSessionOptions,
  errors: MOTLYError[],
  failedClones: FailedClone[],
): void {
  const ctx: ExecContext = { parseId: t.parseId, options };

  switch (t.op.kind) {
    case "setValue":
      applySetValue(t.path, t.op.value, root, ctx, t.span, errors);
      break;
    case "assignValue":
      applyAssignValue(t.path, t.op.value, root, ctx, t.span, errors);
      break;
    case "clearProperties":
      applyClearProperties(t.path, root, ctx, t.span, errors);
      break;
    case "clearAll":
      applyClearAll(t.path, root, ctx, t.span, errors);
      break;
    case "define":
      applyDefine(t.path, root, ctx, t.span, errors);
      break;
    case "delete":
      applyDelete(t.path, root, ctx, t.span, errors);
      break;
    case "link":
      applyLink(t.path, t.op.ups, t.op.refPath, root, ctx, t.span, errors);
      break;
    case "clone":
      applyClone(t.path, t.op.ups, t.op.refPath, root, ctx, t.span, errors, failedClones);
      break;
  }
}

/** Set the value on a node, preserving existing properties (merge semantics). */
function applySetValue(
  path: string[], value: TagValue, root: MOTLYDataNode,
  ctx: ExecContext, span: Span, errors: MOTLYError[],
): void {
  if (path.length === 0) {
    setEqSlot(root, value, ctx, errors);
    return;
  }
  const result = buildAccessPath(root, path, ctx, span, errors);
  if (!result) return;
  const [writeKey, parent] = result;
  const props = getOrCreateProperties(parent);
  if (props[writeKey] === undefined) {
    props[writeKey] = {};
  }
  const target = ensureDataNode(props, writeKey);
  setFirstLocation(target, ctx, span);
  setEqSlot(target, value, ctx, errors);
}

/** Replace a node entirely — fresh node with new value and location (:= semantics). */
function applyAssignValue(
  path: string[], value: TagValue, root: MOTLYDataNode,
  ctx: ExecContext, span: Span, errors: MOTLYError[],
): void {
  if (path.length === 0) {
    delete root.properties;
    root.location = makeLocation(ctx, span);
    setEqSlot(root, value, ctx, errors);
    return;
  }
  const result = buildAccessPath(root, path, ctx, span, errors);
  if (!result) return;
  const [writeKey, parent] = result;
  const fresh: MOTLYDataNode = {};
  fresh.location = makeLocation(ctx, span);
  setEqSlot(fresh, value, ctx, errors);
  getOrCreateProperties(parent)[writeKey] = fresh;
}

/** Clear properties on a node, preserving its value and location. */
function applyClearProperties(
  path: string[], root: MOTLYDataNode,
  ctx: ExecContext, span: Span, errors: MOTLYError[],
): void {
  if (path.length === 0) {
    delete root.properties;
    return;
  }
  const result = buildAccessPath(root, path, ctx, span, errors);
  if (!result) return;
  const [writeKey, parent] = result;
  const props = getOrCreateProperties(parent);
  const existing = props[writeKey];
  if (existing !== undefined && !isRef(existing)) {
    delete existing.properties;
  } else {
    // Ref or missing → replace with fresh empty node
    const fresh: MOTLYDataNode = {};
    fresh.location = makeLocation(ctx, span);
    props[writeKey] = fresh;
  }
}

/** Clear both value and properties (handles `***`). */
function applyClearAll(
  path: string[], root: MOTLYDataNode,
  ctx: ExecContext, span: Span, errors: MOTLYError[],
): void {
  if (path.length === 0) {
    delete root.eq;
    root.properties = {};
    return;
  }
  const result = buildAccessPath(root, path, ctx, span, errors);
  if (!result) return;
  const [writeKey, parent] = result;
  const props = getOrCreateProperties(parent);
  const existing = props[writeKey];
  if (existing !== undefined && !isRef(existing)) {
    delete existing.eq;
    existing.properties = {};
  } else {
    props[writeKey] = {};
  }
}

/** Get-or-create a node (no-op if it already exists). */
function applyDefine(
  path: string[], root: MOTLYDataNode,
  ctx: ExecContext, span: Span, errors: MOTLYError[],
): void {
  const result = buildAccessPath(root, path, ctx, span, errors);
  if (!result) return;
  const [writeKey, parent] = result;
  const props = getOrCreateProperties(parent);
  if (props[writeKey] === undefined) {
    const node: MOTLYDataNode = {};
    node.location = makeLocation(ctx, span);
    props[writeKey] = node;
  }
}

/** Create a deleted-marker node. */
function applyDelete(
  path: string[], root: MOTLYDataNode,
  ctx: ExecContext, span: Span, errors: MOTLYError[],
): void {
  const result = buildAccessPath(root, path, ctx, span, errors);
  if (!result) return;
  const [writeKey, parent] = result;
  const delNode: MOTLYDataNode = { deleted: true };
  delNode.location = makeLocation(ctx, span);
  getOrCreateProperties(parent)[writeKey] = delNode;
}

/** Insert a link reference (read-only alias). */
function applyLink(
  path: string[], ups: number, refPath: RefPathSegment[],
  root: MOTLYDataNode, ctx: ExecContext, span: Span, errors: MOTLYError[],
): void {
  if (ctx.options.disableReferences) {
    errors.push({
      code: "ref-not-allowed",
      message: "References are not allowed in this session. Use := for cloning.",
      begin: span.begin,
      end: span.end,
    });
  }
  const result = buildAccessPath(root, path, ctx, span, errors);
  if (!result) return;
  const [writeKey, parent] = result;
  getOrCreateProperties(parent)[writeKey] = makeRef(ups, refPath);
}

/**
 * Resolve a reference target, deep-copy it, and place the clone at path.
 * Uses the transformer's absolute path for context navigation — this
 * allows relative references to resolve correctly from any depth.
 */
function applyClone(
  path: string[], ups: number, refPath: RefPathSegment[],
  root: MOTLYDataNode, ctx: ExecContext, span: Span, errors: MOTLYError[],
  failedClones: FailedClone[],
): void {
  // Create intermediate nodes first so resolveAndClone can navigate the context
  const result = buildAccessPath(root, path, ctx, span, errors);
  if (!result) return;
  const [writeKey, parent] = result;

  let cloned: MOTLYDataNode;
  try {
    cloned = resolveAndClone(root, path, ups, refPath);
  } catch (err) {
    if (err && typeof err === "object" && "code" in err) {
      const errorIndex = errors.length;
      errors.push(err as MOTLYError);
      failedClones.push({
        sourcePath: serializePath(path),
        targetPath: resolveRefTargetKey(path, ups, refPath),
        errorIndex,
      });
    }
    return;
  }

  sanitizeClonedRefs(cloned, 0, errors);
  cloned.location = makeLocation(ctx, span);
  getOrCreateProperties(parent)[writeKey] = cloned;
}

// ─── Legacy statement executor (used for array element properties) ───

function executeStatement(stmt: Statement, node: MOTLYDataNode, errors: MOTLYError[], ctx: ExecContext): void {
  switch (stmt.kind) {
    case "setEq":
      executeSetEq(node, stmt.path, stmt.value, stmt.properties, errors, ctx, stmt.span);
      break;
    case "assignBoth":
      executeAssignBoth(node, stmt.path, stmt.value, stmt.properties, errors, ctx, stmt.span);
      break;
    case "replaceProperties":
      executeReplaceProperties(node, stmt.path, stmt.properties, errors, ctx, stmt.span);
      break;
    case "updateProperties":
      executeUpdateProperties(node, stmt.path, stmt.properties, errors, ctx, stmt.span);
      break;
    case "define":
      executeDefine(node, stmt.path, stmt.deleted, errors, ctx, stmt.span);
      break;
    case "clearAll":
      delete node.eq;
      node.properties = {};
      break;
  }
}

/** Build a MOTLYLocation from ctx and span. */
function makeLocation(ctx: ExecContext, span: Span): MOTLYLocation {
  return { parseId: ctx.parseId, begin: span.begin, end: span.end };
}

/** Set location on a node only if it doesn't already have one (first-appearance rule). */
function setFirstLocation(node: MOTLYDataNode, ctx: ExecContext, span: Span): void {
  if (!node.location) {
    node.location = makeLocation(ctx, span);
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
  node: MOTLYDataNode,
  path: string[],
  value: TagValue,
  properties: Statement[] | null,
  errors: MOTLYError[],
  ctx: ExecContext,
  span: Span
): void {
  // Special case: reference value → insert as MOTLYRef
  if (value.kind === "scalar" && value.value.kind === "reference") {
    if (ctx.options.disableReferences) {
      errors.push({
        code: "ref-not-allowed",
        message: "References are not allowed in this session. Use := for cloning.",
        begin: span.begin,
        end: span.end,
      });
      // Still create the ref in the tree (disableReferences is a diagnostic, not enforcement)
    }
    if (properties !== null) {
      const zero = { line: 0, column: 0, offset: 0 };
      errors.push({
        code: "ref-with-properties",
        message: "Cannot add properties to a reference. Did you mean := (clone)?",
        begin: zero,
        end: zero,
      });
    }
    const result = buildAccessPath(node, path, ctx, span, errors);
    if (!result) return;
    const [writeKey, parent] = result;
    getOrCreateProperties(parent)[writeKey] = makeRef(value.value.ups, value.value.path);
    return;
  }

  const result = buildAccessPath(node, path, ctx, span, errors);
  if (!result) return;
  const [writeKey, parent] = result;
  const props = getOrCreateProperties(parent);

  // Get or create target (preserves existing node and its properties)
  let targetPv = props[writeKey];
  if (targetPv === undefined) {
    targetPv = {};
    props[writeKey] = targetPv;
  }

  // If it was a ref, convert to empty node
  const target = ensureDataNode(props, writeKey);

  // Set location on first appearance
  setFirstLocation(target, ctx, span);

  // Set the value slot
  setEqSlot(target, value, ctx, errors);

  // If properties block present, MERGE them
  if (properties !== null) {
    for (const s of properties) {
      executeStatement(s, target, errors, ctx);
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
  node: MOTLYDataNode,
  path: string[],
  value: TagValue,
  properties: Statement[] | null,
  errors: MOTLYError[],
  ctx: ExecContext,
  span: Span
): void {
  if (
    value.kind === "scalar" &&
    value.value.kind === "reference"
  ) {
    // CLONE semantics: resolve + deep copy the target
    let cloned: MOTLYDataNode;
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
        executeStatement(s, cloned, errors, ctx);
      }
    }
    // := always sets a new location (it's a full replacement)
    cloned.location = makeLocation(ctx, span);
    const result = buildAccessPath(node, path, ctx, span, errors);
    if (!result) return;
    const [writeKey, parent] = result;
    getOrCreateProperties(parent)[writeKey] = cloned;
  } else {
    // Literal value: create fresh node (replaces everything)
    const fresh: MOTLYDataNode = {};
    // := always sets a new location
    fresh.location = makeLocation(ctx, span);
    setEqSlot(fresh, value, ctx, errors);
    if (properties !== null) {
      for (const s of properties) {
        executeStatement(s, fresh, errors, ctx);
      }
    }
    const result = buildAccessPath(node, path, ctx, span, errors);
    if (!result) return;
    const [writeKey, parent] = result;
    getOrCreateProperties(parent)[writeKey] = fresh;
  }
}

/**
 * `name: { props }` — preserve existing value, replace properties.
 */
function executeReplaceProperties(
  node: MOTLYDataNode,
  path: string[],
  properties: Statement[],
  errors: MOTLYError[],
  ctx: ExecContext,
  span: Span
): void {
  const pathResult = buildAccessPath(node, path, ctx, span, errors);
  if (!pathResult) return;
  const [writeKey, parent] = pathResult;

  const fresh: MOTLYDataNode = {};

  // Always preserve the existing value (if it's a node, not a ref)
  const parentProps = getOrCreateProperties(parent);
  const existing = parentProps[writeKey];
  if (existing !== undefined && !isRef(existing)) {
    fresh.eq = existing.eq;
    // Preserve the existing location (first-appearance rule)
    if (existing.location) {
      fresh.location = existing.location;
    }
  }

  // If no existing location, this is the first appearance
  if (!fresh.location) {
    fresh.location = makeLocation(ctx, span);
  }

  for (const stmt of properties) {
    executeStatement(stmt, fresh, errors, ctx);
  }

  parentProps[writeKey] = fresh;
}

function executeUpdateProperties(
  node: MOTLYDataNode,
  path: string[],
  properties: Statement[],
  errors: MOTLYError[],
  ctx: ExecContext,
  span: Span
): void {
  const pathResult = buildAccessPath(node, path, ctx, span, errors);
  if (!pathResult) return;
  const [writeKey, parent] = pathResult;

  const props = getOrCreateProperties(parent);

  // Cannot merge into a link reference
  if (props[writeKey] !== undefined && isRef(props[writeKey])) {
    errors.push({
      code: "write-through-link",
      message: `Cannot write through link reference "${writeKey}"`,
      begin: span.begin,
      end: span.end,
    });
    return;
  }

  // Get or create the target node (merging semantics - preserves existing)
  if (props[writeKey] === undefined) {
    props[writeKey] = {};
  }

  const target = ensureDataNode(props, writeKey);

  // Set location on first appearance
  setFirstLocation(target, ctx, span);

  for (const stmt of properties) {
    executeStatement(stmt, target, errors, ctx);
  }
}

function executeDefine(
  node: MOTLYDataNode,
  path: string[],
  deleted: boolean,
  errors: MOTLYError[],
  ctx: ExecContext,
  span: Span
): void {
  const pathResult = buildAccessPath(node, path, ctx, span, errors);
  if (!pathResult) return;
  const [writeKey, parent] = pathResult;
  const props = getOrCreateProperties(parent);
  if (deleted) {
    const delNode: MOTLYDataNode = { deleted: true };
    delNode.location = makeLocation(ctx, span);
    props[writeKey] = delNode;
  } else {
    // Get-or-create: if node already exists, leave it alone
    if (props[writeKey] === undefined) {
      const newNode: MOTLYDataNode = {};
      newNode.location = makeLocation(ctx, span);
      props[writeKey] = newNode;
    }
  }
}

/** Navigate to the parent of the final path segment, creating intermediate nodes.
 *  Returns null if the path traverses a link reference (write-through-link error). */
function buildAccessPath(
  node: MOTLYDataNode,
  path: string[],
  ctx: ExecContext,
  span: Span,
  errors: MOTLYError[]
): [string, MOTLYDataNode] | null {
  let current = node;

  for (let i = 0; i < path.length - 1; i++) {
    const segment = path[i];
    const props = getOrCreateProperties(current);

    if (props[segment] !== undefined && isRef(props[segment])) {
      errors.push({
        code: "write-through-link",
        message: `Cannot write through link reference "${segment}"`,
        begin: span.begin,
        end: span.end,
      });
      return null;
    }

    if (props[segment] === undefined) {
      const intermediate: MOTLYDataNode = {};
      intermediate.location = makeLocation(ctx, span);
      props[segment] = intermediate;
    }

    current = ensureDataNode(props, segment);
    // Set location on intermediate nodes (first-appearance)
    setFirstLocation(current, ctx, span);
  }

  return [path[path.length - 1], current];
}

/** Set the eq slot on a target node from a TagValue. */
function setEqSlot(target: MOTLYDataNode, value: TagValue, ctx: ExecContext, errors: MOTLYError[]): void {
  if (value.kind === "array") {
    target.eq = resolveArray(value.elements, errors, ctx);
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

/** Resolve an array of AST elements to MOTLYNodes. */
function resolveArray(elements: ArrayElement[], errors: MOTLYError[], ctx: ExecContext): MOTLYNode[] {
  return elements.map((el) => resolveArrayElement(el, errors, ctx));
}

function resolveArrayElement(el: ArrayElement, errors: MOTLYError[], ctx: ExecContext): MOTLYNode {
  // Check if the element value is a reference → becomes MOTLYRef
  if (el.value !== null && el.value.kind === "scalar" && el.value.value.kind === "reference") {
    if (ctx.options.disableReferences) {
      errors.push({
        code: "ref-not-allowed",
        message: "References are not allowed in this session. Use := for cloning.",
        begin: el.span.begin,
        end: el.span.end,
      });
      // Still create the ref (disableReferences is a diagnostic, not enforcement)
    }
    if (el.properties !== null) {
      const zero = { line: 0, column: 0, offset: 0 };
      errors.push({
        code: "ref-with-properties",
        message: "Cannot add properties to a reference. Did you mean := (clone)?",
        begin: zero,
        end: zero,
      });
    }
    return makeRef(el.value.value.ups, el.value.value.path);
  }

  const node: MOTLYDataNode = {};
  node.location = makeLocation(ctx, el.span);

  if (el.value !== null) {
    setEqSlot(node, el.value, ctx, errors);
  }

  if (el.properties !== null) {
    for (const stmt of el.properties) {
      executeStatement(stmt, node, errors, ctx);
    }
  }

  return node;
}

/** Build a structured MOTLYRef from parsed AST reference data. */
function makeRef(ups: number, path: RefPathSegment[]): MOTLYRef {
  return {
    linkTo: path.map((seg) => seg.kind === "name" ? seg.name : seg.index),
    linkUps: ups,
  };
}

/** Format ups + AST path for error messages (used before ref is constructed). */
function formatRefPath(ups: number, path: RefPathSegment[]): string {
  let s = "$";
  for (let i = 0; i < ups; i++) s += "^";
  let first = true;
  for (const seg of path) {
    if (seg.kind === "name") {
      if (!first || ups > 0) s += ".";
      s += seg.name;
      first = false;
    } else {
      s += `[${seg.index}]`;
    }
  }
  return s;
}

/** Resolve a reference path in the tree and return a deep clone.
 *  Follows link references when encountered along the path or at the target. */
function resolveAndClone(
  root: MOTLYDataNode,
  stmtPath: string[],
  ups: number,
  refPath: RefPathSegment[]
): MOTLYDataNode {
  const refStr = formatRefPath(ups, refPath);
  let start: MOTLYDataNode;

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
        // Try to follow the link
        const resolved = resolveRefFromRoot(root, pv);
        if (!resolved) {
          throw cloneError(`Clone reference ${refStr} could not be resolved: path segment "${stmtPath[i]}" is an unresolvable link reference`);
        }
        start = resolved;
        continue;
      }
      start = pv;
    }
  }

  // Follow refPath segments
  let current: MOTLYDataNode = start;
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
        const resolved = resolveRefFromRoot(root, pv);
        if (!resolved) {
          throw cloneError(`Clone reference ${refStr} could not be resolved: property "${seg.name}" is an unresolvable link reference`);
        }
        current = resolved;
        continue;
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
        const resolved = resolveRefFromRoot(root, elemPv);
        if (!resolved) {
          throw cloneError(`Clone reference ${refStr} could not be resolved: index [${seg.index}] is an unresolvable link reference`);
        }
        current = resolved;
        continue;
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
 * Follow a MOTLYRef from root to its concrete MOTLYDataNode target.
 * Only handles absolute refs (linkUps === 0). Returns null on failure or cycle.
 */
function resolveRefFromRoot(
  root: MOTLYDataNode,
  ref: MOTLYRef,
  visited?: Set<string>
): MOTLYDataNode | null {
  if (!visited) visited = new Set();
  const key = formatRef(ref);
  if (visited.has(key)) return null; // cycle
  visited.add(key);

  // Only handle absolute refs
  if (ref.linkUps > 0) return null;

  let current: MOTLYNode | undefined = root;
  for (const seg of ref.linkTo) {
    if (current === undefined) return null;
    if (isRef(current)) {
      const resolved = resolveRefFromRoot(root, current as MOTLYRef, visited);
      if (!resolved) return null;
      current = resolved;
    }

    const dataNode = current as MOTLYDataNode;
    if (typeof seg === "string") {
      if (!dataNode.properties || !(seg in dataNode.properties)) return null;
      current = dataNode.properties[seg];
    } else {
      if (!dataNode.eq || !Array.isArray(dataNode.eq)) return null;
      if (seg >= dataNode.eq.length) return null;
      current = dataNode.eq[seg];
    }
  }

  // If final result is a ref, follow it
  if (current !== undefined && isRef(current)) {
    return resolveRefFromRoot(root, current as MOTLYRef, visited);
  }

  return (current as MOTLYDataNode) ?? null;
}

/**
 * Walk a cloned subtree and null out any relative (^) references that
 * escape the clone boundary. A reference at depth D with N ups escapes
 * if N > D. Absolute references (ups=0) are left alone.
 */
function sanitizeClonedRefs(
  node: MOTLYDataNode,
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

/** Sanitize a single node within a cloned subtree (in an array context). */
function sanitizeClonedPv(
  arr: MOTLYNode[],
  index: number,
  depth: number,
  errors: MOTLYError[]
): void {
  const pv = arr[index];
  if (isRef(pv)) {
    if (pv.linkUps > 0 && pv.linkUps > depth) {
      const zero = { line: 0, column: 0, offset: 0 };
      errors.push({
        code: "clone-reference-out-of-scope",
        message: `Cloned reference "${formatRef(pv)}" escapes the clone boundary (${pv.linkUps} level(s) up from depth ${depth})`,
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

/** Sanitize a single node within a cloned subtree (in a properties context). */
function sanitizeClonedPvInProps(
  props: Record<string, MOTLYNode>,
  key: string,
  depth: number,
  errors: MOTLYError[]
): void {
  const pv = props[key];
  if (isRef(pv)) {
    if (pv.linkUps > 0 && pv.linkUps > depth) {
      const zero = { line: 0, column: 0, offset: 0 };
      errors.push({
        code: "clone-reference-out-of-scope",
        message: `Cloned reference "${formatRef(pv)}" escapes the clone boundary (${pv.linkUps} level(s) up from depth ${depth})`,
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

/** Get or create the properties object on a MOTLYDataNode. */
function getOrCreateProperties(
  node: MOTLYDataNode
): Record<string, MOTLYNode> {
  if (!node.properties) {
    node.properties = {};
  }
  return node.properties;
}

/**
 * Ensure the node at props[key] is a MOTLYDataNode (not a MOTLYRef).
 * If it's a ref, replace it with an empty node.
 * Returns a mutable reference to the data node.
 */
function ensureDataNode(
  props: Record<string, MOTLYNode>,
  key: string
): MOTLYDataNode {
  const pv = props[key];
  if (isRef(pv)) {
    const node: MOTLYDataNode = {};
    props[key] = node;
    return node;
  }
  return pv;
}
