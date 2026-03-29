use crate::ast::*;
use crate::error::{MOTLYError, Position};
use crate::tree::*;
use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};

// ── Types ──────────────────────────────────────────────────────────

/// Session-level options that control parsing behavior.
#[derive(Debug, Clone, Default)]
pub struct SessionOptions {
    /// When true, `$`-references produce errors. `:= $ref` (clone) is always allowed.
    pub disable_references: bool,
}

/// Per-parse execution context, combining the parse ID with session options.
#[derive(Debug)]
pub struct ExecContext {
    pub parse_id: u32,
    pub options: SessionOptions,
}

/// A flattened operation with absolute path — output of Phase 1.
#[derive(Debug)]
pub struct Transformer {
    pub path: Vec<String>,
    pub op: TransformerOp,
    pub span: Span,
    pub parse_id: u32,
}

/// The operation a transformer performs.
#[derive(Debug)]
pub enum TransformerOp {
    /// Merge semantics: set value, preserve existing properties, first-appearance location.
    SetValue { value: TagValue },
    /// Replace semantics: create fresh node with new value, new location (:= with literal).
    AssignValue { value: TagValue },
    /// Delete properties, preserve value and location.
    ClearProperties,
    /// Clear both value and properties (`***`).
    ClearAll,
    /// Get-or-create a node (no-op if it already exists).
    Define,
    /// Create a deleted-marker node.
    Delete,
    /// Insert a link reference (read-only alias).
    Link { ups: usize, ref_path: Vec<RefPathSegment> },
    /// Resolve + deep-copy the target.
    Clone { ups: usize, ref_path: Vec<RefPathSegment> },
}

/// Output of Phase 2: chunk boundaries and dependency graph.
pub struct ChunkResult {
    pub splits: Vec<usize>,
    pub deps: Vec<Vec<usize>>,
}

/// Output of Phase 3: topologically sorted chunk order.
pub struct TopoSortResult {
    pub order: Vec<usize>,
    pub cycles: Vec<usize>,
}

/// A clone that failed to resolve, tracked for circular dependency detection.
struct FailedClone {
    source_path: String,
    target_path: String,
    error_index: usize,
}

// ── Phase 1: Flatten ───────────────────────────────────────────────

/// Walk the AST depth-first, accumulating absolute paths.
/// Emit one Transformer per leaf operation. Pure — no tree mutation.
pub fn flatten(statements: &[Statement], ctx: &ExecContext) -> Vec<Transformer> {
    let mut out = Vec::new();
    flatten_stmts(statements, &[], ctx.parse_id, &mut out);
    out
}

fn flatten_stmts(
    stmts: &[Statement],
    parent_path: &[String],
    parse_id: u32,
    out: &mut Vec<Transformer>,
) {
    for stmt in stmts {
        flatten_stmt(stmt, parent_path, parse_id, out);
    }
}

fn flatten_stmt(
    stmt: &Statement,
    parent_path: &[String],
    parse_id: u32,
    out: &mut Vec<Transformer>,
) {
    match stmt {
        Statement::SetEq { path, value, properties, span } => {
            let full_path: Vec<String> = parent_path.iter().chain(path.iter()).cloned().collect();
            if let TagValue::Scalar(ScalarValue::Reference { ups, path: ref_path }) = value {
                out.push(Transformer {
                    path: full_path,
                    op: TransformerOp::Link { ups: *ups, ref_path: ref_path.clone() },
                    span: *span,
                    parse_id,
                });
            } else {
                out.push(Transformer {
                    path: full_path.clone(),
                    op: TransformerOp::SetValue { value: value.clone() },
                    span: *span,
                    parse_id,
                });
                if let Some(prop_stmts) = properties {
                    flatten_stmts(prop_stmts, &full_path, parse_id, out);
                }
            }
        }

        Statement::AssignBoth { path, value, properties, span } => {
            let full_path: Vec<String> = parent_path.iter().chain(path.iter()).cloned().collect();
            if let TagValue::Scalar(ScalarValue::Reference { ups, path: ref_path }) = value {
                out.push(Transformer {
                    path: full_path.clone(),
                    op: TransformerOp::Clone { ups: *ups, ref_path: ref_path.clone() },
                    span: *span,
                    parse_id,
                });
                if let Some(prop_stmts) = properties {
                    out.push(Transformer {
                        path: full_path.clone(),
                        op: TransformerOp::ClearProperties,
                        span: *span,
                        parse_id,
                    });
                    flatten_stmts(prop_stmts, &full_path, parse_id, out);
                }
            } else {
                out.push(Transformer {
                    path: full_path.clone(),
                    op: TransformerOp::AssignValue { value: value.clone() },
                    span: *span,
                    parse_id,
                });
                if let Some(prop_stmts) = properties {
                    flatten_stmts(prop_stmts, &full_path, parse_id, out);
                }
            }
        }

        Statement::ReplaceProperties { path, properties, span } => {
            let full_path: Vec<String> = parent_path.iter().chain(path.iter()).cloned().collect();
            out.push(Transformer {
                path: full_path.clone(),
                op: TransformerOp::ClearProperties,
                span: *span,
                parse_id,
            });
            flatten_stmts(properties, &full_path, parse_id, out);
        }

        Statement::UpdateProperties { path, properties, span } => {
            let full_path: Vec<String> = parent_path.iter().chain(path.iter()).cloned().collect();
            out.push(Transformer {
                path: full_path.clone(),
                op: TransformerOp::Define,
                span: *span,
                parse_id,
            });
            flatten_stmts(properties, &full_path, parse_id, out);
        }

        Statement::Define { path, deleted, span } => {
            let full_path: Vec<String> = parent_path.iter().chain(path.iter()).cloned().collect();
            out.push(Transformer {
                path: full_path,
                op: if *deleted { TransformerOp::Delete } else { TransformerOp::Define },
                span: *span,
                parse_id,
            });
        }

        Statement::ClearAll { span } => {
            out.push(Transformer {
                path: parent_path.to_vec(),
                op: TransformerOp::ClearAll,
                span: *span,
                parse_id,
            });
        }
    }
}

// ── Phase 2: Chunk ─────────────────────────────────────────────────

/// Scan transformers for forward references, produce chunk boundaries and
/// a dependency graph. Pure — no tree mutation.
pub fn chunk(transformers: &[Transformer]) -> ChunkResult {
    let n = transformers.len();
    if n == 0 {
        return ChunkResult { splits: vec![0], deps: Vec::new() };
    }

    // Last-write index per serialized path
    let mut last_write: HashMap<String, usize> = HashMap::new();
    for (i, t) in transformers.iter().enumerate() {
        last_write.insert(serialize_path(&t.path), i);
    }

    // Scan for forward references, collect cut points
    let mut cut_set = BTreeSet::new();
    cut_set.insert(0);
    cut_set.insert(n);
    let mut written: BTreeSet<String> = BTreeSet::new();
    let mut forward_refs: Vec<(usize, String)> = Vec::new(); // (refIdx, targetKey)

    for (i, t) in transformers.iter().enumerate() {
        let key = serialize_path(&t.path);

        match &t.op {
            TransformerOp::Link { ups, ref_path } | TransformerOp::Clone { ups, ref_path } => {
                let target_key = resolve_ref_target_key(&t.path, *ups, ref_path);
                if !has_written_to_target(&written, &target_key) {
                    forward_refs.push((i, target_key.clone()));
                    cut_set.insert(i);       // before ref
                    cut_set.insert(i + 1);   // after ref (singleton)
                    let target_idx = find_last_write_to_target(&last_write, &target_key);
                    if let Some(tidx) = target_idx {
                        if tidx > i {
                            cut_set.insert(tidx); // at last write to target
                        }
                    }
                }
            }
            _ => {}
        }

        written.insert(key);
    }

    let splits: Vec<usize> = cut_set.into_iter().collect();
    let num_chunks = splits.len() - 1;

    // Build dependency graph
    let mut dep_sets: Vec<BTreeSet<usize>> = (0..num_chunks).map(|_| BTreeSet::new()).collect();

    for (ref_idx, target_key) in &forward_refs {
        let ref_chunk = chunk_of(&splits, *ref_idx);

        // Reference edge: ref chunk depends on chunk containing last write to target
        if let Some(target_idx) = find_last_write_to_target(&last_write, target_key) {
            let target_chunk = chunk_of(&splits, target_idx);
            if target_chunk != ref_chunk {
                dep_sets[ref_chunk].insert(target_chunk);
            }
        }

        // Write-after-reference edges: later chunks writing to the ref's
        // output path depend on the ref chunk
        let ref_path_key = serialize_path(&transformers[*ref_idx].path);
        let ref_prefix = format!("{}\0", ref_path_key);
        for (i, t) in transformers.iter().enumerate().skip(*ref_idx + 1) {
            let w_key = serialize_path(&t.path);
            if w_key == ref_path_key || w_key.starts_with(&ref_prefix) {
                let writer_chunk = chunk_of(&splits, i);
                if writer_chunk != ref_chunk {
                    dep_sets[writer_chunk].insert(ref_chunk);
                }
            }
        }
    }

    let deps = dep_sets.into_iter().map(|s| s.into_iter().collect()).collect();
    ChunkResult { splits, deps }
}

/// Join path segments with "\0" for use as a map key.
fn serialize_path(path: &[String]) -> String {
    path.join("\0")
}

/// True if the target path or any descendant has been written.
fn has_written_to_target(written: &BTreeSet<String>, target_key: &str) -> bool {
    for key in written.iter() {
        if key == target_key || key.starts_with(&format!("{}\0", target_key)) {
            return true;
        }
    }
    false
}

/// Find the maximum transformer index that writes to the target,
/// any descendant of the target, or any ancestor of the target.
fn find_last_write_to_target(last_write: &HashMap<String, usize>, target_key: &str) -> Option<usize> {
    let mut max_idx: Option<usize> = None;
    let target_prefix = format!("{}\0", target_key);
    for (key, &idx) in last_write.iter() {
        if key == target_key
            || key.starts_with(&target_prefix)
            || target_key.starts_with(&format!("{}\0", key))
        {
            max_idx = Some(max_idx.unwrap_or(0).max(idx));
        }
    }
    max_idx
}

/// Convert a reference's ups + refPath into an absolute serialized path.
fn resolve_ref_target_key(stmt_path: &[String], ups: usize, ref_path: &[RefPathSegment]) -> String {
    let mut parts: Vec<String> = Vec::new();
    if ups > 0 {
        let context_len = (stmt_path.len() as isize - 1 - ups as isize).max(0) as usize;
        for seg in &stmt_path[..context_len] {
            parts.push(seg.clone());
        }
    }
    for seg in ref_path {
        match seg {
            RefPathSegment::Name(name) => parts.push(name.clone()),
            RefPathSegment::Index(idx) => parts.push(format!("[{}]", idx)),
        }
    }
    parts.join("\0")
}

/// Binary search: find the chunk index containing transformer at idx.
fn chunk_of(splits: &[usize], idx: usize) -> usize {
    let mut lo = 0usize;
    let mut hi = splits.len() - 2;
    while lo < hi {
        let mid = (lo + hi).div_ceil(2);
        if splits[mid] <= idx {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    lo
}

// ── Phase 3: Topo-sort ─────────────────────────────────────────────

/// Kahn's algorithm. Returns chunk indices in dependency-respecting order.
/// Uses a FIFO queue so independent chunks preserve their original
/// (source) order — this is critical for correctness.
pub fn topo_sort(deps: &[Vec<usize>]) -> TopoSortResult {
    let n = deps.len();
    let mut indegree = vec![0usize; n];

    // Build reverse adjacency (dependents) and compute indegrees
    let mut dependents: Vec<Vec<usize>> = (0..n).map(|_| Vec::new()).collect();
    for (i, dep_list) in deps.iter().enumerate() {
        for &dep in dep_list {
            dependents[dep].push(i);
            indegree[i] += 1;
        }
    }

    // Seed queue with indegree-0 vertices in index order
    let mut queue: VecDeque<usize> = VecDeque::new();
    for (i, &deg) in indegree.iter().enumerate() {
        if deg == 0 {
            queue.push_back(i);
        }
    }

    let mut order: Vec<usize> = Vec::new();
    while let Some(v) = queue.pop_front() {
        order.push(v);
        for &w in &dependents[v] {
            indegree[w] -= 1;
            if indegree[w] == 0 {
                queue.push_back(w);
            }
        }
    }

    // Anything not in order is in a cycle
    let mut cycles = Vec::new();
    if order.len() < n {
        let in_order: BTreeSet<usize> = order.iter().copied().collect();
        for i in 0..n {
            if !in_order.contains(&i) {
                cycles.push(i);
            }
        }
    }

    TopoSortResult { order, cycles }
}

// ── Phase 4: Execute ───────────────────────────────────────────────

/// Apply transformers to the tree in chunk-sorted order.
/// All dependencies are satisfied by the time each chunk executes.
pub fn execute_chunked(
    transformers: &[Transformer],
    splits: &[usize],
    order: &[usize],
    root: &mut MOTLYDataNode,
    options: &SessionOptions,
) -> Vec<MOTLYError> {
    let mut errors = Vec::new();
    let mut failed_clones: Vec<FailedClone> = Vec::new();

    for &chunk_idx in order {
        let start = splits[chunk_idx];
        let end = splits[chunk_idx + 1];
        for t in &transformers[start..end] {
            apply_transformer(t, root, options, &mut errors, &mut failed_clones);
        }
    }

    replace_circular_clone_errors(&mut errors, &failed_clones);
    errors
}

/// Detect circular dependencies among failed clones and replace their
/// unresolved-clone-reference errors with a single circular-reference error.
fn replace_circular_clone_errors(errors: &mut Vec<MOTLYError>, failed_clones: &[FailedClone]) {
    if failed_clones.len() < 2 {
        return;
    }

    // Build a map: sourcePath → &FailedClone
    let mut by_source: HashMap<&str, &FailedClone> = HashMap::new();
    for fc in failed_clones {
        by_source.insert(&fc.source_path, fc);
    }

    // Find cycles: follow source→target chains
    let mut in_cycle: BTreeSet<usize> = BTreeSet::new();
    let mut visited: BTreeSet<&str> = BTreeSet::new();

    for fc in failed_clones {
        if visited.contains(fc.source_path.as_str()) {
            continue;
        }

        let mut chain: Vec<&FailedClone> = Vec::new();
        let mut chain_set: BTreeSet<&str> = BTreeSet::new();
        let mut current = Some(fc);

        while let Some(cur) = current {
            if chain_set.contains(cur.source_path.as_str()) {
                break;
            }
            if visited.contains(cur.source_path.as_str()) {
                break;
            }
            chain.push(cur);
            chain_set.insert(&cur.source_path);
            current = by_source.get(cur.target_path.as_str()).copied();
        }

        if let Some(cur) = current {
            if chain_set.contains(cur.source_path.as_str()) {
                // Found a cycle — collect members starting from where the cycle begins
                let cycle_start = &cur.source_path;
                let mut cycle_members: Vec<&FailedClone> = Vec::new();
                let mut collecting = false;
                for member in &chain {
                    if member.source_path == *cycle_start {
                        collecting = true;
                    }
                    if collecting {
                        cycle_members.push(member);
                    }
                }

                // Build descriptive message
                let display_path = |p: &str| p.replace('\0', ".");
                let parts: Vec<String> = cycle_members.iter().map(|m| display_path(&m.source_path)).collect();
                let refs: Vec<String> = cycle_members.iter().map(|m| format!("${}", display_path(&m.target_path))).collect();
                let mut desc = parts[0].clone();
                for i in 0..refs.len() {
                    desc.push_str(&format!(" clones {}", refs[i]));
                    if i < refs.len() - 1 {
                        desc.push_str(&format!(", {}", parts[i + 1]));
                    }
                }

                for member in &cycle_members {
                    in_cycle.insert(member.error_index);
                }

                // Replace the first cycle member's error with the circular-reference error
                let first_idx = cycle_members[0].error_index;
                let zero = Position { line: 0, column: 0, offset: 0 };
                errors[first_idx] = MOTLYError {
                    code: "circular-reference".to_string(),
                    message: format!("Circular clone dependency: {}", desc),
                    begin: zero,
                    end: zero,
                };
            }
        }

        for member in &chain {
            visited.insert(&member.source_path);
        }
    }

    // Remove the other cycle errors (iterate backward to preserve indices)
    let mut to_remove: Vec<usize> = in_cycle.into_iter().collect();
    to_remove.sort_by(|a, b| b.cmp(a)); // reverse order
    for idx in to_remove {
        if errors[idx].code != "circular-reference" {
            errors.remove(idx);
        }
    }
}

fn apply_transformer(
    t: &Transformer,
    root: &mut MOTLYDataNode,
    options: &SessionOptions,
    errors: &mut Vec<MOTLYError>,
    failed_clones: &mut Vec<FailedClone>,
) {
    let ctx = ExecContext { parse_id: t.parse_id, options: options.clone() };

    match &t.op {
        TransformerOp::SetValue { value } =>
            apply_set_value(&t.path, value, root, &ctx, t.span, errors),
        TransformerOp::AssignValue { value } =>
            apply_assign_value(&t.path, value, root, &ctx, t.span, errors),
        TransformerOp::ClearProperties =>
            apply_clear_properties(&t.path, root, &ctx, t.span, errors),
        TransformerOp::ClearAll =>
            apply_clear_all(&t.path, root, &ctx, t.span, errors),
        TransformerOp::Define =>
            apply_define(&t.path, root, &ctx, t.span, errors),
        TransformerOp::Delete =>
            apply_delete(&t.path, root, &ctx, t.span, errors),
        TransformerOp::Link { ups, ref_path } =>
            apply_link(&t.path, *ups, ref_path, root, &ctx, t.span, errors),
        TransformerOp::Clone { ups, ref_path } =>
            apply_clone(&t.path, *ups, ref_path, root, &ctx, t.span, errors, failed_clones),
    }
}

/// Set the value on a node, preserving existing properties (merge semantics).
fn apply_set_value(
    path: &[String],
    value: &TagValue,
    root: &mut MOTLYDataNode,
    ctx: &ExecContext,
    span: Span,
    errors: &mut Vec<MOTLYError>,
) {
    if path.is_empty() {
        set_eq_slot(root, value, errors, ctx);
        return;
    }
    let result = build_access_path(root, path, ctx, span, errors);
    if result.is_none() { return; }
    let (write_key, parent) = result.unwrap();
    let props = parent.get_or_create_properties();
    let target_pv = props.entry(write_key).or_insert_with(MOTLYNode::new_data);
    let target = target_pv.ensure_data_node();
    set_first_location(target, ctx, span);
    set_eq_slot(target, value, errors, ctx);
}

/// Replace a node entirely — fresh node with new value and location (:= semantics).
fn apply_assign_value(
    path: &[String],
    value: &TagValue,
    root: &mut MOTLYDataNode,
    ctx: &ExecContext,
    span: Span,
    errors: &mut Vec<MOTLYError>,
) {
    if path.is_empty() {
        root.properties = None;
        root.location = Some(make_location(ctx, span));
        set_eq_slot(root, value, errors, ctx);
        return;
    }
    let result = build_access_path(root, path, ctx, span, errors);
    if result.is_none() { return; }
    let (write_key, parent) = result.unwrap();
    let mut fresh = MOTLYDataNode::new();
    fresh.location = Some(make_location(ctx, span));
    set_eq_slot(&mut fresh, value, errors, ctx);
    parent.get_or_create_properties().insert(write_key, MOTLYNode::Data(fresh));
}

/// Clear properties on a node, preserving its value and location.
fn apply_clear_properties(
    path: &[String],
    root: &mut MOTLYDataNode,
    ctx: &ExecContext,
    span: Span,
    errors: &mut Vec<MOTLYError>,
) {
    if path.is_empty() {
        root.properties = None;
        return;
    }
    let result = build_access_path(root, path, ctx, span, errors);
    if result.is_none() { return; }
    let (write_key, parent) = result.unwrap();
    let props = parent.get_or_create_properties();
    match props.get_mut(&write_key) {
        Some(MOTLYNode::Data(existing)) => {
            existing.properties = None;
        }
        _ => {
            // Ref or missing → replace with fresh empty node
            let mut fresh = MOTLYDataNode::new();
            fresh.location = Some(make_location(ctx, span));
            props.insert(write_key, MOTLYNode::Data(fresh));
        }
    }
}

/// Clear both value and properties (handles `***`).
fn apply_clear_all(
    path: &[String],
    root: &mut MOTLYDataNode,
    ctx: &ExecContext,
    span: Span,
    errors: &mut Vec<MOTLYError>,
) {
    if path.is_empty() {
        root.eq = None;
        root.properties = Some(BTreeMap::new());
        return;
    }
    let result = build_access_path(root, path, ctx, span, errors);
    if result.is_none() { return; }
    let (write_key, parent) = result.unwrap();
    let props = parent.get_or_create_properties();
    match props.get_mut(&write_key) {
        Some(MOTLYNode::Data(existing)) => {
            existing.eq = None;
            existing.properties = Some(BTreeMap::new());
        }
        _ => {
            let mut node = MOTLYDataNode::new();
            node.properties = Some(BTreeMap::new());
            props.insert(write_key, MOTLYNode::Data(node));
        }
    }
}

/// Get-or-create a node (no-op if it already exists).
fn apply_define(
    path: &[String],
    root: &mut MOTLYDataNode,
    ctx: &ExecContext,
    span: Span,
    errors: &mut Vec<MOTLYError>,
) {
    let result = build_access_path(root, path, ctx, span, errors);
    if result.is_none() { return; }
    let (write_key, parent) = result.unwrap();
    let props = parent.get_or_create_properties();
    use std::collections::btree_map::Entry;
    if let Entry::Vacant(e) = props.entry(write_key) {
        let mut node = MOTLYDataNode::new();
        node.location = Some(make_location(ctx, span));
        e.insert(MOTLYNode::Data(node));
    }
}

/// Create a deleted-marker node.
fn apply_delete(
    path: &[String],
    root: &mut MOTLYDataNode,
    ctx: &ExecContext,
    span: Span,
    errors: &mut Vec<MOTLYError>,
) {
    let result = build_access_path(root, path, ctx, span, errors);
    if result.is_none() { return; }
    let (write_key, parent) = result.unwrap();
    let mut del_node = MOTLYDataNode::deleted();
    del_node.location = Some(make_location(ctx, span));
    parent.get_or_create_properties().insert(write_key, MOTLYNode::Data(del_node));
}

/// Insert a link reference (read-only alias).
fn apply_link(
    path: &[String],
    ups: usize,
    ref_path: &[RefPathSegment],
    root: &mut MOTLYDataNode,
    ctx: &ExecContext,
    span: Span,
    errors: &mut Vec<MOTLYError>,
) {
    if ctx.options.disable_references {
        errors.push(MOTLYError {
            code: "ref-not-allowed".to_string(),
            message: "References are not allowed in this session. Use := for cloning.".to_string(),
            begin: span.begin,
            end: span.end,
        });
    }
    let result = build_access_path(root, path, ctx, span, errors);
    if result.is_none() { return; }
    let (write_key, parent) = result.unwrap();
    parent.get_or_create_properties().insert(write_key, make_ref(ups, ref_path));
}

/// Resolve a reference target, deep-copy it, and place the clone at path.
#[allow(clippy::too_many_arguments)]
fn apply_clone(
    path: &[String],
    ups: usize,
    ref_path: &[RefPathSegment],
    root: &mut MOTLYDataNode,
    ctx: &ExecContext,
    span: Span,
    errors: &mut Vec<MOTLYError>,
    failed_clones: &mut Vec<FailedClone>,
) {
    // Create intermediate nodes first so resolve_and_clone can navigate the context
    {
        let _ = build_access_path(root, path, ctx, span, &mut Vec::new());
    }

    // Now resolve and clone (immutable borrow of root)
    let cloned = resolve_and_clone(root, path, ups, ref_path);

    match cloned {
        Ok(mut cloned) => {
            sanitize_cloned_refs(&mut cloned, 0, errors);
            cloned.location = Some(make_location(ctx, span));
            let result = build_access_path(root, path, ctx, span, errors);
            if let Some((write_key, parent)) = result {
                parent.get_or_create_properties().insert(write_key, MOTLYNode::Data(cloned));
            }
        }
        Err(err) => {
            let error_index = errors.len();
            errors.push(err.into());
            failed_clones.push(FailedClone {
                source_path: serialize_path(path),
                target_path: resolve_ref_target_key(path, ups, ref_path),
                error_index,
            });
        }
    }
}

// ── Legacy executor (for array element properties) ─────────────────

/// Execute a single statement recursively against a context node.
/// Used for array element properties which are self-contained.
fn execute_statement(
    stmt: &Statement,
    node: &mut MOTLYDataNode,
    errors: &mut Vec<MOTLYError>,
    ctx: &ExecContext,
) {
    match stmt {
        Statement::SetEq { path, value, properties, span } =>
            execute_set_eq(node, path, value, properties.as_deref(), errors, ctx, *span),
        Statement::AssignBoth { path, value, properties, span } =>
            execute_assign_both(node, path, value, properties.as_deref(), errors, ctx, *span),
        Statement::ReplaceProperties { path, properties, span } =>
            execute_replace_properties(node, path, properties, errors, ctx, *span),
        Statement::UpdateProperties { path, properties, span } =>
            execute_update_properties(node, path, properties, errors, ctx, *span),
        Statement::Define { path, deleted, span } =>
            execute_define(node, path, *deleted, errors, ctx, *span),
        Statement::ClearAll { .. } => {
            node.eq = None;
            node.properties = Some(BTreeMap::new());
        }
    }
}

fn execute_set_eq(
    node: &mut MOTLYDataNode,
    path: &[String],
    value: &TagValue,
    properties: Option<&[Statement]>,
    errors: &mut Vec<MOTLYError>,
    ctx: &ExecContext,
    span: Span,
) {
    // Special case: reference value → insert as MOTLYNode::Ref
    if let TagValue::Scalar(ScalarValue::Reference { ups, path: ref_path }) = value {
        if ctx.options.disable_references {
            errors.push(MOTLYError {
                code: "ref-not-allowed".to_string(),
                message: "References are not allowed in this session. Use := for cloning.".to_string(),
                begin: span.begin,
                end: span.end,
            });
        }
        if properties.is_some() {
            let zero = Position { line: 0, column: 0, offset: 0 };
            errors.push(MOTLYError {
                code: "ref-with-properties".to_string(),
                message: "Cannot add properties to a reference. Did you mean := (clone)?".to_string(),
                begin: zero,
                end: zero,
            });
        }
        let result = build_access_path(node, path, ctx, span, errors);
        if let Some((write_key, parent)) = result {
            parent.get_or_create_properties().insert(write_key, make_ref(*ups, ref_path));
        }
        return;
    }

    let result = build_access_path(node, path, ctx, span, errors);
    if result.is_none() { return; }
    let (write_key, parent) = result.unwrap();
    let props = parent.get_or_create_properties();
    let target_pv = props.entry(write_key).or_insert_with(MOTLYNode::new_data);
    let target = target_pv.ensure_data_node();
    set_first_location(target, ctx, span);
    set_eq_slot(target, value, errors, ctx);
    if let Some(prop_stmts) = properties {
        for s in prop_stmts {
            execute_statement(s, target, errors, ctx);
        }
    }
}

fn execute_assign_both(
    node: &mut MOTLYDataNode,
    path: &[String],
    value: &TagValue,
    properties: Option<&[Statement]>,
    errors: &mut Vec<MOTLYError>,
    ctx: &ExecContext,
    span: Span,
) {
    if let TagValue::Scalar(ScalarValue::Reference { ups, path: ref_path }) = value {
        let cloned = resolve_and_clone(node, path, *ups, ref_path);
        match cloned {
            Ok(mut cloned) => {
                sanitize_cloned_refs(&mut cloned, 0, errors);
                if let Some(prop_stmts) = properties {
                    cloned.properties = Some(BTreeMap::new());
                    for s in prop_stmts {
                        execute_statement(s, &mut cloned, errors, ctx);
                    }
                }
                cloned.location = Some(make_location(ctx, span));
                let result = build_access_path(node, path, ctx, span, errors);
                if let Some((write_key, parent)) = result {
                    parent.get_or_create_properties().insert(write_key, MOTLYNode::Data(cloned));
                }
            }
            Err(err) => {
                errors.push(err.into());
            }
        }
    } else {
        let mut fresh = MOTLYDataNode::new();
        fresh.location = Some(make_location(ctx, span));
        set_eq_slot(&mut fresh, value, errors, ctx);
        if let Some(prop_stmts) = properties {
            for s in prop_stmts {
                execute_statement(s, &mut fresh, errors, ctx);
            }
        }
        let result = build_access_path(node, path, ctx, span, errors);
        if let Some((write_key, parent)) = result {
            parent.get_or_create_properties().insert(write_key, MOTLYNode::Data(fresh));
        }
    }
}

fn execute_replace_properties(
    node: &mut MOTLYDataNode,
    path: &[String],
    properties: &[Statement],
    errors: &mut Vec<MOTLYError>,
    ctx: &ExecContext,
    span: Span,
) {
    let access = build_access_path(node, path, ctx, span, errors);
    if access.is_none() { return; }
    let (write_key, parent) = access.unwrap();

    let mut result = MOTLYDataNode::new();
    let parent_props = parent.get_or_create_properties();
    if let Some(MOTLYNode::Data(existing)) = parent_props.get(&write_key) {
        result.eq = existing.eq.clone();
        result.location = existing.location;
    }
    if result.location.is_none() {
        result.location = Some(make_location(ctx, span));
    }
    for stmt in properties {
        execute_statement(stmt, &mut result, errors, ctx);
    }
    parent_props.insert(write_key, MOTLYNode::Data(result));
}

fn execute_update_properties(
    node: &mut MOTLYDataNode,
    path: &[String],
    properties: &[Statement],
    errors: &mut Vec<MOTLYError>,
    ctx: &ExecContext,
    span: Span,
) {
    let access = build_access_path(node, path, ctx, span, errors);
    if access.is_none() { return; }
    let (write_key, parent) = access.unwrap();
    let props = parent.get_or_create_properties();

    if let Some(MOTLYNode::Ref { .. }) = props.get(&write_key) {
        errors.push(MOTLYError {
            code: "write-through-link".to_string(),
            message: format!("Cannot write through link reference \"{}\"", write_key),
            begin: span.begin,
            end: span.end,
        });
        return;
    }

    let target_pv = props.entry(write_key).or_insert_with(MOTLYNode::new_data);
    let target = target_pv.ensure_data_node();
    set_first_location(target, ctx, span);
    for stmt in properties {
        execute_statement(stmt, target, errors, ctx);
    }
}

fn execute_define(
    node: &mut MOTLYDataNode,
    path: &[String],
    deleted: bool,
    errors: &mut Vec<MOTLYError>,
    ctx: &ExecContext,
    span: Span,
) {
    let access = build_access_path(node, path, ctx, span, errors);
    if access.is_none() { return; }
    let (write_key, parent) = access.unwrap();
    if deleted {
        let mut del_node = MOTLYDataNode::deleted();
        del_node.location = Some(make_location(ctx, span));
        parent.get_or_create_properties().insert(write_key, MOTLYNode::Data(del_node));
    } else {
        let props = parent.get_or_create_properties();
        use std::collections::btree_map::Entry;
        if let Entry::Vacant(e) = props.entry(write_key) {
            let mut new_node = MOTLYDataNode::new();
            new_node.location = Some(make_location(ctx, span));
            e.insert(MOTLYNode::Data(new_node));
        }
    }
}

// ── Shared helpers ─────────────────────────────────────────────────

/// Build a MOTLYLocation from a parse_id and span.
fn make_location(ctx: &ExecContext, span: Span) -> MOTLYLocation {
    MOTLYLocation {
        parse_id: ctx.parse_id,
        begin: span.begin,
        end: span.end,
    }
}

/// Set location on a node only if it doesn't already have one (first-appearance rule).
fn set_first_location(node: &mut MOTLYDataNode, ctx: &ExecContext, span: Span) {
    if node.location.is_none() {
        node.location = Some(make_location(ctx, span));
    }
}

/// Navigate to the parent of the final path segment, creating intermediate
/// nodes as needed. Returns None if a write-through-link is detected.
fn build_access_path<'a>(
    node: &'a mut MOTLYDataNode,
    path: &[String],
    ctx: &ExecContext,
    span: Span,
    errors: &mut Vec<MOTLYError>,
) -> Option<(String, &'a mut MOTLYDataNode)> {
    assert!(!path.is_empty(), "path must not be empty");

    let mut current = node;

    for segment in &path[..path.len() - 1] {
        let props = current.get_or_create_properties();

        // Check for write-through-link
        if let Some(MOTLYNode::Ref { .. }) = props.get(segment) {
            errors.push(MOTLYError {
                code: "write-through-link".to_string(),
                message: format!("Cannot write through link reference \"{}\"", segment),
                begin: span.begin,
                end: span.end,
            });
            return None;
        }

        if !props.contains_key(segment) {
            let mut intermediate = MOTLYDataNode::new();
            intermediate.location = Some(make_location(ctx, span));
            props.insert(segment.clone(), MOTLYNode::Data(intermediate));
        }

        let entry = props.get_mut(segment).unwrap();
        current = entry.ensure_data_node();
        set_first_location(current, ctx, span);
    }

    Some((path.last().unwrap().clone(), current))
}

/// Set the eq slot on a target node from a TagValue.
fn set_eq_slot(target: &mut MOTLYDataNode, value: &TagValue, errors: &mut Vec<MOTLYError>, ctx: &ExecContext) {
    match value {
        TagValue::Array(elements) => {
            target.eq = Some(EqValue::Array(resolve_array(elements, errors, ctx)));
        }
        TagValue::Scalar(sv) => match sv {
            ScalarValue::String(s) => {
                target.eq = Some(EqValue::Scalar(Scalar::String(s.clone())));
            }
            ScalarValue::Number(n) => {
                target.eq = Some(EqValue::Scalar(Scalar::Number(*n)));
            }
            ScalarValue::Boolean(b) => {
                target.eq = Some(EqValue::Scalar(Scalar::Boolean(*b)));
            }
            ScalarValue::Date(d) => {
                target.eq = Some(EqValue::Scalar(Scalar::Date(d.clone())));
            }
            ScalarValue::Reference { .. } => {
                unreachable!("References should be handled before calling set_eq_slot");
            }
            ScalarValue::Env { name } => {
                target.eq = Some(EqValue::EnvRef(name.clone()));
            }
            ScalarValue::None => {
                target.eq = None;
            }
        },
    }
}

/// Resolve an array of AST elements to MOTLYNodes.
fn resolve_array(
    elements: &[ArrayElement],
    errors: &mut Vec<MOTLYError>,
    ctx: &ExecContext,
) -> Vec<MOTLYNode> {
    elements.iter().map(|el| resolve_array_element(el, errors, ctx)).collect()
}

fn resolve_array_element(
    el: &ArrayElement,
    errors: &mut Vec<MOTLYError>,
    ctx: &ExecContext,
) -> MOTLYNode {
    // Check if the element value is a reference → becomes MOTLYNode::Ref
    if let Some(TagValue::Scalar(ScalarValue::Reference { ups, path })) = &el.value {
        if ctx.options.disable_references {
            errors.push(MOTLYError {
                code: "ref-not-allowed".to_string(),
                message: "References are not allowed in this session. Use := for cloning.".to_string(),
                begin: el.span.begin,
                end: el.span.end,
            });
        }
        if el.properties.is_some() {
            let zero = Position { line: 0, column: 0, offset: 0 };
            errors.push(MOTLYError {
                code: "ref-with-properties".to_string(),
                message: "Cannot add properties to a reference. Did you mean := (clone)?".to_string(),
                begin: zero,
                end: zero,
            });
        }
        return make_ref(*ups, path);
    }

    let mut node = MOTLYDataNode::new();
    node.location = Some(make_location(ctx, el.span));

    if let Some(ref value) = el.value {
        set_eq_slot(&mut node, value, errors, ctx);
    }

    if let Some(ref prop_stmts) = el.properties {
        for stmt in prop_stmts {
            execute_statement(stmt, &mut node, errors, ctx);
        }
    }

    MOTLYNode::Data(node)
}

/// Convert AST RefPathSegments to tree RefSegments and build a MOTLYNode::Ref.
fn make_ref(ups: usize, path: &[RefPathSegment]) -> MOTLYNode {
    MOTLYNode::Ref {
        link_to: convert_segments(path),
        link_ups: ups,
    }
}

/// Convert AST RefPathSegments to tree RefSegments.
fn convert_segments(path: &[RefPathSegment]) -> Vec<RefSegment> {
    path.iter()
        .map(|seg| match seg {
            RefPathSegment::Name(name) => RefSegment::Name(name.clone()),
            RefPathSegment::Index(idx) => RefSegment::Index(*idx),
        })
        .collect()
}

// ── Clone support ──────────────────────────────────────────────────

/// Resolve a reference path in the tree and return a deep clone.
/// Follows link references when encountered along the path or at the target.
fn resolve_and_clone(
    root: &MOTLYDataNode,
    stmt_path: &[String],
    ups: usize,
    ref_path: &[RefPathSegment],
) -> Result<MOTLYDataNode, CloneError> {
    let ref_str = format_ref_display(ups, &convert_segments(ref_path));

    let mut current = if ups == 0 {
        root.clone()
    } else {
        let context_len = stmt_path.len().checked_sub(1 + ups);
        match context_len {
            Some(len) => {
                let mut cur = root.clone();
                for seg in &stmt_path[..len] {
                    cur = match cur.properties.as_ref().and_then(|p| p.get(seg)) {
                        Some(MOTLYNode::Data(child)) => child.clone(),
                        Some(MOTLYNode::Ref { link_to, link_ups }) => {
                            match resolve_ref_from_root(root, *link_ups, link_to) {
                                Some(resolved) => resolved,
                                None => {
                                    return Err(clone_error(format!(
                                        "Clone reference {} could not be resolved: path segment \"{}\" is an unresolvable link reference",
                                        ref_str, seg
                                    )));
                                }
                            }
                        }
                        None => {
                            return Err(clone_error(format!(
                                "Clone reference {} could not be resolved: path segment \"{}\" not found",
                                ref_str, seg
                            )));
                        }
                    };
                }
                cur
            }
            None => {
                return Err(clone_error(format!(
                    "Clone reference {} goes {} level(s) up but only {} ancestor(s) available",
                    ref_str, ups, stmt_path.len().saturating_sub(1)
                )));
            }
        }
    };

    // Follow refPath segments
    for seg in ref_path {
        current = match seg {
            RefPathSegment::Name(name) => {
                match current.properties.as_ref().and_then(|p| p.get(name.as_str())) {
                    Some(MOTLYNode::Data(child)) => child.clone(),
                    Some(MOTLYNode::Ref { link_to, link_ups }) => {
                        match resolve_ref_from_root(root, *link_ups, link_to) {
                            Some(resolved) => resolved,
                            None => {
                                return Err(clone_error(format!(
                                    "Clone reference {} could not be resolved: property \"{}\" is an unresolvable link reference",
                                    ref_str, name
                                )));
                            }
                        }
                    }
                    None => {
                        return Err(clone_error(format!(
                            "Clone reference {} could not be resolved: property \"{}\" not found",
                            ref_str, name
                        )));
                    }
                }
            }
            RefPathSegment::Index(idx) => {
                match &current.eq {
                    Some(EqValue::Array(arr)) => {
                        if *idx >= arr.len() {
                            return Err(clone_error(format!(
                                "Clone reference {} could not be resolved: index [{}] out of bounds (array length {})",
                                ref_str, idx, arr.len()
                            )));
                        }
                        match &arr[*idx] {
                            MOTLYNode::Data(child) => child.clone(),
                            MOTLYNode::Ref { link_to, link_ups } => {
                                match resolve_ref_from_root(root, *link_ups, link_to) {
                                    Some(resolved) => resolved,
                                    None => {
                                        return Err(clone_error(format!(
                                            "Clone reference {} could not be resolved: index [{}] is an unresolvable link reference",
                                            ref_str, idx
                                        )));
                                    }
                                }
                            }
                        }
                    }
                    _ => {
                        return Err(clone_error(format!(
                            "Clone reference {} could not be resolved: index [{}] used on non-array",
                            ref_str, idx
                        )));
                    }
                }
            }
        };
    }

    Ok(current)
}

struct CloneError {
    error: MOTLYError,
}

impl From<CloneError> for MOTLYError {
    fn from(ce: CloneError) -> MOTLYError {
        ce.error
    }
}

fn clone_error(message: String) -> CloneError {
    let zero = Position { line: 0, column: 0, offset: 0 };
    CloneError {
        error: MOTLYError {
            code: "unresolved-clone-reference".to_string(),
            message,
            begin: zero,
            end: zero,
        },
    }
}

/// Follow a MOTLYRef from root to its concrete MOTLYDataNode target.
/// Only handles absolute refs (ups == 0). Returns None on failure or cycle.
fn resolve_ref_from_root(
    root: &MOTLYDataNode,
    ups: usize,
    segments: &[RefSegment],
) -> Option<MOTLYDataNode> {
    resolve_ref_from_root_inner(root, ups, segments, &mut Vec::new())
}

fn resolve_ref_from_root_inner(
    root: &MOTLYDataNode,
    ups: usize,
    segments: &[RefSegment],
    visited: &mut Vec<String>,
) -> Option<MOTLYDataNode> {
    if ups > 0 { return None; }

    let key = format_ref_display(ups, segments);
    if visited.contains(&key) { return None; } // cycle
    visited.push(key);

    let mut current = root.clone();

    for seg in segments {
        let pv = match seg {
            RefSegment::Name(name) => {
                current.properties.as_ref()?.get(name)?.clone()
            }
            RefSegment::Index(idx) => {
                match &current.eq {
                    Some(EqValue::Array(arr)) if *idx < arr.len() => arr[*idx].clone(),
                    _ => return None,
                }
            }
        };

        match pv {
            MOTLYNode::Data(d) => current = d,
            MOTLYNode::Ref { link_to, link_ups } => {
                current = resolve_ref_from_root_inner(root, link_ups, &link_to, visited)?;
            }
        }
    }

    Some(current)
}

/// Walk a cloned subtree and null out any relative (^) references that
/// escape the clone boundary.
fn sanitize_cloned_refs(node: &mut MOTLYDataNode, depth: usize, errors: &mut Vec<MOTLYError>) {
    if let Some(EqValue::Array(ref mut arr)) = node.eq {
        for elem in arr.iter_mut() {
            sanitize_cloned_pv(elem, depth + 1, errors);
        }
    }
    if let Some(ref mut props) = node.properties {
        for (_key, child) in props.iter_mut() {
            sanitize_cloned_pv(child, depth + 1, errors);
        }
    }
}

fn sanitize_cloned_pv(pv: &mut MOTLYNode, depth: usize, errors: &mut Vec<MOTLYError>) {
    match pv {
        MOTLYNode::Ref { ref link_to, link_ups } => {
            let ups = *link_ups;
            if ups > 0 && ups > depth {
                let display = format_ref_display(ups, link_to);
                let zero = Position { line: 0, column: 0, offset: 0 };
                errors.push(MOTLYError {
                    code: "clone-reference-out-of-scope".to_string(),
                    message: format!(
                        "Cloned reference \"{}\" escapes the clone boundary ({} level(s) up from depth {})",
                        display, ups, depth
                    ),
                    begin: zero,
                    end: zero,
                });
                *pv = MOTLYNode::Data(MOTLYDataNode::new());
            }
        }
        MOTLYNode::Data(node) => {
            sanitize_cloned_refs(node, depth, errors);
        }
    }
}
