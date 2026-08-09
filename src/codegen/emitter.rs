//! Codegen emitter.
//!
//! Lowers the type-attributed arena into LLVM IR.  Every value lives in
//! an alloca: `emit_expr` returns the address where an expression's value
//! lives, `mem2reg` at `-O2` promotes the scalar cases, and aggregate
//! values (structs, enums, arrays, slices, native handles) flow through
//! memory so match lowering needs no phi nodes.
//!
//! A session is a bundle of the shared state — the LLVM handles, the
//! arenas, and the codegen caches — threaded through the emitter as one
//! argument.  All layout facts are read from the typechecker's attached
//! keys and the program's declarations; nothing is recomputed.

use crate::ast::*;
use crate::codegen::error::*;
use crate::codegen::types::*;
use inkwell::builder::Builder;
use inkwell::context::Context;
use inkwell::module::Module;
use inkwell::targets::TargetData;
use inkwell::types::{BasicMetadataTypeEnum, BasicType, BasicTypeEnum, FunctionType};
use inkwell::values::{
    BasicMetadataValueEnum, BasicValueEnum, FunctionValue, IntValue, PointerValue, ValueKind,
};
use inkwell::AddressSpace;
use inkwell::IntPredicate;

/// `(context, module, builder, target data, names, nodes, lists, type
/// cache, enum info cache, payload struct cache, emitted functions,
/// impls list id)`.  The four caches are owned: the emitter consumes
/// them during emission and nothing uses them afterward.  The impls
/// list is the typechecker's fact table of `(trait sym, for key,
/// methods list)` triples, read only for deferred trait dispatch.
///
/// `'ctx` names the context, which outlives the whole codegen phase;
/// the LLVM types and values the emitter creates carry it.  The module,
/// builder, and target data are borrowed for their own short regions,
/// independent of `'ctx`, and the arenas for `'a` — nothing forces a
/// borrow to outlive its owner.
pub type Session<'ctx, 'm, 'a> = (
    &'ctx Context,
    &'m Module<'ctx>,
    &'m Builder<'ctx>,
    &'m TargetData,
    &'a [String],
    &'a mut Vec<i64>,
    &'a mut Vec<Vec<i64>>,
    KeyTypes<'ctx>,
    EnumInfos,
    PayloadStructs<'ctx>,
    InstFns<'ctx>,
    i64,
);

/// `(mono key, function value)` for every emitted function.
pub type InstFns<'ctx> = Vec<(i64, FunctionValue<'ctx>)>;

/// A local binding: `(name id, declared key, alloca)`.  The key is the
/// typechecker's attached fact for the binding, carried so field chains
/// can be walked without re-inference.
pub type Locals<'ctx> = Vec<(i64, i64, PointerValue<'ctx>)>;

/// A loop context: `(break target, continue target)`.
pub type LoopTargets<'ctx> = Vec<(BasicBlockId<'ctx>, BasicBlockId<'ctx>)>;

/// Opaque handle to a basic block, kept as a pair of ids so the emitter
/// can reference blocks created during match lowering.
pub type BasicBlockId<'ctx> = inkwell::basic_block::BasicBlock<'ctx>;

/// The per-function compilation context threaded through every statement
/// and expression emitter: the function being built, its local bindings,
/// its loop stack, its type-argument substitution (`from` → `to`), and
/// its return key.  Exactly as `Session` threads the phase state, this
/// threads the function state.
pub type FnCtx<'ctx, 'a> = (
    FunctionValue<'ctx>,
    Locals<'ctx>,
    LoopTargets<'ctx>,
    &'a [i64],
    &'a [i64],
    i64,
);

/// A match continuation: the result slot's key, its alloca, and the merge
/// block every arm branches to.  Always travels as one unit.
pub type MatchCont<'ctx> = (i64, PointerValue<'ctx>, BasicBlockId<'ctx>);

/// The value a pattern tests: its key, its pointer, and the block to
/// branch to on mismatch.  Always travels as one unit.
pub type MatchScrut<'ctx> = (i64, PointerValue<'ctx>, BasicBlockId<'ctx>);

// ---------------------------------------------------------------------------
// UTF-8 encoding structure (RFC 3629), used by the String native runtime.
//
// Every test is derived from the encoding's bit layout, not from a
// representation size: a continuation byte is `10xxxxxx` (its top two
// bits are the value 2), an ASCII byte has its top bit clear, and a
// lead byte's range selects the sequence length.  The signedness trap
// is real and handled here: LLVM `i8` is signed, so a raw int-cast
// sign-extends bytes `0x80..=0xFF` to negative `i64`; every encoding
// test masks to the unsigned low byte first.
//
// The lead-byte ranges below are the RFC 3629 table.
// ---------------------------------------------------------------------------

/// Two-byte lead bytes: `0xC2..=0xDF` (excludes overlong encodings).
const UTF8_LEAD_2_MIN: u64 = 0xC2;

/// Two-byte lead bytes: `0xC2..=0xDF` (excludes overlong encodings).
const UTF8_LEAD_2_MAX: u64 = 0xDF;

/// Three-byte lead bytes: `0xE0..=0xEF`.
const UTF8_LEAD_3_MIN: u64 = 0xE0;

/// Three-byte lead bytes: `0xE0..=0xEF`.
const UTF8_LEAD_3_MAX: u64 = 0xEF;

/// Four-byte lead bytes: `0xF0..=0xF4` (excludes surrogates and > U+10FFFF).
const UTF8_LEAD_4_MIN: u64 = 0xF0;

/// Four-byte lead bytes: `0xF0..=0xF4` (excludes surrogates and > U+10FFFF).
const UTF8_LEAD_4_MAX: u64 = 0xF4;

/// Masks an int-cast byte to its unsigned low byte: `i8` is signed in
/// LLVM, so `0x80..=0xFF` sign-extend to negative `i64` and every
/// encoding test below needs the zero-extended value.
fn utf8_byte_value<'ctx>(sess: &mut Session<'ctx, '_, '_>, b: IntValue<'ctx>) -> Result<IntValue<'ctx>, CodegenError> {
    let bw = sess.2.build_int_cast(b, sess.0.i64_type(), "").map_err(builder_fail)?;
    let byte = sess.2.build_and(bw, sess.0.i64_type().const_int(0xFF, false), "").map_err(builder_fail)?;
    Ok(byte)
}

// ---------------------------------------------------------------------------
// Small arena reads.
// ---------------------------------------------------------------------------

fn sym_name(nodes: &[i64], sym: i64) -> i64 {
    node_b(nodes, sym)
}

fn list_len(lists: &[Vec<i64>], id: i64) -> i64 {
    match lists.get(id as usize) {
        Some(items) => items.len() as i64,
        None => 0,
    }
}

fn list_get(lists: &[Vec<i64>], id: i64, idx: i64) -> i64 {
    match lists.get(id as usize) {
        Some(items) => match items.get(idx as usize) {
            Some(value) => *value,
            None => NONE,
        },
        None => NONE,
    }
}

fn list_to_vec(lists: &[Vec<i64>], id: i64) -> Vec<i64> {
    let mut out: Vec<i64> = Vec::new();
    let count = list_len(lists, id);
    let mut idx = 0i64;
    while idx < count {
        out.push(list_get(lists, id, idx));
        idx += 1;
    }
    out
}

/// The declared-parameter keys of a function node, in order.  These are
/// the keys the typechecker attached to the declaration; codegen reads
/// them and substitutes the instance's arguments.
fn fn_declared_param_keys(sess: &mut Session, fn_node: i64) -> Vec<i64> {
    let lists = &sess.6;
    let nodes = &sess.5;
    let tparams = node_b(nodes, fn_node);
    let count = list_len(lists, tparams);
    let mut keys: Vec<i64> = Vec::new();
    let mut idx = 0i64;
    while idx < count {
        let param = list_get(lists, tparams, idx);
        if node_tag(nodes, param) == NODE_TY && node_a(nodes, param) == TY_PARAM {
            keys.push(ty_key_of(nodes, param));
        }
        idx += 1;
    }
    keys
}

// ---------------------------------------------------------------------------
// Values: alloca, load, store, copy, constants.
// ---------------------------------------------------------------------------

fn alloca_typed<'ctx>(sess: &mut Session<'ctx, '_, '_>, key: i64, name: &str) -> Result<PointerValue<'ctx>, CodegenError> {
    let ty = llvm_type(&mut ty_env(sess), key)?;
    sess.2.build_alloca(ty, name).map_err(builder_fail)
}

/// The generic address space, used for every pointer the compiler
/// creates.
fn ptr_ty<'ctx>(sess: &Session<'ctx, '_, '_>) -> inkwell::types::PointerType<'ctx> {
    sess.0.ptr_type(AddressSpace::from(0u16))
}

/// Loads a value of `key` through `ptr` (the pointee type drives the
/// load instruction).
fn load_key<'ctx>(sess: &mut Session<'ctx, '_, '_>, key: i64, ptr: PointerValue<'ctx>) -> Result<BasicValueEnum<'ctx>, CodegenError> {
    let ty = llvm_of(sess, key)?;
    sess.2.build_load(ty, ptr, "").map_err(builder_fail)
}

/// Loads an `i8` value (the U8 runtime representation).
fn load_i8<'ctx>(sess: &mut Session<'ctx, '_, '_>, ptr: PointerValue<'ctx>) -> Result<IntValue<'ctx>, CodegenError> {
    Ok(sess.2.build_load(sess.0.i8_type(), ptr, "").map_err(builder_fail)?.into_int_value())
}

/// Loads an `i64` value (Int, Usize, tags, lengths).
fn load_i64<'ctx>(sess: &mut Session<'ctx, '_, '_>, ptr: PointerValue<'ctx>) -> Result<IntValue<'ctx>, CodegenError> {
    Ok(sess.2.build_load(sess.0.i64_type(), ptr, "").map_err(builder_fail)?.into_int_value())
}

/// Loads a pointer value (a `&T` or `&mut T` slot holds a pointer).
fn load_ptr<'ctx>(sess: &mut Session<'ctx, '_, '_>, ptr: PointerValue<'ctx>) -> Result<PointerValue<'ctx>, CodegenError> {
    Ok(sess.2.build_load(ptr_ty(sess), ptr, "").map_err(builder_fail)?.into_pointer_value())
}

/// A struct-field GEP whose pointee type is `key`'s LLVM type.
fn struct_gep<'ctx>(sess: &mut Session<'ctx, '_, '_>, key: i64, ptr: PointerValue<'ctx>, index: u32, name: &str) -> Result<PointerValue<'ctx>, CodegenError> {
    let ty = llvm_of(sess, key)?;
    sess.2.build_struct_gep(ty, ptr, index, name).map_err(builder_fail)
}

/// A field GEP into the fixed `{data, len}` slice-view layout.
fn slice_gep<'ctx>(sess: &mut Session<'ctx, '_, '_>, ptr: PointerValue<'ctx>, index: u32, name: &str) -> Result<PointerValue<'ctx>, CodegenError> {
    sess.2.build_struct_gep(slice_view_ty(sess.0), ptr, index, name).map_err(builder_fail)
}

fn store_key<'ctx>(
    sess: &mut Session<'ctx, '_, '_>,
    ptr: PointerValue<'ctx>,
    value: BasicValueEnum<'ctx>,
) -> Result<(), CodegenError> {
    sess.2.build_store(ptr, value).map_err(builder_fail)?;
    Ok(())
}

/// True when a key lowers to an aggregate (struct-like) LLVM type, which
/// must be copied with memcpy rather than load/store.
fn is_aggregate_kind(kind: i64) -> bool {
    kind == TYD_STRUCT
        || kind == TYD_ENUM
        || kind == TYD_NATIVE
        || kind == TYD_SLICE
        || kind == TYD_ARRAY
}

fn key_kind_of(nodes: &[i64], key: i64) -> i64 {
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

/// Copies the value at `src` into `dst` (both allocas of `key`): memcpy
/// for aggregates, load/store otherwise.
fn copy_value<'ctx>(
    sess: &mut Session<'ctx, '_, '_>,
    key: i64,
    dst: PointerValue<'ctx>,
    src: PointerValue<'ctx>,
) -> Result<(), CodegenError> {
    let kind = key_kind_of(sess.5, key);
    let elem_kind = key_kind_of(sess.5, key_elem_of(sess.5, key));
    if is_aggregate_kind(kind) || kind == TYD_REF && elem_kind == TYD_SLICE {
        let ty = llvm_type(&mut ty_env(sess), key)?;
        let size = sess.3.get_abi_size(&ty);
        let align = sess.3.get_abi_alignment(&ty);
        let size_val = sess.0.i64_type().const_int(size, false);
        sess.2.build_memcpy(dst, align, src, align, size_val).map_err(builder_fail)?;
    } else {
        let value = load_key(sess, key, src)?;
        store_key(sess, dst, value)?;
    }
    Ok(())
}

fn key_elem_of(nodes: &[i64], key: i64) -> i64 {
    let row = find_tyinfo(nodes, key);
    if row == NONE {
        NONE
    } else {
        node_e(nodes, row)
    }
}

/// The integer constant for a key's scalar representation.
fn const_int_of<'ctx>(sess: &mut Session<'ctx, '_, '_>, key: i64, value: i64) -> Result<BasicValueEnum<'ctx>, CodegenError> {
    let kind = key_kind_of(sess.5, key);
    if kind != TYD_BUILTIN {
        return Err(builder_error(-1, 0, 0, "constant of a non-scalar type"));
    }
    let name = name_text(sess.4, sym_name(sess.5, key_sym_of(sess.5, key)));
    let i8 = sess.0.i8_type();
    let i32 = sess.0.i32_type();
    let i64 = sess.0.i64_type();
    let b = sess.0.bool_type();
    if name == "U8" {
        return Ok(i8.const_int((value & 0xFF) as u64, false).into());
    }
    if name == "U32" {
        return Ok(i32.const_int((value & 0xFFFF_FFFF) as u64, false).into());
    }
    if name == "Bool" {
        return Ok(b.const_int(value as u64, false).into());
    }
    if name == "Int" || name == "Usize" {
        return Ok(i64.const_int(value as u64, false).into());
    }
    Err(builder_error(-1, 0, 0, "unsupported scalar type"))
}

fn key_sym_of(nodes: &[i64], key: i64) -> i64 {
    let row = find_tyinfo(nodes, key);
    if row == NONE {
        NONE
    } else {
        node_c(nodes, row)
    }
}

// ---------------------------------------------------------------------------
// Locals.
// ---------------------------------------------------------------------------

fn bind_local<'ctx>(locals: &mut Locals<'ctx>, name: i64, key: i64, ptr: PointerValue<'ctx>) {
    locals.push((name, key, ptr));
}

fn get_local<'ctx>(locals: &Locals<'ctx>, name: i64) -> Result<PointerValue<'ctx>, CodegenError> {
    let mut idx = locals.len();
    while idx > 0 {
        idx -= 1;
        match locals.get(idx) {
            Some(entry) => {
                if entry.0 == name {
                    return Ok(entry.2);
                }
            }
            None => return Err(builder_error(-1, 0, 0, "internal: unbound local in codegen")),
        }
    }
    Err(builder_error(-1, 0, 0, "internal: unbound local in codegen"))
}

/// The declared key of a local binding, used to walk field chains.
fn get_local_key<'ctx>(locals: &Locals<'ctx>, name: i64) -> Result<i64, CodegenError> {
    let mut idx = locals.len();
    while idx > 0 {
        idx -= 1;
        match locals.get(idx) {
            Some(entry) => {
                if entry.0 == name {
                    return Ok(entry.1);
                }
            }
            None => return Err(builder_error(-1, 0, 0, "internal: unbound local in codegen")),
        }
    }
    Err(builder_error(-1, 0, 0, "internal: unbound local in codegen"))
}

/// Allocates a fresh slot for a value of `key` and returns its pointer.
fn declare_local<'ctx>(sess: &mut Session<'ctx, '_, '_>, key: i64, name: &str) -> Result<PointerValue<'ctx>, CodegenError> {
    alloca_typed(sess, key, name)
}

// ---------------------------------------------------------------------------
// Arena reads (immutable, temporary borrows only) and key substitution.
// ---------------------------------------------------------------------------

fn em_name(sess: &Session, id: i64) -> String {
    name_text(sess.4, id)
}

fn em_expr_ty(sess: &Session, id: i64) -> i64 {
    expr_ty_of(sess.5, id)
}

fn em_expr_sym(sess: &Session, id: i64) -> i64 {
    expr_sym_of(sess.5, id)
}

fn em_sym_decl(sess: &Session, sym: i64) -> i64 {
    node_c(sess.5, sym)
}

fn em_key_kind(sess: &Session, key: i64) -> i64 {
    key_kind_of(sess.5, key)
}

fn em_key_sym(sess: &Session, key: i64) -> i64 {
    key_sym_of(sess.5, key)
}

fn em_key_elem(sess: &Session, key: i64) -> i64 {
    key_elem_of(sess.5, key)
}

/// Builds the type-lowering environment from the session's handles and
/// caches, reborrowed for the duration of one lowering call.
/// Anchors a codegen failure to the function whose compilation produced
/// it.  When the inner failure already carries a real source span that
/// span is kept; a source-less internal failure (a toolchain or
/// invariant error) is anchored to the enclosing function's own
/// declaration span, which is the truthful origin of the failure, and
/// the message names the function so the diagnostic reads clearly.
fn with_fn_span<'ctx>(err: CodegenError, fn_slot: i64, sess: &Session<'ctx, '_, '_>) -> CodegenError {
    if err.span.0 != NO_FILE {
        return err;
    }
    let nodes = &sess.5;
    let name = em_name(sess, node_a(nodes, fn_slot));
    let span = (node_file(nodes, fn_slot), node_start(nodes, fn_slot), node_end(nodes, fn_slot));
    match err.kind {
        CodegenErrorKind::Builder(detail) => builder_error(
            span.0,
            span.1,
            span.2,
            &format!("in '{}': {}", name, detail),
        ),
        CodegenErrorKind::Io(detail) => CodegenError {
            span,
            kind: CodegenErrorKind::Io(format!("in '{}': {}", name, detail)),
        },
        CodegenErrorKind::Tool {
            tool,
            status,
            detail,
        } => CodegenError {
            span,
            kind: CodegenErrorKind::Tool {
                tool,
                status,
                detail: format!("in '{}': {}", name, detail),
            },
        },
    }
}

fn ty_env<'ctx, 'a>(sess: &'a mut Session<'ctx, '_, '_>) -> TyEnv<'ctx, 'a> {
    (
        sess.0,
        sess.3,
        sess.4,
        &mut *sess.5,
        &mut *sess.6,
        &mut sess.7,
        &mut sess.8,
        &mut sess.9,
    )
}

/// The LLVM type of a key, through the session caches.
fn llvm_of<'ctx>(sess: &mut Session<'ctx, '_, '_>, key: i64) -> Result<BasicTypeEnum<'ctx>, CodegenError> {
    llvm_type(&mut ty_env(sess), key)
}

/// Substitutes a key with the current instance's type arguments.  The
/// declared parameter keys are the typechecker's facts; substitution is
/// the mechanical rewrite shared with the typechecker.
fn sub_key(sess: &mut Session, from: &[i64], to: &[i64], key: i64) -> i64 {
    let nodes = &mut sess.5;
    let lists = &mut sess.6;
    subst_key(nodes, lists, key, from, to)
}

fn key_args_of(sess: &Session, key: i64) -> i64 {
    let row = find_tyinfo(sess.5, key);
    if row == NONE {
        NONE
    } else {
        node_d(sess.5, row)
    }
}

fn key_len_of(sess: &Session, key: i64) -> i64 {
    let row = find_tyinfo(sess.5, key);
    if row == NONE {
        0
    } else {
        node_f(sess.5, row)
    }
}

/// The dereferenced key of a reference key, unchanged otherwise.
fn deref_key_of(sess: &Session, key: i64) -> i64 {
    let kind = em_key_kind(sess, key);
    if kind == TYD_REF || kind == TYD_REF_MUT {
        em_key_elem(sess, key)
    } else {
        key
    }
}

fn list_to_vec_of(sess: &Session, id: i64) -> Vec<i64> {
    list_to_vec(sess.6, id)
}

/// The declared-parameter keys of a type declaration item.
fn declared_param_keys_of_item(sess: &Session, item: i64) -> Vec<i64> {
    let is_native = node_a(sess.5, item) == ITEM_NATIVE_TYPE;
    let params = if is_native {
        node_e(sess.5, item)
    } else {
        node_f(sess.5, item)
    };
    let count = list_len(sess.6, params);
    let mut keys: Vec<i64> = Vec::new();
    let mut idx = 0i64;
    while idx < count {
        let param = list_get(sess.6, params, idx);
        if node_tag(sess.5, param) == NODE_TY && node_a(sess.5, param) == TY_PARAM {
            keys.push(ty_key_of(sess.5, param));
        }
        idx += 1;
    }
    keys
}

/// The substituted key of a struct field, read from the declaration.
fn struct_field_key(sess: &mut Session, item: i64, struct_key: i64, fld_idx: i64) -> Result<i64, CodegenError> {
    let fields = node_e(sess.5, item);
    let field = list_get(sess.6, fields, fld_idx);
    if field == NONE {
        return Err(builder_error(-1, 0, 0, "internal: struct field index out of range"));
    }
    let declared = ty_key_of(sess.5, node_b(sess.5, field));
    let from = declared_param_keys_of_item(sess, item);
    let to = list_to_vec_of(sess, key_args_of(sess, struct_key));
    let nodes = &mut sess.5;
    let lists = &mut sess.6;
    Ok(subst_key(nodes, lists, declared, &from, &to))
}

/// The index of the declared field named `name`, or an internal error.
fn struct_field_index(sess: &Session, item: i64, name: i64) -> Result<i64, CodegenError> {
    let fields = node_e(sess.5, item);
    let count = list_len(sess.6, fields);
    let mut idx = 0i64;
    while idx < count {
        let field = list_get(sess.6, fields, idx);
        if node_a(sess.5, field) == name {
            return Ok(idx);
        }
        idx += 1;
    }
    Err(builder_error(-1, 0, 0, "internal: struct field not found"))
}

/// The declared-variant index of `variant_sym` inside its enum, derived
/// from the enum's declared variant order.
fn variant_index_of(sess: &Session, enum_key: i64, variant_sym: i64) -> Result<i64, CodegenError> {
    let enum_sym = em_key_sym(sess, enum_key);
    if enum_sym == NONE {
        return Err(builder_error(-1, 0, 0, "internal: enum key without a symbol"));
    }
    let item = em_sym_decl(sess, enum_sym);
    let variants = node_e(sess.5, item);
    let count = list_len(sess.6, variants);
    let vdecl = node_c(sess.5, variant_sym);
    let mut idx = 0i64;
    while idx < count {
        if list_get(sess.6, variants, idx) == vdecl {
            return Ok(idx);
        }
        idx += 1;
    }
    Err(builder_error(-1, 0, 0, "internal: variant not found in its enum"))
}

/// The declared-variant index of the variant named `text`.
fn variant_index_by_name(sess: &Session, enum_key: i64, text: &str) -> Result<i64, CodegenError> {
    let enum_sym = em_key_sym(sess, enum_key);
    if enum_sym == NONE {
        return Err(builder_error(-1, 0, 0, "internal: enum key without a symbol"));
    }
    let item = em_sym_decl(sess, enum_sym);
    let variants = node_e(sess.5, item);
    let count = list_len(sess.6, variants);
    let mut idx = 0i64;
    while idx < count {
        let variant = list_get(sess.6, variants, idx);
        if name_is(sess.4, node_a(sess.5, variant), text) {
            return Ok(idx);
        }
        idx += 1;
    }
    Err(builder_error(-1, 0, 0, &format!("internal: variant '{}' not found", text)))
}

/// The declared-variant index of the variant named `text` in the enum at
/// `enum_key`, or NONE when the enum declares no such variant.
fn variant_index_by_name_opt(sess: &Session, enum_key: i64, text: &str) -> i64 {
    let enum_sym = em_key_sym(sess, enum_key);
    if enum_sym == NONE {
        return NONE;
    }
    let item = em_sym_decl(sess, enum_sym);
    let variants = node_e(sess.5, item);
    let count = list_len(sess.6, variants);
    let mut idx = 0i64;
    while idx < count {
        let variant = list_get(sess.6, variants, idx);
        if name_is(sess.4, node_a(sess.5, variant), text) {
            return idx;
        }
        idx += 1;
    }
    NONE
}

/// True when the enum key's type is named `text`.
fn enum_name_is(sess: &Session, key: i64, text: &str) -> bool {
    let sym = em_key_sym(sess, key);
    if sym == NONE {
        return false;
    }
    name_is(sess.4, node_b(sess.5, sym), text)
}

/// The declared payload-field count of `(enum_key, variant_idx)`, read
/// from the enum's own declaration.  Zero for unit variants (for
/// example `None`), which carry no payload to construct or copy.
fn variant_payload_count(sess: &Session, enum_key: i64, variant_idx: i64) -> i64 {
    let enum_sym = em_key_sym(sess, enum_key);
    if enum_sym == NONE {
        return 0;
    }
    let item = em_sym_decl(sess, enum_sym);
    let variants = node_e(sess.5, item);
    let variant = list_get(sess.6, variants, variant_idx);
    if variant == NONE {
        return 0;
    }
    list_len(sess.6, node_b(sess.5, variant))
}

/// The substituted key of one declared payload field of a variant.
fn variant_payload_key(sess: &mut Session, enum_key: i64, variant_idx: i64, field_idx: i64) -> Result<i64, CodegenError> {
    let enum_sym = em_key_sym(sess, enum_key);
    if enum_sym == NONE {
        return Err(builder_error(-1, 0, 0, "internal: enum key without a symbol"));
    }
    let item = em_sym_decl(sess, enum_sym);
    let variants = node_e(sess.5, item);
    let variant = list_get(sess.6, variants, variant_idx);
    if variant == NONE {
        return Err(builder_error(-1, 0, 0, "internal: variant index out of range"));
    }
    let payload_decl = node_b(sess.5, variant);
    let declared = ty_key_of(sess.5, list_get(sess.6, payload_decl, field_idx));
    let from = declared_param_keys_of_item(sess, item);
    let to = list_to_vec_of(sess, key_args_of(sess, enum_key));
    let nodes = &mut sess.5;
    let lists = &mut sess.6;
    Ok(subst_key(nodes, lists, declared, &from, &to))
}

/// The payload-region pointer of an enum value at `ptr`, together with
/// the payload struct type of `(enum_key, variant_idx)` (opaque pointers
/// carry no pointee type, so the struct type is returned for the field
/// GEPs that follow).
fn enum_payload_ptr<'ctx>(sess: &mut Session<'ctx, '_, '_>, ptr: PointerValue<'ctx>, enum_key: i64, variant_idx: i64) -> Result<(PointerValue<'ctx>, BasicTypeEnum<'ctx>), CodegenError> {
    let enum_ty = llvm_of(sess, enum_key)?;
    let region = sess.2.build_struct_gep(enum_ty, ptr, 1, "").map_err(builder_fail)?;
    let pty = payload_struct_of(&mut ty_env(sess), enum_key, variant_idx)?;
    Ok((region, pty))
}

/// Builds an enum value in a fresh slot: tag at offset 0, each payload
/// field copied into the payload region.
fn build_enum_value<'ctx>(sess: &mut Session<'ctx, '_, '_>, key: i64, variant_idx: i64, payloads: &[(i64, PointerValue<'ctx>)]) -> Result<PointerValue<'ctx>, CodegenError> {
    let ptr = declare_local(sess, key, "enum")?;
    build_enum_value_into(sess, key, variant_idx, payloads, ptr)?;
    Ok(ptr)
}

/// Stores tag `variant_idx` and its payloads into an already-allocated
/// enum slot `out`.  The one implementation of enum construction, shared
/// by the allocating `build_enum_value` and by division's result branch.
fn build_enum_value_into<'ctx>(sess: &mut Session<'ctx, '_, '_>, key: i64, variant_idx: i64, payloads: &[(i64, PointerValue<'ctx>)], out: PointerValue<'ctx>) -> Result<(), CodegenError> {
    let tag_ptr = struct_gep(sess, key, out, 0, "")?;
    let tag = sess.0.i64_type().const_int(variant_idx as u64, false);
    store_key(sess, tag_ptr, tag.into())?;
    let mut idx = 0usize;
    while idx < payloads.len() {
        let (pkey, pptr) = match payloads.get(idx) {
            Some(pair) => *pair,
            None => break,
        };
        let (region, pty) = enum_payload_ptr(sess, out, key, variant_idx)?;
        let fptr = sess.2.build_struct_gep(pty, region, idx as u32, "").map_err(builder_fail)?;
        copy_value(sess, pkey, fptr, pptr)?;
        idx += 1;
    }
    Ok(())
}

/// The data pointer of a slice view `{ptr, len}`.
fn slice_data<'ctx>(sess: &mut Session<'ctx, '_, '_>, view_ptr: PointerValue<'ctx>) -> Result<PointerValue<'ctx>, CodegenError> {
    let dp = slice_gep(sess, view_ptr, 0, "")?;
    load_ptr(sess, dp)
}

/// The length field of a slice view `{ptr, len}`.
fn slice_len_of<'ctx>(sess: &mut Session<'ctx, '_, '_>, view_ptr: PointerValue<'ctx>) -> Result<IntValue<'ctx>, CodegenError> {
    let lp = slice_gep(sess, view_ptr, 1, "")?;
    load_i64(sess, lp)
}

/// `base + idx * sizeof(elem)` as an element pointer.  The only general
/// GEP inkwell 0.9 exposes is unsafe; the index arithmetic is guarded by
/// the bounds checks emitted at every native surface.
fn offset_elem_ptr<'ctx>(sess: &mut Session<'ctx, '_, '_>, elem_key: i64, base: PointerValue<'ctx>, idx: IntValue<'ctx>) -> Result<PointerValue<'ctx>, CodegenError> {
    let elem_ty = llvm_of(sess, elem_key)?;
    let gep = unsafe { sess.2.build_gep(elem_ty, base, &[idx], "") }.map_err(builder_fail)?;
    Ok(gep)
}

/// True when the current block already ends with a terminator.
/// True when the current block already ends with a terminator.
fn block_terminated<'ctx>(sess: &mut Session<'ctx, '_, '_>) -> bool {
    match sess.2.get_insert_block() {
        Some(block) => block.get_terminator().is_some(),
        None => false,
    }
}

/// Appends a basic block to the function currently being built.
fn new_block<'ctx>(sess: &mut Session<'ctx, '_, '_>, f: FunctionValue<'ctx>, name: &str) -> BasicBlockId<'ctx> {
    sess.0.append_basic_block(f, name)
}

/// Stores tag 0 into an empty enum (the Unit value of `key`).
fn build_unit_value_into<'ctx>(sess: &mut Session<'ctx, '_, '_>, key: i64, ptr: PointerValue<'ctx>) -> Result<(), CodegenError> {
    let tag_ptr = struct_gep(sess, key, ptr, 0, "")?;
    let tag = sess.0.i64_type().const_int(0, false);
    store_key(sess, tag_ptr, tag.into())?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Statements.
// ---------------------------------------------------------------------------

fn emit_stmt_list<'ctx, 'a>(
    sess: &mut Session<'ctx, '_, '_>,
    ctx: &mut FnCtx<'ctx, 'a>,
    list: i64,
) -> Result<(PointerValue<'ctx>, bool), CodegenError> {
    let count = list_len(sess.6, list);
    if count == 0 {
        return Err(builder_error(-1, 0, 0, "internal: empty statement list"));
    }
    let mut out = emit_stmt(sess, ctx, list_get(sess.6, list, 0))?;
    let mut idx = 1i64;
    while idx < count {
        if out.1 {
            break;
        }
        out = emit_stmt(sess, ctx, list_get(sess.6, list, idx))?;
        idx += 1;
    }
    Ok(out)
}

fn emit_stmt<'ctx, 'a>(
    sess: &mut Session<'ctx, '_, '_>,
    ctx: &mut FnCtx<'ctx, 'a>,
    stmt: i64,
) -> Result<(PointerValue<'ctx>, bool), CodegenError> {
    let kind = node_a(sess.5, stmt);
    if kind == STMT_LET {
        return emit_let(sess, ctx, stmt);
    }
    if kind == STMT_ASSIGN {
        return emit_assign(sess, ctx, stmt);
    }
    if kind == STMT_WHILE {
        return emit_while(sess, ctx, stmt);
    }
    if kind == STMT_IF {
        return emit_if(sess, ctx, stmt);
    }
    if kind == STMT_RETURN {
        return emit_return(sess, ctx, stmt);
    }
    if kind == STMT_BREAK {
        return emit_loop_branch(sess, ctx, 0, stmt);
    }
    if kind == STMT_CONTINUE {
        return emit_loop_branch(sess, ctx, 1, stmt);
    }
    let expr = node_b(sess.5, stmt);
    let ptr = emit_expr(sess, ctx, expr)?;
    Ok((ptr, false))
}

fn emit_loop_branch<'ctx, 'a>(
    sess: &mut Session<'ctx, '_, '_>,
    ctx: &mut FnCtx<'ctx, 'a>,
    which: i64,
    stmt: i64,
) -> Result<(PointerValue<'ctx>, bool), CodegenError> {
    let target = match ctx.2.last() {
        Some(pair) => {
            if which == 0 {
                pair.0
            } else {
                pair.1
            }
        }
        None => {
            return Err(builder_error(
                node_file(sess.5, stmt),
                node_start(sess.5, stmt),
                node_end(sess.5, stmt),
                "internal: break or continue outside a loop",
            ));
        }
    };
    let key = sub_key(sess, ctx.3, ctx.4, stmt_ty_of(sess.5, stmt));
    // The slot is allocated before the branch: the block is diverged, so
    // any instruction after the terminator would be invalid IR.  The
    // alloca is dead (nothing reads it) and `-O2` removes it.
    let slot = alloca_typed(sess, key, "lb")?;
    sess.2.build_unconditional_branch(target).map_err(builder_fail)?;
    Ok((slot, true))
}

fn emit_let<'ctx, 'a>(
    sess: &mut Session<'ctx, '_, '_>,
    ctx: &mut FnCtx<'ctx, 'a>,
    stmt: i64,
) -> Result<(PointerValue<'ctx>, bool), CodegenError> {
    let name = node_c(sess.5, stmt);
    let init = node_e(sess.5, stmt);
    let key = sub_key(sess, ctx.3, ctx.4, stmt_ty_of(sess.5, stmt));
    let ptr = declare_local(sess, key, &em_name(sess, name))?;
    let init_ptr = emit_expr(sess, ctx, init)?;
    copy_value(sess, key, ptr, init_ptr)?;
    bind_local(&mut ctx.1, name, key, ptr);
    Ok((ptr, false))
}

fn emit_assign<'ctx, 'a>(
    sess: &mut Session<'ctx, '_, '_>,
    ctx: &mut FnCtx<'ctx, 'a>,
    stmt: i64,
) -> Result<(PointerValue<'ctx>, bool), CodegenError> {
    let target = node_b(sess.5, stmt);
    let value = node_c(sess.5, stmt);
    let key = sub_key(sess, ctx.3, ctx.4, stmt_ty_of(sess.5, stmt));
    let tptr = get_local(&ctx.1, target)?;
    let vptr = emit_expr(sess, ctx, value)?;
    copy_value(sess, key, tptr, vptr)?;
    Ok((tptr, false))
}

fn emit_while<'ctx, 'a>(
    sess: &mut Session<'ctx, '_, '_>,
    ctx: &mut FnCtx<'ctx, 'a>,
    stmt: i64,
) -> Result<(PointerValue<'ctx>, bool), CodegenError> {
    let cond = node_b(sess.5, stmt);
    let body = node_c(sess.5, stmt);
    let cond_block = new_block(sess, ctx.0, "while_cond");
    let body_block = new_block(sess, ctx.0, "while_body");
    let exit_block = new_block(sess, ctx.0, "while_exit");
    sess.2.build_unconditional_branch(cond_block).map_err(builder_fail)?;
    sess.2.position_at_end(cond_block);
    let ckey = sub_key(sess, ctx.3, ctx.4, em_expr_ty(sess, cond));
    let cptr = emit_expr(sess, ctx, cond)?;
    let cv = load_key(sess, ckey, cptr)?.into_int_value();
    sess.2.build_conditional_branch(cv, body_block, exit_block).map_err(builder_fail)?;
    sess.2.position_at_end(body_block);
    ctx.2.push((exit_block, cond_block));
    emit_stmt_list(sess, ctx, body)?;
    ctx.2.pop();
    if !block_terminated(sess) {
        sess.2.build_unconditional_branch(cond_block).map_err(builder_fail)?;
    }
    sess.2.position_at_end(exit_block);
    let key = sub_key(sess, ctx.3, ctx.4, stmt_ty_of(sess.5, stmt));
    let slot = alloca_typed(sess, key, "while")?;
    Ok((slot, false))
}

fn emit_if<'ctx, 'a>(
    sess: &mut Session<'ctx, '_, '_>,
    ctx: &mut FnCtx<'ctx, 'a>,
    stmt: i64,
) -> Result<(PointerValue<'ctx>, bool), CodegenError> {
    let cond = node_b(sess.5, stmt);
    let then_list = node_c(sess.5, stmt);
    let else_list = node_d(sess.5, stmt);
    let ckey = sub_key(sess, ctx.3, ctx.4, em_expr_ty(sess, cond));
    let cptr = emit_expr(sess, ctx, cond)?;
    let cv = load_key(sess, ckey, cptr)?.into_int_value();
    let then_block = new_block(sess, ctx.0, "if_then");
    let else_block = new_block(sess, ctx.0, "if_else");
    let merge_block = new_block(sess, ctx.0, "if_merge");
    sess.2.build_conditional_branch(cv, then_block, else_block).map_err(builder_fail)?;
    sess.2.position_at_end(then_block);
    emit_stmt_list(sess, ctx, then_list)?;
    if !block_terminated(sess) {
        sess.2.build_unconditional_branch(merge_block).map_err(builder_fail)?;
    }
    sess.2.position_at_end(else_block);
    if else_list != NONE {
        emit_stmt_list(sess, ctx, else_list)?;
        if !block_terminated(sess) {
            sess.2.build_unconditional_branch(merge_block).map_err(builder_fail)?;
        }
    } else {
        sess.2.build_unconditional_branch(merge_block).map_err(builder_fail)?;
    }
    sess.2.position_at_end(merge_block);
    let key = sub_key(sess, ctx.3, ctx.4, stmt_ty_of(sess.5, stmt));
    let slot = alloca_typed(sess, key, "if")?;
    Ok((slot, false))
}

fn emit_return<'ctx, 'a>(
    sess: &mut Session<'ctx, '_, '_>,
    ctx: &mut FnCtx<'ctx, 'a>,
    stmt: i64,
) -> Result<(PointerValue<'ctx>, bool), CodegenError> {
    let value = node_b(sess.5, stmt);
    let ret_key = sub_key(sess, ctx.3, ctx.4, ctx.5);
    let ptr = if value == NONE {
        let slot = alloca_typed(sess, ret_key, "ret")?;
        build_unit_value_into(sess, ret_key, slot)?;
        slot
    } else {
        emit_expr(sess, ctx, value)?
    };
    let loaded = load_key(sess, ret_key, ptr)?;
    sess.2.build_return(Some(&loaded)).map_err(builder_fail)?;
    Ok((ptr, true))
}

// ---------------------------------------------------------------------------
// Expressions.
// ---------------------------------------------------------------------------

fn emit_expr<'ctx, 'a>(
    sess: &mut Session<'ctx, '_, '_>,
    ctx: &mut FnCtx<'ctx, 'a>,
    expr: i64,
) -> Result<PointerValue<'ctx>, CodegenError> {
    let kind = node_a(sess.5, expr);
    if kind == EXPR_LIT {
        return emit_lit(sess, ctx, expr);
    }
    if kind == EXPR_PATH {
        return emit_path(sess, ctx, expr);
    }
    if kind == EXPR_UNARY {
        return emit_unary(sess, ctx, expr);
    }
    if kind == EXPR_BINARY {
        return emit_binary(sess, ctx, expr);
    }
    if kind == EXPR_CALL {
        return emit_call(sess, ctx, expr);
    }
    if kind == EXPR_STRUCT_LIT {
        return emit_struct_lit(sess, ctx, expr);
    }
    if kind == EXPR_ARRAY {
        return emit_array(sess, ctx, expr);
    }
    if kind == EXPR_MATCH {
        return emit_match(sess, ctx, expr);
    }
    if kind == EXPR_TRY {
        return emit_try(sess, ctx, expr);
    }
    Err(builder_error(
        node_file(sess.5, expr),
        node_start(sess.5, expr),
        node_end(sess.5, expr),
        "internal: unknown expression kind",
    ))
}

fn emit_lit<'ctx, 'a>(
    sess: &mut Session<'ctx, '_, '_>,
    ctx: &mut FnCtx<'ctx, 'a>,
    expr: i64,
) -> Result<PointerValue<'ctx>, CodegenError> {
    let key = sub_key(sess, ctx.3, ctx.4, em_expr_ty(sess, expr));
    let value = node_c(sess.5, expr);
    let ptr = declare_local(sess, key, "lit")?;
    let cv = const_int_of(sess, key, value)?;
    store_key(sess, ptr, cv)?;
    Ok(ptr)
}

fn emit_path<'ctx, 'a>(
    sess: &mut Session<'ctx, '_, '_>,
    ctx: &mut FnCtx<'ctx, 'a>,
    expr: i64,
) -> Result<PointerValue<'ctx>, CodegenError> {
    let sym = em_expr_sym(sess, expr);
    if sym != NONE {
        let kind = node_a(sess.5, sym);
        if kind == SYM_CONST {
            let key = sub_key(sess, ctx.3, ctx.4, em_expr_ty(sess, expr));
            let ptr = declare_local(sess, key, "const")?;
            if !has_const_value(sess.5, sym) {
                return Err(builder_error(-1, 0, 0, "internal: constant without a folded value"));
            }
            let value = find_const_value(sess.5, sym);
            let cv = const_int_of(sess, key, value)?;
            store_key(sess, ptr, cv)?;
            return Ok(ptr);
        }
        if kind == SYM_VARIANT {
            let key = sub_key(sess, ctx.3, ctx.4, em_expr_ty(sess, expr));
            let idx = variant_index_of(sess, key, sym)?;
            return build_enum_value(sess, key, idx, &[]);
        }
        return Err(builder_error(
            node_file(sess.5, expr),
            node_start(sess.5, expr),
            node_end(sess.5, expr),
            "internal: declaration used as a value",
        ));
    }
    let segs = node_b(sess.5, expr);
    let count = list_len(sess.6, segs);
    let first = list_get(sess.6, segs, 0);
    let mut ptr = get_local(&ctx.1, first)?;
    let mut cur_key = get_local_key(&ctx.1, first)?;
    let mut idx = 1i64;
    while idx < count {
        let field = list_get(sess.6, segs, idx);
        let ckind = em_key_kind(sess, cur_key);
        if ckind == TYD_REF || ckind == TYD_REF_MUT {
            ptr = load_ptr(sess, ptr)?;
            cur_key = em_key_elem(sess, cur_key);
        }
        let sym2 = em_key_sym(sess, cur_key);
        if sym2 == NONE {
            return Err(builder_error(-1, 0, 0, "internal: field access on a non-struct"));
        }
        let item = em_sym_decl(sess, sym2);
        let fld_idx = struct_field_index(sess, item, field)?;
        ptr = struct_gep(sess, cur_key, ptr, fld_idx as u32, "")?;
        cur_key = struct_field_key(sess, item, cur_key, fld_idx)?;
        idx += 1;
    }
    Ok(ptr)
}

fn emit_unary<'ctx, 'a>(
    sess: &mut Session<'ctx, '_, '_>,
    ctx: &mut FnCtx<'ctx, 'a>,
    expr: i64,
) -> Result<PointerValue<'ctx>, CodegenError> {
    let op = node_b(sess.5, expr);
    let inner = node_c(sess.5, expr);
    let key = sub_key(sess, ctx.3, ctx.4, em_expr_ty(sess, expr));
    if op == UN_REF || op == UN_REF_MUT {
        // The value of `&x` is a pointer, materialized in a slot of the
        // reference type (which lowers to a pointer).  Every consumer
        // (call arguments, let/assign copies, returns) loads through
        // the expression's key, so the address must be stored into a
        // slot, not returned raw: returning the referent's storage
        // address directly would make the load read the referent's own
        // first bytes as if they were the pointer.
        let inner_ptr = emit_expr(sess, ctx, inner)?;
        let out = declare_local(sess, key, "ref")?;
        store_key(sess, out, inner_ptr.into())?;
        return Ok(out);
    }
    let ikey = sub_key(sess, ctx.3, ctx.4, em_expr_ty(sess, inner));
    let iptr = emit_expr(sess, ctx, inner)?;
    let iv = load_key(sess, ikey, iptr)?.into_int_value();
    let out = declare_local(sess, key, "un")?;
    if op == UN_NEG {
        let r = sess.2.build_int_neg(iv, "").map_err(builder_fail)?;
        store_key(sess, out, r.into())?;
        return Ok(out);
    }
    let r = sess.2.build_not(iv, "").map_err(builder_fail)?;
    store_key(sess, out, r.into())?;
    Ok(out)
}

/// True when a key is the signed Int builtin (division and comparison
/// signedness follow the declared type).
fn key_is_signed(sess: &Session, key: i64) -> bool {
    let row = find_tyinfo(sess.5, key);
    if row == NONE {
        return false;
    }
    let sym = node_c(sess.5, row);
    name_is(sess.4, node_b(sess.5, sym), "Int")
}

fn bin_lt_pred(sess: &Session, key: i64) -> IntPredicate {
    if key_is_signed(sess, key) {
        IntPredicate::SLT
    } else {
        IntPredicate::ULT
    }
}

fn bin_gt_pred(sess: &Session, key: i64) -> IntPredicate {
    if key_is_signed(sess, key) {
        IntPredicate::SGT
    } else {
        IntPredicate::UGT
    }
}

fn bin_le_pred(sess: &Session, key: i64) -> IntPredicate {
    if key_is_signed(sess, key) {
        IntPredicate::SLE
    } else {
        IntPredicate::ULE
    }
}

fn bin_ge_pred(sess: &Session, key: i64) -> IntPredicate {
    if key_is_signed(sess, key) {
        IntPredicate::SGE
    } else {
        IntPredicate::UGE
    }
}

fn emit_binary<'ctx, 'a>(
    sess: &mut Session<'ctx, '_, '_>,
    ctx: &mut FnCtx<'ctx, 'a>,
    expr: i64,
) -> Result<PointerValue<'ctx>, CodegenError> {
    let op = node_b(sess.5, expr);
    let lhs = node_c(sess.5, expr);
    let rhs = node_d(sess.5, expr);
    let result_key = sub_key(sess, ctx.3, ctx.4, em_expr_ty(sess, expr));
    let lkey = sub_key(sess, ctx.3, ctx.4, em_expr_ty(sess, lhs));
    let rkey = sub_key(sess, ctx.3, ctx.4, em_expr_ty(sess, rhs));
    let lptr = emit_expr(sess, ctx, lhs)?;
    let rptr = emit_expr(sess, ctx, rhs)?;
    let lv = load_key(sess, lkey, lptr)?.into_int_value();
    let rv = load_key(sess, rkey, rptr)?.into_int_value();
    let out = declare_local(sess, result_key, "bin")?;
    let r;
    if op == BIN_DIV || op == BIN_MOD {
        emit_div_rem_result(sess, ctx, op, lkey, (lv, rv), result_key, out)?;
        return Ok(out);
    } else if op == BIN_ADD {
        r = sess.2.build_int_add(lv, rv, "").map_err(builder_fail)?;
    } else if op == BIN_SUB {
        r = sess.2.build_int_sub(lv, rv, "").map_err(builder_fail)?;
    } else if op == BIN_MUL {
        r = sess.2.build_int_mul(lv, rv, "").map_err(builder_fail)?;
    } else if op == BIN_SHL {
        r = sess.2.build_left_shift(lv, rv, "").map_err(builder_fail)?;
    } else if op == BIN_SHR {
        r = if key_is_signed(sess, lkey) {
            sess.2.build_right_shift(lv, rv, true, "").map_err(builder_fail)?
        } else {
            sess.2.build_right_shift(lv, rv, false, "").map_err(builder_fail)?
        };
    } else if op == BIN_BAND {
        r = sess.2.build_and(lv, rv, "").map_err(builder_fail)?;
    } else if op == BIN_BOR {
        r = sess.2.build_or(lv, rv, "").map_err(builder_fail)?;
    } else if op == BIN_BXOR {
        r = sess.2.build_xor(lv, rv, "").map_err(builder_fail)?;
    } else if op == BIN_EQ {
        r = sess.2.build_int_compare(IntPredicate::EQ, lv, rv, "").map_err(builder_fail)?;
    } else if op == BIN_NE {
        r = sess.2.build_int_compare(IntPredicate::NE, lv, rv, "").map_err(builder_fail)?;
    } else if op == BIN_LT {
        r = sess.2.build_int_compare(bin_lt_pred(sess, lkey), lv, rv, "").map_err(builder_fail)?;
    } else if op == BIN_GT {
        r = sess.2.build_int_compare(bin_gt_pred(sess, lkey), lv, rv, "").map_err(builder_fail)?;
    } else if op == BIN_LE {
        r = sess.2.build_int_compare(bin_le_pred(sess, lkey), lv, rv, "").map_err(builder_fail)?;
    } else if op == BIN_GE {
        r = sess.2.build_int_compare(bin_ge_pred(sess, lkey), lv, rv, "").map_err(builder_fail)?;
    } else if op == BIN_AND {
        r = sess.2.build_and(lv, rv, "").map_err(builder_fail)?;
    } else if op == BIN_OR {
        r = sess.2.build_or(lv, rv, "").map_err(builder_fail)?;
    } else {
        return Err(builder_error(
            node_file(sess.5, expr),
            node_start(sess.5, expr),
            node_end(sess.5, expr),
            "internal: unknown binary operator",
        ));
    }
    store_key(sess, out, r.into())?;
    Ok(out)
}

/// The runtime body of `/` and `%`: builds `Ok(quotient)` when the
/// divisor is non-zero and `Err(DivByZero)` when it is zero — a
/// `Result`, never a trap.  The payload type is the operand type `lkey`
/// (arg 0 of the result key); the error tag comes from the declared
/// DivError variants, and both tags from each enum's declared variant
/// order.
fn emit_div_rem_result<'ctx>(
    sess: &mut Session<'ctx, '_, '_>,
    ctx: &mut FnCtx<'ctx, '_>,
    op: i64,
    lkey: i64,
    operands: (IntValue<'ctx>, IntValue<'ctx>),
    result_key: i64,
    out: PointerValue<'ctx>,
) -> Result<(), CodegenError> {
    let (lv, rv) = operands;
    let zero = sess.0.i64_type().const_zero();
    let is_zero = sess.2.build_int_compare(IntPredicate::EQ, rv, zero, "").map_err(builder_fail)?;
    let ok_block = new_block(sess, ctx.0, "div_ok");
    let err_block = new_block(sess, ctx.0, "div_err");
    let merge_block = new_block(sess, ctx.0, "div_merge");
    sess.2.build_conditional_branch(is_zero, err_block, ok_block).map_err(builder_fail)?;
    sess.2.position_at_end(err_block);
    let err_key = result_arg_key(sess, result_key, 1);
    let err_tag = variant_index_by_name(sess, err_key, "DivByZero")?;
    let div_error = declare_local(sess, err_key, "div_err_val")?;
    build_enum_value_into(sess, err_key, err_tag, &[], div_error)?;
    let err_variant = variant_index_by_name(sess, result_key, "Err")?;
    build_enum_value_into(sess, result_key, err_variant, &[(err_key, div_error)], out)?;
    sess.2.build_unconditional_branch(merge_block).map_err(builder_fail)?;
    sess.2.position_at_end(ok_block);
    let q = if op == BIN_DIV {
        if key_is_signed(sess, lkey) {
            sess.2.build_int_signed_div(lv, rv, "").map_err(builder_fail)?
        } else {
            sess.2.build_int_unsigned_div(lv, rv, "").map_err(builder_fail)?
        }
    } else if key_is_signed(sess, lkey) {
        sess.2.build_int_signed_rem(lv, rv, "").map_err(builder_fail)?
    } else {
        sess.2.build_int_unsigned_rem(lv, rv, "").map_err(builder_fail)?
    };
    let quotient = declare_local(sess, lkey, "quo")?;
    store_key(sess, quotient, q.into())?;
    let ok_tag = variant_index_by_name(sess, result_key, "Ok")?;
    build_enum_value_into(sess, result_key, ok_tag, &[(lkey, quotient)], out)?;
    sess.2.build_unconditional_branch(merge_block).map_err(builder_fail)?;
    sess.2.position_at_end(merge_block);
    Ok(())
}

fn emit_array<'ctx, 'a>(
    sess: &mut Session<'ctx, '_, '_>,
    ctx: &mut FnCtx<'ctx, 'a>,
    expr: i64,
) -> Result<PointerValue<'ctx>, CodegenError> {
    let key = sub_key(sess, ctx.3, ctx.4, em_expr_ty(sess, expr));
    let ptr = declare_local(sess, key, "arr")?;
    let elems = node_b(sess.5, expr);
    let count = list_len(sess.6, elems);
    let elem_key = em_key_elem(sess, key);
    let mut idx = 0i64;
    while idx < count {
        let eptr = offset_elem_ptr(sess, elem_key, ptr, sess.0.i64_type().const_int(idx as u64, false))?;
        let vptr = emit_expr(sess, ctx, list_get(sess.6, elems, idx))?;
        copy_value(sess, elem_key, eptr, vptr)?;
        idx += 1;
    }
    Ok(ptr)
}

fn emit_struct_lit<'ctx, 'a>(
    sess: &mut Session<'ctx, '_, '_>,
    ctx: &mut FnCtx<'ctx, 'a>,
    expr: i64,
) -> Result<PointerValue<'ctx>, CodegenError> {
    let sym = em_expr_sym(sess, expr);
    if sym == NONE {
        return Err(builder_error(-1, 0, 0, "internal: struct literal without a symbol"));
    }
    let key = sub_key(sess, ctx.3, ctx.4, em_expr_ty(sess, expr));
    let kind = node_a(sess.5, sym);
    if kind == SYM_STRUCT {
        let item = node_c(sess.5, sym);
        let ptr = declare_local(sess, key, "struct")?;
        let names = node_c(sess.5, expr);
        let values = node_d(sess.5, expr);
        let count = list_len(sess.6, names);
        let mut idx = 0i64;
        while idx < count {
            let name = list_get(sess.6, names, idx);
            let fld_idx = struct_field_index(sess, item, name)?;
            let fkey = struct_field_key(sess, item, key, fld_idx)?;
            let fptr = struct_gep(sess, key, ptr, fld_idx as u32, "")?;
            let vptr = emit_expr(sess, ctx, list_get(sess.6, values, idx))?;
            copy_value(sess, fkey, fptr, vptr)?;
            idx += 1;
        }
        return Ok(ptr);
    }
    if kind == SYM_VARIANT {
        let idx = variant_index_of(sess, key, sym)?;
        let values = node_d(sess.5, expr);
        let count = list_len(sess.6, values);
        let mut payloads: Vec<(i64, PointerValue<'ctx>)> = Vec::new();
        let mut i = 0i64;
        while i < count {
            let pkey = variant_payload_key(sess, key, idx, i)?;
            let pptr = emit_expr(sess, ctx, list_get(sess.6, values, i))?;
            payloads.push((pkey, pptr));
            i += 1;
        }
        return build_enum_value(sess, key, idx, &payloads);
    }
    Err(builder_error(-1, 0, 0, "internal: cannot construct this symbol"))
}

fn emit_match<'ctx, 'a>(
    sess: &mut Session<'ctx, '_, '_>,
    ctx: &mut FnCtx<'ctx, 'a>,
    expr: i64,
) -> Result<PointerValue<'ctx>, CodegenError> {
    let scrutinee = node_b(sess.5, expr);
    let arms = node_c(sess.5, expr);
    let count = list_len(sess.6, arms);
    let result_key = sub_key(sess, ctx.3, ctx.4, em_expr_ty(sess, expr));
    let result_alloca = declare_local(sess, result_key, "match")?;
    let s_key = sub_key(sess, ctx.3, ctx.4, em_expr_ty(sess, scrutinee));
    let scrut_ptr = emit_expr(sess, ctx, scrutinee)?;
    let merge = new_block(sess, ctx.0, "match_merge");
    let continuation: MatchCont<'ctx> = (result_key, result_alloca, merge);
    let mut arm_blocks: Vec<BasicBlockId<'ctx>> = Vec::new();
    let mut b_idx = 0i64;
    while b_idx < count {
        arm_blocks.push(new_block(sess, ctx.0, "arm"));
        b_idx += 1;
    }
    let first = match arm_blocks.first() {
        Some(block) => *block,
        None => return Err(builder_error(-1, 0, 0, "internal: match without arms")),
    };
    sess.2.build_unconditional_branch(first).map_err(builder_fail)?;
    let mut idx = 0i64;
    while idx < count {
        let arm_block = match arm_blocks.get(idx as usize) {
            Some(block) => *block,
            None => break,
        };
        let fail = match arm_blocks.get(idx as usize + 1) {
            Some(block) => *block,
            None => merge,
        };
        let arm = list_get(sess.6, arms, idx);
        let pat = node_a(sess.5, arm);
        let body = node_b(sess.5, arm);
        sess.2.position_at_end(arm_block);
        let saved = ctx.1.len();
        let scrut: MatchScrut<'ctx> = (s_key, scrut_ptr, fail);
        emit_pattern(sess, ctx, pat, scrut, body, continuation)?;
        ctx.1.truncate(saved);
        idx += 1;
    }
    sess.2.position_at_end(merge);
    Ok(result_alloca)
}

/// Emits the arm body, copies its value into the match result, and
/// branches to the merge block (unless the body diverged).
fn emit_arm_body<'ctx, 'a>(
    sess: &mut Session<'ctx, '_, '_>,
    ctx: &mut FnCtx<'ctx, 'a>,
    body: i64,
    continuation: MatchCont<'ctx>,
) -> Result<(), CodegenError> {
    let (result_key, result_alloca, merge) = continuation;
    // An arm body is a single statement stored directly in the arm row.
    let (val_ptr, diverged) = emit_stmt(sess, ctx, body)?;
    if !diverged {
        copy_value(sess, result_key, result_alloca, val_ptr)?;
        sess.2.build_unconditional_branch(merge).map_err(builder_fail)?;
    }
    Ok(())
}

/// Emits the tests for `pat`, binding names along the way.  Every test
/// branches to `fail_block` on mismatch; the pattern's body is emitted in
/// the current block when the whole pattern matched (`body` is NONE when
/// this pattern is a nested step of a larger pattern).
fn emit_pattern<'ctx, 'a>(
    sess: &mut Session<'ctx, '_, '_>,
    ctx: &mut FnCtx<'ctx, 'a>,
    pat: i64,
    scrut: MatchScrut<'ctx>,
    body: i64,
    continuation: MatchCont<'ctx>,
) -> Result<(), CodegenError> {
    let (pat_key, scrut_ptr, fail_block) = scrut;
    let kind = node_a(sess.5, pat);
    if kind == PAT_BIND {
        let name = node_b(sess.5, pat);
        let key = sub_key(sess, ctx.3, ctx.4, pat_key);
        let ptr = declare_local(sess, key, &em_name(sess, name))?;
        copy_value(sess, key, ptr, scrut_ptr)?;
        bind_local(&mut ctx.1, name, key, ptr);
        if body != NONE {
            return emit_arm_body(sess, ctx, body, continuation);
        }
        return Ok(());
    }
    if kind == PAT_LIT {
        let lit_key = sub_key(sess, ctx.3, ctx.4, pat_ty_of(sess.5, pat));
        let lit_value = node_c(sess.5, pat);
        let cv = const_int_of(sess, lit_key, lit_value)?;
        let sv = load_key(sess, pat_key, scrut_ptr)?.into_int_value();
        let cmp = sess.2.build_int_compare(IntPredicate::EQ, sv, cv.into_int_value(), "").map_err(builder_fail)?;
        let cont = new_block(sess, ctx.0, "pat");
        sess.2.build_conditional_branch(cmp, cont, fail_block).map_err(builder_fail)?;
        sess.2.position_at_end(cont);
        if body != NONE {
            return emit_arm_body(sess, ctx, body, continuation);
        }
        return Ok(());
    }
    if kind == PAT_PATH {
        let sym = pat_sym_of(sess.5, pat);
        let idx = variant_index_of(sess, pat_key, sym)?;
        let cont = new_block(sess, ctx.0, "pat");
        let tag_ptr = struct_gep(sess, pat_key, scrut_ptr, 0, "")?;
        let tag = load_i64(sess, tag_ptr)?;
        let want = sess.0.i64_type().const_int(idx as u64, false);
        let cmp = sess.2.build_int_compare(IntPredicate::EQ, tag, want, "").map_err(builder_fail)?;
        sess.2.build_conditional_branch(cmp, cont, fail_block).map_err(builder_fail)?;
        sess.2.position_at_end(cont);
        if body != NONE {
            return emit_arm_body(sess, ctx, body, continuation);
        }
        return Ok(());
    }
    if kind == PAT_VARIANT {
        let sym = pat_sym_of(sess.5, pat);
        let idx = variant_index_of(sess, pat_key, sym)?;
        let cont = new_block(sess, ctx.0, "pat");
        let tag_ptr = struct_gep(sess, pat_key, scrut_ptr, 0, "")?;
        let tag = load_i64(sess, tag_ptr)?;
        let want = sess.0.i64_type().const_int(idx as u64, false);
        let cmp = sess.2.build_int_compare(IntPredicate::EQ, tag, want, "").map_err(builder_fail)?;
        sess.2.build_conditional_branch(cmp, cont, fail_block).map_err(builder_fail)?;
        sess.2.position_at_end(cont);
        let (region, pty) = enum_payload_ptr(sess, scrut_ptr, pat_key, idx)?;
        let payload_pats = node_c(sess.5, pat);
        let pcount = list_len(sess.6, payload_pats);
        let mut i = 0i64;
        while i < pcount {
            let fkey = variant_payload_key(sess, pat_key, idx, i)?;
            let fptr = sess.2.build_struct_gep(pty, region, i as u32, "").map_err(builder_fail)?;
            let subpat = list_get(sess.6, payload_pats, i);
            let sub_body = if i + 1 == pcount { body } else { NONE };
            let sub_scrut: MatchScrut<'ctx> = (fkey, fptr, fail_block);
            emit_pattern(sess, ctx, subpat, sub_scrut, sub_body, continuation)?;
            i += 1;
        }
        if pcount == 0 && body != NONE {
            return emit_arm_body(sess, ctx, body, continuation);
        }
        return Ok(());
    }
    let inner_key = deref_key_of(sess, pat_key);
    let inner_kind = em_key_kind(sess, inner_key);
    let len_val = if inner_kind == TYD_SLICE {
        slice_len_of(sess, scrut_ptr)?
    } else {
        sess.0.i64_type().const_int(key_len_of(sess, inner_key) as u64, false)
    };
    let elems = node_b(sess.5, pat);
    let fixed = list_len(sess.6, elems);
    let rest = node_c(sess.5, pat);
    let cont = new_block(sess, ctx.0, "pat");
    if rest == NONE {
        let want = sess.0.i64_type().const_int(fixed as u64, false);
        let cmp = sess.2.build_int_compare(IntPredicate::EQ, len_val, want, "").map_err(builder_fail)?;
        sess.2.build_conditional_branch(cmp, cont, fail_block).map_err(builder_fail)?;
    } else {
        let want = sess.0.i64_type().const_int(fixed as u64, false);
        let cmp = sess.2.build_int_compare(IntPredicate::SGE, len_val, want, "").map_err(builder_fail)?;
        sess.2.build_conditional_branch(cmp, cont, fail_block).map_err(builder_fail)?;
    }
    sess.2.position_at_end(cont);
    let elem_key = em_key_elem(sess, inner_key);
    let data = if inner_kind == TYD_SLICE {
        slice_data(sess, scrut_ptr)?
    } else {
        scrut_ptr
    };
    let mut i = 0i64;
    while i < fixed {
        let eptr = offset_elem_ptr(sess, elem_key, data, sess.0.i64_type().const_int(i as u64, false))?;
        let subpat = list_get(sess.6, elems, i);
        let sub_scrut: MatchScrut<'ctx> = (elem_key, eptr, fail_block);
        emit_pattern(sess, ctx, subpat, sub_scrut, NONE, continuation)?;
        i += 1;
    }
    if rest != NONE {
        let rest_key = sub_key(sess, ctx.3, ctx.4, pat_rest_key_of(sess.5, pat));
        let rptr = declare_local(sess, rest_key, "rest")?;
        let rdata = slice_gep(sess, rptr, 0, "")?;
        let rest_base = offset_elem_ptr(sess, elem_key, data, sess.0.i64_type().const_int(fixed as u64, false))?;
        store_key(sess, rdata, rest_base.into())?;
        let rlen = slice_gep(sess, rptr, 1, "")?;
        let sub = sess.2.build_int_sub(len_val, sess.0.i64_type().const_int(fixed as u64, false), "").map_err(builder_fail)?;
        store_key(sess, rlen, sub.into())?;
        bind_local(&mut ctx.1, rest, rest_key, rptr);
    }
    if body != NONE {
        return emit_arm_body(sess, ctx, body, continuation);
    }
    Ok(())
}

fn emit_try<'ctx, 'a>(
    sess: &mut Session<'ctx, '_, '_>,
    ctx: &mut FnCtx<'ctx, 'a>,
    expr: i64,
) -> Result<PointerValue<'ctx>, CodegenError> {
    let inner = node_b(sess.5, expr);
    let inner_key = sub_key(sess, ctx.3, ctx.4, em_expr_ty(sess, inner));
    let result_key = sub_key(sess, ctx.3, ctx.4, em_expr_ty(sess, expr));
    let ret_key = sub_key(sess, ctx.3, ctx.4, ctx.5);
    let inner_ptr = emit_expr(sess, ctx, inner)?;
    let ok_name;
    let err_name;
    if enum_name_is(sess, inner_key, "Result") {
        ok_name = "Ok";
        err_name = "Err";
    } else {
        ok_name = "Some";
        err_name = "None";
    }
    let ok_tag = variant_index_by_name(sess, inner_key, ok_name)?;
    let err_tag = variant_index_by_name(sess, inner_key, err_name)?;
    let ret_err_tag = variant_index_by_name(sess, ret_key, err_name)?;
    let err_block = new_block(sess, ctx.0, "try_err");
    let ok_block = new_block(sess, ctx.0, "try_ok");
    let tag_ptr = struct_gep(sess, inner_key, inner_ptr, 0, "")?;
    let tag = load_i64(sess, tag_ptr)?;
    let want = sess.0.i64_type().const_int(err_tag as u64, false);
    let cmp = sess.2.build_int_compare(IntPredicate::EQ, tag, want, "").map_err(builder_fail)?;
    sess.2.build_conditional_branch(cmp, err_block, ok_block).map_err(builder_fail)?;
    sess.2.position_at_end(err_block);
    let ret_alloca = declare_local(sess, ret_key, "try_err")?;
    let rtag_ptr = struct_gep(sess, ret_key, ret_alloca, 0, "")?;
    let rtag = sess.0.i64_type().const_int(ret_err_tag as u64, false);
    store_key(sess, rtag_ptr, rtag.into())?;
    // Copy the error payload only when the error variant declares one:
    // `None` (an Option error) has no payload to move.
    if variant_payload_count(sess, inner_key, err_tag) > 0 {
        let (inner_region, inner_pty) = enum_payload_ptr(sess, inner_ptr, inner_key, err_tag)?;
        let inner_payload = sess.2.build_struct_gep(inner_pty, inner_region, 0, "").map_err(builder_fail)?;
        let err_payload_key = variant_payload_key(sess, inner_key, err_tag, 0)?;
        let (ret_region, ret_pty) = enum_payload_ptr(sess, ret_alloca, ret_key, ret_err_tag)?;
        let ret_payload = sess.2.build_struct_gep(ret_pty, ret_region, 0, "").map_err(builder_fail)?;
        copy_value(sess, err_payload_key, ret_payload, inner_payload)?;
    }
    let loaded = load_key(sess, ret_key, ret_alloca)?;
    sess.2.build_return(Some(&loaded)).map_err(builder_fail)?;
    sess.2.position_at_end(ok_block);
    let (ok_region, ok_pty) = enum_payload_ptr(sess, inner_ptr, inner_key, ok_tag)?;
    let ok_payload = sess.2.build_struct_gep(ok_pty, ok_region, 0, "").map_err(builder_fail)?;
    let out = declare_local(sess, result_key, "try_ok")?;
    copy_value(sess, result_key, out, ok_payload)?;
    Ok(out)
}

// ---------------------------------------------------------------------------
// Calls and function instances.
// ---------------------------------------------------------------------------

fn into_meta<'ctx>(value: BasicValueEnum<'ctx>) -> BasicMetadataValueEnum<'ctx> {
    value.into()
}

fn emit_call<'ctx, 'a>(
    sess: &mut Session<'ctx, '_, '_>,
    ctx: &mut FnCtx<'ctx, 'a>,
    expr: i64,
) -> Result<PointerValue<'ctx>, CodegenError> {
    let inst = em_expr_sym(sess, expr);
    if inst == NONE {
        let trow = find_trait_call(sess.5, expr);
        if trow != NONE && trait_call_inst(sess.5, trow) == NONE {
            return emit_deferred_trait_call(sess, ctx, expr, trow);
        }
        return Err(builder_error(
            node_file(sess.5, expr),
            node_start(sess.5, expr),
            node_end(sess.5, expr),
            "internal: call without an instance",
        ));
    }
    let fn_slot = inst_fn_of(sess.5, inst);
    if node_tag(sess.5, fn_slot) == NODE_SYM {
        return emit_native_call(sess, ctx, expr, inst, fn_slot);
    }
    let args_list = inst_args_of(sess.5, inst);
    let mono = inst_mono_of(sess.5, inst);
    let params_list = inst_params_of(sess.5, inst);
    let ret_key = sub_key(sess, ctx.3, ctx.4, inst_ret_of(sess.5, inst));
    let caller_block = sess.2.get_insert_block();
    let fn_val = get_or_emit_fn(sess, fn_slot, args_list, mono, params_list, ret_key)?;
    match caller_block {
        Some(block) => sess.2.position_at_end(block),
        None => return Err(builder_error(-1, 0, 0, "internal: no insertion block")),
    }
    let arg_vals = emit_call_args(sess, ctx, expr)?;
    let call = sess.2.build_call(fn_val, &arg_vals, "").map_err(builder_fail)?;
    let out = declare_local(sess, ret_key, "call")?;
    let ret_val = match call.try_as_basic_value() {
        ValueKind::Basic(bv) => bv,
        ValueKind::Instruction(inst) => {
            return Err(builder_error(
                -1,
                0,
                0,
                &format!("internal: void return from a call ({:?})", inst.get_opcode()),
            ));
        }
    };
    store_key(sess, out, ret_val)?;
    Ok(out)
}

/// Emits a deferred trait call: the receiver's concrete type is known
/// only after the enclosing instance's substitution, so the impl-method
/// instance is resolved here from the typechecker's impl table — the
/// same `(trait sym, for key, methods list)` triples the typechecker
/// used to verify the call.  Nothing is re-inferred: the impl table and
/// the attached type keys are the typechecker's facts.
fn emit_deferred_trait_call<'ctx, 'a>(
    sess: &mut Session<'ctx, '_, '_>,
    ctx: &mut FnCtx<'ctx, 'a>,
    expr: i64,
    trow: i64,
) -> Result<PointerValue<'ctx>, CodegenError> {
    let trait_sym = trait_call_trait(sess.5, trow);
    let method_name = trait_call_method(sess.5, trow);
    let arg_exprs = node_d(sess.5, expr);
    let receiver = list_get(sess.6, arg_exprs, 0);
    if receiver == NONE {
        return Err(builder_error(
            node_file(sess.5, expr),
            node_start(sess.5, expr),
            node_end(sess.5, expr),
            "internal: deferred trait call without a receiver",
        ));
    }
    let recv_key = sub_key(sess, ctx.3, ctx.4, expr_ty_of(sess.5, receiver));
    let for_key = deref_key_of(sess, recv_key);
    let impls = sess.11;
    let impl_count = list_len(sess.6, impls);
    let mut impl_idx = 0i64;
    let mut found_method = NONE;
    while impl_idx < impl_count {
        let tsym = list_get(sess.6, impls, impl_idx);
        let fkey = list_get(sess.6, impls, impl_idx + 1);
        if tsym == trait_sym && fkey == for_key {
            let methods = list_get(sess.6, impls, impl_idx + 2);
            let mcount = list_len(sess.6, methods);
            let mut midx = 0i64;
            while midx < mcount {
                let method = list_get(sess.6, methods, midx);
                if node_a(sess.5, method) == method_name {
                    found_method = method;
                }
                midx += 1;
            }
        }
        impl_idx += 3;
    }
    if found_method == NONE {
        return Err(builder_error(
            node_file(sess.5, expr),
            node_start(sess.5, expr),
            node_end(sess.5, expr),
            "internal: no impl method for a deferred trait call",
        ));
    }
    let fn_node = found_method;
    let params = node_c(sess.5, fn_node);
    let pcount = list_len(sess.6, params);
    let param_keys = alloc_list(sess.6);
    let mut pidx = 0i64;
    while pidx < pcount {
        let param = list_get(sess.6, params, pidx);
        list_push(sess.6, param_keys, ty_key_of(sess.5, node_b(sess.5, param)));
        pidx += 1;
    }
    let result = ty_key_of(sess.5, node_d(sess.5, fn_node));
    let mono = canon_tyinfo(sess.5, sess.6, TYD_MONO, fn_node, NONE, NONE, NONE);
    let caller_block = sess.2.get_insert_block();
    let fn_val = get_or_emit_fn(sess, fn_node, NONE, mono, param_keys, result)?;
    match caller_block {
        Some(block) => sess.2.position_at_end(block),
        None => return Err(builder_error(-1, 0, 0, "internal: no insertion block")),
    }
    let arg_vals = emit_call_args(sess, ctx, expr)?;
    let call = sess.2.build_call(fn_val, &arg_vals, "").map_err(builder_fail)?;
    let out = declare_local(sess, result, "call")?;
    let ret_val = match call.try_as_basic_value() {
        ValueKind::Basic(bv) => bv,
        ValueKind::Instruction(instr) => {
            return Err(builder_error(
                -1,
                0,
                0,
                &format!("internal: void return from a trait call ({:?})", instr.get_opcode()),
            ));
        }
    };
    store_key(sess, out, ret_val)?;
    Ok(out)
}

fn emit_call_args<'ctx, 'a>(
    sess: &mut Session<'ctx, '_, '_>,
    ctx: &mut FnCtx<'ctx, 'a>,
    expr: i64,
) -> Result<Vec<BasicMetadataValueEnum<'ctx>>, CodegenError> {
    let arg_exprs = node_d(sess.5, expr);
    let count = list_len(sess.6, arg_exprs);
    let mut vals: Vec<BasicMetadataValueEnum<'ctx>> = Vec::new();
    let mut idx = 0i64;
    while idx < count {
        let arg = list_get(sess.6, arg_exprs, idx);
        let akey = sub_key(sess, ctx.3, ctx.4, em_expr_ty(sess, arg));
        let ptr = emit_expr(sess, ctx, arg)?;
        let value = load_key(sess, akey, ptr)?;
        vals.push(into_meta(value));
        idx += 1;
    }
    Ok(vals)
}

fn find_inst_fn<'ctx>(sess: &Session<'ctx, '_, '_>, mono: i64) -> Option<FunctionValue<'ctx>> {
    let mut idx = sess.10.len();
    while idx > 0 {
        idx -= 1;
        if let Some(entry) = sess.10.get(idx)
            && entry.0 == mono {
                return Some(entry.1);
            }
    }
    None
}

fn build_fn_sig<'ctx>(sess: &mut Session<'ctx, '_, '_>, params_list: i64, ret_key: i64) -> Result<FunctionType<'ctx>, CodegenError> {
    let count = list_len(sess.6, params_list);
    let mut param_tys: Vec<BasicMetadataTypeEnum<'ctx>> = Vec::new();
    let mut idx = 0i64;
    while idx < count {
        let pkey = list_get(sess.6, params_list, idx);
        let ty = llvm_of(sess, pkey)?;
        param_tys.push(ty.into());
        idx += 1;
    }
    let ret_ty = llvm_of(sess, ret_key)?;
    Ok(ret_ty.fn_type(&param_tys, false))
}

/// Returns the instance's LLVM function, emitting its body the first
/// time.  The function is inserted into the instance cache before its
/// body is emitted so recursive calls resolve.
fn get_or_emit_fn<'ctx>(
    sess: &mut Session<'ctx, '_, '_>,
    fn_slot: i64,
    args_list: i64,
    mono: i64,
    params_list: i64,
    ret_key: i64,
) -> Result<FunctionValue<'ctx>, CodegenError> {
    if let Some(existing) = find_inst_fn(sess, mono) {
        return Ok(existing);
    }
    let fn_name = em_name(sess, node_a(sess.5, fn_slot));
    let llvm_name = format!("{}_{}", fn_name, mono);
    let sig = build_fn_sig(sess, params_list, ret_key)?;
    let fn_val = sess.1.add_function(&llvm_name, sig, None);
    sess.10.push((mono, fn_val));
    let from = fn_declared_param_keys(sess, fn_slot);
    let to = list_to_vec_of(sess, args_list);
    let param_decls = node_c(sess.5, fn_slot);
    let body = node_f(sess.5, fn_slot);
    let entry = sess.0.append_basic_block(fn_val, "entry");
    sess.2.position_at_end(entry);
    let mut body_locals: Locals<'ctx> = Vec::new();
    let fn_loops: LoopTargets<'ctx> = Vec::new();
    let param_values = fn_val.get_params();
    let pcount = list_len(sess.6, params_list);
    let mut idx = 0i64;
    while idx < pcount {
        let pkey = list_get(sess.6, params_list, idx);
        let pname = node_a(sess.5, list_get(sess.6, param_decls, idx));
        let ptr = declare_local(sess, pkey, &em_name(sess, pname))?;
        let pval = match param_values.get(idx as usize) {
            Some(value) => *value,
            None => break,
        };
        store_key(sess, ptr, pval)?;
        bind_local(&mut body_locals, pname, pkey, ptr);
        idx += 1;
    }
    let mut ctx: FnCtx<'ctx, '_> = (fn_val, body_locals, fn_loops, from.as_slice(), to.as_slice(), ret_key);
    let body_result = emit_stmt_list(sess, &mut ctx, body);
    if let Err(err) = body_result {
        return Err(with_fn_span(err, fn_slot, sess));
    }
    if !block_terminated(sess) {
        let ret_kind = em_key_kind(sess, ret_key);
        if ret_kind == TYD_ENUM {
            // A Unit-like fall-off (or a dead merge block after
            // all-arms-returning matches on an enum): store tag 0.
            let slot = declare_local(sess, ret_key, "fall")?;
            build_unit_value_into(sess, ret_key, slot)?;
            let loaded = load_key(sess, ret_key, slot)?;
            sess.2.build_return(Some(&loaded)).map_err(builder_fail)?;
        } else {
            // Scalar/other returns can only fall off when every path
            // already returned (an exhaustive match whose arms all
            // return); the block is unreachable.
            sess.2.build_unreachable().map_err(builder_fail)?;
        }
    }
    Ok(fn_val)
}

// ---------------------------------------------------------------------------
// Native surfaces.  Each native's signature is read from the instance the
// typechecker built; the body is the runtime ABI keyed by the native's
// qualified name.  Every memory access is bounds-checked and every size
// comes from the target data layout.
// ---------------------------------------------------------------------------

/// Declares (or reuses) the runtime helper `name`.  Every native body
/// may call the same libc function, and a second `add_function` would
/// make LLVM mangle the duplicate (`malloc.1`, `free.2`, …), leaving
/// the linker with undefined references; the first declaration is
/// shared by all callers.
fn extern_fn<'ctx>(sess: &mut Session<'ctx, '_, '_>, name: &str, sig: FunctionType<'ctx>) -> FunctionValue<'ctx> {
    match sess.1.get_function(name) {
        Some(existing) => existing,
        None => sess.1.add_function(name, sig, None),
    }
}

fn extern_malloc<'ctx>(sess: &mut Session<'ctx, '_, '_>) -> FunctionValue<'ctx> {
    let i8p = ptr_ty(sess);
    extern_fn(sess, "malloc", i8p.fn_type(&[sess.0.i64_type().into()], false))
}

fn extern_free<'ctx>(sess: &mut Session<'ctx, '_, '_>) -> FunctionValue<'ctx> {
    let i8p = ptr_ty(sess);
    extern_fn(sess, "free", sess.0.void_type().fn_type(&[i8p.into()], false))
}

fn extern_realloc<'ctx>(sess: &mut Session<'ctx, '_, '_>) -> FunctionValue<'ctx> {
    let i8p = ptr_ty(sess);
    extern_fn(
        sess,
        "realloc",
        i8p.fn_type(&[i8p.into(), sess.0.i64_type().into()], false),
    )
}

fn extern_memcmp<'ctx>(sess: &mut Session<'ctx, '_, '_>) -> FunctionValue<'ctx> {
    let i8p = ptr_ty(sess);
    extern_fn(
        sess,
        "memcmp",
        sess.0.i32_type().fn_type(&[i8p.into(), i8p.into(), sess.0.i64_type().into()], false),
    )
}

fn extern_write<'ctx>(sess: &mut Session<'ctx, '_, '_>) -> FunctionValue<'ctx> {
    let i8p = ptr_ty(sess);
    extern_fn(
        sess,
        "write",
        sess.0
            .i64_type()
            .fn_type(&[sess.0.i32_type().into(), i8p.into(), sess.0.i64_type().into()], false),
    )
}

fn emit_native_call<'ctx, 'a>(
    sess: &mut Session<'ctx, '_, '_>,
    ctx: &mut FnCtx<'ctx, 'a>,
    expr: i64,
    inst: i64,
    sym: i64,
) -> Result<PointerValue<'ctx>, CodegenError> {
    let params_list = inst_params_of(sess.5, inst);
    let ret_key = sub_key(sess, ctx.3, ctx.4, inst_ret_of(sess.5, inst));
    let mono = inst_mono_of(sess.5, inst);
    let name = em_name(sess, node_b(sess.5, sym));
    let native_val = match find_inst_fn(sess, mono) {
        Some(existing) => existing,
        None => {
            let caller_block = match sess.2.get_insert_block() {
                Some(block) => block,
                None => return Err(builder_error(-1, 0, 0, "internal: no insertion block")),
            };
            let sig = build_fn_sig(sess, params_list, ret_key)?;
            let llvm_name = format!("{}_{}", name.replace('.', "_"), mono);
            let fn_val = sess.1.add_function(&llvm_name, sig, None);
            sess.10.push((mono, fn_val));
            emit_native_body(sess, &name, params_list, ret_key, fn_val)?;
            sess.2.position_at_end(caller_block);
            fn_val
        }
    };
    let arg_vals = emit_call_args(sess, ctx, expr)?;
    let call = sess.2.build_call(native_val, &arg_vals, "").map_err(builder_fail)?;
    let out = declare_local(sess, ret_key, "call")?;
    let ret_val = match call.try_as_basic_value() {
        ValueKind::Basic(bv) => bv,
        ValueKind::Instruction(inst) => {
            return Err(builder_error(
                -1,
                0,
                0,
                &format!("internal: void return from a native call ({:?})", inst.get_opcode()),
            ));
        }
    };
    store_key(sess, out, ret_val)?;
    Ok(out)
}

/// Emits the runtime body of one native into `fn_val`.  The argument
/// values are bound by position into fresh allocas; the body is the
/// native's ABI, keyed by the qualified name.
fn emit_native_body<'ctx>(
    sess: &mut Session<'ctx, '_, '_>,
    name: &str,
    params_list: i64,
    ret_key: i64,
    fn_val: FunctionValue<'ctx>,
) -> Result<(), CodegenError> {
    let entry = sess.0.append_basic_block(fn_val, "entry");
    sess.2.position_at_end(entry);
    let mut body_locals: Locals<'ctx> = Vec::new();
    let param_values = fn_val.get_params();
    let pcount = list_len(sess.6, params_list);
    let mut idx = 0i64;
    while idx < pcount {
        let pkey = list_get(sess.6, params_list, idx);
        let ptr = declare_local(sess, pkey, "p")?;
        let pval = match param_values.get(idx as usize) {
            Some(value) => *value,
            None => break,
        };
        store_key(sess, ptr, pval)?;
        bind_local(&mut body_locals, idx, pkey, ptr);
        idx += 1;
    }
    let out = dispatch_native(sess, &mut body_locals, fn_val, name, params_list, ret_key)?;
    let loaded = load_key(sess, ret_key, out)?;
    sess.2.build_return(Some(&loaded)).map_err(builder_fail)?;
    Ok(())
}

fn native_arg_key(sess: &Session, params_list: i64, idx: i64) -> i64 {
    list_get(sess.6, params_list, idx)
}

fn result_ok_tag(sess: &Session, key: i64) -> Result<i64, CodegenError> {
    variant_index_by_name(sess, key, "Ok")
}

fn result_err_tag(sess: &Session, key: i64) -> Result<i64, CodegenError> {
    variant_index_by_name(sess, key, "Err")
}

fn build_result_ok<'ctx>(sess: &mut Session<'ctx, '_, '_>, result_key: i64, payload_key: i64, payload_ptr: PointerValue<'ctx>) -> Result<PointerValue<'ctx>, CodegenError> {
    let ok_tag = result_ok_tag(sess, result_key)?;
    build_enum_value(sess, result_key, ok_tag, &[(payload_key, payload_ptr)])
}

fn build_result_err<'ctx>(sess: &mut Session<'ctx, '_, '_>, result_key: i64, payload_key: i64, payload_ptr: PointerValue<'ctx>) -> Result<PointerValue<'ctx>, CodegenError> {
    let err_tag = result_err_tag(sess, result_key)?;
    build_enum_value(sess, result_key, err_tag, &[(payload_key, payload_ptr)])
}

fn build_unit_value<'ctx>(sess: &mut Session<'ctx, '_, '_>, unit_key: i64) -> Result<PointerValue<'ctx>, CodegenError> {
    let ptr = declare_local(sess, unit_key, "unit")?;
    build_unit_value_into(sess, unit_key, ptr)?;
    Ok(ptr)
}

fn result_arg_key(sess: &Session, result_key: i64, idx: i64) -> i64 {
    list_get(sess.6, key_args_of(sess, result_key), idx)
}

fn is_null_ptr<'ctx>(sess: &mut Session<'ctx, '_, '_>, ptr: PointerValue<'ctx>) -> Result<IntValue<'ctx>, CodegenError> {
    let i = sess.2.build_ptr_to_int(ptr, sess.0.i64_type(), "").map_err(builder_fail)?;
    let zero = sess.0.i64_type().const_zero();
    sess.2.build_int_compare(IntPredicate::EQ, i, zero, "").map_err(builder_fail)
}

fn byte_offset<'ctx>(sess: &mut Session<'ctx, '_, '_>, base: PointerValue<'ctx>, offset: IntValue<'ctx>) -> Result<PointerValue<'ctx>, CodegenError> {
    let i8_ty = sess.0.i8_type();
    let cast = sess.2.build_pointer_cast(base, ptr_ty(sess), "").map_err(builder_fail)?;
    let gep = unsafe { sess.2.build_gep(i8_ty, cast, &[offset], "") }.map_err(builder_fail)?;
    Ok(gep)
}

fn copy_to_out<'ctx>(sess: &mut Session<'ctx, '_, '_>, key: i64, out: PointerValue<'ctx>, src: PointerValue<'ctx>) -> Result<(), CodegenError> {
    copy_value(sess, key, out, src)
}

fn dispatch_native<'ctx>(
    sess: &mut Session<'ctx, '_, '_>,
    locals: &mut Locals<'ctx>,
    f: FunctionValue<'ctx>,
    name: &str,
    params_list: i64,
    ret_key: i64,
) -> Result<PointerValue<'ctx>, CodegenError> {
    let out = declare_local(sess, ret_key, "ret")?;
    if name == "U8.from_u8" || name == "U32.from_u8" || name == "Int.from_u8" || name == "Usize.from_u8" {
        native_from_u8(sess, locals, ret_key, out)?;
        return Ok(out);
    }
    if name == "Slice.len" {
        native_slice_len(sess, locals, out)?;
        return Ok(out);
    }
    if name == "Memory.allocate" {
        return native_allocate(sess, f, locals, ret_key, out);
    }
    if name == "Memory.deallocate" {
        native_deallocate(sess, locals, ret_key, out)?;
        return Ok(out);
    }
    if name == "Memory.write_u8" {
        native_write_u8(sess, f, locals, ret_key, out)?;
        return Ok(out);
    }
    if name == "Memory.read_u8" {
        native_read_u8(sess, f, locals, ret_key, out)?;
        return Ok(out);
    }
    if name == "Collections.vec_new" {
        native_vec_new(sess, ret_key, out)?;
        return Ok(out);
    }
    if name == "Collections.vec_push" {
        return native_vec_push(sess, f, locals, params_list, ret_key, out);
    }
    if name == "Collections.vec_view" {
        native_vec_view(sess, locals, out)?;
        return Ok(out);
    }
    if name == "Collections.vec_free" {
        native_vec_free(sess, locals, ret_key, out)?;
        return Ok(out);
    }
    if name == "Collections.string_from_slice" {
        return native_string_from_slice(sess, f, locals, ret_key, out);
    }
    if name == "Collections.string_len" {
        native_string_len(sess, locals, out)?;
        return Ok(out);
    }
    if name == "Collections.string_free" {
        native_string_free(sess, locals, ret_key, out)?;
        return Ok(out);
    }
    if name == "Collections.hash_map_new" {
        native_hash_map_new(sess, ret_key, out)?;
        return Ok(out);
    }
    if name == "Collections.hash_map_insert" {
        return native_hash_map_insert(sess, f, locals, params_list, ret_key, out);
    }
    if name == "Collections.hash_map_get" {
        return native_hash_map_get(sess, f, locals, params_list, ret_key, out);
    }
    if name == "Collections.hash_map_free" {
        native_hash_map_free(sess, locals, ret_key, out)?;
        return Ok(out);
    }
    if name == "Runtime.self_check" {
        native_self_check(sess, ret_key, out)?;
        return Ok(out);
    }
    if name == "Terminal.print" || name == "Terminal.print_line" || name == "Terminal.eprint" {
        let stderr = name == "Terminal.eprint";
        let newline = name == "Terminal.print_line";
        native_print(sess, locals, ret_key, out, stderr, newline)?;
        return Ok(out);
    }
    Err(builder_error(-1, 0, 0, &format!("native '{}' has no runtime body", name)))
}

fn native_from_u8<'ctx>(sess: &mut Session<'ctx, '_, '_>, locals: &Locals<'ctx>, ret_key: i64, out: PointerValue<'ctx>) -> Result<(), CodegenError> {
    let p0 = get_local(locals, 0)?;
    let k0 = get_local_key(locals, 0)?;
    let v = load_key(sess, k0, p0)?.into_int_value();
    let ret_ty = llvm_of(sess, ret_key)?.into_int_type();
    let src_ty = v.get_type();
    // U8 is unsigned: widening must zero-extend, never sign-extend
    // (0xF0 is 240, not -16).  A raw int-cast sign-extends from i8.
    let r = if src_ty.get_bit_width() < ret_ty.get_bit_width() {
        sess.2.build_int_z_extend(v, ret_ty, "").map_err(builder_fail)?
    } else if src_ty.get_bit_width() > ret_ty.get_bit_width() {
        sess.2.build_int_truncate(v, ret_ty, "").map_err(builder_fail)?
    } else {
        v
    };
    store_key(sess, out, r.into())?;
    Ok(())
}

fn native_slice_len<'ctx>(sess: &mut Session<'ctx, '_, '_>, locals: &Locals<'ctx>, out: PointerValue<'ctx>) -> Result<(), CodegenError> {
    let p0 = get_local(locals, 0)?;
    let len = slice_len_of(sess, p0)?;
    store_key(sess, out, len.into())?;
    Ok(())
}

fn native_allocate<'ctx>(sess: &mut Session<'ctx, '_, '_>, f: FunctionValue<'ctx>, locals: &Locals<'ctx>, ret_key: i64, out: PointerValue<'ctx>) -> Result<PointerValue<'ctx>, CodegenError> {
    let p0 = get_local(locals, 0)?;
    let k0 = get_local_key(locals, 0)?;
    let size = load_key(sess, k0, p0)?.into_int_value();
    let malloc = extern_malloc(sess);
    let call = sess.2.build_call(malloc, &[into_meta(size.into())], "").map_err(builder_fail)?;
    let data = match call.try_as_basic_value() {
        ValueKind::Basic(bv) => bv.into_pointer_value(),
        ValueKind::Instruction(inst) => {
            return Err(builder_error(-1, 0, 0, &format!("internal: malloc returned void ({:?})", inst.get_opcode())));
        }
    };
    let null_cmp = is_null_ptr(sess, data)?;
    let fail_block = new_block(sess, f, "alloc_fail");
    let ok_block = new_block(sess, f, "alloc_ok");
    let after = new_block(sess, f, "alloc_after");
    sess.2.build_conditional_branch(null_cmp, fail_block, ok_block).map_err(builder_fail)?;
    sess.2.position_at_end(fail_block);
    let err_key = result_arg_key(sess, ret_key, 1);
    let alloc_fail_tag = variant_index_by_name(sess, err_key, "AllocationFailed")?;
    let fkey = variant_payload_key(sess, err_key, alloc_fail_tag, 0)?;
    let fail_val = build_enum_value(sess, err_key, alloc_fail_tag, &[(fkey, p0)])?;
    let err_result = build_result_err(sess, ret_key, err_key, fail_val)?;
    copy_to_out(sess, ret_key, out, err_result)?;
    sess.2.build_unconditional_branch(after).map_err(builder_fail)?;
    sess.2.position_at_end(ok_block);
    let block_key = result_arg_key(sess, ret_key, 0);
    let block_val = declare_local(sess, block_key, "block")?;
    let bd = struct_gep(sess, block_key, block_val, 0, "")?;
    store_key(sess, bd, data.into())?;
    let bl = struct_gep(sess, block_key, block_val, 1, "")?;
    store_key(sess, bl, size.into())?;
    let ok_result = build_result_ok(sess, ret_key, block_key, block_val)?;
    copy_to_out(sess, ret_key, out, ok_result)?;
    sess.2.build_unconditional_branch(after).map_err(builder_fail)?;
    sess.2.position_at_end(after);
    Ok(out)
}

fn native_deallocate<'ctx>(sess: &mut Session<'ctx, '_, '_>, locals: &Locals<'ctx>, ret_key: i64, out: PointerValue<'ctx>) -> Result<(), CodegenError> {
    let p0 = get_local(locals, 0)?;
    let block_key = deref_key_of(sess, get_local_key(locals, 0)?);
    let bd = struct_gep(sess, block_key, p0, 0, "")?;
    let data = load_ptr(sess, bd)?;
    let free = extern_free(sess);
    sess.2.build_call(free, &[into_meta(data.into())], "").map_err(builder_fail)?;
    build_unit_value_into(sess, ret_key, out)?;
    Ok(())
}

fn native_write_u8<'ctx>(sess: &mut Session<'ctx, '_, '_>, f: FunctionValue<'ctx>, locals: &Locals<'ctx>, ret_key: i64, out: PointerValue<'ctx>) -> Result<(), CodegenError> {
    let p0 = get_local(locals, 0)?;
    let p1 = get_local(locals, 1)?;
    let p2 = get_local(locals, 2)?;
    let block_key = deref_key_of(sess, get_local_key(locals, 0)?);
    let block_ref = load_ptr(sess, p0)?;
    let bd = struct_gep(sess, block_key, block_ref, 0, "")?;
    let data = load_ptr(sess, bd)?;
    let bl = struct_gep(sess, block_key, block_ref, 1, "")?;
    let len = load_i64(sess, bl)?;
    let offset = load_i64(sess, p1)?;
    let value = load_i8(sess, p2)?;
    let ok_cmp = sess.2.build_int_compare(IntPredicate::ULT, offset, len, "").map_err(builder_fail)?;
    let fail_block = new_block(sess, f, "w_fail");
    let ok_block = new_block(sess, f, "w_ok");
    let after = new_block(sess, f, "w_after");
    sess.2.build_conditional_branch(ok_cmp, ok_block, fail_block).map_err(builder_fail)?;
    sess.2.position_at_end(fail_block);
    let err_key = result_arg_key(sess, ret_key, 1);
    let oob_tag = variant_index_by_name(sess, err_key, "AccessOutOfBounds")?;
    let f0 = variant_payload_key(sess, err_key, oob_tag, 0)?;
    let f1 = variant_payload_key(sess, err_key, oob_tag, 1)?;
    let e0 = declare_local(sess, f0, "o0")?;
    copy_value(sess, f0, e0, p1)?;
    let e1 = declare_local(sess, f1, "o1")?;
    store_key(sess, e1, len.into())?;
    let fail_val = build_enum_value(sess, err_key, oob_tag, &[(f0, e0), (f1, e1)])?;
    let err_result = build_result_err(sess, ret_key, err_key, fail_val)?;
    copy_to_out(sess, ret_key, out, err_result)?;
    sess.2.build_unconditional_branch(after).map_err(builder_fail)?;
    sess.2.position_at_end(ok_block);
    let target = byte_offset(sess, data, offset)?;
    store_key(sess, target, value.into())?;
    let unit_key = result_arg_key(sess, ret_key, 0);
    let unit_val = build_unit_value(sess, unit_key)?;
    let ok_result = build_result_ok(sess, ret_key, unit_key, unit_val)?;
    copy_to_out(sess, ret_key, out, ok_result)?;
    sess.2.build_unconditional_branch(after).map_err(builder_fail)?;
    sess.2.position_at_end(after);
    Ok(())
}

fn native_read_u8<'ctx>(sess: &mut Session<'ctx, '_, '_>, f: FunctionValue<'ctx>, locals: &Locals<'ctx>, ret_key: i64, out: PointerValue<'ctx>) -> Result<(), CodegenError> {
    let p0 = get_local(locals, 0)?;
    let p1 = get_local(locals, 1)?;
    let block_key = deref_key_of(sess, get_local_key(locals, 0)?);
    let block_ref = load_ptr(sess, p0)?;
    let bd = struct_gep(sess, block_key, block_ref, 0, "")?;
    let data = load_ptr(sess, bd)?;
    let bl = struct_gep(sess, block_key, block_ref, 1, "")?;
    let len = load_i64(sess, bl)?;
    let offset = load_i64(sess, p1)?;
    let ok_cmp = sess.2.build_int_compare(IntPredicate::ULT, offset, len, "").map_err(builder_fail)?;
    let fail_block = new_block(sess, f, "r_fail");
    let ok_block = new_block(sess, f, "r_ok");
    let after = new_block(sess, f, "r_after");
    sess.2.build_conditional_branch(ok_cmp, ok_block, fail_block).map_err(builder_fail)?;
    sess.2.position_at_end(fail_block);
    let err_key = result_arg_key(sess, ret_key, 1);
    let oob_tag = variant_index_by_name(sess, err_key, "AccessOutOfBounds")?;
    let f0 = variant_payload_key(sess, err_key, oob_tag, 0)?;
    let f1 = variant_payload_key(sess, err_key, oob_tag, 1)?;
    let e0 = declare_local(sess, f0, "o0")?;
    copy_value(sess, f0, e0, p1)?;
    let e1 = declare_local(sess, f1, "o1")?;
    store_key(sess, e1, len.into())?;
    let fail_val = build_enum_value(sess, err_key, oob_tag, &[(f0, e0), (f1, e1)])?;
    let err_result = build_result_err(sess, ret_key, err_key, fail_val)?;
    copy_to_out(sess, ret_key, out, err_result)?;
    sess.2.build_unconditional_branch(after).map_err(builder_fail)?;
    sess.2.position_at_end(ok_block);
    let target = byte_offset(sess, data, offset)?;
    let byte = load_i8(sess, target)?;
    let u8_key = result_arg_key(sess, ret_key, 0);
    let u8_val = declare_local(sess, u8_key, "byte")?;
    store_key(sess, u8_val, byte.into())?;
    let ok_result = build_result_ok(sess, ret_key, u8_key, u8_val)?;
    copy_to_out(sess, ret_key, out, ok_result)?;
    sess.2.build_unconditional_branch(after).map_err(builder_fail)?;
    sess.2.position_at_end(after);
    Ok(())
}

fn store_null_data<'ctx>(sess: &mut Session<'ctx, '_, '_>, key: i64, ptr: PointerValue<'ctx>) -> Result<(), CodegenError> {
    let null_ptr = ptr_ty(sess).const_null();
    let d = struct_gep(sess, key, ptr, 0, "")?;
    store_key(sess, d, null_ptr.into())?;
    let l = struct_gep(sess, key, ptr, 1, "")?;
    store_key(sess, l, sess.0.i64_type().const_zero().into())?;
    let c = struct_gep(sess, key, ptr, 2, "")?;
    store_key(sess, c, sess.0.i64_type().const_zero().into())?;
    Ok(())
}

fn native_vec_new<'ctx>(sess: &mut Session<'ctx, '_, '_>, ret_key: i64, out: PointerValue<'ctx>) -> Result<(), CodegenError> {
    let vec_key = result_arg_key(sess, ret_key, 0);
    let vec_val = declare_local(sess, vec_key, "vec")?;
    store_null_data(sess, vec_key, vec_val)?;
    let ok_result = build_result_ok(sess, ret_key, vec_key, vec_val)?;
    copy_to_out(sess, ret_key, out, ok_result)?;
    Ok(())
}

fn native_vec_push<'ctx>(sess: &mut Session<'ctx, '_, '_>, f: FunctionValue<'ctx>, locals: &Locals<'ctx>, params_list: i64, ret_key: i64, out: PointerValue<'ctx>) -> Result<PointerValue<'ctx>, CodegenError> {
    let p0 = get_local(locals, 0)?;
    let p1 = get_local(locals, 1)?;
    let t_key = native_arg_key(sess, params_list, 1);
    let vec_key = deref_key_of(sess, get_local_key(locals, 0)?);
    let vec_ref = load_ptr(sess, p0)?;
    let dptr = struct_gep(sess, vec_key, vec_ref, 0, "")?;
    let lptr = struct_gep(sess, vec_key, vec_ref, 1, "")?;
    let cptr = struct_gep(sess, vec_key, vec_ref, 2, "")?;
    let len = load_i64(sess, lptr)?;
    let cap = load_i64(sess, cptr)?;
    let need_grow = sess.2.build_int_compare(IntPredicate::EQ, len, cap, "").map_err(builder_fail)?;
    let grow_block = new_block(sess, f, "push_grow");
    let store_block = new_block(sess, f, "push_store");
    let fail_block = new_block(sess, f, "push_fail");
    let after = new_block(sess, f, "push_after");
    sess.2.build_conditional_branch(need_grow, grow_block, store_block).map_err(builder_fail)?;
    sess.2.position_at_end(grow_block);
    let old_data = load_ptr(sess, dptr)?;
    let zero = sess.0.i64_type().const_zero();
    let is_empty = sess.2.build_int_compare(IntPredicate::EQ, cap, zero, "").map_err(builder_fail)?;
    let four = sess.0.i64_type().const_int(4, false);
    let two = sess.0.i64_type().const_int(2, false);
    let doubled = sess.2.build_int_mul(cap, two, "").map_err(builder_fail)?;
    let newcap = sess.2.build_select(is_empty, four, doubled, "").map_err(builder_fail)?;
    let esize = sess.3.get_abi_size(&llvm_of(sess, t_key)?);
    let stride = sess.0.i64_type().const_int(esize, false);
    let needed = sess.2.build_int_mul(newcap.into_int_value(), stride, "").map_err(builder_fail)?;
    let realloc = extern_realloc(sess);
    let call = sess.2.build_call(realloc, &[into_meta(old_data.into()), into_meta(needed.into())], "").map_err(builder_fail)?;
    let new_data = match call.try_as_basic_value() {
        ValueKind::Basic(bv) => bv.into_pointer_value(),
        ValueKind::Instruction(inst) => {
            return Err(builder_error(-1, 0, 0, &format!("internal: realloc returned void ({:?})", inst.get_opcode())));
        }
    };
    let null_cmp = is_null_ptr(sess, new_data)?;
    let grow_ok = new_block(sess, f, "push_grow_ok");
    sess.2.build_conditional_branch(null_cmp, fail_block, grow_ok).map_err(builder_fail)?;
    sess.2.position_at_end(grow_ok);
    store_key(sess, dptr, new_data.into())?;
    store_key(sess, cptr, newcap)?;
    sess.2.build_unconditional_branch(store_block).map_err(builder_fail)?;
    sess.2.position_at_end(store_block);
    let data2 = load_ptr(sess, dptr)?;
    let len2 = load_i64(sess, lptr)?;
    let target = offset_elem_ptr(sess, t_key, data2, len2)?;
    copy_value(sess, t_key, target, p1)?;
    let one = sess.0.i64_type().const_int(1, false);
    let len3 = sess.2.build_int_add(len2, one, "").map_err(builder_fail)?;
    store_key(sess, lptr, len3.into())?;
    let unit_key = result_arg_key(sess, ret_key, 0);
    let unit_val = build_unit_value(sess, unit_key)?;
    let ok_result = build_result_ok(sess, ret_key, unit_key, unit_val)?;
    copy_to_out(sess, ret_key, out, ok_result)?;
    sess.2.build_unconditional_branch(after).map_err(builder_fail)?;
    sess.2.position_at_end(fail_block);
    let err_key = result_arg_key(sess, ret_key, 1);
    let alloc_fail_tag = variant_index_by_name(sess, err_key, "AllocationFailed")?;
    let fkey = variant_payload_key(sess, err_key, alloc_fail_tag, 0)?;
    let fval = declare_local(sess, fkey, "need")?;
    store_key(sess, fval, needed.into())?;
    let fail_val = build_enum_value(sess, err_key, alloc_fail_tag, &[(fkey, fval)])?;
    let err_result = build_result_err(sess, ret_key, err_key, fail_val)?;
    copy_to_out(sess, ret_key, out, err_result)?;
    sess.2.build_unconditional_branch(after).map_err(builder_fail)?;
    sess.2.position_at_end(after);
    Ok(out)
}

fn native_vec_view<'ctx>(sess: &mut Session<'ctx, '_, '_>, locals: &Locals<'ctx>, out: PointerValue<'ctx>) -> Result<(), CodegenError> {
    let p0 = get_local(locals, 0)?;
    let vec_key = deref_key_of(sess, get_local_key(locals, 0)?);
    let vec_ref = load_ptr(sess, p0)?;
    let d = struct_gep(sess, vec_key, vec_ref, 0, "")?;
    let data = load_ptr(sess, d)?;
    let l = struct_gep(sess, vec_key, vec_ref, 1, "")?;
    let len = load_i64(sess, l)?;
    let od = slice_gep(sess, out, 0, "")?;
    store_key(sess, od, data.into())?;
    let ol = slice_gep(sess, out, 1, "")?;
    store_key(sess, ol, len.into())?;
    Ok(())
}

fn native_vec_free<'ctx>(sess: &mut Session<'ctx, '_, '_>, locals: &Locals<'ctx>, ret_key: i64, out: PointerValue<'ctx>) -> Result<(), CodegenError> {
    let p0 = get_local(locals, 0)?;
    let vec_key = deref_key_of(sess, get_local_key(locals, 0)?);
    let d = struct_gep(sess, vec_key, p0, 0, "")?;
    let data = load_ptr(sess, d)?;
    let free = extern_free(sess);
    sess.2.build_call(free, &[into_meta(data.into())], "").map_err(builder_fail)?;
    build_unit_value_into(sess, ret_key, out)?;
    Ok(())
}

fn native_string_len<'ctx>(sess: &mut Session<'ctx, '_, '_>, locals: &Locals<'ctx>, out: PointerValue<'ctx>) -> Result<(), CodegenError> {
    let p0 = get_local(locals, 0)?;
    let str_key = deref_key_of(sess, get_local_key(locals, 0)?);
    let str_ref = load_ptr(sess, p0)?;
    let l = struct_gep(sess, str_key, str_ref, 1, "")?;
    let len = load_i64(sess, l)?;
    store_key(sess, out, len.into())?;
    Ok(())
}

/// The runtime body of the Terminal printing natives.  Writes the
/// string's bytes to fd 1 (stdout) or fd 2 (stderr), appending a
/// newline for `print_line`, then returns Unit.  The string's {data,
/// len} layout comes from the declared native type; the libc `write` is
/// declared once and shared by all three natives.
fn native_print<'ctx>(
    sess: &mut Session<'ctx, '_, '_>,
    locals: &Locals<'ctx>,
    ret_key: i64,
    out: PointerValue<'ctx>,
    stderr: bool,
    newline: bool,
) -> Result<(), CodegenError> {
    let p0 = get_local(locals, 0)?;
    let str_key = deref_key_of(sess, get_local_key(locals, 0)?);
    let str_ref = load_ptr(sess, p0)?;
    let data_ptr = struct_gep(sess, str_key, str_ref, 0, "")?;
    let data = load_ptr(sess, data_ptr)?;
    let len_ptr = struct_gep(sess, str_key, str_ref, 1, "")?;
    let len = load_i64(sess, len_ptr)?;
    let write = extern_write(sess);
    let fd = sess.0.i32_type().const_int(if stderr { 2 } else { 1 }, false);
    sess.2
        .build_call(write, &[into_meta(fd.into()), into_meta(data.into()), into_meta(len.into())], "")
        .map_err(builder_fail)?;
    if newline {
        let nl_slot = sess.2.build_alloca(sess.0.i8_type(), "nl").map_err(builder_fail)?;
        let nl = sess.0.i8_type().const_int(10, false);
        sess.2.build_store(nl_slot, nl).map_err(builder_fail)?;
        let one = sess.0.i64_type().const_int(1, false);
        sess.2
            .build_call(write, &[into_meta(fd.into()), into_meta(nl_slot.into()), into_meta(one.into())], "")
            .map_err(builder_fail)?;
    }
    build_unit_value_into(sess, ret_key, out)?;
    Ok(())
}

fn native_string_free<'ctx>(sess: &mut Session<'ctx, '_, '_>, locals: &Locals<'ctx>, ret_key: i64, out: PointerValue<'ctx>) -> Result<(), CodegenError> {
    let p0 = get_local(locals, 0)?;
    let str_key = deref_key_of(sess, get_local_key(locals, 0)?);
    let d = struct_gep(sess, str_key, p0, 0, "")?;
    let data = load_ptr(sess, d)?;
    let free = extern_free(sess);
    sess.2.build_call(free, &[into_meta(data.into())], "").map_err(builder_fail)?;
    build_unit_value_into(sess, ret_key, out)?;
    Ok(())
}

fn native_hash_map_new<'ctx>(sess: &mut Session<'ctx, '_, '_>, ret_key: i64, out: PointerValue<'ctx>) -> Result<(), CodegenError> {
    let map_key = result_arg_key(sess, ret_key, 0);
    let map_val = declare_local(sess, map_key, "map")?;
    store_null_data(sess, map_key, map_val)?;
    let ok_result = build_result_ok(sess, ret_key, map_key, map_val)?;
    copy_to_out(sess, ret_key, out, ok_result)?;
    Ok(())
}

fn native_hash_map_insert<'ctx>(sess: &mut Session<'ctx, '_, '_>, f: FunctionValue<'ctx>, locals: &Locals<'ctx>, params_list: i64, ret_key: i64, out: PointerValue<'ctx>) -> Result<PointerValue<'ctx>, CodegenError> {
    let p0 = get_local(locals, 0)?;
    let p1 = get_local(locals, 1)?;
    let p2 = get_local(locals, 2)?;
    let k_key = native_arg_key(sess, params_list, 1);
    let v_key = native_arg_key(sess, params_list, 2);
    let ksize = sess.3.get_abi_size(&llvm_of(sess, k_key)?);
    let vsize = sess.3.get_abi_size(&llvm_of(sess, v_key)?);
    let stride_const = sess.0.i64_type().const_int(ksize + vsize, false);
    let ksize_const = sess.0.i64_type().const_int(ksize, false);
    let map_key = deref_key_of(sess, get_local_key(locals, 0)?);
    let map_ref = load_ptr(sess, p0)?;
    let dptr = struct_gep(sess, map_key, map_ref, 0, "")?;
    let lptr = struct_gep(sess, map_key, map_ref, 1, "")?;
    let cptr = struct_gep(sess, map_key, map_ref, 2, "")?;
    let data = load_ptr(sess, dptr)?;
    let len = load_i64(sess, lptr)?;
    let key_base = sess.2.build_pointer_cast(p1, ptr_ty(sess), "").map_err(builder_fail)?;
    let i_slot = sess.2.build_alloca(sess.0.i64_type(), "i").map_err(builder_fail)?;
    store_key(sess, i_slot, sess.0.i64_type().const_zero().into())?;
    let scan_cond = new_block(sess, f, "map_cond");
    let scan_body = new_block(sess, f, "map_body");
    let found_block = new_block(sess, f, "map_found");
    let append_block = new_block(sess, f, "map_append");
    let after = new_block(sess, f, "map_after");
    sess.2.build_unconditional_branch(scan_cond).map_err(builder_fail)?;
    sess.2.position_at_end(scan_cond);
    let i = load_i64(sess, i_slot)?;
    let done = sess.2.build_int_compare(IntPredicate::ULT, i, len, "").map_err(builder_fail)?;
    let next_i = new_block(sess, f, "map_next");
    sess.2.build_conditional_branch(done, scan_body, append_block).map_err(builder_fail)?;
    sess.2.position_at_end(scan_body);
    let off = sess.2.build_int_mul(i, stride_const, "").map_err(builder_fail)?;
    let keyptr = byte_offset(sess, data, off)?;
    let memcmp = extern_memcmp(sess);
    let cmp_call = sess.2.build_call(
        memcmp,
        &[into_meta(keyptr.into()), into_meta(key_base.into()), into_meta(ksize_const.into())],
        "",
    ).map_err(builder_fail)?;
    let cmpv = match cmp_call.try_as_basic_value() {
        ValueKind::Basic(bv) => bv.into_int_value(),
        ValueKind::Instruction(inst) => {
            return Err(builder_error(-1, 0, 0, &format!("internal: memcmp returned void ({:?})", inst.get_opcode())));
        }
    };
    let eq = sess.2.build_int_compare(IntPredicate::EQ, cmpv, sess.0.i32_type().const_zero(), "").map_err(builder_fail)?;
    sess.2.build_conditional_branch(eq, found_block, next_i).map_err(builder_fail)?;
    sess.2.position_at_end(next_i);
    let one = sess.0.i64_type().const_int(1, false);
    let i2 = sess.2.build_int_add(i, one, "").map_err(builder_fail)?;
    store_key(sess, i_slot, i2.into())?;
    sess.2.build_unconditional_branch(scan_cond).map_err(builder_fail)?;
    sess.2.position_at_end(found_block);
    let voff = sess.2.build_int_add(off, ksize_const, "").map_err(builder_fail)?;
    let valueptr = byte_offset(sess, data, voff)?;
    copy_value(sess, v_key, valueptr, p2)?;
    let unit_key = result_arg_key(sess, ret_key, 0);
    let unit_val = build_unit_value(sess, unit_key)?;
    let ok_result = build_result_ok(sess, ret_key, unit_key, unit_val)?;
    copy_to_out(sess, ret_key, out, ok_result)?;
    sess.2.build_unconditional_branch(after).map_err(builder_fail)?;
    sess.2.position_at_end(append_block);
    let cap = load_i64(sess, cptr)?;
    let need_grow = sess.2.build_int_compare(IntPredicate::EQ, len, cap, "").map_err(builder_fail)?;
    let grow_block = new_block(sess, f, "map_grow");
    let do_append = new_block(sess, f, "map_do_append");
    let fail_block = new_block(sess, f, "map_fail");
    sess.2.build_conditional_branch(need_grow, grow_block, do_append).map_err(builder_fail)?;
    sess.2.position_at_end(grow_block);
    let old_data = load_ptr(sess, dptr)?;
    let zero = sess.0.i64_type().const_zero();
    let is_empty = sess.2.build_int_compare(IntPredicate::EQ, cap, zero, "").map_err(builder_fail)?;
    let four = sess.0.i64_type().const_int(4, false);
    let two = sess.0.i64_type().const_int(2, false);
    let doubled = sess.2.build_int_mul(cap, two, "").map_err(builder_fail)?;
    let newcap = sess.2.build_select(is_empty, four, doubled, "").map_err(builder_fail)?;
    let needed = sess.2.build_int_mul(newcap.into_int_value(), stride_const, "").map_err(builder_fail)?;
    let realloc = extern_realloc(sess);
    let call = sess.2.build_call(realloc, &[into_meta(old_data.into()), into_meta(needed.into())], "").map_err(builder_fail)?;
    let new_data = match call.try_as_basic_value() {
        ValueKind::Basic(bv) => bv.into_pointer_value(),
        ValueKind::Instruction(inst) => {
            return Err(builder_error(-1, 0, 0, &format!("internal: realloc returned void ({:?})", inst.get_opcode())));
        }
    };
    let null_cmp = is_null_ptr(sess, new_data)?;
    let grow_ok = new_block(sess, f, "map_grow_ok");
    sess.2.build_conditional_branch(null_cmp, fail_block, grow_ok).map_err(builder_fail)?;
    sess.2.position_at_end(grow_ok);
    store_key(sess, dptr, new_data.into())?;
    store_key(sess, cptr, newcap)?;
    sess.2.build_unconditional_branch(do_append).map_err(builder_fail)?;
    sess.2.position_at_end(do_append);
    let data2 = load_ptr(sess, dptr)?;
    let len2 = load_i64(sess, lptr)?;
    let entry_off = sess.2.build_int_mul(len2, stride_const, "").map_err(builder_fail)?;
    let keyptr2 = byte_offset(sess, data2, entry_off)?;
    copy_value(sess, k_key, keyptr2, p1)?;
    let voff2 = sess.2.build_int_add(entry_off, ksize_const, "").map_err(builder_fail)?;
    let valueptr2 = byte_offset(sess, data2, voff2)?;
    copy_value(sess, v_key, valueptr2, p2)?;
    let len3 = sess.2.build_int_add(len2, one, "").map_err(builder_fail)?;
    store_key(sess, lptr, len3.into())?;
    let unit_key2 = result_arg_key(sess, ret_key, 0);
    let unit_val2 = build_unit_value(sess, unit_key2)?;
    let ok_result2 = build_result_ok(sess, ret_key, unit_key2, unit_val2)?;
    copy_to_out(sess, ret_key, out, ok_result2)?;
    sess.2.build_unconditional_branch(after).map_err(builder_fail)?;
    sess.2.position_at_end(fail_block);
    let err_key = result_arg_key(sess, ret_key, 1);
    let alloc_fail_tag = variant_index_by_name(sess, err_key, "AllocationFailed")?;
    let fkey = variant_payload_key(sess, err_key, alloc_fail_tag, 0)?;
    let fval = declare_local(sess, fkey, "need")?;
    store_key(sess, fval, needed.into())?;
    let fail_val = build_enum_value(sess, err_key, alloc_fail_tag, &[(fkey, fval)])?;
    let err_result = build_result_err(sess, ret_key, err_key, fail_val)?;
    copy_to_out(sess, ret_key, out, err_result)?;
    sess.2.build_unconditional_branch(after).map_err(builder_fail)?;
    sess.2.position_at_end(after);
    Ok(out)
}

fn native_hash_map_get<'ctx>(sess: &mut Session<'ctx, '_, '_>, f: FunctionValue<'ctx>, locals: &Locals<'ctx>, params_list: i64, ret_key: i64, out: PointerValue<'ctx>) -> Result<PointerValue<'ctx>, CodegenError> {
    let p0 = get_local(locals, 0)?;
    let p1 = get_local(locals, 1)?;
    let k_key = native_arg_key(sess, params_list, 1);
    let v_key = result_arg_key(sess, ret_key, 0);
    let ksize = sess.3.get_abi_size(&llvm_of(sess, k_key)?);
    let vsize = sess.3.get_abi_size(&llvm_of(sess, v_key)?);
    let stride_const = sess.0.i64_type().const_int(ksize + vsize, false);
    let ksize_const = sess.0.i64_type().const_int(ksize, false);
    let map_key = deref_key_of(sess, get_local_key(locals, 0)?);
    let map_ref = load_ptr(sess, p0)?;
    let dptr = struct_gep(sess, map_key, map_ref, 0, "")?;
    let data = load_ptr(sess, dptr)?;
    let lptr = struct_gep(sess, map_key, map_ref, 1, "")?;
    let len = load_i64(sess, lptr)?;
    let key_base = sess.2.build_pointer_cast(p1, ptr_ty(sess), "").map_err(builder_fail)?;
    let i_slot = sess.2.build_alloca(sess.0.i64_type(), "i").map_err(builder_fail)?;
    store_key(sess, i_slot, sess.0.i64_type().const_zero().into())?;
    let scan_cond = new_block(sess, f, "g_cond");
    let scan_body = new_block(sess, f, "g_body");
    let found_block = new_block(sess, f, "g_found");
    let missing_block = new_block(sess, f, "g_missing");
    let after = new_block(sess, f, "g_after");
    sess.2.build_unconditional_branch(scan_cond).map_err(builder_fail)?;
    sess.2.position_at_end(scan_cond);
    let i = load_i64(sess, i_slot)?;
    let done = sess.2.build_int_compare(IntPredicate::ULT, i, len, "").map_err(builder_fail)?;
    let next_i = new_block(sess, f, "g_next");
    sess.2.build_conditional_branch(done, scan_body, missing_block).map_err(builder_fail)?;
    sess.2.position_at_end(scan_body);
    let off = sess.2.build_int_mul(i, stride_const, "").map_err(builder_fail)?;
    let keyptr = byte_offset(sess, data, off)?;
    let memcmp = extern_memcmp(sess);
    let cmp_call = sess.2.build_call(
        memcmp,
        &[into_meta(keyptr.into()), into_meta(key_base.into()), into_meta(ksize_const.into())],
        "",
    ).map_err(builder_fail)?;
    let cmpv = match cmp_call.try_as_basic_value() {
        ValueKind::Basic(bv) => bv.into_int_value(),
        ValueKind::Instruction(inst) => {
            return Err(builder_error(-1, 0, 0, &format!("internal: memcmp returned void ({:?})", inst.get_opcode())));
        }
    };
    let eq = sess.2.build_int_compare(IntPredicate::EQ, cmpv, sess.0.i32_type().const_zero(), "").map_err(builder_fail)?;
    sess.2.build_conditional_branch(eq, found_block, next_i).map_err(builder_fail)?;
    sess.2.position_at_end(next_i);
    let one = sess.0.i64_type().const_int(1, false);
    let i2 = sess.2.build_int_add(i, one, "").map_err(builder_fail)?;
    store_key(sess, i_slot, i2.into())?;
    sess.2.build_unconditional_branch(scan_cond).map_err(builder_fail)?;
    sess.2.position_at_end(found_block);
    let voff = sess.2.build_int_add(off, ksize_const, "").map_err(builder_fail)?;
    let valueptr = byte_offset(sess, data, voff)?;
    let v_val = declare_local(sess, v_key, "got")?;
    copy_value(sess, v_key, v_val, valueptr)?;
    let ok_result = build_result_ok(sess, ret_key, v_key, v_val)?;
    copy_to_out(sess, ret_key, out, ok_result)?;
    sess.2.build_unconditional_branch(after).map_err(builder_fail)?;
    sess.2.position_at_end(missing_block);
    let err_key = result_arg_key(sess, ret_key, 1);
    let key_missing_tag = variant_index_by_name(sess, err_key, "KeyNotFound")?;
    let fail_val = build_enum_value(sess, err_key, key_missing_tag, &[])?;
    let err_result = build_result_err(sess, ret_key, err_key, fail_val)?;
    copy_to_out(sess, ret_key, out, err_result)?;
    sess.2.build_unconditional_branch(after).map_err(builder_fail)?;
    sess.2.position_at_end(after);
    Ok(out)
}

fn native_hash_map_free<'ctx>(sess: &mut Session<'ctx, '_, '_>, locals: &Locals<'ctx>, ret_key: i64, out: PointerValue<'ctx>) -> Result<(), CodegenError> {
    let p0 = get_local(locals, 0)?;
    let map_key = deref_key_of(sess, get_local_key(locals, 0)?);
    let d = struct_gep(sess, map_key, p0, 0, "")?;
    let data = load_ptr(sess, d)?;
    let free = extern_free(sess);
    sess.2.build_call(free, &[into_meta(data.into())], "").map_err(builder_fail)?;
    build_unit_value_into(sess, ret_key, out)?;
    Ok(())
}

fn native_self_check<'ctx>(sess: &mut Session<'ctx, '_, '_>, ret_key: i64, out: PointerValue<'ctx>) -> Result<(), CodegenError> {
    let unit_key = result_arg_key(sess, ret_key, 0);
    let unit_val = build_unit_value(sess, unit_key)?;
    let ok_result = build_result_ok(sess, ret_key, unit_key, unit_val)?;
    copy_to_out(sess, ret_key, out, ok_result)?;
    Ok(())
}

/// Emits one continuation-byte check at offset `k` from the current
/// index: in bounds and a continuation byte (`10xxxxxx` — top two bits
/// are the value 2), else branch to `bad`.  Returns the block where the
/// check passed.
fn emit_cont_step<'ctx>(
    sess: &mut Session<'ctx, '_, '_>,
    f: FunctionValue<'ctx>,
    i: IntValue<'ctx>,
    len: IntValue<'ctx>,
    data: PointerValue<'ctx>,
    k: i64,
    bad: BasicBlockId<'ctx>,
) -> Result<BasicBlockId<'ctx>, CodegenError> {
    let kc = sess.0.i64_type().const_int(k as u64, false);
    let idx = sess.2.build_int_add(i, kc, "").map_err(builder_fail)?;
    let inb = sess.2.build_int_compare(IntPredicate::ULT, idx, len, "").map_err(builder_fail)?;
    let ok1 = new_block(sess, f, "utf8_c");
    let fail1 = new_block(sess, f, "utf8_oob");
    sess.2.build_conditional_branch(inb, ok1, fail1).map_err(builder_fail)?;
    sess.2.position_at_end(fail1);
    sess.2.build_unconditional_branch(bad).map_err(builder_fail)?;
    sess.2.position_at_end(ok1);
    let bptr = byte_offset(sess, data, idx)?;
    let b = load_i8(sess, bptr)?;
    let byte = utf8_byte_value(sess, b)?;
    let top2 = sess.2.build_right_shift(byte, sess.0.i64_type().const_int(6, false), false, "").map_err(builder_fail)?;
    let is_cont = sess.2.build_int_compare(IntPredicate::EQ, top2, sess.0.i64_type().const_int(2, false), "").map_err(builder_fail)?;
    let ok2 = new_block(sess, f, "utf8_okc");
    sess.2.build_conditional_branch(is_cont, ok2, bad).map_err(builder_fail)?;
    sess.2.position_at_end(ok2);
    Ok(ok2)
}

fn native_string_from_slice<'ctx>(sess: &mut Session<'ctx, '_, '_>, f: FunctionValue<'ctx>, locals: &Locals<'ctx>, ret_key: i64, out: PointerValue<'ctx>) -> Result<PointerValue<'ctx>, CodegenError> {
    let p0 = get_local(locals, 0)?;
    let data = slice_data(sess, p0)?;
    let len = slice_len_of(sess, p0)?;
    let i_slot = sess.2.build_alloca(sess.0.i64_type(), "i").map_err(builder_fail)?;
    store_key(sess, i_slot, sess.0.i64_type().const_zero().into())?;
    let loop_cond = new_block(sess, f, "utf8_cond");
    let loop_body = new_block(sess, f, "utf8_body");
    let valid_block = new_block(sess, f, "utf8_valid");
    let invalid_block = new_block(sess, f, "utf8_invalid");
    sess.2.build_unconditional_branch(loop_cond).map_err(builder_fail)?;
    sess.2.position_at_end(loop_cond);
    let i = load_i64(sess, i_slot)?;
    let done = sess.2.build_int_compare(IntPredicate::ULT, i, len, "").map_err(builder_fail)?;
    sess.2.build_conditional_branch(done, loop_body, valid_block).map_err(builder_fail)?;
    sess.2.position_at_end(loop_body);
    let bptr = byte_offset(sess, data, i)?;
    let b = load_i8(sess, bptr)?;
    let byte = utf8_byte_value(sess, b)?;
    let top_bit = sess.2.build_right_shift(byte, sess.0.i64_type().const_int(7, false), false, "").map_err(builder_fail)?;
    let lt80 = sess.2.build_int_compare(IntPredicate::EQ, top_bit, sess.0.i64_type().const_int(0, false), "").map_err(builder_fail)?;
    let adv1 = new_block(sess, f, "utf8_adv1");
    let test2 = new_block(sess, f, "utf8_t2");
    sess.2.build_conditional_branch(lt80, adv1, test2).map_err(builder_fail)?;
    sess.2.position_at_end(test2);
    let ge_c2 = sess.2.build_int_compare(IntPredicate::UGE, byte, sess.0.i64_type().const_int(UTF8_LEAD_2_MIN, false), "").map_err(builder_fail)?;
    let le_c2 = sess.2.build_int_compare(IntPredicate::ULE, byte, sess.0.i64_type().const_int(UTF8_LEAD_2_MAX, false), "").map_err(builder_fail)?;
    let in_c2 = sess.2.build_and(ge_c2, le_c2, "").map_err(builder_fail)?;
    let chk1 = new_block(sess, f, "utf8_chk1");
    let test3 = new_block(sess, f, "utf8_t3");
    sess.2.build_conditional_branch(in_c2, chk1, test3).map_err(builder_fail)?;
    sess.2.position_at_end(test3);
    let ge_e0 = sess.2.build_int_compare(IntPredicate::UGE, byte, sess.0.i64_type().const_int(UTF8_LEAD_3_MIN, false), "").map_err(builder_fail)?;
    let le_e0 = sess.2.build_int_compare(IntPredicate::ULE, byte, sess.0.i64_type().const_int(UTF8_LEAD_3_MAX, false), "").map_err(builder_fail)?;
    let in_e0 = sess.2.build_and(ge_e0, le_e0, "").map_err(builder_fail)?;
    let chk2 = new_block(sess, f, "utf8_chk2");
    let test4 = new_block(sess, f, "utf8_t4");
    sess.2.build_conditional_branch(in_e0, chk2, test4).map_err(builder_fail)?;
    sess.2.position_at_end(test4);
    let ge_f0 = sess.2.build_int_compare(IntPredicate::UGE, byte, sess.0.i64_type().const_int(UTF8_LEAD_4_MIN, false), "").map_err(builder_fail)?;
    let le_f4 = sess.2.build_int_compare(IntPredicate::ULE, byte, sess.0.i64_type().const_int(UTF8_LEAD_4_MAX, false), "").map_err(builder_fail)?;
    let in_f0 = sess.2.build_and(ge_f0, le_f4, "").map_err(builder_fail)?;
    let chk3 = new_block(sess, f, "utf8_chk3");
    let bad = new_block(sess, f, "utf8_bad");
    sess.2.build_conditional_branch(in_f0, chk3, bad).map_err(builder_fail)?;
    sess.2.position_at_end(chk1);
    let c1 = emit_cont_step(sess, f, i, len, data, 1, bad)?;
    sess.2.position_at_end(c1);
    let two = sess.0.i64_type().const_int(2, false);
    let i3 = sess.2.build_int_add(i, two, "").map_err(builder_fail)?;
    store_key(sess, i_slot, i3.into())?;
    sess.2.build_unconditional_branch(loop_cond).map_err(builder_fail)?;
    sess.2.position_at_end(chk2);
    let c2a = emit_cont_step(sess, f, i, len, data, 1, bad)?;
    sess.2.position_at_end(c2a);
    let c2b = emit_cont_step(sess, f, i, len, data, 2, bad)?;
    sess.2.position_at_end(c2b);
    let three = sess.0.i64_type().const_int(3, false);
    let i4 = sess.2.build_int_add(i, three, "").map_err(builder_fail)?;
    store_key(sess, i_slot, i4.into())?;
    sess.2.build_unconditional_branch(loop_cond).map_err(builder_fail)?;
    sess.2.position_at_end(chk3);
    let c3a = emit_cont_step(sess, f, i, len, data, 1, bad)?;
    sess.2.position_at_end(c3a);
    let c3b = emit_cont_step(sess, f, i, len, data, 2, bad)?;
    sess.2.position_at_end(c3b);
    let c3c = emit_cont_step(sess, f, i, len, data, 3, bad)?;
    sess.2.position_at_end(c3c);
    let four = sess.0.i64_type().const_int(4, false);
    let i5 = sess.2.build_int_add(i, four, "").map_err(builder_fail)?;
    store_key(sess, i_slot, i5.into())?;
    sess.2.build_unconditional_branch(loop_cond).map_err(builder_fail)?;
    sess.2.position_at_end(adv1);
    let one = sess.0.i64_type().const_int(1, false);
    let i2 = sess.2.build_int_add(i, one, "").map_err(builder_fail)?;
    store_key(sess, i_slot, i2.into())?;
    sess.2.build_unconditional_branch(loop_cond).map_err(builder_fail)?;
    sess.2.position_at_end(bad);
    sess.2.build_unconditional_branch(invalid_block).map_err(builder_fail)?;
    sess.2.position_at_end(invalid_block);
    let err_key = result_arg_key(sess, ret_key, 1);
    let invalid_tag = variant_index_by_name(sess, err_key, "InvalidUtf8")?;
    let fail_val = build_enum_value(sess, err_key, invalid_tag, &[])?;
    let err_result = build_result_err(sess, ret_key, err_key, fail_val)?;
    copy_to_out(sess, ret_key, out, err_result)?;
    let after = new_block(sess, f, "str_after");
    sess.2.build_unconditional_branch(after).map_err(builder_fail)?;
    sess.2.position_at_end(valid_block);
    let zero = sess.0.i64_type().const_zero();
    let is_zero = sess.2.build_int_compare(IntPredicate::EQ, len, zero, "").map_err(builder_fail)?;
    let one2 = sess.0.i64_type().const_int(1, false);
    let alloc_size = sess.2.build_select(is_zero, one2, len, "").map_err(builder_fail)?;
    let malloc = extern_malloc(sess);
    let call = sess.2.build_call(malloc, &[into_meta(alloc_size)], "").map_err(builder_fail)?;
    let raw = match call.try_as_basic_value() {
        ValueKind::Basic(bv) => bv.into_pointer_value(),
        ValueKind::Instruction(inst) => {
            return Err(builder_error(-1, 0, 0, &format!("internal: malloc returned void ({:?})", inst.get_opcode())));
        }
    };
    let null_cmp = is_null_ptr(sess, raw)?;
    let fail_alloc = new_block(sess, f, "str_alloc_fail");
    let copy_block = new_block(sess, f, "str_copy");
    sess.2.build_conditional_branch(null_cmp, fail_alloc, copy_block).map_err(builder_fail)?;
    sess.2.position_at_end(fail_alloc);
    let err_key2 = result_arg_key(sess, ret_key, 1);
    let alloc_fail_tag = variant_index_by_name(sess, err_key2, "AllocationFailed")?;
    let fkey = variant_payload_key(sess, err_key2, alloc_fail_tag, 0)?;
    let fval = declare_local(sess, fkey, "need")?;
    store_key(sess, fval, len.into())?;
    let fail_val2 = build_enum_value(sess, err_key2, alloc_fail_tag, &[(fkey, fval)])?;
    let err_result2 = build_result_err(sess, ret_key, err_key2, fail_val2)?;
    copy_to_out(sess, ret_key, out, err_result2)?;
    sess.2.build_unconditional_branch(after).map_err(builder_fail)?;
    sess.2.position_at_end(copy_block);
    sess.2.build_memcpy(raw, 1, data, 1, len).map_err(builder_fail)?;
    let str_key = result_arg_key(sess, ret_key, 0);
    let str_val = declare_local(sess, str_key, "str")?;
    let sd = struct_gep(sess, str_key, str_val, 0, "")?;
    store_key(sess, sd, raw.into())?;
    let sl = struct_gep(sess, str_key, str_val, 1, "")?;
    store_key(sess, sl, len.into())?;
    let ok_result = build_result_ok(sess, ret_key, str_key, str_val)?;
    copy_to_out(sess, ret_key, out, ok_result)?;
    sess.2.build_unconditional_branch(after).map_err(builder_fail)?;
    sess.2.position_at_end(after);
    Ok(out)
}

// ---------------------------------------------------------------------------
// Program entry: the cinnabar `main` and the process `main` wrapper.
// ---------------------------------------------------------------------------

/// The fn node of the program's `main` function, or NONE.  A function
/// symbol's declaration is its item row; the fn node is the item's `d`
/// slot, the same normalization the typechecker's `fn_node_of` applies
/// for calls.
fn find_main_fn(sess: &Session) -> i64 {
    let mut idx = 0i64;
    while idx < sess.5.len() as i64 / NODE_STRIDE {
        if node_tag(sess.5, idx) == NODE_SYM
            && node_a(sess.5, idx) == SYM_FUN
            && name_is(sess.4, node_b(sess.5, idx), "main")
        {
            let decl = node_c(sess.5, idx);
            if node_tag(sess.5, decl) == NODE_FN {
                return decl;
            }
            return node_d(sess.5, decl);
        }
        idx += 1;
    }
    NONE
}

/// The concrete param-key list of a fn node (read from the declaration;
/// the typechecker attached every key).
fn fn_param_key_list(sess: &mut Session, fn_slot: i64) -> Result<i64, CodegenError> {
    let param_decls = node_c(sess.5, fn_slot);
    let count = list_len(sess.6, param_decls);
    let list = alloc_list(sess.6);
    let mut idx = 0i64;
    while idx < count {
        let param = list_get(sess.6, param_decls, idx);
        list_push(sess.6, list, ty_key_of(sess.5, node_b(sess.5, param)));
        idx += 1;
    }
    Ok(list)
}

/// Emits the process `main` wrapper: calls the cinnabar `main` and maps
/// the returned ExitCode (ExitSuccess, ExitFailure, ExitDiagnostic) to a
/// process exit code.  A program without a `main` function is an error:
/// the compiler never invents an entry point the source did not declare.
pub fn emit_program<'ctx>(sess: &mut Session<'ctx, '_, '_>) -> Result<(), CodegenError> {
    let main_fn = find_main_fn(sess);
    if main_fn == NONE {
        return Err(builder_error(-1, 0, 0, "program has no main function"));
    }
    let params_list = fn_param_key_list(sess, main_fn)?;
    let ret_key = ty_key_of(sess.5, node_d(sess.5, main_fn));
    let nodes = &mut sess.5;
    let lists = &mut sess.6;
    let mono = canon_tyinfo(nodes, lists, TYD_MONO, main_fn, NONE, NONE, NONE);
    let main_val = get_or_emit_fn(sess, main_fn, NONE, mono, params_list, ret_key)?;
    let exit_key = ret_key;
    let i32_ty = sess.0.i32_type();
    let sig = i32_ty.fn_type(&[], false);
    let main_wrapper = sess.1.add_function("main", sig, None);
    let entry = sess.0.append_basic_block(main_wrapper, "entry");
    sess.2.position_at_end(entry);
    let call = sess.2.build_call(main_val, &[], "").map_err(builder_fail)?;
    let exit_val = match call.try_as_basic_value() {
        ValueKind::Basic(bv) => bv,
        ValueKind::Instruction(inst) => {
            return Err(builder_error(-1, 0, 0, &format!("internal: main returned void ({:?})", inst.get_opcode())));
        }
    };
    let exit_alloca = declare_local(sess, exit_key, "exit")?;
    store_key(sess, exit_alloca, exit_val)?;
    let exit_kind = em_key_kind(sess, exit_key);
    if exit_kind == TYD_BUILTIN {
        // A scalar return is the process exit code; `Unit` exits zero.
        let scalar_sym = em_key_sym(sess, exit_key);
        if scalar_sym == NONE {
            return Err(builder_error(-1, 0, 0, "internal: scalar main return without a symbol"));
        }
        if name_is(sess.4, node_b(sess.5, scalar_sym), "Unit") {
            let code0 = i32_ty.const_zero();
            sess.2.build_return(Some(&code0)).map_err(builder_fail)?;
        } else {
            let code_val = load_key(sess, exit_key, exit_alloca)?.into_int_value();
            let code = sess.2.build_int_cast(code_val, i32_ty, "").map_err(builder_fail)?;
            sess.2.build_return(Some(&code)).map_err(builder_fail)?;
        }
        return Ok(());
    }
    let tag_ptr = struct_gep(sess, exit_key, exit_alloca, 0, "")?;
    let tag = load_i64(sess, tag_ptr)?;
    let zero = sess.0.i64_type().const_zero();
    let one = sess.0.i64_type().const_int(1, false);
    let is_success = sess.2.build_int_compare(IntPredicate::EQ, tag, zero, "").map_err(builder_fail)?;
    let success_block = new_block(sess, main_wrapper, "exit_0");
    let failure_block = new_block(sess, main_wrapper, "exit_1");
    sess.2.build_conditional_branch(is_success, success_block, failure_block).map_err(builder_fail)?;
    sess.2.position_at_end(success_block);
    let code0 = i32_ty.const_zero();
    sess.2.build_return(Some(&code0)).map_err(builder_fail)?;
    sess.2.position_at_end(failure_block);
    let is_failure = sess.2.build_int_compare(IntPredicate::EQ, tag, one, "").map_err(builder_fail)?;
    let fail_ret = new_block(sess, main_wrapper, "exit_fail");
    let diag_block = new_block(sess, main_wrapper, "exit_diag");
    sess.2.build_conditional_branch(is_failure, fail_ret, diag_block).map_err(builder_fail)?;
    sess.2.position_at_end(fail_ret);
    let code1 = i32_ty.const_int(1, false);
    sess.2.build_return(Some(&code1)).map_err(builder_fail)?;
    let diag_tag = variant_index_by_name_opt(sess, exit_key, "ExitDiagnostic");
    sess.2.position_at_end(diag_block);
    if diag_tag != NONE {
        let (region, pty) = enum_payload_ptr(sess, exit_alloca, exit_key, diag_tag)?;
        let payload = sess.2.build_struct_gep(pty, region, 0, "").map_err(builder_fail)?;
        let diag_key = variant_payload_key(sess, exit_key, diag_tag, 0)?;
        let diag = load_key(sess, diag_key, payload)?.into_int_value();
        let code = sess.2.build_int_cast(diag, i32_ty, "").map_err(builder_fail)?;
        sess.2.build_return(Some(&code)).map_err(builder_fail)?;
    } else {
        let code1 = i32_ty.const_int(1, false);
        sess.2.build_return(Some(&code1)).map_err(builder_fail)?;
    }
    Ok(())
}
