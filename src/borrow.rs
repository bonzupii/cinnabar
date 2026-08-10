//! Cinnabar borrow checker.
//!
//! Two independent static analyses, both real fixpoint computations over
//! each function's control-flow graph of basic blocks (computed here,
//! before codegen):
//!
//! 1. **Linear-handle consumption.** Every binding whose type transitively
//!    contains a linear handle (`Memory.Block`, `Collections.Vec`,
//!    `Collections.String`, `Collections.HashMap`, or a struct/enum/array
//!    whose fields, variant payloads, or elements do) tracks a state per
//!    CFG point drawn from the lattice `Unbound < Live < Moved` (plus a
//!    Partial state for values whose fields were moved individually).
//!    Reassigning a Live linear binding is an error; assigning to a Moved
//!    binding relives it; every scope-diverging exit (`return`, `break`,
//!    `try` error propagation, falling off the end) requires every linear
//!    binding in scope to be Moved; merges join incoming states and report
//!    inconsistent consumption.  Field projections are tracked as their
//!    own sub-nodes: moving a parent moves every child path, and moving a
//!    child makes the parent partial.
//!
//! 2. **Flow-sensitive borrows.** A Polonius-style fact set over CFG
//!    points: `borrow_issued(loan, owner, point)`,
//!    `origin_contains(ref_var, loan, point)`,
//!    `var_live_at(var, point)` (standard backward liveness), and
//!    `invalidates(point, loan)`.  A write, move, or new `&mut` borrow of
//!    a variable is an error when a conflicting loan is still contained in
//!    a live reference's origin, which yields the exclusive-XOR-shared
//!    rule.  A function returning a borrow must trace the returned
//!    reference's origin to exactly one input reference parameter; an
//!    origin touching a local or more than one parameter is an error.
//!
//! Both analyses are intraprocedural: references cannot be stored in
//! struct/enum fields and closures cannot capture references, so borrow
//! lifetimes never escape the function they are created in.  Access is
//! binary (shared or exclusive); no fractional accounting is modelled.
//!
//! The checker consumes the typechecker's attached facts (expression
//! types, statement binding types, pattern types, instance rows, and the
//! per-key linearity flag) and the resolver's resolved symbols.  It never
//! re-derives a fact an earlier stage established; the only facts it
//! computes are the two dataflow analyses above.  Linearity is read from
//! the flag the typechecker stored on every canonical descriptor row.

use crate::ast::*;

// ---------------------------------------------------------------------------
// Analysis constants.
// ---------------------------------------------------------------------------

/// Linear-state lattice: Unbound < Live < Moved.  Partial marks a linear
/// struct or enum whose value was partially moved field by field; it can
/// never be consumed whole (the language has no field re-initialization),
/// so it behaves as an absorbing error state.
const ST_UNBOUND: i64 = 0;
const ST_LIVE: i64 = 1;
const ST_MOVED: i64 = 2;
const ST_PARTIAL: i64 = 3;

/// Effect op kinds recorded per statement block.
const OP_READ: i64 = 0; // binding read (liveness use)
const OP_MOVE: i64 = 1; // linear value consumed (root or sub-node path)
const OP_BORROW: i64 = 2; // shared borrow of the binding
const OP_BORROW_M: i64 = 3; // exclusive borrow of the binding
const OP_ASSIGN: i64 = 4; // write to the target binding
const OP_BIND: i64 = 5; // definition of a binding (let, param, pattern)
const OP_EXIT: i64 = 6; // scope-diverging exit (return/break/try)
const OP_RET_REF: i64 = 7; // returned-borrow origin check

/// The exit kind carried by OP_EXIT (aux slot 2).
const EX_RETURN: i64 = 0;
const EX_BREAK: i64 = 1;
const EX_TRY: i64 = 2;

/// Loan kinds.
const L_SHARED: i64 = 0;
const L_MUT: i64 = 1;

/// Element-borrow index keys carried on loans (row slot 3) and borrow ops
/// (aux slot 2): NONE means a whole-value loan (it overlaps every
/// element), IDX_DYN means an element borrow through a non-constant index
/// (never provably disjoint from anything), and a list id holds the
/// constant index path, inner-most level first.
const IDX_DYN: i64 = -2;

/// Block kinds.
const BLK_ENTRY: i64 = 0;
const BLK_STMT: i64 = 1;
const BLK_JOIN: i64 = 2;
const BLK_EXIT: i64 = 3;

/// Expression consumption modes: a value position (a linear path is
/// moved), a shared-borrow position, or an exclusive-borrow position.
const MODE_VALUE: i64 = 0;
const MODE_BORROW: i64 = 1;
const MODE_MUT: i64 = 2;

// ---------------------------------------------------------------------------
// Per-function tables.
//
// All state is stored as flat tuples of parallel arrays (no structs, no
// enums), mirroring the arena style of the rest of the compiler.  Index
// access goes through `.get` everywhere; an invalid index yields NONE
// rather than panicking.
// ---------------------------------------------------------------------------

/// The mutable build/analysis state for one function:
/// (bindings, bspans, paths, loans, ops, oloans, blocks, bscopes, bsuccs,
/// bpreds, bspans).
///
/// - bindings: `(name, key, is_linear, is_ref, is_param, is_mut,
///   root_path)`; `root_path` is the path-table index of the binding's
///   whole-value node (NONE when the binding is not linear).
/// - bspans: `(file, start, end)` of each binding declaration.
/// - paths: `(parent, field, root)` sub-node trie over a linear binding's
///   whole-value node; the root node has `parent` NONE and `root` itself.
/// - loans: `(owner_binding, kind, synthetic, index_key)`; a synthetic
///   loan is the standing borrow a reference parameter carries in from
///   its caller.  `index_key` is NONE for a whole-value loan, IDX_DYN for
///   an element borrow through a non-constant index, or a list id holding
///   the constant first-level index (`a[i]` is `[i]`).  Only the first
///   level is tracked: a nested `a[i][j]` keeps the inner `[i]`, which is
///   conservative — same-first-level borrows conflict, disjoint first
///   levels are allowed.
/// - ops: `(kind, binding, path, aux1, aux2, aux3, file, start, end)`.
/// - oloans: per-op origin-loan lists; entries are loan ids, or
///   `REF_ORIGIN(binding)` markers (a negative encoding) meaning "the
///   binding's own origin".
/// - blocks: `(kind, stmt, first_op, last_op)`; the block's ops run from
///   `first_op` to `last_op` inclusive of the lower bound.
/// - bscopes: per-block snapshot of the binding indices in scope entering
///   the block; joins filter state through it.
/// - bsuccs/bpreds: the CFG edge lists.
/// - bspans: `(file, start, end)` of each block's originating construct.
type F = (
    Vec<(i64, i64, i64, i64, i64, i64, i64)>,
    Vec<(i64, i64, i64)>,
    Vec<(i64, i64, i64)>,
    Vec<(i64, i64, i64, i64)>,
    Vec<(i64, i64, i64, i64, i64, i64, i64, i64, i64)>,
    Vec<Vec<i64>>,
    Vec<(i64, i64, i64, i64)>,
    Vec<Vec<i64>>,
    Vec<Vec<i64>>,
    Vec<Vec<i64>>,
    Vec<(i64, i64, i64)>,
);

/// Transient build state for one function:
/// (name_stack, loop_stack, scope_bindings, pending_loans, current,
/// fn_exit, last_borrow_op, pending_index_key).
///
/// - name_stack: `(name, binding)` pairs; shadowing pushes a new pair and
///   scope exit pops back to the saved length.
/// - loop_stack: `(while_block, join_block, scope_start)` for resolving
///   `break` (exit check from `scope_start`) and `continue` (back-edge to
///   the while block).
/// - scope_bindings: the binding indices in scope at the current build
///   position; snapshotted into `bscopes` at every block.
/// - pending_loans: loans issued within the current statement that have
///   not yet been captured into a reference binding's origin; they die at
///   the statement boundary and are used for same-statement conflict
///   checks.
/// - current: the block the next op is emitted into (expression forks
///   move this cursor; it is never a lexical position).
/// - fn_exit: the function's single exit block.
/// - last_borrow_op: the op id of the most recent borrow op emitted
///   during the current expression walk; an enclosing `arr[i]` arm reads
///   it to stamp the element index onto the loan and the op (stale values
///   are harmless: arms compare against the value saved at entry).
/// - pending_index_key: the index key of the innermost enclosing `arr[i]`
///   arm being built, read by `check_pending_conflict` so same-statement
///   element borrows compare by index; NONE outside any index expression.
type B = (
    Vec<(i64, i64)>,
    Vec<(i64, i64, i64)>,
    Vec<i64>,
    Vec<i64>,
    i64,
    i64,
    i64,
    i64,
);

/// The shared arena context: (names, nodes, lists, errors).
type Ctx<'a> = (
    &'a mut Vec<String>,
    &'a mut Vec<i64>,
    &'a mut Vec<Vec<i64>>,
    &'a mut Vec<Diag>,
    // Slot 4: callee-origin summaries — function node -> the
    // input-parameter positions its returned borrow can trace to.
    // Populated to a fixpoint before the authoritative walk; call sites
    // read it so a returned borrow's origin seeds from only the
    // parameters the callee can actually return a borrow of (bug-report
    // d7d), falling back to the conservative union of every reference
    // argument when a callee has no summary.
    &'a mut Vec<(i64, Vec<i64>)>,
);

/// A `REF_ORIGIN(binding)` production marker: the value's origin is the
/// named binding's origin, resolved against the converged origin facts.
/// Negative so it can never collide with a loan id.
fn ref_origin(binding: i64) -> i64 {
    -1 - binding
}

fn is_ref_origin(entry: i64) -> bool {
    entry < 0
}

// ---------------------------------------------------------------------------
// Safe table access.  Every table read goes through these so no index can
// escape the table; an invalid index yields NONE (or an empty tuple), and
// callers treat NONE as "no fact" rather than fabricating a value.
// ---------------------------------------------------------------------------

fn binding_at(f: &F, id: i64) -> (i64, i64, i64, i64, i64, i64, i64) {
    match f.0.get(id as usize) {
        Some(row) => *row,
        None => (NONE, NONE, NONE, NONE, NONE, NONE, NONE),
    }
}

fn loan_at(f: &F, id: i64) -> (i64, i64, i64, i64) {
    match f.3.get(id as usize) {
        Some(row) => *row,
        None => (NONE, NONE, 0, NONE),
    }
}

fn path_at(f: &F, id: i64) -> (i64, i64, i64) {
    match f.2.get(id as usize) {
        Some(row) => *row,
        None => (NONE, NONE, NONE),
    }
}

fn op_at(f: &F, id: i64) -> (i64, i64, i64, i64, i64, i64, i64, i64, i64) {
    match f.4.get(id as usize) {
        Some(row) => *row,
        None => (NONE, NONE, NONE, NONE, NONE, NONE, NONE, NONE, NONE),
    }
}

fn op_loans_at(f: &F, id: i64) -> Vec<i64> {
    match f.5.get(id as usize) {
        Some(list) => list.clone(),
        None => Vec::new(),
    }
}

fn block_at(f: &F, id: i64) -> (i64, i64, i64, i64) {
    match f.6.get(id as usize) {
        Some(row) => *row,
        None => (NONE, NONE, NONE, NONE),
    }
}

fn block_span_at(f: &F, id: i64) -> (i64, i64, i64) {
    match f.10.get(id as usize) {
        Some(span) => *span,
        None => (NO_FILE, 0, 0),
    }
}

fn succ_of(f: &F, block: i64) -> Vec<i64> {
    match f.8.get(block as usize) {
        Some(list) => list.clone(),
        None => Vec::new(),
    }
}

fn pred_of(f: &F, block: i64) -> Vec<i64> {
    match f.9.get(block as usize) {
        Some(list) => list.clone(),
        None => Vec::new(),
    }
}

fn list_has(list: &[i64], value: i64) -> bool {
    let mut idx = 0usize;
    while idx < list.len() {
        match list.get(idx) {
            Some(cell) => {
                if *cell == value {
                    return true;
                }
            }
            None => break,
        }
        idx += 1;
    }
    false
}

fn state_at(state: &[i64], path: i64) -> i64 {
    match state.get(path as usize) {
        Some(value) => *value,
        None => ST_UNBOUND,
    }
}

fn state_set(state: &mut [i64], path: i64, value: i64) {
    if path < 0 {
        return;
    }
    if let Some(cell) = state.get_mut(path as usize) {
        *cell = value;
    }
}

// ---------------------------------------------------------------------------
// Type facts.  The typechecker canonicalized every type into descriptor
// rows and stored a per-key linearity flag; these reads consume that
// table.  Nothing here re-derives a fact an earlier stage established.
// ---------------------------------------------------------------------------

fn ty_kind_of(nodes: &[i64], key: i64) -> i64 {
    if key < 0 {
        return TYD_UNKNOWN;
    }
    let row = find_tyinfo(nodes, key);
    if row == NONE {
        TYD_UNKNOWN
    } else {
        node_b(nodes, row)
    }
}

fn ty_sym_of(nodes: &[i64], key: i64) -> i64 {
    let row = find_tyinfo(nodes, key);
    if row == NONE {
        NONE
    } else {
        node_c(nodes, row)
    }
}

fn ty_elem_of(nodes: &[i64], key: i64) -> i64 {
    let row = find_tyinfo(nodes, key);
    if row == NONE {
        NONE
    } else {
        node_e(nodes, row)
    }
}

/// A reference-like key: shared, exclusive, or a bare slice.  Reference
/// values are tracked for borrow-origin analysis; they are never linear.
fn is_ref_key(nodes: &[i64], key: i64) -> bool {
    let kind = ty_kind_of(nodes, key);
    kind == TYD_REF || kind == TYD_REF_MUT || kind == TYD_SLICE
}

/// Whether `key` is (or transitively contains) a linear handle.  The
/// typechecker computed the flag once over every canonical descriptor
/// row at the end of checking; this is a plain read of that stored fact,
/// never a re-derivation from names or declarations.
fn is_linear_key(ctx: &mut Ctx, key: i64) -> i64 {
    if key == NONE {
        return 0;
    }
    if tyinfo_is_linear(ctx.1, key) == 1 {
        1
    } else {
        0
    }
}

// ---------------------------------------------------------------------------
// Table builders.
// ---------------------------------------------------------------------------

/// The block the next op is emitted into.
fn cur(b: &B) -> i64 {
    b.4
}

/// Closes the current block: its op range ends at the current op-table
/// length.
fn close_current(f: &mut F, b: &mut B) {
    let id = b.4;
    if id < 0 {
        return;
    }
    if let Some(row) = f.6.get_mut(id as usize) {
        row.3 = f.4.len() as i64;
    }
}

/// Creates a fresh block with `kind` and `stmt`, closing the current
/// block, snapshotting the current scope bindings, and making the new
/// block the emission cursor.
fn new_block(f: &mut F, b: &mut B, kind: i64, stmt: i64, span: (i64, i64, i64)) -> i64 {
    close_current(f, b);
    let id = f.6.len() as i64;
    let first_op = f.4.len() as i64;
    f.6.push((kind, stmt, first_op, first_op));
    f.7.push(b.2.clone());
    f.8.push(Vec::new());
    f.9.push(Vec::new());
    f.10.push(span);
    b.4 = id;
    id
}

/// Resumes emission into `block` (an existing block), closing whatever
/// block the cursor was on.  Used when an expression fork (a match) joins
/// back into its continuation.
fn resume(f: &mut F, b: &mut B, block: i64) {
    close_current(f, b);
    // A resumed block opens a fresh emission phase: its op range starts at
    // the current op-table length.  Without this, a block created before
    // nested sub-CFGs were built (a match's join) records a stale first-op
    // and sweeps the nested blocks' ops into its own range, replaying
    // foreign effects in the analysis.
    if let Some(row) = f.6.get_mut(block as usize) {
        row.2 = f.4.len() as i64;
    }
    b.4 = block;
}

fn add_edge(f: &mut F, from: i64, to: i64) {
    if from < 0 || to < 0 {
        return;
    }
    if let Some(list) = f.8.get_mut(from as usize) {
        list.push(to);
    }
    if let Some(list) = f.9.get_mut(to as usize) {
        list.push(from);
    }
}

/// The index one past the block's last op.
fn block_op_end(f: &F, block: i64) -> i64 {
    let row = block_at(f, block);
    let mut end = row.3;
    if end < row.2 {
        end = row.2;
    }
    end
}

/// Appends an op to the current emission cursor.  Returns the op index
/// (used to attach origin-loan lists).
fn emit_op(f: &mut F, kind: i64, binding: i64, path: i64, aux: (i64, i64, i64), span: (i64, i64, i64)) -> i64 {
    f.4.push((kind, binding, path, aux.0, aux.1, aux.2, span.0, span.1, span.2));
    f.5.push(Vec::new());
    f.4.len() as i64 - 1
}

/// Records the origin-loan list of a just-emitted op (used for OP_BIND of
/// a reference binding, OP_ASSIGN of a reference target, and OP_RET_REF).
fn set_op_loans(f: &mut F, op: i64, loans: &[i64]) {
    if let Some(list) = f.5.get_mut(op as usize) {
        list.clear();
        let mut idx = 0usize;
        while idx < loans.len() {
            match loans.get(idx) {
                Some(loan) => list.push(*loan),
                None => break,
            }
            idx += 1;
        }
    }
}

/// Allocates a loan on `owner` with `kind` and returns its id.  The
/// element `index_key` starts NONE (a whole-value loan); an enclosing
/// `arr[i]` arm stamps the element path onto it before any later borrow
/// compares indices.
fn alloc_loan(f: &mut F, owner: i64, kind: i64, synthetic: i64, index_key: i64) -> i64 {
    let id = f.3.len() as i64;
    f.3.push((owner, kind, synthetic, index_key));
    id
}

/// The path-table node for a linear binding's whole value.
fn root_path_of(f: &F, binding: i64) -> i64 {
    binding_at(f, binding).6
}

/// Returns the existing sub-node of `path` for `field`, or NONE.
fn find_child(f: &F, path: i64, field: i64) -> i64 {
    let mut idx = 0usize;
    while idx < f.2.len() {
        let row = path_at(f, idx as i64);
        if row.0 == path && row.1 == field {
            return idx as i64;
        }
        idx += 1;
    }
    NONE
}

/// The sub-node for `field` under `path`, allocating it on first use.  The
/// node shares the root of its parent.
fn child_path(f: &mut F, path: i64, field: i64) -> i64 {
    let existing = find_child(f, path, field);
    if existing != NONE {
        return existing;
    }
    let root = path_at(f, path).2;
    let id = f.2.len() as i64;
    f.2.push((path, field, root));
    id
}

/// Eagerly creates the sub-node trie for a linear binding: every struct
/// field that is itself linear gets a child path, recursing into nested
/// structs.  A struct literal initializes every field, so each materialized
/// sub-node is Live from the binding's OP_BIND onward; without this, an
/// untracked sub-node would stay Unbound and a field move would silently
/// no-op instead of marking the field Moved and its ancestors Partial.
/// Linearity is read from the typechecker's per-key flag, never re-derived.
fn materialize_linear_subpaths(f: &mut F, ctx: &mut Ctx, key: i64, path: i64) {
    if key == NONE || path < 0 {
        return;
    }
    let sym = ty_sym_of(ctx.1, key);
    if sym == NONE {
        return;
    }
    let decl = node_c(ctx.1, sym);
    if decl == NONE || node_a(ctx.1, decl) != ITEM_STRUCT {
        return;
    }
    let fields = node_e(ctx.1, decl);
    let count = list_len(ctx.2, fields);
    let mut idx = 0i64;
    while idx < count {
        let fnode = list_get(ctx.2, fields, idx);
        let field = node_a(ctx.1, fnode);
        let fkey = field_key_of(ctx, key, field);
        if is_linear_key(ctx, fkey) == 1 {
            let child = child_path(f, path, field);
            materialize_linear_subpaths(f, ctx, fkey, child);
        }
        idx += 1;
    }
}

/// Marks a freshly bound linear binding's root and every materialized
/// sub-node Live: a struct literal initializes all fields, so the whole
/// trie is live from the binding's definition.
fn bind_state_live(f: &F, state: &mut [i64], binding: i64) {
    let root = root_path_of(f, binding);
    if root < 0 {
        return;
    }
    state_set(state, root, ST_LIVE);
    let mut idx = 0usize;
    while idx < f.2.len() {
        if path_at(f, idx as i64).2 == root {
            state_set(state, idx as i64, ST_LIVE);
        }
        idx += 1;
    }
}

/// True when `path` is a whole-value node.
fn is_root_path(f: &F, path: i64) -> bool {
    path_at(f, path).0 == NONE
}

/// Binds a new local with the given type key and span.  `flags` carries
/// the precomputed `(is_linear, is_ref, is_param, is_mut)` facts (derived
/// from the key by the caller).  Returns the binding id.
fn bind_var(f: &mut F, b: &mut B, ctx: &mut Ctx, name: i64, key: i64, flags: (i64, i64, i64, i64), span: (i64, i64, i64)) -> i64 {
    let id = f.0.len() as i64;
    let root = if flags.0 == 1 {
        let r = f.2.len() as i64;
        f.2.push((NONE, NONE, r));
        materialize_linear_subpaths(f, ctx, key, r);
        r
    } else if flags.1 == 1 && flags.2 == 1 && ty_kind_of(ctx.1, key) == TYD_REF_MUT && is_linear_key(ctx, ty_elem_of(ctx.1, key)) == 1 {
        // A `&mut` parameter whose referent is linear: the callee borrows
        // the caller's handles, so its referent fields get their own
        // tracked trie (entered Live), letting the linear rules -- consume,
        // reassign-over-Live, restore-before-return -- apply inside the
        // callee as well.
        let r = f.2.len() as i64;
        f.2.push((NONE, NONE, r));
        materialize_linear_subpaths(f, ctx, ty_elem_of(ctx.1, key), r);
        r
    } else {
        NONE
    };
    f.0.push((name, key, flags.0, flags.1, flags.2, flags.3, root));
    f.1.push(span);
    b.2.push(id);
    b.0.push((name, id));
    id
}

fn stmt_span(nodes: &[i64], stmt: i64) -> (i64, i64, i64) {
    (node_file(nodes, stmt), node_start(nodes, stmt), node_end(nodes, stmt))
}

fn expr_span(nodes: &[i64], expr: i64) -> (i64, i64, i64) {
    (node_file(nodes, expr), node_start(nodes, expr), node_end(nodes, expr))
}

// ---------------------------------------------------------------------------
// Build: one function's CFG.
// ---------------------------------------------------------------------------

/// Builds the CFG and op table for one function body.  Returns true when
/// the body built (a NONE body means a signature-only declaration, e.g. a
/// native function or a trait method, which has nothing to check).
fn build_fn(f: &mut F, b: &mut B, ctx: &mut Ctx, fn_node: i64) -> bool {
    let body = node_f(ctx.1, fn_node);
    if body == NONE {
        return false;
    }
    let ret = ty_key_of(ctx.1, node_d(ctx.1, fn_node));
    let params = node_c(ctx.1, fn_node);
    let count = list_len(ctx.2, params);
    let fn_span = (node_file(ctx.1, fn_node), node_start(ctx.1, fn_node), node_end(ctx.1, fn_node));

    // Entry block: bind every parameter.  Linear parameters are Live on
    // entry; reference parameters carry a synthetic loan on themselves so
    // a returned borrow can trace back to them.
    let entry = new_block(f, b, BLK_ENTRY, NONE, fn_span);
    let mut idx = 0i64;
    while idx < count {
        let param = list_get(ctx.2, params, idx);
        let name = node_a(ctx.1, param);
        let key = ty_key_of(ctx.1, node_b(ctx.1, param));
        let span = stmt_span(ctx.1, param);
        let flags = (is_linear_key(ctx, key), if is_ref_key(ctx.1, key) { 1 } else { 0 }, 1, 0);
        let binding = bind_var(f, b, ctx, name, key, flags, span);
        if flags.1 == 1 {
            let op = emit_op(f, OP_BIND, binding, NONE, (1, 1, 0), span);
            let loan = alloc_loan(f, binding, if ty_kind_of(ctx.1, key) == TYD_REF_MUT { L_MUT } else { L_SHARED }, 1, NONE);
            let loans = [loan];
            set_op_loans(f, op, &loans);
        } else {
            emit_op(f, OP_BIND, binding, NONE, (0, 1, 0), span);
        }
        idx += 1;
    }

    // The exit block carries the falling-off-the-end consumption check.
    let exit = new_block(f, b, BLK_EXIT, NONE, fn_span);
    b.5 = exit;
    emit_op(f, OP_EXIT, NONE, NONE, (0, EX_RETURN, 0), fn_span);

    let entry_of_body = build_list(f, b, ctx, body, exit, ret, &mut Vec::new());
    add_edge(f, entry, entry_of_body);

    // The exit is the end of every body scope: it must see every binding
    // ever declared in the function so the fall-off-end check catches any
    // linear value that was never consumed.
    close_current(f, b);
    let mut all: Vec<i64> = Vec::new();
    let mut bidx = 0usize;
    while bidx < f.0.len() {
        all.push(bidx as i64);
        bidx += 1;
    }
    if let Some(list) = f.7.get_mut(exit as usize) {
        list.clear();
        let mut j = 0usize;
        while j < all.len() {
            match all.get(j) {
                Some(v) => list.push(*v),
                None => break,
            }
            j += 1;
        }
    }
    true
}

/// Builds a statement list, threading `out` as the fall-through target of
/// the last reachable statement and appending the origin-loan list of the
/// list's final value into `prod` (used for match arms).  Returns the
/// list's entry block.  The fall-through chaining is internal; callers
/// only connect the incoming edge to the entry.
fn build_list(f: &mut F, b: &mut B, ctx: &mut Ctx, list: i64, out: i64, ret: i64, prod: &mut Vec<i64>) -> i64 {
    let scope_start = b.2.len();
    let name_start = b.0.len();
    let count = list_len(ctx.2, list);
    if count == 0 {
        let stub = new_block(f, b, BLK_JOIN, NONE, block_span_at(f, out));
        // Close the stub immediately: no statement ever emits into it, so
        // leaving it open would let its range sweep the following
        // statements' ops.
        if let Some(row) = f.6.get_mut(stub as usize) {
            row.3 = f.4.len() as i64;
        }
        add_edge(f, stub, out);
        return stub;
    }
    let mut entry = NONE;
    let mut after = NONE;
    let mut production: Vec<i64> = Vec::new();
    let mut fell = true;
    let mut idx = 0i64;
    while idx < count {
        let stmt = list_get(ctx.2, list, idx);
        b.3.clear();
        let (ent, aft, stmt_prod) = build_stmt(f, b, ctx, stmt, out, ret);
        if entry == NONE {
            entry = ent;
        }
        if fell && after != NONE {
            add_edge(f, after, ent);
        }
        if fell && aft != NONE {
            after = aft;
            production = stmt_prod;
        }
        if aft == NONE {
            fell = false;
        }
        idx += 1;
    }
    if fell && after != NONE {
        add_edge(f, after, out);
    }
    // The list's final value can be a reference produced inside a nested
    // construct (a match arm body ending in a reborrowed reference), so
    // the production is aggregated with the marker-preserving append: a
    // `REF_ORIGIN` marker dropped here would leave a reference bound from
    // the list's value with an empty origin, and later borrows would never
    // see the loan it actually carries.
    append_prod_unique(prod, &production);
    while b.2.len() > scope_start {
        b.2.pop();
    }
    while b.0.len() > name_start {
        b.0.pop();
    }
    entry
}

/// Builds one statement.  Returns `(entry, after, production)`: `after`
/// is the block where control continues (NONE when the statement
/// diverges), `production` is the final value's origin loans.
fn build_stmt(f: &mut F, b: &mut B, ctx: &mut Ctx, stmt: i64, out: i64, ret: i64) -> (i64, i64, Vec<i64>) {
    let kind = node_a(ctx.1, stmt);
    let span = stmt_span(ctx.1, stmt);
    if kind == STMT_LET {
        let block = new_block(f, b, BLK_STMT, stmt, span);
        let name = node_c(ctx.1, stmt);
        let is_mut = node_b(ctx.1, stmt);
        let init = node_e(ctx.1, stmt);
        let has_init = if init == NONE { 0 } else { 1 };
        let binding_key = stmt_ty_of(ctx.1, stmt);
        let mut prod: Vec<i64> = Vec::new();
        let cont = if has_init == 1 {
            expr_effects(f, b, ctx, init, MODE_VALUE, ret, &mut prod)
        } else {
            block
        };
        let flags = (is_linear_key(ctx, binding_key), if is_ref_key(ctx.1, binding_key) { 1 } else { 0 }, 0, is_mut);
        let binding = bind_var(f, b, ctx, name, binding_key, flags, span);
        let op = emit_op(f, OP_BIND, binding, NONE, (flags.1, has_init, 0), span);
        if flags.1 == 1 && has_init == 1 {
            set_op_loans(f, op, &prod);
        }
        return (block, cont, prod);
    }
    if kind == STMT_ASSIGN {
        let block = new_block(f, b, BLK_STMT, stmt, span);
        let target = node_b(ctx.1, stmt);
        let value = node_c(ctx.1, stmt);
        let mut prod: Vec<i64> = Vec::new();
        let cont = expr_effects(f, b, ctx, value, MODE_VALUE, ret, &mut prod);
        // Decompose the target into (binding, path, kind, owner): a
        // whole-value assign of a name, a field assign on a mutable
        // local, or a field write through a `&mut` reference.  The op's
        // aux slots carry the kind and the referent binding; the path
        // slot carries the referent's sub-node for the linear rule.
        let (binding, path, tkind, owner) = assign_target_of(f, b, ctx, target);
        // The is_ref flag is only meaningful for a whole-value re-point
        // (kind 0): it makes the origin fixpoint overwrite the variable's
        // origin with the value's loans.  A write through a reference
        // (kind 1) and a field assign on a local (kind 2) must not touch
        // origins, so they carry the flag clear.
        let is_ref = if tkind == 0 && binding >= 0 { binding_at(f, binding).3 } else { 0 };
        let op = emit_op(f, OP_ASSIGN, binding, path, (is_ref, tkind, owner), span);
        if tkind == 0 && is_ref == 1 {
            set_op_loans(f, op, &prod);
        }
        return (block, cont, Vec::new());
    }
    if kind == STMT_WHILE {
        let block = new_block(f, b, BLK_STMT, stmt, span);
        let cond = node_b(ctx.1, stmt);
        let body = node_c(ctx.1, stmt);
        let join = new_block(f, b, BLK_JOIN, NONE, span);
        let scope_start = b.2.len() as i64;
        b.1.push((block, join, scope_start));
        let cont = expr_effects(f, b, ctx, cond, MODE_VALUE, ret, &mut Vec::new());
        // The loop header feeds the condition block; without this edge the
        // condition's effects (and the state from the loop back-edge) are
        // unreachable in the CFG.
        add_edge(f, block, cont);
        let body_entry = build_list(f, b, ctx, body, block, ret, &mut Vec::new());
        b.1.pop();
        add_edge(f, cont, body_entry);
        add_edge(f, cont, join);
        add_edge(f, join, out);
        return (block, join, Vec::new());
    }
    if kind == STMT_IF {
        let block = new_block(f, b, BLK_STMT, stmt, span);
        let cond = node_b(ctx.1, stmt);
        let then_list = node_c(ctx.1, stmt);
        let else_list = node_d(ctx.1, stmt);
        let join = new_block(f, b, BLK_JOIN, NONE, span);
        let cont = expr_effects(f, b, ctx, cond, MODE_VALUE, ret, &mut Vec::new());
        // The statement's entry block feeds the condition block; without
        // this edge the condition's effects (and the state entering the
        // branches) are unreachable in the CFG.
        add_edge(f, block, cont);
        let then_entry = build_list(f, b, ctx, then_list, join, ret, &mut Vec::new());
        add_edge(f, cont, then_entry);
        if else_list != NONE {
            let else_entry = build_list(f, b, ctx, else_list, join, ret, &mut Vec::new());
            add_edge(f, cont, else_entry);
        } else {
            add_edge(f, cont, join);
        }
        add_edge(f, join, out);
        return (block, join, Vec::new());
    }
    if kind == STMT_RETURN {
        let block = new_block(f, b, BLK_STMT, stmt, span);
        let value = node_b(ctx.1, stmt);
        // A return is a dead end in the CFG: it carries its own
        // consumption check and must not feed the falling-off-the-end
        // exit block (which would double-report on the same leak).
        if value == NONE {
            emit_op(f, OP_EXIT, NONE, NONE, (0, EX_RETURN, 0), span);
            return (block, NONE, Vec::new());
        }
        let mut prod: Vec<i64> = Vec::new();
        // The value's effects end on the current block: a forking value
        // already resumed into its join, and a straight-line value leaves
        // the cursor on the block its ops were emitted into.  Re-resuming
        // here would reset the block's op range and orphan the value's
        // moves and reads from every block's range, so the exit check
        // would run against a state the moves never reached.
        expr_effects(f, b, ctx, value, MODE_VALUE, ret, &mut prod);
        let ret_kind = ty_kind_of(ctx.1, ret);
        if ret_kind == TYD_REF || ret_kind == TYD_REF_MUT || ret_kind == TYD_SLICE {
            let op = emit_op(f, OP_RET_REF, NONE, NONE, (0, 0, 0), span);
            set_op_loans(f, op, &prod);
        }
        emit_op(f, OP_EXIT, NONE, NONE, (0, EX_RETURN, 0), span);
        return (block, NONE, Vec::new());
    }
    if kind == STMT_BREAK {
        let block = new_block(f, b, BLK_STMT, stmt, span);
        let loop_scope = match b.1.last() {
            Some(row) => row.2,
            None => 0,
        };
        let join = match b.1.last() {
            Some(row) => row.1,
            None => out,
        };
        emit_op(f, OP_EXIT, NONE, NONE, (loop_scope, EX_BREAK, 0), span);
        add_edge(f, block, join);
        return (block, NONE, Vec::new());
    }
    if kind == STMT_CONTINUE {
        let block = new_block(f, b, BLK_STMT, stmt, span);
        let header = match b.1.last() {
            Some(row) => row.0,
            None => out,
        };
        add_edge(f, block, header);
        return (block, NONE, Vec::new());
    }
    // STMT_EXPR: a bare expression statement.
    let block = new_block(f, b, BLK_STMT, stmt, span);
    let expr = node_b(ctx.1, stmt);
    let mut prod: Vec<i64> = Vec::new();
    let cont = expr_effects(f, b, ctx, expr, MODE_VALUE, ret, &mut prod);
    (block, cont, prod)
}

/// Resolves a name to its current binding, or NONE.
fn lookup_name(b: &B, name: i64) -> i64 {
    let mut depth = b.0.len();
    while depth > 0 {
        depth -= 1;
        match b.0.get(depth) {
            Some(pair) => {
                if pair.0 == name {
                    return pair.1;
                }
            }
            None => break,
        }
    }
    NONE
}

// ---------------------------------------------------------------------------
// Build: expressions.
// ---------------------------------------------------------------------------

/// Records the effects of `expr` at the current build position, writing
/// the origin-loan list of the expression's value into `prod` (empty for
/// a fresh or non-reference value) and returning the block where the
/// enclosing statement continues (expressions with control flow fork
/// here).  `mode` is MODE_VALUE, MODE_BORROW, or MODE_MUT.  Callers that
/// do not consume the production pass an empty list and leave it unread.
fn expr_effects(f: &mut F, b: &mut B, ctx: &mut Ctx, expr: i64, mode: i64, ret: i64, prod: &mut Vec<i64>) -> i64 {
    if node_tag(ctx.1, expr) != NODE_EXPR {
        return cur(b);
    }
    let kind = node_a(ctx.1, expr);
    if kind == EXPR_LIT {
        return cur(b);
    }
    if kind == EXPR_PATH {
        return path_effects(f, b, ctx, expr, mode, prod);
    }
    if kind == EXPR_UNARY {
        return unary_effects(f, b, ctx, expr, ret, prod);
    }
    if kind == EXPR_BINARY {
        return binary_effects(f, b, ctx, expr, ret, prod);
    }
    if kind == EXPR_CALL {
        return call_effects(f, b, ctx, expr, ret, prod);
    }
    if kind == EXPR_STRUCT_LIT {
        return struct_effects(f, b, ctx, expr, ret, prod);
    }
    if kind == EXPR_ARRAY {
        return array_effects(f, b, ctx, expr, ret, prod);
    }
    if kind == EXPR_MATCH {
        return match_effects(f, b, ctx, expr, ret, prod);
    }
    if kind == EXPR_TRY {
        return try_effects(f, b, ctx, expr, ret, prod);
    }
    if kind == EXPR_INDEX {
        // An index reads its base in the enclosing mode (a value read, a
        // shared borrow, or an exclusive borrow of the element) and its
        // index in a value position.  The index never moves the base: a
        // value-position index of a linear-element array or slice is
        // rejected by the typechecker, so only reads and borrows reach
        // this arm.  A borrowed element issues its loan on the root
        // binding through the base's borrow effect; this arm then stamps
        // the element index onto the loan and the borrow op, so later
        // element borrows compare by index (disjoint constant indices are
        // allowed; an equal or nested index, or any dynamic index, is a
        // conflict).
        let base = node_b(ctx.1, expr);
        let index = node_c(ctx.1, expr);
        let base_kind = node_a(ctx.1, base);
        let level_key = index_key_of_literal(ctx, index);
        let level_list = if level_key == IDX_DYN {
            IDX_DYN
        } else {
            let list = alloc_list(ctx.2);
            list_push(ctx.2, list, level_key);
            list
        };
        let saved_op = b.6;
        let saved_idx = b.7;
        b.7 = level_list;
        let cont = expr_effects(f, b, ctx, base, mode, ret, prod);
        b.7 = saved_idx;
        if b.6 != saved_op && !prod.is_empty() {
            // The base issued a borrow for this element.  Extend the path
            // on its loan(s) and stamp the borrow op with it.  REF_ORIGIN
            // markers (a reborrow of a reference base) are skipped, so an
            // element borrow through a reference keeps a whole-value loan
            // on the reference variable; the typechecker gates the
            // fixed-array path today (indexing a reference to an array is
            // rejected), so no element-vs-element comparison is lost.
            // Bases that are not a path, field access, or nested index (a
            // call, match, or other expression) are left unstamped: their
            // loans stay whole-value, which is conservative and sound.
            if base_kind == EXPR_PATH || base_kind == EXPR_FIELD_ACCESS || base_kind == EXPR_INDEX {
                let mut stamped: i64 = NONE;
                let mut pi = 0usize;
                while pi < prod.len() {
                    match prod.get(pi) {
                        Some(entry) => {
                            if *entry >= 0 {
                                stamped = extend_loan_index(f, ctx, *entry, level_key);
                            }
                        }
                        None => break,
                    }
                    pi += 1;
                }
                if stamped != NONE {
                    stamp_op_index(f, b.6, stamped);
                }
            }
        }
        expr_effects(f, b, ctx, index, MODE_VALUE, ret, &mut Vec::new());
        b.6 = saved_op;
        return cont;
    }
    if kind == EXPR_FIELD_ACCESS {
        // Field access on a non-path base.  Reading a field through a
        // reference borrows the base and derives the value's origin from
        // the reference; reading a field of a value reads the base in the
        // enclosing mode; a borrow-position access puts the base in the
        // same mode so the loan lands on the root binding.
        let base = node_b(ctx.1, expr);
        let base_key = expr_ty_of(ctx.1, base);
        let base_kind = ty_kind_of(ctx.1, base_key);
        let base_mode = if base_kind == TYD_REF || base_kind == TYD_REF_MUT {
            if mode == MODE_VALUE {
                MODE_BORROW
            } else {
                mode
            }
        } else {
            mode
        };
        let mut base_prod: Vec<i64> = Vec::new();
        let cont = expr_effects(f, b, ctx, base, base_mode, ret, &mut base_prod);
        if mode == MODE_VALUE && (base_kind == TYD_REF || base_kind == TYD_REF_MUT) {
            prod.clear();
            append_list_unique(prod, &base_prod);
        }
        return cont;
    }
    cur(b)
}

/// Effects of a name/path expression.  A single-segment path names a
/// local binding; a longer path walks fields.  Reading through a
/// reference never moves; a value-position access of a linear value moves
/// it (a whole binding or a field sub-node); a borrow-position access
/// issues a loan on the root binding.
fn path_effects(f: &mut F, b: &mut B, ctx: &mut Ctx, expr: i64, mode: i64, prod: &mut Vec<i64>) -> i64 {
    let segs = node_b(ctx.1, expr);
    let first = list_first(ctx.2, segs);
    let binding = lookup_name(b, first);
    if binding == NONE {
        // A path to a declaration (const, unit variant, module member):
        // it has no local effects and produces a fresh value.
        return cur(b);
    }
    let row = binding_at(f, binding);
    let is_lin = row.2;
    let seg_count = list_len(ctx.2, segs);
    let span = expr_span(ctx.1, expr);

    if seg_count == 1 {
        if mode == MODE_BORROW || mode == MODE_MUT {
            return borrow_binding(f, b, ctx, binding, mode, span, prod);
        }
        if is_lin == 1 {
            check_pending_conflict(f, b, ctx, binding, MODE_VALUE, span);
            emit_op(f, OP_MOVE, binding, row.6, (0, 0, 0), span);
            return cur(b);
        }
        emit_op(f, OP_READ, binding, NONE, (0, 0, 0), span);
        if is_ref_key(ctx.1, row.1) {
            prod.clear();
            prod.push(ref_origin(binding));
        }
        return cur(b);
    }

    // Field chain: walk the base type to classify each access.
    walk_field_chain(f, b, ctx, expr, binding, mode, prod);
    cur(b)
}

/// Issues a borrow of `binding` (or a temporary loan on it when the
/// binding is a reference being reborrowed), checking pending conflicts
/// and writing the loan set for the value's origin into `prod`.
fn borrow_binding(f: &mut F, b: &mut B, ctx: &mut Ctx, binding: i64, mode: i64, span: (i64, i64, i64), prod: &mut Vec<i64>) -> i64 {
    let row = binding_at(f, binding);
    let op_kind = if mode == MODE_MUT { OP_BORROW_M } else { OP_BORROW };
    let loan_kind = if mode == MODE_MUT { L_MUT } else { L_SHARED };
    prod.clear();
    if row.3 == 1 {
        // A reference variable is reborrowed: the loan is on the
        // reference variable itself, and the value's origin is the
        // variable's own origin.
        let op = emit_op(f, op_kind, binding, NONE, (0, 0, NONE), span);
        check_pending_conflict(f, b, ctx, binding, mode, span);
        b.6 = op;
        let loan = alloc_loan(f, binding, loan_kind, 0, NONE);
        b.3.push(loan);
        prod.push(ref_origin(binding));
        return cur(b);
    }
    check_pending_conflict(f, b, ctx, binding, mode, span);
    let op = emit_op(f, op_kind, binding, row.6, (0, 0, NONE), span);
    b.6 = op;
    let loan = alloc_loan(f, binding, loan_kind, 0, NONE);
    b.3.push(loan);
    prod.push(loan);
    cur(b)
}

/// Emits the pending-conflict diagnostic for a new borrow of `binding`:
/// a shared borrow conflicts with pending exclusive loans; an exclusive
/// borrow conflicts with pending loans of either kind; a move conflicts
/// with any pending loan.  Same-statement conflicts cannot be resolved by
/// liveness, so they are hard errors at build time.  Element borrows are
/// compared by index: a pending loan on a provably-disjoint constant
/// element does not conflict.
fn check_pending_conflict(f: &mut F, b: &mut B, ctx: &mut Ctx, binding: i64, mode: i64, span: (i64, i64, i64)) {
    // The new borrow's element key: the innermost enclosing `arr[i]`
    // arm's index (NONE outside an index expression, IDX_DYN for a
    // non-constant index, or the constant level path).
    let new_key = b.7;
    let mut idx = 0usize;
    while idx < b.3.len() {
        let loan_id = match b.3.get(idx) {
            Some(id) => *id,
            None => break,
        };
        let loan = loan_at(f, loan_id);
        if loan.0 == binding && index_keys_conflict(ctx, new_key, loan.3) {
            // A move or an exclusive borrow conflicts with any pending
            // loan; a shared borrow only with pending exclusive loans.
            let conflicting = if mode == MODE_BORROW {
                loan.1 == L_MUT
            } else {
                true
            };
            if conflicting {
                let row = binding_at(f, binding);
                let name = format!("{}{}", name_text(ctx.0, row.0), index_suffix(ctx, new_key));
                if mode == MODE_VALUE {
                    push_error(ctx.3, &format!("cannot move '{}' while it is borrowed in the same expression", name), span.0, span.1, span.2);
                } else if mode == MODE_MUT {
                    if loan.1 == L_SHARED {
                        push_error(ctx.3, &format!("cannot mutably borrow '{}' while it is shared-borrowed in the same expression", name), span.0, span.1, span.2);
                    } else {
                        push_error(ctx.3, &format!("cannot mutably borrow '{}' twice in the same expression", name), span.0, span.1, span.2);
                    }
                } else {
                    push_error(ctx.3, &format!("cannot borrow '{}' while it is mutably borrowed in the same expression", name), span.0, span.1, span.2);
                }
            }
        }
        idx += 1;
    }
}

/// The element index key of an index expression: a constant integer
/// literal is provably that element; any other expression (a variable,
/// arithmetic, a call) is conservatively dynamic.
fn index_key_of_literal(ctx: &Ctx, expr: i64) -> i64 {
    if node_tag(ctx.1, expr) == NODE_EXPR && node_a(ctx.1, expr) == EXPR_LIT {
        let kind = node_b(ctx.1, expr);
        if kind == LIT_INT || kind == LIT_HEX {
            return node_c(ctx.1, expr);
        }
    }
    IDX_DYN
}

/// Whether two element-borrow index keys overlap: a whole-value loan
/// (NONE) overlaps every element, a dynamic index (IDX_DYN) is never
/// provably disjoint from anything, and two constant index paths overlap
/// when one is a prefix of the other (`a[i]` contains `a[i][j]`).
fn index_keys_conflict(ctx: &Ctx, a: i64, b: i64) -> bool {
    if a == NONE || b == NONE {
        return true;
    }
    if a == IDX_DYN || b == IDX_DYN {
        return true;
    }
    let na = list_len(ctx.2, a);
    let nb = list_len(ctx.2, b);
    let (short, nshort, long) = if na <= nb { (a, na, b) } else { (b, nb, a) };
    let mut idx = 0i64;
    while idx < nshort {
        if list_get(ctx.2, short, idx) != list_get(ctx.2, long, idx) {
            return false;
        }
        idx += 1;
    }
    true
}

/// The display suffix of an index key: `[0][1]` for a constant path,
/// `[?]` for a dynamic index, empty for a whole-value loan.
fn index_suffix(ctx: &Ctx, key: i64) -> String {
    if key == NONE {
        return String::new();
    }
    if key == IDX_DYN {
        return String::from("[?]");
    }
    let mut out = String::new();
    let count = list_len(ctx.2, key);
    let mut idx = 0i64;
    while idx < count {
        let value = list_get(ctx.2, key, idx);
        if value == IDX_DYN {
            out.push_str("[?]");
        } else {
            out.push('[');
            out.push_str(&value.to_string());
            out.push(']');
        }
        idx += 1;
    }
    out
}

/// Extends a loan's element index path with the current level and returns
/// the new key: a non-constant level poisons the whole path to IDX_DYN
/// and a fresh loan starts at `[level]`.  A nested `a[i][j]` never reaches
/// the extension branch: the inner index arm restores the borrow-op cursor
/// before the outer arm reads it, so a nested borrow keeps the inner
/// `[i]` — conservative, see the loans-table note.
fn extend_loan_index(f: &mut F, ctx: &mut Ctx, loan: i64, level_key: i64) -> i64 {
    let cur = loan_at(f, loan).3;
    let new_key = if level_key == IDX_DYN {
        IDX_DYN
    } else if cur == NONE {
        let list = alloc_list(ctx.2);
        list_push(ctx.2, list, level_key);
        list
    } else if cur == IDX_DYN {
        IDX_DYN
    } else {
        let list = alloc_list(ctx.2);
        let count = list_len(ctx.2, cur);
        let mut idx = 0i64;
        while idx < count {
            let value = list_get(ctx.2, cur, idx);
            list_push(ctx.2, list, value);
            idx += 1;
        }
        list_push(ctx.2, list, level_key);
        list
    };
    if let Some(row) = f.3.get_mut(loan as usize) {
        row.3 = new_key;
    }
    new_key
}

/// Stamps the element index key onto a borrow op (op row aux slot 2), the
/// fact `conflicts_at` reads for the new borrow's element.
fn stamp_op_index(f: &mut F, op: i64, key: i64) {
    if op < 0 {
        return;
    }
    if let Some(row) = f.4.get_mut(op as usize) {
        row.5 = key;
    }
}

/// Walks the field segments of a path starting at `from`, advancing `path`
/// through every linear struct field.  Returns the final key and path.
fn walk_field_segments(f: &mut F, ctx: &mut Ctx, segs: i64, from: i64, mut cur_key: i64, mut cur_path: i64) -> (i64, i64) {
    let count = list_len(ctx.2, segs);
    let mut idx = from;
    while idx < count {
        let field = list_get(ctx.2, segs, idx);
        let base_kind = ty_kind_of(ctx.1, cur_key);
        if base_kind == TYD_STRUCT {
            let fkey = field_key_of(ctx, cur_key, field);
            if is_linear_key(ctx, fkey) == 1 && cur_path != NONE {
                cur_path = child_path(f, cur_path, field);
            }
            cur_key = fkey;
        }
        idx += 1;
    }
    (cur_key, cur_path)
}

/// The dotted display name of a path: `base.seg1.seg2...` built from the
/// binding's name and the path's field segments (the binding itself is
/// segment 0).  Used by diagnostics that have no tracked path node.
fn dotted_seg_name(ctx: &mut Ctx, base: i64, segs: i64, count: i64) -> String {
    let mut text = name_text(ctx.0, base);
    let mut si = 1i64;
    while si < count {
        text = format!("{}.{}", text, name_text(ctx.0, list_get(ctx.2, segs, si)));
        si += 1;
    }
    text
}

/// Walks a multi-segment path starting at `binding`, deciding for each
/// field whether the access moves a linear sub-node, reads through a
/// reference, or copies a non-linear field.  A borrow of the final value
/// writes its loan into `prod`.
fn walk_field_chain(f: &mut F, b: &mut B, ctx: &mut Ctx, expr: i64, binding: i64, mode: i64, prod: &mut Vec<i64>) {
    let row = binding_at(f, binding);
    let segs = node_b(ctx.1, expr);
    let count = list_len(ctx.2, segs);
    let span = expr_span(ctx.1, expr);
    let bkind = ty_kind_of(ctx.1, row.1);
    let through_mut = bkind == TYD_REF_MUT;
    let through_shared = bkind == TYD_REF;
    let mut cur_key = row.1;
    let cur_path = row.6;
    // References cannot be stored inside structs, so a reference only ever
    // appears at the root of a path.  Strip the binding's own reference
    // levels from the key WITHOUT consuming path segments (`r.tag` is the
    // binding plus one field segment): the linearity guard must key on the
    // accessed field's type, not on the referent struct's type.
    let start = 1i64;
    while ty_kind_of(ctx.1, cur_key) == TYD_REF || ty_kind_of(ctx.1, cur_key) == TYD_REF_MUT {
        cur_key = ty_elem_of(ctx.1, cur_key);
    }
    let (final_key, final_path) = walk_field_segments(f, ctx, segs, start, cur_key, cur_path);
    let final_is_lin = is_linear_key(ctx, final_key);
    if mode == MODE_BORROW || mode == MODE_MUT {
        // Borrow of the final accessed value: the loan is on the root
        // binding (conservative but sound — moving the root would
        // invalidate the borrowed field).
        let op_kind = if mode == MODE_MUT { OP_BORROW_M } else { OP_BORROW };
        let loan_kind = if mode == MODE_MUT { L_MUT } else { L_SHARED };
        check_pending_conflict(f, b, ctx, binding, mode, span);
        let op = emit_op(f, op_kind, binding, NONE, (0, 0, NONE), span);
        b.6 = op;
        let loan = alloc_loan(f, binding, loan_kind, 0, NONE);
        b.3.push(loan);
        prod.clear();
        prod.push(loan);
        return;
    }
    if final_is_lin == 1 {
        if through_mut {
            // A value-position access of a linear field through a `&mut`
            // reference consumes the referent's handle: move the referent
            // sub-node (a local owner, or a `&mut` parameter's tracked
            // referent) instead of copying it.  This is the sanctioned
            // `deallocate(ref.field)` form.
            let owner = referent_binding_of(f, binding);
            let rroot = if owner != NONE {
                root_path_of(f, owner)
            } else {
                root_path_of(f, binding)
            };
            if rroot < 0 {
                // The referent is not a tracked local (e.g. a `&mut` of a
                // temporary such as `&mut make_holder()`): consuming the
                // handle out of it would silently duplicate the linear
                // value.  Inconclusive consumption is a compile error.
                let text = dotted_seg_name(ctx, row.0, segs, count);
                push_error(ctx.3, &format!("cannot consume linear value '{}' through a mutable reference to an untracked temporary; bind the referent to a local first", text), span.0, span.1, span.2);
                return;
            }
            let elem_key = ty_elem_of(ctx.1, row.1);
            let (_, rpath) = walk_field_segments(f, ctx, segs, 1, elem_key, rroot);
            if rpath == NONE || rpath == rroot {
                // Consuming the whole referent (or an unresolvable field
                // chain) through a `&mut` would move the caller's storage
                // out from under the borrow; reject rather than guess.
                let text = dotted_seg_name(ctx, row.0, segs, count);
                push_error(ctx.3, &format!("cannot consume the whole value '{}' through a mutable reference; move the referent into a local and consume that instead", text), span.0, span.1, span.2);
                return;
            }
            let target = if owner != NONE { owner } else { binding };
            check_pending_conflict(f, b, ctx, target, MODE_VALUE, span);
            emit_op(f, OP_MOVE, target, rpath, (0, 0, 0), span);
            return;
        } else if through_shared {
            // Copying a linear value out of a shared reference duplicates
            // the handle; the only sanctioned consumption goes through
            // `&mut`, never `&`.
            let text = dotted_seg_name(ctx, row.0, segs, count);
            push_error(ctx.3, &format!("cannot copy linear value '{}' out of a shared reference", text), span.0, span.1, span.2);
            return;
        }
    }
    if final_is_lin == 1 && final_path != NONE {
        check_pending_conflict(f, b, ctx, binding, MODE_VALUE, span);
        emit_op(f, OP_MOVE, binding, final_path, (0, 0, 0), span);
        return;
    }
    emit_op(f, OP_READ, binding, NONE, (0, 0, 0), span);
}

/// The substituted key of a struct field named `field` on the type `key`.
fn field_key_of(ctx: &mut Ctx, key: i64, field: i64) -> i64 {
    let sym = ty_sym_of(ctx.1, key);
    if sym == NONE {
        return NONE;
    }
    let decl = node_c(ctx.1, sym);
    if decl == NONE || node_a(ctx.1, decl) != ITEM_STRUCT {
        return NONE;
    }
    let fields = node_e(ctx.1, decl);
    let count = list_len(ctx.2, fields);
    let mut idx = 0i64;
    while idx < count {
        let fnode = list_get(ctx.2, fields, idx);
        if node_a(ctx.1, fnode) == field {
            let declared = ty_key_of(ctx.1, node_b(ctx.1, fnode));
            return subst_declared(ctx, decl, key, declared);
        }
        idx += 1;
    }
    NONE
}

/// Substitutes a declared member type against the concrete type arguments
/// of `key` (the struct or enum instantiation), matching the declaration's
/// type parameters by key.
fn subst_declared(ctx: &mut Ctx, decl: i64, key: i64, declared: i64) -> i64 {
    if declared == NONE {
        return declared;
    }
    let params = node_f(ctx.1, decl);
    let args = ty_args_of(ctx.1, key);
    let pcount = list_len(ctx.2, params);
    let acount = list_len(ctx.2, args);
    if pcount == 0 || pcount != acount {
        return declared;
    }
    let mut from: Vec<i64> = Vec::new();
    let mut to: Vec<i64> = Vec::new();
    let mut idx = 0i64;
    while idx < pcount {
        let param = list_get(ctx.2, params, idx);
        if node_tag(ctx.1, param) == NODE_TY && node_a(ctx.1, param) == TY_PARAM {
            from.push(ty_key_of(ctx.1, param));
            to.push(list_get(ctx.2, args, idx));
        }
        idx += 1;
    }
    if from.is_empty() {
        return declared;
    }
    subst_key(ctx.1, ctx.2, declared, &from, &to)
}

fn ty_args_of(nodes: &[i64], key: i64) -> i64 {
    let row = find_tyinfo(nodes, key);
    if row == NONE {
        NONE
    } else {
        node_d(nodes, row)
    }
}

// ---------------------------------------------------------------------------
// Assignment targets.
//
// An assignment target is a place: a plain name (whole-value assign), a
// field chain on a mutable local (a sub-node assign), or a field write
// through a `&mut` reference.  Each decomposes to `(binding, path, kind,
// owner)`; the op carries `(kind, owner)` in its aux slots and the
// referent's sub-node in its path slot.
// ---------------------------------------------------------------------------

/// Decomposes an assignment target expression.  `kind` is 0 (whole-value
/// assign of `binding`), 1 (field write through a `&mut` reference;
/// `binding` is the reference variable, `owner` the referent's local
/// value binding when statically a single one, else NONE), or 2 (field
/// assign on a non-reference base; `binding` is the root local and
/// `path` its sub-node).
fn assign_target_of(f: &mut F, b: &mut B, ctx: &mut Ctx, target: i64) -> (i64, i64, i64, i64) {
    if node_tag(ctx.1, target) != NODE_EXPR {
        return (NONE, NONE, 0, NONE);
    }
    let kind = node_a(ctx.1, target);
    if kind == EXPR_PATH {
        let segs = node_b(ctx.1, target);
        let count = list_len(ctx.2, segs);
        let first = list_first(ctx.2, segs);
        let binding = lookup_name(b, first);
        if count == 1 {
            return (binding, NONE, 0, NONE);
        }
        if binding == NONE {
            return (NONE, NONE, 1, NONE);
        }
        return field_assign_target(f, ctx, binding, segs);
    }
    if kind == EXPR_FIELD_ACCESS {
        let base = node_b(ctx.1, target);
        let field = node_c(ctx.1, target);
        if node_tag(ctx.1, base) == NODE_EXPR && node_a(ctx.1, base) == EXPR_PATH {
            let segs = node_b(ctx.1, base);
            let count = list_len(ctx.2, segs);
            let first = list_first(ctx.2, segs);
            let binding = lookup_name(b, first);
            if binding == NONE {
                expr_effects(f, b, ctx, base, MODE_MUT, NONE, &mut Vec::new());
                return (NONE, NONE, 1, NONE);
            }
            if count == 1 {
                let row = binding_at(f, binding);
                let bkind = ty_kind_of(ctx.1, row.1);
            if bkind == TYD_REF || bkind == TYD_REF_MUT {
                let owner = referent_binding_of(f, binding);
                let self_owned = bkind == TYD_REF_MUT && root_path_of(f, binding) >= 0;
                let cur_key = ty_elem_of(ctx.1, row.1);
                let mut cur_path = if owner != NONE {
                    root_path_of(f, owner)
                } else if self_owned {
                    root_path_of(f, binding)
                } else {
                    NONE
                };
                let fkey = field_key_of(ctx, cur_key, field);
                if is_linear_key(ctx, fkey) == 1 && cur_path != NONE {
                    cur_path = child_path(f, cur_path, field);
                }
                return (binding, cur_path, 1, if owner != NONE { owner } else if self_owned { binding } else { NONE });
            }
                let cur_key = row.1;
                let mut cur_path = row.6;
                let fkey = field_key_of(ctx, cur_key, field);
                if is_linear_key(ctx, fkey) == 1 && cur_path != NONE {
                    cur_path = child_path(f, cur_path, field);
                }
                return (binding, cur_path, 2, NONE);
            }
            return field_assign_target(f, ctx, binding, segs);
        }
        // An ephemeral base (`(try &mut arr[i]).field`): its borrow
        // effects are recorded in exclusive mode and the write has no
        // statically-known referent to track linearly.
        expr_effects(f, b, ctx, base, MODE_MUT, NONE, &mut Vec::new());
        return (NONE, NONE, 1, NONE);
    }
    (NONE, NONE, 0, NONE)
}

/// The target form of a field chain rooted at `binding` (the chain
/// excludes the first segment).  When the root is a `&mut` reference the
/// write goes through it: the conflict checks key on the reference
/// binding, and the linear rule keys on the referent's sub-node when the
/// referent is statically a single local value binding.
fn field_assign_target(f: &mut F, ctx: &mut Ctx, binding: i64, segs: i64) -> (i64, i64, i64, i64) {
    let row = binding_at(f, binding);
    let bkind = ty_kind_of(ctx.1, row.1);
    if bkind == TYD_REF || bkind == TYD_REF_MUT {
        let owner = referent_binding_of(f, binding);
        let self_owned = bkind == TYD_REF_MUT && root_path_of(f, binding) >= 0;
        let mut cur_key = ty_elem_of(ctx.1, row.1);
        let mut cur_path = if owner != NONE {
            root_path_of(f, owner)
        } else if self_owned {
            root_path_of(f, binding)
        } else {
            NONE
        };
        let count = list_len(ctx.2, segs);
        let mut idx = 1i64;
        while idx < count {
            let field = list_get(ctx.2, segs, idx);
            let fkey = field_key_of(ctx, cur_key, field);
            if is_linear_key(ctx, fkey) == 1 && cur_path != NONE {
                cur_path = child_path(f, cur_path, field);
            }
            cur_key = fkey;
            idx += 1;
        }
        return (binding, cur_path, 1, if owner != NONE { owner } else if self_owned { binding } else { NONE });
    }
    let mut cur_key = row.1;
    let mut cur_path = row.6;
    let count = list_len(ctx.2, segs);
    let mut idx = 1i64;
    while idx < count {
        let field = list_get(ctx.2, segs, idx);
        let fkey = field_key_of(ctx, cur_key, field);
        if is_linear_key(ctx, fkey) == 1 && cur_path != NONE {
            cur_path = child_path(f, cur_path, field);
        }
        cur_key = fkey;
        idx += 1;
    }
    (binding, cur_path, 2, NONE)
}

/// The local value binding a reference binding's origin ultimately
/// borrows, when that is statically a single local value (NONE when the
/// referent is caller-owned or ambiguous).  Read from the op that set
/// the reference's origin, following reborrow chains; the origin loans
/// are facts already on the op table.
fn referent_binding_of(f: &F, binding: i64) -> i64 {
    let mut owner = NONE;
    let mut ok = true;
    let mut visited: Vec<i64> = Vec::new();
    origin_owners_of(f, binding, &mut owner, &mut ok, &mut visited);
    if ok {
        owner
    } else {
        NONE
    }
}

/// Collects the local value bindings a reference binding's origin
/// ultimately borrows into `owner` (NONE until the first is found).
/// `ok` turns false when the origin touches a caller-owned referent (a
/// reference parameter) or more than one local value.
fn origin_owners_of(f: &F, binding: i64, owner: &mut i64, ok: &mut bool, visited: &mut Vec<i64>) {
    if !*ok || list_has(visited, binding) {
        return;
    }
    visited.push(binding);
    let mut op = 0i64;
    while op < f.4.len() as i64 {
        let row = op_at(f, op);
        if row.1 == binding && (row.0 == OP_BIND || row.0 == OP_ASSIGN) && row.3 == 1 {
            let loans = op_loans_at(f, op);
            let mut idx = 0usize;
            while idx < loans.len() {
                let entry = match loans.get(idx) {
                    Some(value) => *value,
                    None => break,
                };
                if is_ref_origin(entry) {
                    origin_owners_of(f, -1 - entry, owner, ok, visited);
                } else {
                    let lrow = loan_at(f, entry);
                    let o = lrow.0;
                    if o < 0 {
                        idx += 1;
                        continue;
                    }
                    let orow = binding_at(f, o);
                    if orow.3 == 1 {
                        if orow.4 == 1 {
                            // A reference parameter: its referent is
                            // caller-owned, not a local here.
                            *ok = false;
                        } else {
                            origin_owners_of(f, o, owner, ok, visited);
                        }
                    } else if *owner == NONE {
                        *owner = o;
                    } else if *owner != o {
                        *ok = false;
                    }
                }
                idx += 1;
            }
            return;
        }
        op += 1;
    }
}

/// The diagnostic name of an assignment target: `binding.field` for a
/// sub-node (walking the trie upward), or the bare binding name.
fn dotted_name_of(f: &F, ctx: &mut Ctx, binding: i64, path: i64) -> String {
    let base = name_text(ctx.0, binding_at(f, binding).0);
    if path == NONE || is_root_path(f, path) {
        return base;
    }
    let mut names: Vec<i64> = Vec::new();
    let mut p = path;
    while p != NONE && !is_root_path(f, p) {
        names.push(path_at(f, p).1);
        p = path_at(f, p).0;
    }
    let mut text = base;
    let mut idx = names.len();
    while idx > 0 {
        idx -= 1;
        match names.get(idx) {
            Some(name) => text = format!("{}.{}", text, name_text(ctx.0, *name)),
            None => break,
        }
    }
    text
}

/// True when every directly-tracked child of `parent` is live (Moved or
/// Partial children block their parent's restoration).
fn all_children_live(f: &F, state: &[i64], parent: i64) -> bool {
    let mut idx = 0usize;
    while idx < f.2.len() {
        if path_at(f, idx as i64).0 == parent {
            let cst = state_at(state, idx as i64);
            if cst == ST_MOVED || cst == ST_PARTIAL {
                return false;
            }
        }
        idx += 1;
    }
    true
}

/// Effects of unary expressions.  `&x` and `&mut x` put the operand in
/// borrow position; the operand's loans flow into `prod`.  The unary
/// operator decides the operand's mode (a `&` borrows, negation reads),
/// so the mode of the unary expression itself is never consulted.
fn unary_effects(f: &mut F, b: &mut B, ctx: &mut Ctx, expr: i64, ret: i64, prod: &mut Vec<i64>) -> i64 {
    let op = node_b(ctx.1, expr);
    let operand = node_c(ctx.1, expr);
    if op == UN_REF {
        return expr_effects(f, b, ctx, operand, MODE_BORROW, ret, prod);
    }
    if op == UN_REF_MUT {
        return expr_effects(f, b, ctx, operand, MODE_MUT, ret, prod);
    }
    expr_effects(f, b, ctx, operand, MODE_VALUE, ret, prod)
}

/// Effects of binary expressions: both operands are value positions.
/// The result's loans are the operands' loans (empty for the arithmetic
/// and logical operators the typechecker admits here).
fn binary_effects(f: &mut F, b: &mut B, ctx: &mut Ctx, expr: i64, ret: i64, prod: &mut Vec<i64>) -> i64 {
    let lhs = node_c(ctx.1, expr);
    let rhs = node_d(ctx.1, expr);
    expr_effects(f, b, ctx, lhs, MODE_VALUE, ret, prod);
    expr_effects(f, b, ctx, rhs, MODE_VALUE, ret, prod)
}

/// The trait method declaration behind a call, or NONE.  A deferred trait
/// call (generic receiver) carries no concrete instance row; its argument
/// shapes come from the trait method's declared signature, the same
/// source fact the typechecker's deferred path canonicalized.
fn trait_method_of(ctx: &mut Ctx, expr: i64) -> i64 {
    let trow = find_trait_call(ctx.1, expr);
    if trow == NONE {
        return NONE;
    }
    let trait_sym = trait_call_trait(ctx.1, trow);
    let method_name = trait_call_method(ctx.1, trow);
    if trait_sym == NONE || method_name == NONE {
        return NONE;
    }
    let trait_item = node_c(ctx.1, trait_sym);
    if trait_item == NONE || node_tag(ctx.1, trait_item) != NODE_ITEM || node_a(ctx.1, trait_item) != ITEM_TRAIT {
        return NONE;
    }
    let methods = node_e(ctx.1, trait_item);
    let count = list_len(ctx.2, methods);
    let mut idx = 0i64;
    while idx < count {
        let method = list_get(ctx.2, methods, idx);
        if node_tag(ctx.1, method) == NODE_FN && node_a(ctx.1, method) == method_name {
            return method;
        }
        idx += 1;
    }
    NONE
}

/// The consumption mode of a declared parameter, read from its declared
/// type node: `&T` and `&mut T` are borrow positions, everything else is
/// a value position.
fn param_mode_of(nodes: &[i64], ty_node: i64) -> i64 {
    if node_tag(nodes, ty_node) != NODE_TY {
        return MODE_VALUE;
    }
    let kind = node_a(nodes, ty_node);
    if kind == TY_REF {
        MODE_BORROW
    } else if kind == TY_REF_MUT {
        MODE_MUT
    } else {
        MODE_VALUE
    }
}

/// Whether a declared return type is a reference (shared, exclusive, or a
/// slice), read from the return type node.
fn ret_is_ref_node(nodes: &[i64], ty_node: i64) -> i64 {
    if node_tag(nodes, ty_node) != NODE_TY {
        return 0;
    }
    let kind = node_a(nodes, ty_node);
    if kind == TY_REF || kind == TY_REF_MUT || kind == TY_SLICE {
        1
    } else {
        0
    }
}

/// Effects of a call.  Reference parameters borrow their arguments
/// (temporary loans, or reborrows when the argument is a reference
/// variable); by-value parameters consume theirs (a linear argument is
/// moved).  Only a call returning a reference forwards the union of its
/// reference arguments' loans into `prod`; a non-reference-returning call
/// clears it (its borrows are temporary and die at the statement).  A
/// deferred trait call (no concrete instance) is classified from the
/// trait method's declared signature.
/// Seeds `prod` with the productions of the arguments whose parameter
/// positions the callee's returned borrow can trace to (its callee-origin
/// summary).  A callee without a summary — a signature-only or native
/// declaration, one whose returned-borrow trace could not be resolved, or
/// a `None` key (the deferred trait path, whose default-body summary
/// cannot cover impl overrides) — falls back to the conservative union of
/// every argument's production: sound, and as precise as the available
/// facts allow.
fn call_origin_of(ctx: &mut Ctx, summary_key: Option<i64>, arg_prods: &[Vec<i64>], prod: &mut Vec<i64>) {
    let positions = match summary_key {
        Some(fn_node) => summary_of(ctx.4, fn_node),
        None => None,
    };
    match positions {
        Some(positions) => {
            let mut idx = 0usize;
            while idx < positions.len() {
                match positions.get(idx) {
                    Some(position) => {
                        if let Some(arg_prod) = arg_prods.get(*position as usize) {
                            append_prod_unique(prod, arg_prod);
                        }
                    }
                    None => break,
                }
                idx += 1;
            }
        }
        None => {
            let mut idx = 0usize;
            while idx < arg_prods.len() {
                match arg_prods.get(idx) {
                    Some(arg_prod) => append_prod_unique(prod, arg_prod),
                    None => break,
                }
                idx += 1;
            }
        }
    }
}

fn call_effects(f: &mut F, b: &mut B, ctx: &mut Ctx, expr: i64, ret: i64, prod: &mut Vec<i64>) -> i64 {
    let inst = expr_sym_of(ctx.1, expr);
    let args = node_d(ctx.1, expr);
    let argc = list_len(ctx.2, args);
    if node_tag(ctx.1, inst) != NODE_INST {
        let method = trait_method_of(ctx, expr);
        if method == NONE {
            push_internal(ctx.3, "internal error: call without an instance or trait-dispatch row in borrow checking");
            let mut idx = 0i64;
            while idx < argc {
                expr_effects(f, b, ctx, list_get(ctx.2, args, idx), MODE_VALUE, ret, prod);
                idx += 1;
            }
            prod.clear();
            return cur(b);
        }
        let params = node_c(ctx.1, method);
        let mut arg_prods: Vec<Vec<i64>> = Vec::new();
        let mut idx = 0i64;
        while idx < argc {
            let arg = list_get(ctx.2, args, idx);
            let pty = node_b(ctx.1, list_get(ctx.2, params, idx));
            let mode = param_mode_of(ctx.1, pty);
            // Each argument borrows into its own fresh production so no
            // argument's `borrow_binding` clear can wipe an earlier
            // argument's loans.  When the call returns a reference, only
            // the summarized parameters' productions seed the returned
            // borrow's origin (see `call_origin_of`); the productions are
            // buffered so the selection happens after every argument has
            // been evaluated.
            let mut arg_prod: Vec<i64> = Vec::new();
            expr_effects(f, b, ctx, arg, mode, ret, &mut arg_prod);
            arg_prods.push(arg_prod);
            idx += 1;
        }
        if ret_is_ref_node(ctx.1, node_d(ctx.1, method)) == 0 {
            prod.clear();
        } else {
            // The deferred trait path is deliberately conservative (`None`
            // key): the trait declaration's default-body summary cannot
            // cover an impl override whose returned borrow traces to a
            // different parameter, so the returned borrow's origin is the
            // union of every argument's production.
            call_origin_of(ctx, None, &arg_prods, prod);
        }
        return cur(b);
    }
    let params = inst_params_of(ctx.1, inst);
    let mut arg_prods: Vec<Vec<i64>> = Vec::new();
    let mut idx = 0i64;
    while idx < argc {
        let arg = list_get(ctx.2, args, idx);
        let pkey = list_get(ctx.2, params, idx);
        let pkind = ty_kind_of(ctx.1, pkey);
        let mode = if pkind == TYD_REF {
            MODE_BORROW
        } else if pkind == TYD_REF_MUT {
            MODE_MUT
        } else {
            MODE_VALUE
        };
        // Each argument borrows into its own fresh production so no
        // argument's `borrow_binding` clear can wipe an earlier
        // argument's loans.  When the call returns a reference, only the
        // summarized parameters' productions seed the returned borrow's
        // origin (see `call_origin_of`); the productions are buffered so
        // the selection happens after every argument has been evaluated.
        let mut arg_prod: Vec<i64> = Vec::new();
        expr_effects(f, b, ctx, arg, mode, ret, &mut arg_prod);
        arg_prods.push(arg_prod);
        idx += 1;
    }
    let ret_key = inst_ret_of(ctx.1, inst);
    if is_ref_key(ctx.1, ret_key) {
        // Slot a of the instance row is the callee's function node (the
        // declaration for user functions, the symbol for natives, which
        // has no summary and falls back to the conservative union).
        call_origin_of(ctx, Some(inst_fn_of(ctx.1, inst)), &arg_prods, prod);
    } else {
        prod.clear();
    }
    cur(b)
}

/// Effects of a struct or variant literal: each field value is a value
/// position (a linear field's value is moved; the attached expression
/// types decide).  The field values' loans flow into `prod`.
fn struct_effects(f: &mut F, b: &mut B, ctx: &mut Ctx, expr: i64, ret: i64, prod: &mut Vec<i64>) -> i64 {
    let values = node_d(ctx.1, expr);
    let count = list_len(ctx.2, values);
    let mut idx = 0i64;
    while idx < count {
        expr_effects(f, b, ctx, list_get(ctx.2, values, idx), MODE_VALUE, ret, prod);
        idx += 1;
    }
    cur(b)
}

/// Effects of an array literal: elements are value positions; their
/// loans flow into `prod`.
fn array_effects(f: &mut F, b: &mut B, ctx: &mut Ctx, expr: i64, ret: i64, prod: &mut Vec<i64>) -> i64 {
    let elems = node_b(ctx.1, expr);
    let count = list_len(ctx.2, elems);
    let mut idx = 0i64;
    while idx < count {
        expr_effects(f, b, ctx, list_get(ctx.2, elems, idx), MODE_VALUE, ret, prod);
        idx += 1;
    }
    cur(b)
}

/// The root binding of a path expression, or NONE when the path does not
/// name a local (a declaration reference or a multi-segment type path).
fn path_root_binding_of(ctx: &mut Ctx, b: &B, expr: i64) -> i64 {
    if node_tag(ctx.1, expr) != NODE_EXPR || node_a(ctx.1, expr) != EXPR_PATH {
        return NONE;
    }
    let segs = node_b(ctx.1, expr);
    let first = list_first(ctx.2, segs);
    lookup_name(b, first)
}

/// Wraps a single statement in a fresh one-element statement list.  The
/// parser stores each match arm body as one statement (not a list), and
/// `build_list` operates on statement lists, so an arm body must be
/// wrapped before building.
fn wrap_stmt_list(lists: &mut Vec<Vec<i64>>, stmt: i64) -> i64 {
    let list = alloc_list(lists);
    list_push(lists, list, stmt);
    list
}

/// Effects of a match.  The scrutinee is a value position (a linear
/// scrutinee is moved into the match); each arm's pattern binds names
/// scoped to the arm, and the arm body is a sub-CFG merging at a join.
/// The match's production is the union of the arms' productions.
fn match_effects(f: &mut F, b: &mut B, ctx: &mut Ctx, expr: i64, ret: i64, prod: &mut Vec<i64>) -> i64 {
    let scrutinee = node_b(ctx.1, expr);
    let arms = node_c(ctx.1, expr);
    let span = expr_span(ctx.1, expr);
    let mut scrut_prod: Vec<i64> = Vec::new();
    let cont = expr_effects(f, b, ctx, scrutinee, MODE_VALUE, ret, &mut scrut_prod);
    let scrut_root = path_root_binding_of(ctx, b, scrutinee);
    let join = new_block(f, b, BLK_JOIN, NONE, span);
    let count = list_len(ctx.2, arms);
    let mut idx = 0i64;
    while idx < count {
        let arm = list_get(ctx.2, arms, idx);
        let pat = node_a(ctx.1, arm);
        let body_stmt = node_b(ctx.1, arm);
        let arm_entry = new_block(f, b, BLK_STMT, arm, span);
        let scrut = (&scrut_prod[..], scrut_root);
        pattern_effects(f, b, ctx, pat, scrut);
        // The parser stores each arm body as a single statement, not a
        // statement list; wrap it before building so the arm's effects
        // (moves, borrows, returns) are actually checked.
        let body = wrap_stmt_list(ctx.2, body_stmt);
        let body_entry = build_list(f, b, ctx, body, join, ret, prod);
        add_edge(f, arm_entry, body_entry);
        add_edge(f, cont, arm_entry);
        idx += 1;
    }
    resume(f, b, join);
    join
}

/// Binds the names a pattern introduces, scoped to the current build
/// position.  `scrut` carries the scrutinee's origin loans and root
/// binding: reference-typed binders (rest patterns) inherit the
/// scrutinee's loans, or borrow the scrutinee itself when it is a value.
fn pattern_effects(f: &mut F, b: &mut B, ctx: &mut Ctx, pat: i64, scrut: (&[i64], i64)) {
    if node_tag(ctx.1, pat) != NODE_PAT {
        return;
    }
    let kind = node_a(ctx.1, pat);
    let span = (node_file(ctx.1, pat), node_start(ctx.1, pat), node_end(ctx.1, pat));
    if kind == PAT_BIND {
        let name = node_b(ctx.1, pat);
        let key = pat_ty_of(ctx.1, pat);
        bind_pattern_name(f, b, ctx, name, key, span, scrut);
        return;
    }
    if kind == PAT_VARIANT {
        let payloads = node_c(ctx.1, pat);
        let count = list_len(ctx.2, payloads);
        let mut idx = 0i64;
        while idx < count {
            pattern_effects(f, b, ctx, list_get(ctx.2, payloads, idx), scrut);
            idx += 1;
        }
        return;
    }
    if kind == PAT_ARRAY {
        let elems = node_b(ctx.1, pat);
        let count = list_len(ctx.2, elems);
        let mut idx = 0i64;
        while idx < count {
            pattern_effects(f, b, ctx, list_get(ctx.2, elems, idx), scrut);
            idx += 1;
        }
        let rest = node_c(ctx.1, pat);
        if rest != NONE {
            let rest_key = pat_rest_key_of(ctx.1, pat);
            bind_pattern_name(f, b, ctx, rest, rest_key, span, scrut);
        }
    }
}

fn bind_pattern_name(f: &mut F, b: &mut B, ctx: &mut Ctx, name: i64, key: i64, span: (i64, i64, i64), scrut: (&[i64], i64)) {
    let flags = (is_linear_key(ctx, key), if is_ref_key(ctx.1, key) { 1 } else { 0 }, 0, 0);
    let binding = bind_var(f, b, ctx, name, key, flags, span);
    let op = emit_op(f, OP_BIND, binding, NONE, (flags.1, 1, 0), span);
    if flags.1 == 1 {
        let mut loans: Vec<i64> = Vec::new();
        let mut idx = 0usize;
        while idx < scrut.0.len() {
            match scrut.0.get(idx) {
                Some(entry) => loans.push(*entry),
                None => break,
            }
            idx += 1;
        }
        if loans.is_empty() && scrut.1 != NONE {
            // A rest binder over a value scrutinee borrows the scrutinee
            // itself.
            let loan = alloc_loan(f, scrut.1, L_SHARED, 0, NONE);
            loans.push(loan);
        }
        set_op_loans(f, op, &loans);
    }
}

/// Effects of a `try`: the operand is consumed, and the error path is a
/// scope-diverging exit of the enclosing function.
fn try_effects(f: &mut F, b: &mut B, ctx: &mut Ctx, expr: i64, ret: i64, prod: &mut Vec<i64>) -> i64 {
    let inner = node_b(ctx.1, expr);
    let span = expr_span(ctx.1, expr);
    let cont = expr_effects(f, b, ctx, inner, MODE_VALUE, ret, prod);
    emit_op(f, OP_EXIT, NONE, NONE, (0, EX_TRY, 0), span);
    cont
}

// ---------------------------------------------------------------------------
// Entry: walk every function in the program (entry file plus every
// external module) and check it.
// ---------------------------------------------------------------------------

/// The pipeline entry point.  Returns true when no borrow or consumption
/// diagnostic was produced.
/// Collects every function node reachable from an item list (free
/// functions, native declarations, impl methods, trait method bodies) in
/// declaration order, descending into modules.  The summary fixpoint
/// visits every body before any call site reads a summary, so a function
/// defined after its caller is still summarized.
fn collect_fn_nodes(nodes: &[i64], lists: &[Vec<i64>], list: i64, out: &mut Vec<i64>) {
    let count = list_len(lists, list);
    let mut idx = 0i64;
    while idx < count {
        let item = list_get(lists, list, idx);
        if node_tag(nodes, item) == NODE_ITEM {
            let kind = node_a(nodes, item);
            if kind == ITEM_MODULE {
                collect_fn_nodes(nodes, lists, node_e(nodes, item), out);
            } else if kind == ITEM_FUN || kind == ITEM_NATIVE_FUN {
                out.push(node_d(nodes, item));
            } else if kind == ITEM_IMPL {
                collect_fn_list_nodes(nodes, lists, node_f(nodes, item), out);
            } else if kind == ITEM_TRAIT {
                collect_fn_list_nodes(nodes, lists, node_e(nodes, item), out);
            }
        }
        idx += 1;
    }
}

/// The fn-node variant of `collect_fn_nodes` for method lists (impl and
/// trait method lists hold NODE_FN rows directly, matching the walk in
/// `check_fn_list`).
fn collect_fn_list_nodes(nodes: &[i64], lists: &[Vec<i64>], list: i64, out: &mut Vec<i64>) {
    let count = list_len(lists, list);
    let mut idx = 0i64;
    while idx < count {
        let item = list_get(lists, list, idx);
        if node_tag(nodes, item) == NODE_FN {
            out.push(item);
        }
        idx += 1;
    }
}

/// The largest parameter count across the given function nodes (0 when
/// there are none); it sizes the summary-fixpoint iteration cap.
fn max_fn_params(nodes: &[i64], lists: &[Vec<i64>], fns: &[i64]) -> i64 {
    let mut max = 0i64;
    let mut idx = 0usize;
    while idx < fns.len() {
        match fns.get(idx) {
            Some(fn_node) => {
                let count = list_len(lists, node_c(nodes, *fn_node));
                if count > max {
                    max = count;
                }
            }
            None => break,
        }
        idx += 1;
    }
    max
}

pub fn borrow_check(
    names: &mut Vec<String>,
    nodes: &mut Vec<i64>,
    lists: &mut Vec<Vec<i64>>,
    errors: &mut Vec<Diag>,
    root: i64,
    ext_mods: &[(i64, i64)],
) -> bool {
    let before = errors.len();

    // Phase 1: callee-origin summaries, refined to a fixpoint.
    //
    // Every function that returns a reference records which input
    // parameter positions its returned borrow can trace to.  Callers read
    // that summary instead of conservatively unioning every reference
    // argument, so freeing an argument the callee provably never borrows
    // is legal (bug-report d7d).  A function starts with no summary,
    // which callers treat as the conservative top (every reference
    // parameter), and each round refines it to the traced subset, so the
    // table is monotone-decreasing and converges; the data-derived cap is
    // a non-convergence compile error, never a silent stop.  Phase-1
    // builds report into a scratch buffer that is discarded — the
    // authoritative diagnostics come from the phase-2 walk.
    let mut summaries: Vec<(i64, Vec<i64>)> = Vec::new();
    let mut scratch: Vec<Diag> = Vec::new();
    let mut fns: Vec<i64> = Vec::new();
    collect_fn_nodes(nodes, lists, root, &mut fns);
    let mut m = 0usize;
    while m < ext_mods.len() {
        match ext_mods.get(m) {
            Some(pair) => collect_fn_nodes(nodes, lists, pair.1, &mut fns),
            None => break,
        }
        m += 1;
    }
    let cap = fns.len() as i64 * (max_fn_params(nodes, lists, &fns) + 1) + fns.len() as i64 + 1;
    let mut round = 0i64;
    loop {
        let mut changed = false;
        let mut fi = 0usize;
        while fi < fns.len() {
            match fns.get(fi) {
                Some(fn_node) => {
                    let sources = {
                        let mut f: F = (
                            Vec::new(),
                            Vec::new(),
                            Vec::new(),
                            Vec::new(),
                            Vec::new(),
                            Vec::new(),
                            Vec::new(),
                            Vec::new(),
                            Vec::new(),
                            Vec::new(),
                            Vec::new(),
                        );
                        let mut b: B = (Vec::new(), Vec::new(), Vec::new(), Vec::new(), NONE, NONE, NONE, NONE);
                        let mut ctx: Ctx = (names, nodes, lists, &mut scratch, &mut summaries);
                        if build_fn(&mut f, &mut b, &mut ctx, *fn_node) {
                            compute_summary(&f, &mut ctx, 0)
                        } else {
                            None
                        }
                    };
                    scratch.clear();
                    changed |= refine_summary(&mut summaries, *fn_node, sources);
                }
                None => break,
            }
            fi += 1;
        }
        round += 1;
        if !changed {
            break;
        }
        if round > cap {
            push_internal(errors, "internal: callee-origin summaries did not converge");
            return false;
        }
    }

    // Phase 2: the authoritative per-function analysis with the converged
    // summaries and real diagnostics.
    check_item_list(names, nodes, lists, errors, &mut summaries, root);
    let mut idx = 0usize;
    while idx < ext_mods.len() {
        match ext_mods.get(idx) {
            Some(pair) => check_item_list(names, nodes, lists, errors, &mut summaries, pair.1),
            None => break,
        }
        idx += 1;
    }
    errors.len() == before
}

/// Walks an item list, descending into modules and checking every
/// function body (free functions, impl methods, trait method bodies).
fn check_item_list(
    names: &mut Vec<String>,
    nodes: &mut Vec<i64>,
    lists: &mut Vec<Vec<i64>>,
    errors: &mut Vec<Diag>,
    summaries: &mut Vec<(i64, Vec<i64>)>,
    list: i64,
) {
    let count = list_len(lists, list);
    let mut idx = 0i64;
    while idx < count {
        let item = list_get(lists, list, idx);
        if node_tag(nodes, item) == NODE_ITEM {
            let kind = node_a(nodes, item);
            if kind == ITEM_MODULE {
                check_item_list(names, nodes, lists, errors, summaries, node_e(nodes, item));
            } else if kind == ITEM_FUN || kind == ITEM_NATIVE_FUN {
                check_fn(names, nodes, lists, errors, summaries, node_d(nodes, item));
            } else if kind == ITEM_IMPL {
                check_fn_list(names, nodes, lists, errors, summaries, node_f(nodes, item));
            } else if kind == ITEM_TRAIT {
                check_fn_list(names, nodes, lists, errors, summaries, node_e(nodes, item));
            }
        }
        idx += 1;
    }
}

fn check_fn_list(
    names: &mut Vec<String>,
    nodes: &mut Vec<i64>,
    lists: &mut Vec<Vec<i64>>,
    errors: &mut Vec<Diag>,
    summaries: &mut Vec<(i64, Vec<i64>)>,
    list: i64,
) {
    let count = list_len(lists, list);
    let mut idx = 0i64;
    while idx < count {
        check_fn(names, nodes, lists, errors, summaries, list_get(lists, list, idx));
        idx += 1;
    }
}

/// Builds and checks one function body.  Signature-only declarations
/// (native functions, trait method signatures) have nothing to check.
fn check_fn(
    names: &mut Vec<String>,
    nodes: &mut Vec<i64>,
    lists: &mut Vec<Vec<i64>>,
    errors: &mut Vec<Diag>,
    summaries: &mut Vec<(i64, Vec<i64>)>,
    fn_node: i64,
) {
    if node_tag(nodes, fn_node) != NODE_FN {
        return;
    }
    let mut f: F = (
        Vec::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
    );
    let mut b: B = (Vec::new(), Vec::new(), Vec::new(), Vec::new(), NONE, NONE, NONE, NONE);
    let mut ctx: Ctx = (names, nodes, lists, errors, summaries);
    if !build_fn(&mut f, &mut b, &mut ctx, fn_node) {
        return;
    }
    let entry = 0i64;
    analyze_fn(&mut f, &mut ctx, entry);
}

/// Runs the two dataflow analyses to fixpoint and then a single
/// authoritative reporting walk over the converged facts.
fn analyze_fn(f: &mut F, ctx: &mut Ctx, entry: i64) {
    let live_after = compute_liveness(f);
    let (entry_state, inconsistencies) = linear_fixpoint(f, ctx, entry);
    let entry_origins = origin_fixpoint(f, ctx, entry);
    report(f, ctx, &live_after, &entry_state, &inconsistencies, &entry_origins);
}

// ---------------------------------------------------------------------------
// Set helpers for the dataflow state.
// ---------------------------------------------------------------------------

fn same_set(a: &[i64], b: &[i64]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut idx = 0usize;
    while idx < a.len() {
        match a.get(idx) {
            Some(v) => {
                if !list_has(b, *v) {
                    return false;
                }
            }
            None => break,
        }
        idx += 1;
    }
    true
}

fn remove_value(set: &mut Vec<i64>, value: i64) {
    let mut idx = 0usize;
    while idx < set.len() {
        match set.get(idx) {
            Some(v) => {
                if *v == value {
                    set.remove(idx);
                    return;
                }
            }
            None => break,
        }
        idx += 1;
    }
}

fn append_unique(set: &mut Vec<i64>, value: i64) {
    if value >= 0 && !list_has(set, value) {
        set.push(value);
    }
}

fn append_list_unique(set: &mut Vec<i64>, other: &[i64]) {
    let mut idx = 0usize;
    while idx < other.len() {
        match other.get(idx) {
            Some(v) => append_unique(set, *v),
            None => break,
        }
        idx += 1;
    }
}

/// Appends the entries of `other` to `set`, deduplicating.  Unlike
/// `append_list_unique`, negative `REF_ORIGIN` markers are preserved: a
/// returned borrow's production can be a marker (the reborrow of a
/// reference variable), and dropping it would leave the returned
/// reference's origin untraceable.
fn append_prod_unique(set: &mut Vec<i64>, other: &[i64]) {
    let mut idx = 0usize;
    while idx < other.len() {
        match other.get(idx) {
            Some(v) => {
                if !list_has(set, *v) {
                    set.push(*v);
                }
            }
            None => break,
        }
        idx += 1;
    }
}

/// The op range of a block: `(first, last)` inclusive; `last < first` for
/// an empty block.
fn block_op_range(f: &F, block: i64) -> (i64, i64) {
    let row = block_at(f, block);
    let first = row.2;
    let mut last = block_op_end(f, block) - 1;
    if last < first {
        last = first - 1;
    }
    (first, last)
}

/// The binding an op reads (a liveness use), or NONE.  A write through a
/// `&mut` reference (an OP_ASSIGN of target kind 1) reads the reference to
/// locate its referent, so the reference — and the borrow it carries — is
/// live up to that write.  It is also not killed by it (`op_defs` skips
/// target-kind-1 assigns), so a reference used only by writing through it
/// keeps its loan live across later borrows, which is exactly the
/// aliasing the exclusivity rule must reject.
fn op_uses(f: &F, op: i64) -> i64 {
    let row = op_at(f, op);
    if row.0 == OP_READ
        || row.0 == OP_MOVE
        || row.0 == OP_BORROW
        || row.0 == OP_BORROW_M
        || (row.0 == OP_ASSIGN && row.4 == 1)
    {
        row.1
    } else {
        NONE
    }
}

/// The binding an op writes (a liveness def), or NONE.  A write through a
/// `&mut` reference does not redefine the reference, so target-kind-1
/// assigns are uses, not defs.
fn op_defs(f: &F, op: i64) -> i64 {
    let row = op_at(f, op);
    if row.0 == OP_BIND || (row.0 == OP_ASSIGN && row.4 != 1) {
        row.1
    } else {
        NONE
    }
}

/// The reference variables reborrowed inside a block.  A reborrow keeps
/// its reference variable live to the end of the block (the call it
/// feeds), so a by-value move of the referent in the same call conflicts.
fn block_reborrows(f: &F, block: i64) -> Vec<i64> {
    let mut out: Vec<i64> = Vec::new();
    let (first, last) = block_op_range(f, block);
    let mut op = first;
    while op <= last {
        let row = op_at(f, op);
        if (row.0 == OP_BORROW || row.0 == OP_BORROW_M) && row.1 >= 0 && binding_at(f, row.1).3 == 1 {
            append_unique(&mut out, row.1);
        }
        op += 1;
    }
    out
}

// ---------------------------------------------------------------------------
// Backward liveness: var_live_at facts.
// ---------------------------------------------------------------------------

/// Computes, for every op, the set of bindings live immediately after it
/// (the `var_live_at` facts).  A standard backward dataflow over the CFG
/// with per-block gen/kill, plus the reborrow extension.
fn compute_liveness(f: &F) -> Vec<Vec<i64>> {
    let nblocks = f.6.len() as i64;
    let mut live_in: Vec<Vec<i64>> = Vec::new();
    let mut live_out: Vec<Vec<i64>> = Vec::new();
    let mut blk = 0i64;
    while blk < nblocks {
        live_in.push(Vec::new());
        live_out.push(Vec::new());
        blk += 1;
    }
    let mut guard = 0usize;
    loop {
        guard += 1;
        if guard > 64 {
            break;
        }
        let mut changed = false;
        let mut blk = 0i64;
        while blk < nblocks {
            let mut out: Vec<i64> = Vec::new();
            let succs = succ_of(f, blk);
            let mut si = 0usize;
            while si < succs.len() {
                match succs.get(si) {
                    Some(s) => {
                        if let Some(list) = live_in.get(*s as usize) {
                            append_list_unique(&mut out, list);
                        }
                    }
                    None => break,
                }
                si += 1;
            }
            append_list_unique(&mut out, &block_reborrows(f, blk));
            let mut inn = out.clone();
            let (first, last) = block_op_range(f, blk);
            let mut op = first;
            while op <= last {
                let def = op_defs(f, op);
                if def >= 0 {
                    remove_value(&mut inn, def);
                }
                let useb = op_uses(f, op);
                if useb >= 0 {
                    append_unique(&mut inn, useb);
                }
                op += 1;
            }
            if !same_set(&live_out[blk as usize], &out) {
                live_out[blk as usize] = out;
                changed = true;
            }
            if !same_set(&live_in[blk as usize], &inn) {
                live_in[blk as usize] = inn;
                changed = true;
            }
            blk += 1;
        }
        if !changed {
            break;
        }
    }
    let mut live_after: Vec<Vec<i64>> = Vec::new();
    let mut opi = 0i64;
    while opi < f.4.len() as i64 {
        live_after.push(Vec::new());
        opi += 1;
    }
    let mut blk = 0i64;
    while blk < nblocks {
        let mut set = live_out[blk as usize].clone();
        let (first, last) = block_op_range(f, blk);
        let mut op = last;
        while op >= first {
            if let Some(slot) = live_after.get_mut(op as usize) {
                slot.clear();
                let mut j = 0usize;
                while j < set.len() {
                    match set.get(j) {
                        Some(v) => slot.push(*v),
                        None => break,
                    }
                    j += 1;
                }
            }
            let useb = op_uses(f, op);
            if useb >= 0 {
                append_unique(&mut set, useb);
            }
            let def = op_defs(f, op);
            if def >= 0 {
                remove_value(&mut set, def);
            }
            op -= 1;
        }
        blk += 1;
    }
    live_after
}

// ---------------------------------------------------------------------------
// Analysis 1: linear-handle consumption.
// ---------------------------------------------------------------------------

fn unbound_state(npaths: i64) -> Vec<i64> {
    let mut out: Vec<i64> = Vec::new();
    let mut idx = 0i64;
    while idx < npaths {
        out.push(ST_UNBOUND);
        idx += 1;
    }
    out
}

fn same_state(a: &[i64], b: &[i64]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut idx = 0usize;
    while idx < a.len() {
        match a.get(idx) {
            Some(va) => match b.get(idx) {
                Some(vb) => {
                    if va != vb {
                        return false;
                    }
                }
                None => return false,
            },
            None => break,
        }
        idx += 1;
    }
    true
}

fn inc_has(inc: &[(i64, i64)], block: i64, path: i64) -> bool {
    let mut idx = 0usize;
    while idx < inc.len() {
        match inc.get(idx) {
            Some(pair) => {
                if pair.0 == block && pair.1 == path {
                    return true;
                }
            }
            None => break,
        }
        idx += 1;
    }
    false
}

/// Joins one predecessor's exit state into the running entry state.  The
/// lattice is Unbound < Live < Moved with Partial absorbing; a Live/Moved
/// disagreement is recorded as an inconsistency (reported once) and the
/// join continues as Moved so downstream paths do not cascade.
fn join_linear(inn: &mut [i64], pout: &[i64], block: i64, inc: &mut Vec<(i64, i64)>) {
    let mut idx = 0usize;
    while idx < inn.len() {
        let a = state_at(inn, idx as i64);
        let b = state_at(pout, idx as i64);
        let joined;
        if a == ST_UNBOUND {
            joined = b;
        } else if b == ST_UNBOUND || a == b {
            joined = a;
        } else if a == ST_PARTIAL || b == ST_PARTIAL {
            joined = ST_PARTIAL;
        } else {
            joined = ST_MOVED;
            if !inc_has(inc, block, idx as i64) {
                inc.push((block, idx as i64));
            }
        }
        state_set(inn, idx as i64, joined);
        idx += 1;
    }
}

/// Whether `path` is a descendant (transitively) of `ancestor`.
fn path_descends(f: &F, path: i64, ancestor: i64) -> bool {
    let mut cur = path_at(f, path).0;
    while cur != NONE {
        if cur == ancestor {
            return true;
        }
        cur = path_at(f, cur).0;
    }
    false
}

/// Marks every descendant of `path` with `value` (a root move moves every
/// field; a root re-initialization relives every field).
fn mark_descendants(f: &F, state: &mut [i64], path: i64, value: i64) {
    let mut idx = 0usize;
    while idx < f.2.len() {
        if path_descends(f, idx as i64, path) {
            state_set(state, idx as i64, value);
        }
        idx += 1;
    }
}

/// Applies a move of `path` on `binding`.  Moves a whole value (root) or
/// a single field sub-node; a moved root moves every descendant, and a
/// moved child marks its ancestors partial.
fn apply_move(f: &F, state: &mut [i64], binding: i64, path: i64, report: bool, ctx: &mut Ctx, span: (i64, i64, i64)) {
    if path < 0 {
        return;
    }
    let st = state_at(state, path);
    if report {
        let name = dotted_name_of(f, ctx, binding, path);
        if st == ST_MOVED {
            push_error(ctx.3, &format!("use of moved value '{}'", name), span.0, span.1, span.2);
        } else if st == ST_PARTIAL {
            push_error(ctx.3, &format!("cannot move out of partially moved value '{}'", name), span.0, span.1, span.2);
        }
    }
    if st == ST_UNBOUND || st == ST_MOVED || st == ST_PARTIAL {
        return;
    }
    state_set(state, path, ST_MOVED);
    mark_descendants(f, state, path, ST_MOVED);
    if !is_root_path(f, path) {
        let mut p = path_at(f, path).0;
        while p != NONE {
            let pst = state_at(state, p);
            if pst == ST_LIVE {
                state_set(state, p, ST_PARTIAL);
            }
            if pst == ST_MOVED {
                break;
            }
            p = path_at(f, p).0;
        }
    }
}

/// Applies an assignment to `binding` (the whole value when `path` is
/// NONE, a field sub-node otherwise).  Reassigning an effectively-Live
/// linear value is an error; assigning to a Moved value re-initializes
/// it (a whole value relives every descendant; a re-filled field
/// restores its moved-out ancestors once every tracked child is live
/// again).  Writing a field of a whole-consumed value is a use of moved
/// value.
fn apply_assign(f: &F, state: &mut [i64], binding: i64, path: i64, report: bool, ctx: &mut Ctx, span: (i64, i64, i64)) {
    let root = root_path_of(f, binding);
    if root < 0 {
        return;
    }
    let target = if path == NONE { root } else { path };
    let name = dotted_name_of(f, ctx, binding, path);
    // A sub-node not separately tracked yet inherits its root's state.
    let st = state_at(state, target);
    let eff = if st == ST_UNBOUND && !is_root_path(f, target) {
        state_at(state, root)
    } else {
        st
    };
    if report {
        if eff == ST_LIVE {
            push_error(ctx.3, &format!("linear value '{}' is reassigned without being consumed", name), span.0, span.1, span.2);
        } else if eff == ST_PARTIAL {
            push_error(ctx.3, &format!("cannot reassign partially moved value '{}'", name), span.0, span.1, span.2);
        }
    }
    if eff == ST_LIVE || eff == ST_PARTIAL {
        return;
    }
    if !is_root_path(f, target) && state_at(state, root) == ST_MOVED {
        // The whole value was consumed; writing one of its fields is a
        // use of moved value.
        if report {
            push_error(ctx.3, &format!("use of moved value '{}'", name), span.0, span.1, span.2);
        }
        return;
    }
    state_set(state, target, ST_LIVE);
    mark_descendants(f, state, target, ST_LIVE);
    if !is_root_path(f, target) {
        // Restore moved-out ancestors once every tracked child is live.
        let mut p = path_at(f, target).0;
        while p != NONE {
            if state_at(state, p) != ST_PARTIAL {
                break;
            }
            if all_children_live(f, state, p) {
                state_set(state, p, ST_LIVE);
                p = path_at(f, p).0;
            } else {
                break;
            }
        }
    }
}

/// Applies every op of a block to a linear state.  `report` selects the
/// silent fixpoint pass or the authoritative reporting pass; the state
/// transformation is identical either way.
fn apply_block_linear(f: &F, block: i64, state: &mut [i64], report: bool, ctx: &mut Ctx) {
    let (first, last) = block_op_range(f, block);
    let mut op = first;
    while op <= last {
        let row = op_at(f, op);
        let kind = row.0;
        let binding = row.1;
        if kind == OP_BIND {
            if binding >= 0 && root_path_of(f, binding) != NONE {
                bind_state_live(f, state, binding);
            }
        } else if kind == OP_MOVE {
            apply_move(f, state, binding, row.2, report, ctx, (row.6, row.7, row.8));
        } else if kind == OP_ASSIGN {
            let tkind = row.4;
            if tkind == 1 && row.5 >= 0 && root_path_of(f, row.5) != NONE {
                // A field write through a reference: the linear rule
                // applies to the referent's sub-node (a local owner, or a
                // `&mut` parameter's tracked referent).
                apply_assign(f, state, row.5, row.2, report, ctx, (row.6, row.7, row.8));
            } else if tkind != 1 && binding >= 0 && root_path_of(f, binding) != NONE {
                let path = if tkind == 2 { row.2 } else { NONE };
                apply_assign(f, state, binding, path, report, ctx, (row.6, row.7, row.8));
            }
        }
        op += 1;
    }
}

/// The linear-consumption fixpoint.  Returns the converged per-block
/// entry states and the join-inconsistency list `(block, path)`.
fn linear_fixpoint(f: &F, ctx: &mut Ctx, entry: i64) -> (Vec<Vec<i64>>, Vec<(i64, i64)>) {
    let nblocks = f.6.len() as i64;
    let npaths = f.2.len() as i64;
    let mut entry_state: Vec<Vec<i64>> = Vec::new();
    let mut exit_state: Vec<Vec<i64>> = Vec::new();
    let mut blk = 0i64;
    while blk < nblocks {
        entry_state.push(unbound_state(npaths));
        exit_state.push(unbound_state(npaths));
        blk += 1;
    }
    let mut inconsistencies: Vec<(i64, i64)> = Vec::new();
    // Four lattice states per path, so each (block, path) entry and exit
    // can change at most three times; the cap is derived from the data.
    // Hitting it with changes still pending is a non-convergence compile
    // error, never silent acceptance of un-converged facts.
    let cap = nblocks * npaths * 8 + 1;
    let mut guard = 0usize;
    loop {
        guard += 1;
        if guard > cap as usize {
            push_internal(ctx.3, "internal: linear-consumption analysis did not converge");
            break;
        }
        let mut changed = false;
        let mut blk = 0i64;
        while blk < nblocks {
            let mut inn = unbound_state(npaths);
            if blk != entry {
                let preds = pred_of(f, blk);
                let mut pi = 0usize;
                while pi < preds.len() {
                    match preds.get(pi) {
                        Some(p) => {
                            if let Some(list) = exit_state.get(*p as usize) {
                                join_linear(&mut inn, list, blk, &mut inconsistencies);
                            }
                        }
                        None => break,
                    }
                    pi += 1;
                }
            }
            let mut out = inn.clone();
            apply_block_linear(f, blk, &mut out, false, ctx);
            if !same_state(&entry_state[blk as usize], &inn) {
                entry_state[blk as usize] = inn;
                changed = true;
            }
            if !same_state(&exit_state[blk as usize], &out) {
                exit_state[blk as usize] = out;
                changed = true;
            }
            blk += 1;
        }
        if !changed {
            break;
        }
    }
    (entry_state, inconsistencies)
}

// ---------------------------------------------------------------------------
// Analysis 2: borrow origins (Polonius-style facts).
// ---------------------------------------------------------------------------

fn empty_origins(nbind: i64) -> Vec<Vec<i64>> {
    let mut out: Vec<Vec<i64>> = Vec::new();
    let mut idx = 0i64;
    while idx < nbind {
        out.push(Vec::new());
        idx += 1;
    }
    out
}

/// Whether a block's ops (a bind or assign of a reference binding) change
/// any origin set, recording the result into `origins`.
fn apply_block_origins(f: &F, block: i64, origins: &mut [Vec<i64>]) {
    let (first, last) = block_op_range(f, block);
    let mut op = first;
    while op <= last {
        let row = op_at(f, op);
        let kind = row.0;
        let binding = row.1;
        if binding >= 0 && (kind == OP_BIND || kind == OP_ASSIGN) && row.3 == 1 {
            let loans = op_loans_at(f, op);
            if let Some(slot) = origins.get_mut(binding as usize) {
                slot.clear();
                let mut j = 0usize;
                while j < loans.len() {
                    match loans.get(j) {
                        Some(loan) => slot.push(*loan),
                        None => break,
                    }
                    j += 1;
                }
            }
        }
        op += 1;
    }
}

/// The origin fixpoint.  A reference binding's origin is set exactly once
/// (at its bind or re-assign), so the union join converges quickly; the
/// result is the per-block entry origin set for every binding.
fn origin_fixpoint(f: &F, ctx: &mut Ctx, entry: i64) -> Vec<Vec<Vec<i64>>> {
    let nblocks = f.6.len() as i64;
    let nbind = f.0.len() as i64;
    let nloans = f.3.len() as i64;
    let mut entry_origins: Vec<Vec<Vec<i64>>> = Vec::new();
    let mut exit_origins: Vec<Vec<Vec<i64>>> = Vec::new();
    let mut blk = 0i64;
    while blk < nblocks {
        entry_origins.push(empty_origins(nbind));
        exit_origins.push(empty_origins(nbind));
        blk += 1;
    }
    // Each binding's origin set can gain at most `nloans` elements, and
    // a block's exit origin is either its entry (bindings it never
    // re-points) or a fixed constant (re-pointed bindings), so only the
    // entry unions grow; the cap is derived from the data.  Hitting it
    // with changes still pending is a non-convergence compile error,
    // never silent acceptance of un-converged facts.
    let cap = nblocks * nbind * (nloans + 1) + 1;
    let mut guard = 0usize;
    loop {
        guard += 1;
        if guard > cap as usize {
            push_internal(ctx.3, "internal: borrow-origin analysis did not converge");
            break;
        }
        let mut changed = false;
        let mut blk = 0i64;
        while blk < nblocks {
            let mut inn = empty_origins(nbind);
            if blk != entry {
                let preds = pred_of(f, blk);
                let mut pi = 0usize;
                while pi < preds.len() {
                    match preds.get(pi) {
                        Some(p) => {
                            if let Some(list) = exit_origins.get(*p as usize) {
                                let mut bi = 0usize;
                                while bi < nbind as usize {
                                    if let (Some(dst), Some(src)) = (inn.get_mut(bi), list.get(bi)) {
                                        // Origin sets legitimately contain negative `REF_ORIGIN`
                                        // markers (a binding whose origin is another reference's
                                        // origin); the marker-preserving append keeps them, so the
                                        // fixpoint's facts match what the op table records and the
                                        // report walk never has to re-derive an origin from the op
                                        // table.
                                        append_prod_unique(dst, src);
                                    }
                                    bi += 1;
                                }
                            }
                        }
                        None => break,
                    }
                    pi += 1;
                }
            }
            let mut out = inn.clone();
            apply_block_origins(f, blk, &mut out);
            let mut eq_in = true;
            let mut eq_out = true;
            let mut bi = 0usize;
            while bi < nbind as usize {
                if !same_set(&entry_origins[blk as usize][bi], &inn[bi]) {
                    eq_in = false;
                }
                if !same_set(&exit_origins[blk as usize][bi], &out[bi]) {
                    eq_out = false;
                }
                bi += 1;
            }
            if !eq_in {
                entry_origins[blk as usize] = inn;
                changed = true;
            }
            if !eq_out {
                exit_origins[blk as usize] = out;
                changed = true;
            }
            blk += 1;
        }
        if !changed {
            break;
        }
    }
    entry_origins
}

// ---------------------------------------------------------------------------
// Enforcement: the authoritative reporting walk over converged facts.
// ---------------------------------------------------------------------------

/// The text describing the scope-diverging exit an OP_EXIT checks.
fn exit_where(kind: i64) -> &'static str {
    if kind == EX_BREAK {
        "before this break"
    } else if kind == EX_TRY {
        "on this error path"
    } else {
        "before returning"
    }
}

/// The first materialized linear sub-node of `root` that is not fully
/// consumed (not Moved), or NONE when every linear sub-node was consumed.
/// A partially moved parent's resources are its children, so only a
/// still-live child (or a child that is itself partial) blocks the exit.
fn first_live_subnode(f: &F, state: &[i64], root: i64) -> i64 {
    let mut idx = 0usize;
    while idx < f.2.len() {
        let row = path_at(f, idx as i64);
        if row.2 == root && row.0 != NONE {
            let st = state_at(state, idx as i64);
            if st != ST_MOVED {
                return idx as i64;
            }
        }
        idx += 1;
    }
    NONE
}

/// The first materialized linear sub-node of `root` that was consumed
/// (Moved or Partial) but not restored, or NONE when every sub-node is
/// still Live (or was never touched).  Used for `&mut` parameter
/// referents, which must be left exactly as the caller handed them over.
/// `ST_UNBOUND` is deliberately ignored: a reachable block's sub-nodes are
/// Live from their OP_BIND onward, while the fall-off-end exit of a
/// function that ends in `return` has no predecessors and is seeded
/// all-Unbound -- the dead exit must not report a phantom consume.
fn first_not_live_subnode(f: &F, state: &[i64], root: i64) -> i64 {
    let mut idx = 0usize;
    while idx < f.2.len() {
        let row = path_at(f, idx as i64);
        if row.2 == root && row.0 != NONE {
            let st = state_at(state, idx as i64);
            if st == ST_MOVED || st == ST_PARTIAL {
                return idx as i64;
            }
        }
        idx += 1;
    }
    NONE
}

/// Checks every linear binding the exit requires to be consumed.  The
/// op's aux slot carries the scope start (the binding index from which
/// the exit owns every in-scope binding).
fn exit_check(f: &F, ctx: &mut Ctx, op: i64, state: &[i64]) {
    let row = op_at(f, op);
    let mut scope_start = row.3;
    if scope_start < 0 {
        scope_start = 0;
    }
    let kind = row.4;
    let mut bidx = scope_start;
    while bidx < f.0.len() as i64 {
        let brow = binding_at(f, bidx);
        let root = brow.6;
        let name = name_text(ctx.0, brow.0);
        if brow.2 == 1 {
            let st = state_at(state, root);
            if st == ST_LIVE {
                push_error(ctx.3, &format!("linear value '{}' must be consumed {}", name, exit_where(kind)), row.6, row.7, row.8);
            } else if st == ST_PARTIAL {
                // A partially moved parent can be left behind exactly when
                // every linear sub-node was consumed: the parent's linear
                // resources are its children, and a consumed child is what
                // makes the parent partial.  A still-live sub-node means an
                // unconsumed handle remains inside the parent.
                let live = first_live_subnode(f, state, root);
                if live != NONE {
                    let field_name = dotted_name_of(f, ctx, bidx, live);
                    push_error(ctx.3, &format!("partially moved value '{}' cannot be left behind {}: field '{}' is not fully consumed", name, exit_where(kind), field_name), row.6, row.7, row.8);
                }
            }
        } else if root >= 0 && ty_kind_of(ctx.1, brow.1) == TYD_REF_MUT {
            // A `&mut` parameter's linear referent is borrowed from the
            // caller: the callee must leave every tracked field exactly
            // as it found it (Live).  A consumed (Moved or Partial)
            // sub-node means a handle was taken without being restored,
            // which would leave the caller's bookkeeping dangling.
            let not_live = first_not_live_subnode(f, state, root);
            if not_live != NONE {
                let field_name = dotted_name_of(f, ctx, bidx, not_live);
                push_error(ctx.3, &format!("linear field '{}' consumed through a &mut parameter is not restored {}", field_name, exit_where(kind)), row.6, row.7, row.8);
            }
        }
        bidx += 1;
    }
}

/// Appends the direct loan ids reachable from a reference binding's
/// origin, resolving REF_ORIGIN markers (a reference produced by a match
/// arm or a ref-returning call re-produces another reference's origin as
/// a marker).  The primary source is the per-block origin table, which is
/// exact where the binding is in scope; when that entry is empty — an
/// arm-scoped pattern binding is not in scope past its arm — the
/// binding's own bind/re-point op loans are used, a build-time fact that
/// is block-independent.  The walk mirrors the returned-borrow trace so
/// the two can never drift apart; conflicts compare the resolved loans'
/// owners and element keys.
fn collect_origin_loans(f: &F, origins: &[Vec<i64>], binding: i64, out: &mut Vec<i64>, visited: &mut Vec<i64>, current_op: i64) {
    if binding < 0 || list_has(visited, binding) {
        return;
    }
    visited.push(binding);
    let mut fixed: Vec<i64> = Vec::new();
    if let Some(list) = origins.get(binding as usize) {
        let mut idx = 0usize;
        while idx < list.len() {
            match list.get(idx) {
                Some(value) => fixed.push(*value),
                None => break,
            }
            idx += 1;
        }
    }
    if fixed.is_empty() {
        // The fixpoint normally carries every origin; this fallback only
        // fires when a binding's origin was never established on a path to
        // this point and reconstructs it from the op table.  The scan is
        // bounded by the current op so a reference rebound LATER in the
        // function cannot leak its later origin into an earlier point.
        let mut op = if current_op >= 0 && current_op < f.4.len() as i64 {
            current_op
        } else {
            f.4.len() as i64 - 1
        };
        while op >= 0 {
            let row = op_at(f, op);
            if row.1 == binding && (row.0 == OP_BIND || row.0 == OP_ASSIGN) && row.3 == 1 {
                let loans = op_loans_at(f, op);
                let mut li = 0usize;
                while li < loans.len() {
                    match loans.get(li) {
                        Some(value) => fixed.push(*value),
                        None => break,
                    }
                    li += 1;
                }
                break;
            }
            op -= 1;
        }
    }
    let mut idx = 0usize;
    while idx < fixed.len() {
        match fixed.get(idx) {
            Some(entry) => {
                if is_ref_origin(*entry) {
                    collect_origin_loans(f, origins, -1 - entry, out, visited, current_op);
                } else if *entry >= 0 {
                    out.push(*entry);
                }
            }
            None => break,
        }
        idx += 1;
    }
}

/// Emits a borrow-conflict diagnostic: a write, move, or new exclusive
/// borrow of `binding` while a conflicting loan on it is still contained
/// in a live reference's origin.
fn conflicts_at(f: &F, ctx: &mut Ctx, op: i64, origins: &[Vec<i64>], live_after: &[Vec<i64>]) {
    let row = op_at(f, op);
    let kind = row.0;
    if kind != OP_BORROW && kind != OP_BORROW_M && kind != OP_MOVE && kind != OP_ASSIGN {
        return;
    }
    let binding = row.1;
    if binding < 0 {
        return;
    }
    if kind == OP_ASSIGN && row.3 == 1 && row.4 == 0 {
        // Re-pointing a reference variable overwrites its own origin; it
        // is not a write through the borrow.
        return;
    }
    // The new borrow's element key (NONE for the whole-value ops — a move
    // or write of the whole binding overlaps every element loan).
    let new_key = if kind == OP_BORROW || kind == OP_BORROW_M { row.5 } else { NONE };
    let live = match live_after.get(op as usize) {
        Some(list) => list,
        None => return,
    };
    let name = format!("{}{}", name_text(ctx.0, binding_at(f, binding).0), index_suffix(ctx, new_key));
    let mut li = 0usize;
    while li < live.len() {
        let r = match live.get(li) {
            Some(v) => *v,
            None => break,
        };
        // A reference reborrowed through itself (a `&mut` binding passed
        // to a `&mut` parameter) derives from its own standing loan; that
        // loan is the reborrow's source, not a conflicting borrow.  Only
        // loans held by *other* live references can conflict.
        if r == binding {
            li += 1;
            continue;
        }
        if binding_at(f, r).3 == 1 {
            // A live reference's origin may hold direct loans or
            // REF_ORIGIN markers; resolve the markers through the origin
            // table so every loan the reference actually borrows is
            // compared.
            let mut loans: Vec<i64> = Vec::new();
            let mut visited: Vec<i64> = Vec::new();
            collect_origin_loans(f, origins, r, &mut loans, &mut visited, op);
            let mut oi = 0usize;
            while oi < loans.len() {
                let loan = match loans.get(oi) {
                    Some(v) => *v,
                    None => break,
                };
                let lrow = loan_at(f, loan);
                if lrow.0 == binding && index_keys_conflict(ctx, new_key, lrow.3) {
                    let conflict = if kind == OP_MOVE || kind == OP_ASSIGN || kind == OP_BORROW_M {
                        true
                    } else {
                        lrow.1 == L_MUT
                    };
                    if conflict {
                        let message = if kind == OP_MOVE {
                            format!("cannot move '{}' while it is borrowed", name)
                        } else if kind == OP_ASSIGN {
                            format!("cannot assign to '{}' while it is borrowed", name)
                        } else if kind == OP_BORROW_M {
                            format!("cannot mutably borrow '{}' while it is borrowed", name)
                        } else {
                            format!("cannot borrow '{}' while it is mutably borrowed", name)
                        };
                        push_error(ctx.3, &message, row.6, row.7, row.8);
                    }
                }
                oi += 1;
            }
        }
        li += 1;
    }
}

/// Traces an origin-loan list at a point to the set of input reference
/// parameters it ultimately derives from, marking `local` when any part
/// of the origin is a local value (or a non-parameter slot).  `visited`
/// guards reborrow chains against cycles.
fn trace_origin(f: &F, origins: &[Vec<i64>], loans: &[i64], sources: &mut Vec<i64>, local: &mut bool, visited: &mut Vec<i64>) {
    let mut idx = 0usize;
    while idx < loans.len() {
        let entry = match loans.get(idx) {
            Some(v) => *v,
            None => break,
        };
        if is_ref_origin(entry) {
            let binding = -1 - entry;
            if !list_has(visited, binding) {
                visited.push(binding);
                let origin = match origins.get(binding as usize) {
                    Some(list) => list.clone(),
                    None => Vec::new(),
                };
                trace_origin(f, origins, &origin, sources, local, visited);
            }
        } else {
            let lrow = loan_at(f, entry);
            let owner = lrow.0;
            if owner < 0 {
                idx += 1;
                continue;
            }
            if lrow.2 == 1 {
                append_unique(sources, owner);
            } else {
                let orow = binding_at(f, owner);
                if orow.3 == 1 && orow.4 == 1 {
                    // A borrow through a reference parameter: the memory
                    // behind the parameter outlives the function.
                    append_unique(sources, owner);
                } else {
                    *local = true;
                }
            }
        }
        idx += 1;
    }
}

/// The comma-joined names of the given binding ids.
fn binding_names(ctx: &mut Ctx, f: &F, ids: &[i64]) -> String {
    let mut names: Vec<String> = Vec::new();
    let mut idx = 0usize;
    while idx < ids.len() {
        match ids.get(idx) {
            Some(binding) => names.push(name_text(ctx.0, binding_at(f, *binding).0)),
            None => break,
        }
        idx += 1;
    }
    names.join(", ")
}

/// The returned-borrow rule: the returned reference's origin must trace
/// to exactly one input reference parameter.  A local in the origin, an
/// empty origin, or more than one parameter within a single return is
/// reported here.  When the return passes, its parameter sources are
/// returned so the caller can union them across every return of the
/// function and reject a function whose returned borrow derives from
/// more than one input parameter overall (the ambiguity rule).  None
/// means an error was already reported for this return.
fn ret_ref_check(f: &F, ctx: &mut Ctx, op: i64, origins: &[Vec<i64>]) -> Option<Vec<i64>> {
    let prod = op_loans_at(f, op);
    let mut sources: Vec<i64> = Vec::new();
    let mut local = false;
    let mut visited: Vec<i64> = Vec::new();
    trace_origin(f, origins, &prod, &mut sources, &mut local, &mut visited);
    let row = op_at(f, op);
    if local {
        push_error(ctx.3, "returned borrow does not outlive the function", row.6, row.7, row.8);
        return None;
    }
    if sources.is_empty() {
        push_error(ctx.3, "returned borrow has no traceable origin: it does not derive from any input reference parameter", row.6, row.7, row.8);
        return None;
    }
    if sources.len() > 1 {
        let names = binding_names(ctx, f, &sources);
        push_error(
            ctx.3,
            &format!("ambiguous returned borrow: it derives from more than one input reference parameter ({})", names),
            row.6,
            row.7,
            row.8,
        );
        return None;
    }
    Some(sources)
}

/// Re-points `origins[binding]` from the op's loan list when the op binds
/// or re-assigns a reference variable, so a returned borrow flowing
/// through a (re)bound reference traces against the updated origin.  This
/// is the single implementation of that write, shared by the authoritative
/// report walk and the summary trace so the two can never drift apart.
fn repoint_origin(f: &F, row: (i64, i64, i64, i64, i64, i64, i64, i64, i64), op: i64, origins: &mut [Vec<i64>]) {
    let kind = row.0;
    let binding = row.1;
    let is_ref_bind = kind == OP_BIND && row.3 == 1 && row.4 == 1;
    let is_ref_assign = kind == OP_ASSIGN && row.3 == 1 && row.4 == 0;
    if binding >= 0 && (is_ref_bind || is_ref_assign) {
        let loans = op_loans_at(f, op);
        if let Some(slot) = origins.get_mut(binding as usize) {
            slot.clear();
            let mut j = 0usize;
            while j < loans.len() {
                match loans.get(j) {
                    Some(loan) => slot.push(*loan),
                    None => break,
                }
                j += 1;
            }
        }
    }
}

/// The callee-origin summary of a built function: the union of the traced
/// parameter sources across every OP_RET_REF op, mirroring the
/// function-level union in `report`.  Returns None when any return path is
/// unresolved (a local origin or an empty origin): the caller keeps the
/// conservative top summary, and the phase-2 walk reports the real error
/// at the return site.
fn compute_summary(f: &F, ctx: &mut Ctx, entry: i64) -> Option<Vec<i64>> {
    let entry_origins = origin_fixpoint(f, ctx, entry);
    let mut sources: Vec<i64> = Vec::new();
    let mut local = false;
    let nblocks = f.6.len() as i64;
    let mut blk = 0i64;
    while blk < nblocks {
        // Mirror `report`'s walk: clone the block's entry origins and
        // re-point reference bindings as ops are applied, so a returned
        // borrow that flows through an in-block (re)bound reference
        // variable traces identically to the diagnostic walk.
        let mut origins = match entry_origins.get(blk as usize) {
            Some(list) => list.clone(),
            None => Vec::new(),
        };
        let (first, last) = block_op_range(f, blk);
        let mut op = first;
        while op <= last {
            let row = op_at(f, op);
            if row.0 == OP_BIND || row.0 == OP_ASSIGN {
                repoint_origin(f, row, op, &mut origins);
            } else if row.0 == OP_RET_REF {
                let prod = op_loans_at(f, op);
                let mut visited: Vec<i64> = Vec::new();
                trace_origin(f, &origins, &prod, &mut sources, &mut local, &mut visited);
            }
            op += 1;
        }
        blk += 1;
    }
    if local || sources.is_empty() {
        None
    } else {
        Some(sources)
    }
}

/// Inserts or refines the callee-origin summary of `fn_node`; returns true
/// when the table changed so the fixpoint iterates again.  A None trace
/// keeps whatever is recorded (the conservative top on a first sighting):
/// an unresolved trace must never under-approximate a returned borrow's
/// origin.  (A recorded summary can never trace to None in a later round —
/// callee productions only shrink and a reference-parameter source never
/// becomes a local — so this branch is defensive, not a refinement path.)
fn refine_summary(table: &mut Vec<(i64, Vec<i64>)>, fn_node: i64, sources: Option<Vec<i64>>) -> bool {
    let sources = match sources {
        Some(sources) => sources,
        None => return false,
    };
    let mut idx = 0usize;
    while idx < table.len() {
        match table.get(idx) {
            Some(pair) => {
                if pair.0 == fn_node {
                    if same_set(&pair.1, &sources) {
                        return false;
                    }
                    break;
                }
            }
            None => break,
        }
        idx += 1;
    }
    if idx < table.len() {
        match table.get_mut(idx) {
            Some(pair) => {
                pair.1 = sources;
                true
            }
            None => false,
        }
    } else {
        table.push((fn_node, sources));
        true
    }
}

/// The recorded callee-origin summary of `fn_node`, or None when there is
/// none; callers then conservatively treat every reference parameter as a
/// possible origin.
fn summary_of(table: &[(i64, Vec<i64>)], fn_node: i64) -> Option<&Vec<i64>> {
    let mut idx = 0usize;
    while idx < table.len() {
        match table.get(idx) {
            Some(pair) => {
                if pair.0 == fn_node {
                    return Some(&pair.1);
                }
            }
            None => break,
        }
        idx += 1;
    }
    None
}

/// The authoritative walk: applies every op once with the converged entry
/// facts, reporting all consumption, conflict, exit, and returned-borrow
/// diagnostics.
fn report(
    f: &F,
    ctx: &mut Ctx,
    live_after: &[Vec<i64>],
    entry_state: &[Vec<i64>],
    inconsistencies: &[(i64, i64)],
    entry_origins: &[Vec<Vec<i64>>],
) {
    let mut idx = 0usize;
    while idx < inconsistencies.len() {
        match inconsistencies.get(idx) {
            Some(pair) => {
                let path = pair.1;
                let root = path_at(f, path).2;
                let mut name = String::new();
                let mut bidx = 0i64;
                while bidx < f.0.len() as i64 {
                    if root_path_of(f, bidx) == root {
                        name = name_text(ctx.0, binding_at(f, bidx).0);
                        break;
                    }
                    bidx += 1;
                }
                let span = block_span_at(f, pair.0);
                push_error(
                    ctx.3,
                    &format!("linear value '{}' is consumed on some paths but not on all paths", name),
                    span.0,
                    span.1,
                    span.2,
                );
            }
            None => break,
        }
        idx += 1;
    }
    // The returned-borrow ambiguity rule: the union of the input
    // reference parameters every return of this function derives from must
    // be a single parameter.  A per-return error (local or empty origin)
    // already rejects the function, so the union is only checked when no
    // per-return check failed.
    let mut fn_ret_sources: Vec<i64> = Vec::new();
    let mut fn_ret_errored = false;
    let nblocks = f.6.len() as i64;
    let mut blk = 0i64;
    while blk < nblocks {
        let mut state = match entry_state.get(blk as usize) {
            Some(list) => list.clone(),
            None => Vec::new(),
        };
        let mut origins = match entry_origins.get(blk as usize) {
            Some(list) => list.clone(),
            None => Vec::new(),
        };
        let (first, last) = block_op_range(f, blk);
        let mut op = first;
        while op <= last {
            let row = op_at(f, op);
            let kind = row.0;
            let binding = row.1;
            if kind == OP_BIND {
                repoint_origin(f, row, op, &mut origins);
                if binding >= 0 && root_path_of(f, binding) != NONE {
                    bind_state_live(f, &mut state, binding);
                }
            } else if kind == OP_MOVE {
                conflicts_at(f, ctx, op, &origins, live_after);
                apply_move(f, &mut state, binding, row.2, true, ctx, (row.6, row.7, row.8));
            } else if kind == OP_ASSIGN {
                if row.3 == 1 && row.4 == 0 && binding >= 0 {
                    // Re-pointing a reference variable: its origin is the
                    // value's origin.
                    repoint_origin(f, row, op, &mut origins);
                } else {
                    conflicts_at(f, ctx, op, &origins, live_after);
                }
                let tkind = row.4;
                if tkind == 1 && row.5 >= 0 && root_path_of(f, row.5) != NONE {
                    apply_assign(f, &mut state, row.5, row.2, true, ctx, (row.6, row.7, row.8));
                } else if tkind != 1 && binding >= 0 && root_path_of(f, binding) != NONE {
                    let path = if tkind == 2 { row.2 } else { NONE };
                    apply_assign(f, &mut state, binding, path, true, ctx, (row.6, row.7, row.8));
                }
            } else if kind == OP_BORROW || kind == OP_BORROW_M {
                conflicts_at(f, ctx, op, &origins, live_after);
            } else if kind == OP_EXIT {
                exit_check(f, ctx, op, &state);
            } else if kind == OP_RET_REF {
                match ret_ref_check(f, ctx, op, &origins) {
                    Some(sources) => append_prod_unique(&mut fn_ret_sources, &sources),
                    None => fn_ret_errored = true,
                }
            }
            op += 1;
        }
        blk += 1;
    }
    if !fn_ret_errored && fn_ret_sources.len() > 1 {
        let fn_span = block_span_at(f, 0);
        let names = binding_names(ctx, f, &fn_ret_sources);
        push_error(
            ctx.3,
            &format!("ambiguous returned borrow: function returns a reference deriving from more than one input reference parameter ({})", names),
            fn_span.0,
            fn_span.1,
            fn_span.2,
        );
    }
}
