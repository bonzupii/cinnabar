//! Lowering of the typed arena to LLVM IR.
//!
//! `emit_program` locates `main` by the resolver's `SYM_FUN_MAIN` tag,
//! monomorphizes every reachable function through `get_or_emit_fn`, and
//! lowers expressions, statements, and match arms to inkwell builder
//! calls; the native surface dispatches on `NAT_*` opcodes. Self-tail-
//! recursive calls jump to the function's `body_header` block with the
//! parameter slots overwritten; other calls emit `call` instructions, with
//! the `tail` marker on the calls the typechecker's `NODE_CALLFACT` fact
//! declares safe.
//!
//! **Invariants:**
//! - Loop jumps and `tail` markers are emitted only where `NODE_CALLFACT`
//!   reports the call tail-safe, never derived from an argument's type.
//! - Types, symbols, variant tags, and field offsets are read from earlier
//!   stages' attached rows; nothing is re-resolved or re-inferred here.
//! - The only `unsafe` FFI calls are `offset_array_elem_ptr` and
//!   `offset_buffer_elem_ptr` (`byte_elem_ptr` is the `i8` buffer form);
//!   struct fields are reached through `build_struct_gep`, never a
//!   raw pointer cast.
//! - A failure is a typed `CodegenError` carrying a real span, never a
//!   panic.

use crate::ast::*;
use crate::codegen::error::*;
use crate::codegen::types::*;
use crate::target::Target;
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
    Seeds,
    Target,
);

pub type InstFns<'ctx> = Vec<(i64, FunctionValue<'ctx>)>;

pub type Locals<'ctx> = Vec<(i64, i64, PointerValue<'ctx>)>;

pub type LoopTargets<'ctx> = Vec<(BasicBlockId<'ctx>, BasicBlockId<'ctx>)>;

pub type BasicBlockId<'ctx> = inkwell::basic_block::BasicBlock<'ctx>;

pub type FnCtx<'ctx, 'a> = (
    FunctionValue<'ctx>,
    Locals<'ctx>,
    LoopTargets<'ctx>,
    &'a [i64],
    &'a [i64],
    i64,
    BasicBlockId<'ctx>,
    Vec<PointerValue<'ctx>>,
    i64,
    i64,
);

pub type MatchCont<'ctx> = (i64, PointerValue<'ctx>, BasicBlockId<'ctx>);

pub type MatchScrut<'ctx> = (i64, PointerValue<'ctx>, BasicBlockId<'ctx>);

const UTF8_LEAD_2_MIN: u64 = 0xC2;
const UTF8_LEAD_2_MAX: u64 = 0xDF;

const UTF8_LEAD_3_MIN: u64 = 0xE0;
const UTF8_LEAD_3_MAX: u64 = 0xEF;

const UTF8_LEAD_4_MIN: u64 = 0xF0;
const UTF8_LEAD_4_MAX: u64 = 0xF4;

// Minimum decodable code point per sequence length (any smaller value is
// an overlong encoding), the maximum Unicode code point, and the UTF-16
// surrogate window.
const UTF8_CP_3_MIN: u64 = 0x800;
const UTF8_CP_4_MIN: u64 = 0x10000;
const UTF8_CP_MAX: u64 = 0x10FFFF;
const UTF8_SURROGATE_MIN: u64 = 0xD800;
const UTF8_SURROGATE_MAX: u64 = 0xDFFF;

fn utf8_byte_value<'ctx>(sess: &mut Session<'ctx, '_, '_>, b: IntValue<'ctx>) -> Result<IntValue<'ctx>, CodegenError> {
    let bw = sess.2.build_int_cast(b, sess.0.i64_type(), "").map_err(builder_fail)?;
    let byte = sess.2.build_and(bw, sess.0.i64_type().const_int(0xFF, false), "").map_err(builder_fail)?;
    Ok(byte)
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

fn alloca_raw<'ctx>(
    sess: &mut Session<'ctx, '_, '_>,
    ty: BasicTypeEnum<'ctx>,
    name: &str,
    span: (i64, i64, i64),
) -> Result<PointerValue<'ctx>, CodegenError> {
    let entry = match sess
        .2
        .get_insert_block()
        .and_then(|block| block.get_parent())
        .and_then(|fun| fun.get_first_basic_block())
    {
        Some(block) => block,
        None => return Err(builder_error(span.0, span.1, span.2, "internal: alloca outside a function body")),
    };
    let alloca_builder = sess.0.create_builder();
    match entry.get_first_instruction() {
        Some(first) => alloca_builder.position_before(&first),
        None => alloca_builder.position_at_end(entry),
    }
    alloca_builder.build_alloca(ty, name).map_err(builder_fail)
}

fn alloca_typed<'ctx>(sess: &mut Session<'ctx, '_, '_>, key: i64, name: &str, span: (i64, i64, i64)) -> Result<PointerValue<'ctx>, CodegenError> {
    let ty = llvm_type(&mut ty_env(sess), key, span)?;
    alloca_raw(sess, ty, name, span)
}

fn ptr_ty<'ctx>(sess: &Session<'ctx, '_, '_>) -> inkwell::types::PointerType<'ctx> {
    sess.0.ptr_type(AddressSpace::from(0u16))
}

fn load_key<'ctx>(sess: &mut Session<'ctx, '_, '_>, key: i64, ptr: PointerValue<'ctx>, span: (i64, i64, i64)) -> Result<BasicValueEnum<'ctx>, CodegenError> {
    let ty = llvm_of(sess, key, span)?;
    sess.2.build_load(ty, ptr, "").map_err(builder_fail)
}

fn load_i8<'ctx>(sess: &mut Session<'ctx, '_, '_>, ptr: PointerValue<'ctx>) -> Result<IntValue<'ctx>, CodegenError> {
    Ok(sess.2.build_load(sess.0.i8_type(), ptr, "").map_err(builder_fail)?.into_int_value())
}

fn load_i64<'ctx>(sess: &mut Session<'ctx, '_, '_>, ptr: PointerValue<'ctx>) -> Result<IntValue<'ctx>, CodegenError> {
    Ok(sess.2.build_load(sess.0.i64_type(), ptr, "").map_err(builder_fail)?.into_int_value())
}

fn load_ptr<'ctx>(sess: &mut Session<'ctx, '_, '_>, ptr: PointerValue<'ctx>) -> Result<PointerValue<'ctx>, CodegenError> {
    Ok(sess.2.build_load(ptr_ty(sess), ptr, "").map_err(builder_fail)?.into_pointer_value())
}

fn struct_gep<'ctx>(sess: &mut Session<'ctx, '_, '_>, key: i64, ptr: PointerValue<'ctx>, index: u32, name: &str, span: (i64, i64, i64)) -> Result<PointerValue<'ctx>, CodegenError> {
    let ty = llvm_of(sess, key, span)?;
    sess.2.build_struct_gep(ty, ptr, index, name).map_err(builder_fail)
}

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

fn copy_value<'ctx>(
    sess: &mut Session<'ctx, '_, '_>,
    key: i64,
    dst: PointerValue<'ctx>,
    src: PointerValue<'ctx>,
    span: (i64, i64, i64),
) -> Result<(), CodegenError> {
    let kind = key_kind_of(sess.5, key);
    let elem_kind = key_kind_of(sess.5, key_elem_of(sess.5, key));
    if is_aggregate_kind(kind) || kind == TYD_REF && elem_kind == TYD_SLICE {
        let ty = llvm_type(&mut ty_env(sess), key, span)?;
        let size = sess.3.get_abi_size(&ty);
        let align = sess.3.get_abi_alignment(&ty);
        let size_val = sess.0.i64_type().const_int(size, false);
        sess.2.build_memcpy(dst, align, src, align, size_val).map_err(builder_fail)?;
    } else {
        let value = load_key(sess, key, src, span)?;
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

fn const_int_of<'ctx>(sess: &mut Session<'ctx, '_, '_>, key: i64, value: i64, span: (i64, i64, i64)) -> Result<BasicValueEnum<'ctx>, CodegenError> {
    let kind = key_kind_of(sess.5, key);
    if kind != TYD_BUILTIN {
        return Err(builder_error(span.0, span.1, span.2, "constant of a non-scalar type"));
    }
    let sub = key_builtin_of(sess, key);
    if sub == BUILTIN_BOOL {
        return Ok(sess.0.bool_type().const_int(value as u64, false).into());
    }
    // Every integer width is emitted from the shared width/mask metadata:
    // the stored bit pattern is masked to the width before const emission.
    let width = builtin_int_width(sub);
    let bits = (value as u64) & builtin_int_mask(sub);
    if width == 8 {
        return Ok(sess.0.i8_type().const_int(bits, false).into());
    }
    if width == 16 {
        return Ok(sess.0.i16_type().const_int(bits, false).into());
    }
    if width == 32 {
        return Ok(sess.0.i32_type().const_int(bits, false).into());
    }
    if width == 64 {
        return Ok(sess.0.i64_type().const_int(bits, false).into());
    }
    Err(builder_error(span.0, span.1, span.2, "unsupported scalar type"))
}

fn key_sym_of(nodes: &[i64], key: i64) -> i64 {
    let row = find_tyinfo(nodes, key);
    if row == NONE {
        NONE
    } else {
        node_c(nodes, row)
    }
}

fn bind_local<'ctx>(locals: &mut Locals<'ctx>, name: i64, key: i64, ptr: PointerValue<'ctx>) {
    locals.push((name, key, ptr));
}

fn get_local<'ctx>(locals: &Locals<'ctx>, name: i64, span: (i64, i64, i64)) -> Result<PointerValue<'ctx>, CodegenError> {
    let mut idx = locals.len();
    while idx > 0 {
        idx -= 1;
        match locals.get(idx) {
            Some(entry) => {
                if entry.0 == name {
                    return Ok(entry.2);
                }
            }
            None => return Err(builder_error(span.0, span.1, span.2, "internal: unbound local in codegen")),
        }
    }
    Err(builder_error(span.0, span.1, span.2, "internal: unbound local in codegen"))
}

fn get_local_key<'ctx>(locals: &Locals<'ctx>, name: i64, span: (i64, i64, i64)) -> Result<i64, CodegenError> {
    let mut idx = locals.len();
    while idx > 0 {
        idx -= 1;
        match locals.get(idx) {
            Some(entry) => {
                if entry.0 == name {
                    return Ok(entry.1);
                }
            }
            None => return Err(builder_error(span.0, span.1, span.2, "internal: unbound local in codegen")),
        }
    }
    Err(builder_error(span.0, span.1, span.2, "internal: unbound local in codegen"))
}

fn declare_local<'ctx>(sess: &mut Session<'ctx, '_, '_>, key: i64, name: &str, span: (i64, i64, i64)) -> Result<PointerValue<'ctx>, CodegenError> {
    alloca_typed(sess, key, name, span)
}

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

fn llvm_of<'ctx>(sess: &mut Session<'ctx, '_, '_>, key: i64, span: (i64, i64, i64)) -> Result<BasicTypeEnum<'ctx>, CodegenError> {
    llvm_type(&mut ty_env(sess), key, span)
}

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

fn key_builtin_of(sess: &Session, key: i64) -> i64 {
    let row = find_tyinfo(sess.5, key);
    if row == NONE {
        NONE
    } else {
        node_f(sess.5, row)
    }
}

// True for `&[U8]`, the type of a string literal and therefore of a string
// constant.  Nothing else in the language folds to a constant of this type,
// so this is how the constant path recognizes that the value it stored is
// the interned name id of a byte sequence rather than a number.
fn is_byte_slice_key(sess: &Session, key: i64) -> bool {
    if em_key_kind(sess, key) != TYD_REF {
        return false;
    }
    let slice = em_key_elem(sess, key);
    if em_key_kind(sess, slice) != TYD_SLICE {
        return false;
    }
    let elem = em_key_elem(sess, slice);
    em_key_kind(sess, elem) == TYD_BUILTIN && key_builtin_of(sess, elem) == BUILTIN_U8
}

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

// The attached fact row for `name` on the canonical struct key, filled by
// the typechecker; no ITEM_STRUCT re-walk and no re-run of generic
// substitution here.  Callers read the index and key
// slots they consume.
fn struct_field_fact_row(sess: &Session, struct_key: i64, name: i64, span: (i64, i64, i64)) -> Result<i64, CodegenError> {
    let row = find_fieldkey(sess.5, struct_key, name);
    if row == NONE {
        return Err(builder_error(span.0, span.1, span.2, "internal: struct field fact not found"));
    }
    Ok(row)
}

fn variant_index_of(sess: &Session, variant_sym: i64, span: (i64, i64, i64)) -> Result<i64, CodegenError> {
    if variant_sym == NONE {
        return Err(builder_error(span.0, span.1, span.2, "internal: variant not found in its enum"));
    }
    let idx = sym_variant_tag_of(sess.5, variant_sym);
    if idx == NONE {
        return Err(builder_error(span.0, span.1, span.2, "internal: variant not found in its enum"));
    }
    Ok(idx)
}

// The tag of a sealed protocol variant, loaded in O(1) from the
// resolver-seeded symbol slot in the Seeds table via `sym_variant_tag_of`.
fn seeded_enum_variant_tag(sess: &Session, seed_sym_slot: usize, span: (i64, i64, i64)) -> Result<i64, CodegenError> {
    let variant_sym = sess.12.sym(seed_sym_slot);
    if variant_sym == NONE {
        return Err(builder_error(span.0, span.1, span.2, "internal: seeded protocol variant symbol is absent"));
    }
    let tag = sym_variant_tag_of(sess.5, variant_sym);
    if tag == NONE {
        return Err(builder_error(span.0, span.1, span.2, "internal: seeded protocol variant tag is absent"));
    }
    Ok(tag)
}

// The third declared variant when it carries one int payload, else NONE.
fn exit_diag_tag_of(sess: &mut Session, exit_key: i64, span: (i64, i64, i64)) -> Result<i64, CodegenError> {
    let enum_sym = em_key_sym(sess, exit_key);
    if enum_sym == NONE {
        return Ok(NONE);
    }
    let item = em_sym_decl(sess, enum_sym);
    if item == NONE || node_tag(sess.5, item) != NODE_ITEM || node_a(sess.5, item) != ITEM_ENUM {
        return Ok(NONE);
    }
    let variants = node_e(sess.5, item);
    if list_len(sess.6, variants) <= EXIT_DIAG_VARIANT_INDEX {
        return Ok(NONE);
    }
    if variant_payload_count(sess, exit_key, EXIT_DIAG_VARIANT_INDEX) != 1 {
        return Ok(NONE);
    }
    let payload_key = variant_payload_key(sess, exit_key, EXIT_DIAG_VARIANT_INDEX, 0, span)?;
    if em_key_kind(sess, payload_key) == TYD_BUILTIN && builtin_int_is_int(key_builtin_of(sess, payload_key)) {
        Ok(EXIT_DIAG_VARIANT_INDEX)
    } else {
        Ok(NONE)
    }
}

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

fn variant_payload_key(sess: &mut Session, enum_key: i64, variant_idx: i64, field_idx: i64, span: (i64, i64, i64)) -> Result<i64, CodegenError> {
    // The precomputed NODE_PAYLOADKEY fact for (enum key, variant, field);
    // no ITEM_ENUM re-walk and no generic substitution happens here.
    let row = find_payloadkey(sess.5, enum_key, variant_idx, field_idx);
    if row == NONE {
        return Err(builder_error(span.0, span.1, span.2, "internal: variant payload key fact not found"));
    }
    Ok(payloadkey_key_of(sess.5, row))
}

fn enum_payload_ptr<'ctx>(sess: &mut Session<'ctx, '_, '_>, ptr: PointerValue<'ctx>, enum_key: i64, variant_idx: i64, span: (i64, i64, i64)) -> Result<(PointerValue<'ctx>, BasicTypeEnum<'ctx>), CodegenError> {
    let enum_ty = llvm_of(sess, enum_key, span)?;
    let region = sess.2.build_struct_gep(enum_ty, ptr, 1, "").map_err(builder_fail)?;
    let pty = payload_struct_of(&mut ty_env(sess), enum_key, variant_idx, span)?;
    Ok((region, pty))
}

fn build_enum_value<'ctx>(sess: &mut Session<'ctx, '_, '_>, key: i64, variant_idx: i64, payloads: &[(i64, PointerValue<'ctx>)], span: (i64, i64, i64)) -> Result<PointerValue<'ctx>, CodegenError> {
    let ptr = declare_local(sess, key, "enum", span)?;
    build_enum_value_into(sess, key, variant_idx, payloads, ptr, span)?;
    Ok(ptr)
}

fn build_enum_value_into<'ctx>(sess: &mut Session<'ctx, '_, '_>, key: i64, variant_idx: i64, payloads: &[(i64, PointerValue<'ctx>)], out: PointerValue<'ctx>, span: (i64, i64, i64)) -> Result<(), CodegenError> {
    let tag_ptr = struct_gep(sess, key, out, 0, "", span)?;
    let tag = sess.0.i64_type().const_int(variant_idx as u64, false);
    store_key(sess, tag_ptr, tag.into())?;
    let mut idx = 0usize;
    while idx < payloads.len() {
        let (pkey, pptr) = match payloads.get(idx) {
            Some(pair) => *pair,
            None => break,
        };
        let (region, pty) = enum_payload_ptr(sess, out, key, variant_idx, span)?;
        let fptr = sess.2.build_struct_gep(pty, region, idx as u32, "").map_err(builder_fail)?;
        copy_value(sess, pkey, fptr, pptr, span)?;
        idx += 1;
    }
    Ok(())
}

fn slice_data<'ctx>(sess: &mut Session<'ctx, '_, '_>, view_ptr: PointerValue<'ctx>) -> Result<PointerValue<'ctx>, CodegenError> {
    let dp = slice_gep(sess, view_ptr, 0, "")?;
    load_ptr(sess, dp)
}

fn slice_len_of<'ctx>(sess: &mut Session<'ctx, '_, '_>, view_ptr: PointerValue<'ctx>) -> Result<IntValue<'ctx>, CodegenError> {
    let lp = slice_gep(sess, view_ptr, 1, "")?;
    load_i64(sess, lp)
}

// Two-tier addressing: `offset_array_elem_ptr` GEPs `[0, idx]` against
// `[N x T]`; `offset_buffer_elem_ptr` GEPs `[idx]` against `T` (`byte_elem_ptr`
// is the `i8` form).  Precondition: `base` is an allocation of the named
// type and `idx` is within its bounds.
fn offset_array_elem_ptr<'ctx>(sess: &mut Session<'ctx, '_, '_>, array_ty: BasicTypeEnum<'ctx>, base: PointerValue<'ctx>, idx: IntValue<'ctx>) -> Result<PointerValue<'ctx>, CodegenError> {
    let zero = sess.0.i64_type().const_zero();
    let gep = unsafe { sess.2.build_gep(array_ty, base, &[zero, idx], "") }.map_err(builder_fail)?;
    Ok(gep)
}

fn offset_buffer_elem_ptr<'ctx>(sess: &mut Session<'ctx, '_, '_>, elem_ty: BasicTypeEnum<'ctx>, base: PointerValue<'ctx>, idx: IntValue<'ctx>) -> Result<PointerValue<'ctx>, CodegenError> {
    let gep = unsafe { sess.2.build_gep(elem_ty, base, &[idx], "") }.map_err(builder_fail)?;
    Ok(gep)
}

fn byte_elem_ptr<'ctx>(sess: &mut Session<'ctx, '_, '_>, base: PointerValue<'ctx>, idx: IntValue<'ctx>) -> Result<PointerValue<'ctx>, CodegenError> {
    offset_buffer_elem_ptr(sess, sess.0.i8_type().into(), base, idx)
}

fn block_terminated<'ctx>(sess: &mut Session<'ctx, '_, '_>) -> bool {
    match sess.2.get_insert_block() {
        Some(block) => block.get_terminator().is_some(),
        None => false,
    }
}

fn new_block<'ctx>(sess: &mut Session<'ctx, '_, '_>, f: FunctionValue<'ctx>, name: &str) -> BasicBlockId<'ctx> {
    sess.0.append_basic_block(f, name)
}

fn build_unit_value_into<'ctx>(sess: &mut Session<'ctx, '_, '_>, key: i64, ptr: PointerValue<'ctx>, span: (i64, i64, i64)) -> Result<(), CodegenError> {
    let tag_ptr = struct_gep(sess, key, ptr, 0, "", span)?;
    let tag = sess.0.i64_type().const_int(0, false);
    store_key(sess, tag_ptr, tag.into())?;
    Ok(())
}

fn emit_stmt_list<'ctx, 'a>(
    sess: &mut Session<'ctx, '_, '_>,
    ctx: &mut FnCtx<'ctx, 'a>,
    list: i64,
    expected_key: i64,
    span: (i64, i64, i64),
) -> Result<(PointerValue<'ctx>, bool), CodegenError> {
    let count = list_len(sess.6, list);
    if count == 0 {
        // An empty block evaluates to its expected type.  The typechecker
        // attaches Unit to empty while/if bodies and to the fall-through of
        // Unit-returning functions; build a tag-0 value into a fresh slot
        // instead of failing, so empty blocks lower cleanly.
        let slot = alloca_typed(sess, expected_key, "empty", span)?;
        build_unit_value_into(sess, expected_key, slot, span)?;
        return Ok((slot, false));
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
    Ok((ptr, block_terminated(sess)))
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
    let span = (node_file(sess.5, stmt), node_start(sess.5, stmt), node_end(sess.5, stmt));
    let key = sub_key(sess, ctx.3, ctx.4, stmt_ty_of(sess.5, stmt));
    let slot = alloca_typed(sess, key, "lb", span)?;
    sess.2.build_unconditional_branch(target).map_err(builder_fail)?;
    Ok((slot, true))
}

fn emit_let<'ctx, 'a>(
    sess: &mut Session<'ctx, '_, '_>,
    ctx: &mut FnCtx<'ctx, 'a>,
    stmt: i64,
) -> Result<(PointerValue<'ctx>, bool), CodegenError> {
    let span = (node_file(sess.5, stmt), node_start(sess.5, stmt), node_end(sess.5, stmt));
    let name = node_c(sess.5, stmt);
    let init = node_e(sess.5, stmt);
    let key = sub_key(sess, ctx.3, ctx.4, stmt_ty_of(sess.5, stmt));
    let ptr = declare_local(sess, key, &em_name(sess, name), span)?;
    let init_ptr = emit_expr(sess, ctx, init)?;
    copy_value(sess, key, ptr, init_ptr, span)?;
    bind_local(&mut ctx.1, name, key, ptr);
    Ok((ptr, false))
}

fn emit_assign<'ctx, 'a>(
    sess: &mut Session<'ctx, '_, '_>,
    ctx: &mut FnCtx<'ctx, 'a>,
    stmt: i64,
) -> Result<(PointerValue<'ctx>, bool), CodegenError> {
    let span = (node_file(sess.5, stmt), node_start(sess.5, stmt), node_end(sess.5, stmt));
    let target = node_b(sess.5, stmt);
    let value = node_c(sess.5, stmt);
    let key = sub_key(sess, ctx.3, ctx.4, stmt_ty_of(sess.5, stmt));
    let tptr = emit_place(sess, ctx, target)?;
    let vptr = emit_expr(sess, ctx, value)?;
    copy_value(sess, key, tptr, vptr, span)?;
    Ok((tptr, false))
}

fn emit_place<'ctx, 'a>(
    sess: &mut Session<'ctx, '_, '_>,
    ctx: &mut FnCtx<'ctx, 'a>,
    expr: i64,
) -> Result<PointerValue<'ctx>, CodegenError> {
    if node_a(sess.5, expr) == EXPR_PATH {
        return emit_path(sess, ctx, expr);
    }
    if node_a(sess.5, expr) == EXPR_FIELD_ACCESS {
        return emit_field_access(sess, ctx, expr);
    }
    Err(builder_error(
        node_file(sess.5, expr),
        node_start(sess.5, expr),
        node_end(sess.5, expr),
        "internal: invalid assignment target",
    ))
}

fn emit_field_access<'ctx, 'a>(
    sess: &mut Session<'ctx, '_, '_>,
    ctx: &mut FnCtx<'ctx, 'a>,
    expr: i64,
) -> Result<PointerValue<'ctx>, CodegenError> {
    let span = (node_file(sess.5, expr), node_start(sess.5, expr), node_end(sess.5, expr));
    let base = node_b(sess.5, expr);
    let field = node_c(sess.5, expr);
    let mut ptr = emit_expr(sess, ctx, base)?;
    let mut cur_key = sub_key(sess, ctx.3, ctx.4, em_expr_ty(sess, base));
    let ckind = em_key_kind(sess, cur_key);
    if ckind == TYD_REF || ckind == TYD_REF_MUT {
        ptr = load_ptr(sess, ptr)?;
        cur_key = em_key_elem(sess, cur_key);
    }
    let row = struct_field_fact_row(sess, cur_key, field, span)?;
    let fld_idx = fieldkey_idx_of(sess.5, row);
    struct_gep(sess, cur_key, ptr, fld_idx as u32, "", span)
}

fn emit_while<'ctx, 'a>(
    sess: &mut Session<'ctx, '_, '_>,
    ctx: &mut FnCtx<'ctx, 'a>,
    stmt: i64,
) -> Result<(PointerValue<'ctx>, bool), CodegenError> {
    let span = (node_file(sess.5, stmt), node_start(sess.5, stmt), node_end(sess.5, stmt));
    let cond = node_b(sess.5, stmt);
    let body = node_c(sess.5, stmt);
    let cond_block = new_block(sess, ctx.0, "while_cond");
    let body_block = new_block(sess, ctx.0, "while_body");
    let exit_block = new_block(sess, ctx.0, "while_exit");
    sess.2.build_unconditional_branch(cond_block).map_err(builder_fail)?;
    sess.2.position_at_end(cond_block);
    let ckey = sub_key(sess, ctx.3, ctx.4, em_expr_ty(sess, cond));
    let cptr = emit_expr(sess, ctx, cond)?;
    let cv = load_key(sess, ckey, cptr, span)?.into_int_value();
    sess.2.build_conditional_branch(cv, body_block, exit_block).map_err(builder_fail)?;
    sess.2.position_at_end(body_block);
    ctx.2.push((exit_block, cond_block));
    let key = sub_key(sess, ctx.3, ctx.4, stmt_ty_of(sess.5, stmt));
    emit_stmt_list(sess, ctx, body, key, span)?;
    ctx.2.pop();
    if !block_terminated(sess) {
        sess.2.build_unconditional_branch(cond_block).map_err(builder_fail)?;
    }
    sess.2.position_at_end(exit_block);
    let slot = alloca_typed(sess, key, "while", span)?;
    Ok((slot, false))
}

fn emit_if<'ctx, 'a>(
    sess: &mut Session<'ctx, '_, '_>,
    ctx: &mut FnCtx<'ctx, 'a>,
    stmt: i64,
) -> Result<(PointerValue<'ctx>, bool), CodegenError> {
    let span = (node_file(sess.5, stmt), node_start(sess.5, stmt), node_end(sess.5, stmt));
    let cond = node_b(sess.5, stmt);
    let then_list = node_c(sess.5, stmt);
    let else_list = node_d(sess.5, stmt);
    let ckey = sub_key(sess, ctx.3, ctx.4, em_expr_ty(sess, cond));
    let cptr = emit_expr(sess, ctx, cond)?;
    let cv = load_key(sess, ckey, cptr, span)?.into_int_value();
    let then_block = new_block(sess, ctx.0, "if_then");
    let else_block = new_block(sess, ctx.0, "if_else");
    let merge_block = new_block(sess, ctx.0, "if_merge");
    sess.2.build_conditional_branch(cv, then_block, else_block).map_err(builder_fail)?;
    sess.2.position_at_end(then_block);
    let key = sub_key(sess, ctx.3, ctx.4, stmt_ty_of(sess.5, stmt));
    emit_stmt_list(sess, ctx, then_list, key, span)?;
    if !block_terminated(sess) {
        sess.2.build_unconditional_branch(merge_block).map_err(builder_fail)?;
    }
    sess.2.position_at_end(else_block);
    if else_list != NONE {
        emit_stmt_list(sess, ctx, else_list, key, span)?;
        if !block_terminated(sess) {
            sess.2.build_unconditional_branch(merge_block).map_err(builder_fail)?;
        }
    } else {
        sess.2.build_unconditional_branch(merge_block).map_err(builder_fail)?;
    }
    sess.2.position_at_end(merge_block);
    let slot = alloca_typed(sess, key, "if", span)?;
    Ok((slot, false))
}

fn emit_return<'ctx, 'a>(
    sess: &mut Session<'ctx, '_, '_>,
    ctx: &mut FnCtx<'ctx, 'a>,
    stmt: i64,
) -> Result<(PointerValue<'ctx>, bool), CodegenError> {
    let span = (node_file(sess.5, stmt), node_start(sess.5, stmt), node_end(sess.5, stmt));
    let value = node_b(sess.5, stmt);
    let ret_key = sub_key(sess, ctx.3, ctx.4, ctx.5);
    let ptr = if value == NONE {
        let slot = alloca_typed(sess, ret_key, "ret", span)?;
        build_unit_value_into(sess, ret_key, slot, span)?;
        slot
    } else {
        emit_expr(sess, ctx, value)?
    };
    if block_terminated(sess) {
        return Ok((ptr, true));
    }
    let loaded = load_key(sess, ret_key, ptr, span)?;
    sess.2.build_return(Some(&loaded)).map_err(builder_fail)?;
    Ok((ptr, true))
}

fn emit_expr<'ctx, 'a>(
    sess: &mut Session<'ctx, '_, '_>,
    ctx: &mut FnCtx<'ctx, 'a>,
    expr: i64,
) -> Result<PointerValue<'ctx>, CodegenError> {
    let kind = node_a(sess.5, expr);
    if kind == EXPR_CALL {
        return emit_call(sess, ctx, expr);
    }
    if kind == EXPR_MATCH {
        emit_match(sess, ctx, expr)
    } else {
        if kind == EXPR_LIT {
            emit_lit(sess, ctx, expr)
        } else if kind == EXPR_PATH {
            emit_path(sess, ctx, expr)
        } else if kind == EXPR_UNARY {
            emit_unary(sess, ctx, expr)
        } else if kind == EXPR_BINARY {
            emit_binary(sess, ctx, expr)
        } else if kind == EXPR_STRUCT_LIT {
            emit_struct_lit(sess, ctx, expr)
        } else if kind == EXPR_ARRAY {
            emit_array(sess, ctx, expr)
        } else if kind == EXPR_TRY {
            emit_try(sess, ctx, expr)
        } else if kind == EXPR_INDEX {
            emit_index(sess, ctx, expr)
        } else if kind == EXPR_FIELD_ACCESS {
            emit_field_access(sess, ctx, expr)
        } else {
            Err(builder_error(
                node_file(sess.5, expr),
                node_start(sess.5, expr),
                node_end(sess.5, expr),
                "internal: unknown expression kind",
            ))
        }
    }
}

// The `.rodata` global holding a string literal's bytes, created on first
// use and reused afterwards.
//
// The global's name is derived from the literal's interned name id, so the
// module itself is the reuse table: two occurrences of the same literal
// intern to the same id, resolve to the same global, and share one copy of
// the bytes. There is no side table to keep in step with the name arena.
//
// The bytes are stored exactly as the lexer decoded them, with no trailing
// NUL: a Cinnabar byte slice carries its own length, and appending a
// terminator would make the global disagree with the length the slice
// reports.
fn string_literal_global<'ctx>(sess: &mut Session<'ctx, '_, '_>, name_id: i64) -> Result<(PointerValue<'ctx>, u64), CodegenError> {
    let text = em_name(sess, name_id);
    let bytes = text.as_bytes();
    let symbol = format!(".cnb.str.{}", name_id);
    let existing = sess.1.get_global(&symbol);
    let global = match existing {
        Some(found) => found,
        None => {
            let data = sess.0.const_string(bytes, false);
            let created = sess.1.add_global(data.get_type(), Some(AddressSpace::from(0u16)), &symbol);
            created.set_initializer(&data);
            created.set_constant(true);
            // Private and unnamed_addr: the literal has no linkage-visible
            // identity, only its contents, so the linker is free to merge
            // equal literals across translation units.
            created.set_linkage(inkwell::module::Linkage::Private);
            created.set_unnamed_address(inkwell::values::UnnamedAddress::Global);
            created
        }
    };
    Ok((global.as_pointer_value(), bytes.len() as u64))
}

// Materializes a string literal as a `&[U8]` slice value: the `.rodata`
// pointer and the byte length, in the same `{ ptr, i64 }` shape every other
// slice in the language uses.  Shared by the inline-literal path and the
// `const` path so the two cannot diverge.
fn emit_string_slice<'ctx>(sess: &mut Session<'ctx, '_, '_>, key: i64, name_id: i64, span: (i64, i64, i64)) -> Result<PointerValue<'ctx>, CodegenError> {
    let (data, len) = string_literal_global(sess, name_id)?;
    let view = declare_local(sess, key, "str", span)?;
    let dp = slice_gep(sess, view, 0, "")?;
    store_key(sess, dp, data.into())?;
    let lp = slice_gep(sess, view, 1, "")?;
    store_key(sess, lp, sess.0.i64_type().const_int(len, false).into())?;
    Ok(view)
}

fn emit_lit<'ctx, 'a>(
    sess: &mut Session<'ctx, '_, '_>,
    ctx: &mut FnCtx<'ctx, 'a>,
    expr: i64,
) -> Result<PointerValue<'ctx>, CodegenError> {
    let span = (node_file(sess.5, expr), node_start(sess.5, expr), node_end(sess.5, expr));
    let key = sub_key(sess, ctx.3, ctx.4, em_expr_ty(sess, expr));
    let value = node_c(sess.5, expr);
    if node_b(sess.5, expr) == LIT_STRING {
        return emit_string_slice(sess, key, value, span);
    }
    let ptr = declare_local(sess, key, "lit", span)?;
    let cv = const_int_of(sess, key, value, span)?;
    store_key(sess, ptr, cv)?;
    Ok(ptr)
}

fn emit_path<'ctx, 'a>(
    sess: &mut Session<'ctx, '_, '_>,
    ctx: &mut FnCtx<'ctx, 'a>,
    expr: i64,
) -> Result<PointerValue<'ctx>, CodegenError> {
    let span = (node_file(sess.5, expr), node_start(sess.5, expr), node_end(sess.5, expr));
    let sym = em_expr_sym(sess, expr);
    if sym != NONE {
        let kind = node_a(sess.5, sym);
        if kind == SYM_CONST {
            let key = sub_key(sess, ctx.3, ctx.4, em_expr_ty(sess, expr));
            if !has_const_value(sess.5, sym) {
                return Err(builder_error(span.0, span.1, span.2, "internal: constant without a folded value"));
            }
            let value = find_const_value(sess.5, sym);
            if is_byte_slice_key(sess, key) {
                // A string constant folded to the interned name id of its
                // bytes, so it materializes through exactly the same global
                // as an inline literal of the same text.
                return emit_string_slice(sess, key, value, span);
            }
            let ptr = declare_local(sess, key, "const", span)?;
            let cv = const_int_of(sess, key, value, span)?;
            store_key(sess, ptr, cv)?;
            return Ok(ptr);
        }
        if kind == SYM_VARIANT {
            let key = sub_key(sess, ctx.3, ctx.4, em_expr_ty(sess, expr));
            let idx = variant_index_of(sess, sym, span)?;
            return build_enum_value(sess, key, idx, &[], span);
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
    let mut ptr = get_local(&ctx.1, first, span)?;
    let mut cur_key = get_local_key(&ctx.1, first, span)?;
    let mut idx = 1i64;
    while idx < count {
        let field = list_get(sess.6, segs, idx);
        let ckind = em_key_kind(sess, cur_key);
        if ckind == TYD_REF || ckind == TYD_REF_MUT {
            ptr = load_ptr(sess, ptr)?;
            cur_key = em_key_elem(sess, cur_key);
        }
        let row = struct_field_fact_row(sess, cur_key, field, span)?;
        let fkey = fieldkey_key_of(sess.5, row);
        let fld_idx = fieldkey_idx_of(sess.5, row);
        ptr = struct_gep(sess, cur_key, ptr, fld_idx as u32, "", span)?;
        cur_key = fkey;
        idx += 1;
    }
    Ok(ptr)
}

fn emit_unary<'ctx, 'a>(
    sess: &mut Session<'ctx, '_, '_>,
    ctx: &mut FnCtx<'ctx, 'a>,
    expr: i64,
) -> Result<PointerValue<'ctx>, CodegenError> {
    let span = (node_file(sess.5, expr), node_start(sess.5, expr), node_end(sess.5, expr));
    let op = node_b(sess.5, expr);
    let inner = node_c(sess.5, expr);
    let key = sub_key(sess, ctx.3, ctx.4, em_expr_ty(sess, expr));
    if op == UN_REF || op == UN_REF_MUT {
        let inner_ptr = emit_expr(sess, ctx, inner)?;
        // `&arr[i]` and `&mut arr[i]` are typed as one expression: the
        // typechecker hands the borrow down into the index, so a fallible
        // index yields `Result(&T, IndexError)` and the value `emit_index`
        // already produced *is* this expression's value -- there is nothing
        // left to borrow.  Which of the two it is comes from the access
        // fact on the index node, not from the shape of the key here, so
        // the fallible path is decided in exactly one place for indexing.
        if node_d(sess.5, inner) == INDEX_FALLIBLE {
            return Ok(inner_ptr);
        }
        let out = declare_local(sess, key, "ref", span)?;
        let ref_elem = em_key_elem(sess, key);
        let inner_key = sub_key(sess, ctx.3, ctx.4, em_expr_ty(sess, inner));
        // The array-to-slice coercion, for `&` and `&mut` alike: the
        // borrow becomes a `{ ptr, len }` view over the array's storage.
        if em_key_kind(sess, ref_elem) == TYD_SLICE && em_key_kind(sess, inner_key) == TYD_ARRAY {
            let d = slice_gep(sess, out, 0, "")?;
            store_key(sess, d, inner_ptr.into())?;
            let l = slice_gep(sess, out, 1, "")?;
            let len = sess.0.i64_type().const_int(key_len_of(sess, inner_key) as u64, false);
            store_key(sess, l, len.into())?;
            return Ok(out);
        }
        if em_key_kind(sess, ref_elem) == TYD_SLICE && em_key_kind(sess, inner_key) != TYD_SLICE {
            return Err(builder_error(
                node_file(sess.5, expr),
                node_start(sess.5, expr),
                node_end(sess.5, expr),
                "internal: slice-view borrow of a non-slice, non-array operand",
            ));
        }
        store_key(sess, out, inner_ptr.into())?;
        return Ok(out);
    }
    let ikey = sub_key(sess, ctx.3, ctx.4, em_expr_ty(sess, inner));
    let iptr = emit_expr(sess, ctx, inner)?;
    let iv = load_key(sess, ikey, iptr, span)?.into_int_value();
    let out = declare_local(sess, key, "un", span)?;
    // A negated literal adopts the expected width (`val x: I8 = -1` types
    // the literal as I64 and the negation as I8), so the operand's key can
    // be wider or narrower than the result key.  Coerce the operand to the
    // result width first: a castless store of a differently sized value
    // would write past the result slot and corrupt the frame.
    let coerced = coerce_int(sess, ikey, iv, key, span)?;
    if op == UN_NEG {
        let r = sess.2.build_int_neg(coerced, "").map_err(builder_fail)?;
        store_key(sess, out, r.into())?;
        return Ok(out);
    }
    let r = sess.2.build_not(coerced, "").map_err(builder_fail)?;
    store_key(sess, out, r.into())?;
    Ok(out)
}

// Recasts an integer value from its own width to `to_key`'s width: widening
// sign-extends a signed source and zero-extends an unsigned one, narrowing
// truncates, equal widths are a no-op.  The single place an integer value is
// width-coerced, so every store matches the width of the slot it targets.
fn coerce_int<'ctx>(
    sess: &mut Session<'ctx, '_, '_>,
    from_key: i64,
    v: IntValue<'ctx>,
    to_key: i64,
    span: (i64, i64, i64),
) -> Result<IntValue<'ctx>, CodegenError> {
    let ret_ty = llvm_of(sess, to_key, span)?.into_int_type();
    let src_ty = v.get_type();
    if src_ty.get_bit_width() < ret_ty.get_bit_width() {
        if key_is_signed(sess, from_key) {
            sess.2.build_int_s_extend(v, ret_ty, "").map_err(builder_fail)
        } else {
            sess.2.build_int_z_extend(v, ret_ty, "").map_err(builder_fail)
        }
    } else if src_ty.get_bit_width() > ret_ty.get_bit_width() {
        sess.2.build_int_truncate(v, ret_ty, "").map_err(builder_fail)
    } else {
        Ok(v)
    }
}

fn key_is_signed(sess: &Session, key: i64) -> bool {
    em_key_kind(sess, key) == TYD_BUILTIN && builtin_int_is_signed(key_builtin_of(sess, key))
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

// Loads a binary operator's operand as the scalar the operator needs.
//
// The typechecker already rejects an aggregate operand (arithmetic
// requires integers, comparison requires integers or `Bool`), so reaching
// here with one means an earlier stage let something through. That is an
// internal error with a real source span, not a Rust panic: `into_int_value`
// aborts the process on a mismatch, which would take the compiler down with
// a backtrace and no diagnostic, in a codebase whose rule is that codegen
// failures are never a panic.
fn load_scalar_operand<'ctx>(
    sess: &mut Session<'ctx, '_, '_>,
    key: i64,
    ptr: PointerValue<'ctx>,
    op: i64,
    span: (i64, i64, i64),
) -> Result<IntValue<'ctx>, CodegenError> {
    let value = load_key(sess, key, ptr, span)?;
    match value {
        BasicValueEnum::IntValue(int) => Ok(int),
        BasicValueEnum::ArrayValue(other) => Err(non_scalar_operand(op, other.get_type().to_string(), span)),
        BasicValueEnum::FloatValue(other) => Err(non_scalar_operand(op, other.get_type().to_string(), span)),
        BasicValueEnum::PointerValue(other) => Err(non_scalar_operand(op, other.get_type().to_string(), span)),
        BasicValueEnum::StructValue(other) => Err(non_scalar_operand(op, other.get_type().to_string(), span)),
        BasicValueEnum::VectorValue(other) => Err(non_scalar_operand(op, other.get_type().to_string(), span)),
        BasicValueEnum::ScalableVectorValue(other) => Err(non_scalar_operand(op, other.get_type().to_string(), span)),
    }
}

fn non_scalar_operand(op: i64, lowered: String, span: (i64, i64, i64)) -> CodegenError {
    builder_error(
        span.0,
        span.1,
        span.2,
        &format!("internal: binary operator '{}' reached codegen with a non-scalar operand lowered as {}", op_text(op), lowered),
    )
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
    let span = (node_file(sess.5, expr), node_start(sess.5, expr), node_end(sess.5, expr));
    let lv = load_scalar_operand(sess, lkey, lptr, op, span)?;
    let rv = load_scalar_operand(sess, rkey, rptr, op, span)?;
    let out = declare_local(sess, result_key, "bin", span)?;
    let r;
    if op == BIN_DIV || op == BIN_MOD {
        emit_div_rem_result(sess, ctx, (op, lkey, result_key), (lv, rv), out, span)?;
        return Ok(out);
    } else if op == BIN_ADD {
        r = sess.2.build_int_add(lv, rv, "").map_err(builder_fail)?;
    } else if op == BIN_SUB {
        r = sess.2.build_int_sub(lv, rv, "").map_err(builder_fail)?;
    } else if op == BIN_MUL {
        r = sess.2.build_int_mul(lv, rv, "").map_err(builder_fail)?;
    } else if op == BIN_SHL || op == BIN_SHR {
        // A shift amount >= the operand bit width is poison in LLVM, so it
        // is masked by bit_width - 1 (7 for i8, 31 for i32, 63 for i64),
        // matching the constant folder's wrapping_shl/wrapping_shr mask.
        let rhs_ty = rv.get_type();
        let width = rhs_ty.get_bit_width();
        let mask_const = rhs_ty.const_int((width - 1) as u64, false);
        let rmasked = sess.2.build_and(rv, mask_const, "").map_err(builder_fail)?;
        if op == BIN_SHL {
            r = sess.2.build_left_shift(lv, rmasked, "").map_err(builder_fail)?;
        } else if key_is_signed(sess, lkey) {
            r = sess.2.build_right_shift(lv, rmasked, true, "").map_err(builder_fail)?
        } else {
            r = sess.2.build_right_shift(lv, rmasked, false, "").map_err(builder_fail)?
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

fn emit_div_rem_result<'ctx>(
    sess: &mut Session<'ctx, '_, '_>,
    ctx: &mut FnCtx<'ctx, '_>,
    desc: (i64, i64, i64),
    operands: (IntValue<'ctx>, IntValue<'ctx>),
    out: PointerValue<'ctx>,
    span: (i64, i64, i64),
) -> Result<(), CodegenError> {
    let (op, lkey, result_key) = desc;
    let (lv, rv) = operands;
    // Division constants are typed from the operand's own LLVM type so the
    // width-typed compares and stores agree at every width (U8, I16, ...).
    let zero = lv.get_type().const_zero();
    let is_zero = sess.2.build_int_compare(IntPredicate::EQ, rv, zero, "").map_err(builder_fail)?;
    let ok_block = new_block(sess, ctx.0, "div_ok");
    let err_block = new_block(sess, ctx.0, "div_err");
    let merge_block = new_block(sess, ctx.0, "div_merge");
    sess.2.build_conditional_branch(is_zero, err_block, ok_block).map_err(builder_fail)?;
    sess.2.position_at_end(err_block);
    let err_key = result_arg_key(sess, result_key, 1);
    let err_tag = BUILTIN_DIV_ERROR_DIV_BY_ZERO;
    let div_error = declare_local(sess, err_key, "div_err_val", span)?;
    build_enum_value_into(sess, err_key, err_tag, &[], div_error, span)?;
    build_enum_value_into(sess, result_key, BUILTIN_RESULT_ERR, &[(err_key, div_error)], out, span)?;
    sess.2.build_unconditional_branch(merge_block).map_err(builder_fail)?;
    sess.2.position_at_end(ok_block);
    let signed = key_is_signed(sess, lkey);
    let quotient = declare_local(sess, lkey, "quo", span)?;
    if signed {
        let neg_one = lv.get_type().const_int(u64::MAX, false);
        let is_neg1 = sess.2.build_int_compare(IntPredicate::EQ, rv, neg_one, "").map_err(builder_fail)?;
        let neg1_block = new_block(sess, ctx.0, "div_neg1");
        let gen_block = new_block(sess, ctx.0, "div_gen");
        let after_block = new_block(sess, ctx.0, "div_after");
        sess.2.build_conditional_branch(is_neg1, neg1_block, gen_block).map_err(builder_fail)?;
        sess.2.position_at_end(neg1_block);
        let q_neg1 = sess.2.build_int_neg(lv, "").map_err(builder_fail)?;
        let neg1_val = if op == BIN_DIV { q_neg1 } else { zero };
        store_key(sess, quotient, neg1_val.into())?;
        sess.2.build_unconditional_branch(after_block).map_err(builder_fail)?;
        sess.2.position_at_end(gen_block);
        let q = if op == BIN_DIV {
            sess.2.build_int_signed_div(lv, rv, "").map_err(builder_fail)?
        } else {
            sess.2.build_int_signed_rem(lv, rv, "").map_err(builder_fail)?
        };
        let rem = if op == BIN_DIV {
            sess.2.build_int_signed_rem(lv, rv, "").map_err(builder_fail)?
        } else {
            q
        };
        let neg = sess.2.build_int_compare(IntPredicate::SLT, rem, zero, "").map_err(builder_fail)?;
        let b_neg = sess.2.build_int_compare(IntPredicate::SLT, rv, zero, "").map_err(builder_fail)?;
        let neg_rv = sess.2.build_int_neg(rv, "").map_err(builder_fail)?;
        let abs_b = sess.2.build_select(b_neg, neg_rv, rv, "").map_err(builder_fail)?.into_int_value();
        let rem_adj = sess.2.build_int_add(rem, abs_b, "").map_err(builder_fail)?;
        let one = lv.get_type().const_int(1, false);
        let q_plus = sess.2.build_int_add(q, one, "").map_err(builder_fail)?;
        let q_minus = sess.2.build_int_sub(q, one, "").map_err(builder_fail)?;
        let q_step = sess.2.build_select(b_neg, q_plus, q_minus, "").map_err(builder_fail)?.into_int_value();
        let q_adj = sess.2.build_select(neg, q_step, q, "").map_err(builder_fail)?.into_int_value();
        let rem_final = sess.2.build_select(neg, rem_adj, rem, "").map_err(builder_fail)?.into_int_value();
        let gen_val = if op == BIN_DIV { q_adj } else { rem_final };
        store_key(sess, quotient, gen_val.into())?;
        sess.2.build_unconditional_branch(after_block).map_err(builder_fail)?;
        sess.2.position_at_end(after_block);
    } else {
        let q = if op == BIN_DIV {
            sess.2.build_int_unsigned_div(lv, rv, "").map_err(builder_fail)?
        } else {
            sess.2.build_int_unsigned_rem(lv, rv, "").map_err(builder_fail)?
        };
        store_key(sess, quotient, q.into())?;
    }
    build_enum_value_into(sess, result_key, BUILTIN_RESULT_OK, &[(lkey, quotient)], out, span)?;
    sess.2.build_unconditional_branch(merge_block).map_err(builder_fail)?;
    sess.2.position_at_end(merge_block);
    Ok(())
}

fn emit_array<'ctx, 'a>(
    sess: &mut Session<'ctx, '_, '_>,
    ctx: &mut FnCtx<'ctx, 'a>,
    expr: i64,
) -> Result<PointerValue<'ctx>, CodegenError> {
    let span = (node_file(sess.5, expr), node_start(sess.5, expr), node_end(sess.5, expr));
    let key = sub_key(sess, ctx.3, ctx.4, em_expr_ty(sess, expr));
    let ptr = declare_local(sess, key, "arr", span)?;
    let elems = node_b(sess.5, expr);
    let count = list_len(sess.6, elems);
    let elem_key = em_key_elem(sess, key);
    let mut idx = 0i64;
    while idx < count {
        let array_ty = llvm_of(sess, key, span)?;
        let eptr = offset_array_elem_ptr(sess, array_ty, ptr, sess.0.i64_type().const_int(idx as u64, false))?;
        let vptr = emit_expr(sess, ctx, list_get(sess.6, elems, idx))?;
        copy_value(sess, elem_key, eptr, vptr, span)?;
        idx += 1;
    }
    Ok(ptr)
}

fn emit_struct_lit<'ctx, 'a>(
    sess: &mut Session<'ctx, '_, '_>,
    ctx: &mut FnCtx<'ctx, 'a>,
    expr: i64,
) -> Result<PointerValue<'ctx>, CodegenError> {
    let span = (node_file(sess.5, expr), node_start(sess.5, expr), node_end(sess.5, expr));
    let sym = em_expr_sym(sess, expr);
    if sym == NONE {
        return Err(builder_error(span.0, span.1, span.2, "internal: struct literal without a symbol"));
    }
    let key = sub_key(sess, ctx.3, ctx.4, em_expr_ty(sess, expr));
    let kind = node_a(sess.5, sym);
    if kind == SYM_STRUCT {
        let ptr = declare_local(sess, key, "struct", span)?;
        let names = node_c(sess.5, expr);
        let values = node_d(sess.5, expr);
        let count = list_len(sess.6, names);
        let mut idx = 0i64;
        while idx < count {
            let name = list_get(sess.6, names, idx);
            let row = struct_field_fact_row(sess, key, name, span)?;
            let fkey = fieldkey_key_of(sess.5, row);
            let fld_idx = fieldkey_idx_of(sess.5, row);
            let fptr = struct_gep(sess, key, ptr, fld_idx as u32, "", span)?;
            let vptr = emit_expr(sess, ctx, list_get(sess.6, values, idx))?;
            copy_value(sess, fkey, fptr, vptr, span)?;
            idx += 1;
        }
        return Ok(ptr);
    }
    if kind == SYM_VARIANT {
        let idx = variant_index_of(sess, sym, span)?;
        let values = node_d(sess.5, expr);
        let count = list_len(sess.6, values);
        let mut payloads: Vec<(i64, PointerValue<'ctx>)> = Vec::new();
        let mut i = 0i64;
        while i < count {
            let pkey = variant_payload_key(sess, key, idx, i, span)?;
            let pptr = emit_expr(sess, ctx, list_get(sess.6, values, i))?;
            payloads.push((pkey, pptr));
            i += 1;
        }
        return build_enum_value(sess, key, idx, &payloads, span);
    }
    Err(builder_error(span.0, span.1, span.2, "internal: cannot construct this symbol"))
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
    let span = (node_file(sess.5, expr), node_start(sess.5, expr), node_end(sess.5, expr));
    let result_alloca = declare_local(sess, result_key, "match", span)?;
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
        None => return Err(builder_error(span.0, span.1, span.2, "internal: match without arms")),
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

fn emit_arm_body<'ctx, 'a>(
    sess: &mut Session<'ctx, '_, '_>,
    ctx: &mut FnCtx<'ctx, 'a>,
    body: i64,
    continuation: MatchCont<'ctx>,
) -> Result<(), CodegenError> {
    let span = (node_file(sess.5, body), node_start(sess.5, body), node_end(sess.5, body));
    let (result_key, result_alloca, merge) = continuation;
    let (val_ptr, diverged) = emit_stmt(sess, ctx, body)?;
    if !diverged {
        copy_value(sess, result_key, result_alloca, val_ptr, span)?;
        sess.2.build_unconditional_branch(merge).map_err(builder_fail)?;
    }
    Ok(())
}

fn emit_pattern<'ctx, 'a>(
    sess: &mut Session<'ctx, '_, '_>,
    ctx: &mut FnCtx<'ctx, 'a>,
    pat: i64,
    scrut: MatchScrut<'ctx>,
    body: i64,
    continuation: MatchCont<'ctx>,
) -> Result<(), CodegenError> {
    let span = (node_file(sess.5, pat), node_start(sess.5, pat), node_end(sess.5, pat));
    let (pat_key, scrut_ptr, fail_block) = scrut;
    let kind = node_a(sess.5, pat);
    if kind == PAT_BIND {
        let name = node_b(sess.5, pat);
        let key = sub_key(sess, ctx.3, ctx.4, pat_key);
        let ptr = declare_local(sess, key, &em_name(sess, name), span)?;
        copy_value(sess, key, ptr, scrut_ptr, span)?;
        bind_local(&mut ctx.1, name, key, ptr);
        if body != NONE {
            return emit_arm_body(sess, ctx, body, continuation);
        }
        return Ok(());
    }
    if kind == PAT_LIT {
        let lit_key = sub_key(sess, ctx.3, ctx.4, pat_ty_of(sess.5, pat));
        let lit_value = node_c(sess.5, pat);
        let cv = const_int_of(sess, lit_key, lit_value, span)?;
        let sv = load_key(sess, pat_key, scrut_ptr, span)?.into_int_value();
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
        let idx = variant_index_of(sess, sym, span)?;
        let cont = new_block(sess, ctx.0, "pat");
        let tag_ptr = struct_gep(sess, pat_key, scrut_ptr, 0, "", span)?;
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
        let idx = variant_index_of(sess, sym, span)?;
        let cont = new_block(sess, ctx.0, "pat");
        let tag_ptr = struct_gep(sess, pat_key, scrut_ptr, 0, "", span)?;
        let tag = load_i64(sess, tag_ptr)?;
        let want = sess.0.i64_type().const_int(idx as u64, false);
        let cmp = sess.2.build_int_compare(IntPredicate::EQ, tag, want, "").map_err(builder_fail)?;
        sess.2.build_conditional_branch(cmp, cont, fail_block).map_err(builder_fail)?;
        sess.2.position_at_end(cont);
        let (region, pty) = enum_payload_ptr(sess, scrut_ptr, pat_key, idx, span)?;
        let payload_pats = node_c(sess.5, pat);
        let pcount = list_len(sess.6, payload_pats);
        let mut i = 0i64;
        while i < pcount {
            let fkey = variant_payload_key(sess, pat_key, idx, i, span)?;
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
    let elem_ty = llvm_of(sess, elem_key, span)?;
    let inner_ty = llvm_of(sess, inner_key, span)?;
    let mut i = 0i64;
    while i < fixed {
        let eptr = if inner_kind == TYD_SLICE {
            offset_buffer_elem_ptr(sess, elem_ty, data, sess.0.i64_type().const_int(i as u64, false))?
        } else {
            offset_array_elem_ptr(sess, inner_ty, data, sess.0.i64_type().const_int(i as u64, false))?
        };
        let subpat = list_get(sess.6, elems, i);
        let sub_scrut: MatchScrut<'ctx> = (elem_key, eptr, fail_block);
        emit_pattern(sess, ctx, subpat, sub_scrut, NONE, continuation)?;
        i += 1;
    }
    if rest != NONE {
        let rest_key = sub_key(sess, ctx.3, ctx.4, pat_rest_key_of(sess.5, pat));
        let rptr = declare_local(sess, rest_key, "rest", span)?;
        let rdata = slice_gep(sess, rptr, 0, "")?;
        let rest_base = if inner_kind == TYD_SLICE {
            offset_buffer_elem_ptr(sess, elem_ty, data, sess.0.i64_type().const_int(fixed as u64, false))?
        } else {
            offset_array_elem_ptr(sess, inner_ty, data, sess.0.i64_type().const_int(fixed as u64, false))?
        };
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
    let span = (node_file(sess.5, expr), node_start(sess.5, expr), node_end(sess.5, expr));
    let inner = node_b(sess.5, expr);
    let inner_key = sub_key(sess, ctx.3, ctx.4, em_expr_ty(sess, inner));
    let result_key = sub_key(sess, ctx.3, ctx.4, em_expr_ty(sess, expr));
    let ret_key = sub_key(sess, ctx.3, ctx.4, ctx.5);
    let inner_ptr = emit_expr(sess, ctx, inner)?;
    let inner_sym = em_key_sym(sess, inner_key);
    let is_result = sym_prim_kind(sess.5, inner_sym) == PRIM_RESULT;
    let ok_tag = if is_result { BUILTIN_RESULT_OK } else { BUILTIN_OPTION_SOME };
    let err_tag = if is_result { BUILTIN_RESULT_ERR } else { BUILTIN_OPTION_NONE };
    let ret_sym = em_key_sym(sess, ret_key);
    let ret_is_result = sym_prim_kind(sess.5, ret_sym) == PRIM_RESULT;
    let ret_err_tag = if ret_is_result { BUILTIN_RESULT_ERR } else { BUILTIN_OPTION_NONE };
    let err_block = new_block(sess, ctx.0, "try_err");
    let ok_block = new_block(sess, ctx.0, "try_ok");
    let tag_ptr = struct_gep(sess, inner_key, inner_ptr, 0, "", span)?;
    let tag = load_i64(sess, tag_ptr)?;
    let want = sess.0.i64_type().const_int(err_tag as u64, false);
    let cmp = sess.2.build_int_compare(IntPredicate::EQ, tag, want, "").map_err(builder_fail)?;
    sess.2.build_conditional_branch(cmp, err_block, ok_block).map_err(builder_fail)?;
    sess.2.position_at_end(err_block);
    let ret_alloca = declare_local(sess, ret_key, "try_err", span)?;
    let rtag_ptr = struct_gep(sess, ret_key, ret_alloca, 0, "", span)?;
    let rtag = sess.0.i64_type().const_int(ret_err_tag as u64, false);
    store_key(sess, rtag_ptr, rtag.into())?;
    if variant_payload_count(sess, inner_key, err_tag) > 0 {
        let (inner_region, inner_pty) = enum_payload_ptr(sess, inner_ptr, inner_key, err_tag, span)?;
        let inner_payload = sess.2.build_struct_gep(inner_pty, inner_region, 0, "").map_err(builder_fail)?;
        let err_payload_key = variant_payload_key(sess, inner_key, err_tag, 0, span)?;
        let (ret_region, ret_pty) = enum_payload_ptr(sess, ret_alloca, ret_key, ret_err_tag, span)?;
        let ret_payload = sess.2.build_struct_gep(ret_pty, ret_region, 0, "").map_err(builder_fail)?;
        copy_value(sess, err_payload_key, ret_payload, inner_payload, span)?;
    }
    let loaded = load_key(sess, ret_key, ret_alloca, span)?;
    sess.2.build_return(Some(&loaded)).map_err(builder_fail)?;
    sess.2.position_at_end(ok_block);
    let (ok_region, ok_pty) = enum_payload_ptr(sess, inner_ptr, inner_key, ok_tag, span)?;
    let ok_payload = sess.2.build_struct_gep(ok_pty, ok_region, 0, "").map_err(builder_fail)?;
    let out = declare_local(sess, result_key, "try_ok", span)?;
    copy_value(sess, result_key, out, ok_payload, span)?;
    Ok(out)
}

fn emit_index<'ctx, 'a>(
    sess: &mut Session<'ctx, '_, '_>,
    ctx: &mut FnCtx<'ctx, 'a>,
    expr: i64,
) -> Result<PointerValue<'ctx>, CodegenError> {
    let base = node_b(sess.5, expr);
    let index = node_c(sess.5, expr);
    let key = sub_key(sess, ctx.3, ctx.4, em_expr_ty(sess, expr));
    let base_key = sub_key(sess, ctx.3, ctx.4, em_expr_ty(sess, base));
    let base_ptr = emit_expr(sess, ctx, base)?;
    let span = (node_file(sess.5, expr), node_start(sess.5, expr), node_end(sess.5, expr));
    let idx_key = sub_key(sess, ctx.3, ctx.4, em_expr_ty(sess, index));
    let idx_ptr = emit_expr(sess, ctx, index)?;
    let idx_val = load_key(sess, idx_key, idx_ptr, span)?.into_int_value();

    let base_kind = em_key_kind(sess, base_key);
    let elem_key;
    let len_val;
    let data_ptr;
    if base_kind == TYD_ARRAY {
        elem_key = em_key_elem(sess, base_key);
        data_ptr = base_ptr;
        len_val = sess.0.i64_type().const_int(key_len_of(sess, base_key) as u64, false);
    } else {
        let inner_key = em_key_elem(sess, base_key);
        elem_key = if em_key_kind(sess, inner_key) == TYD_SLICE {
            em_key_elem(sess, inner_key)
        } else {
            NONE
        };
        if elem_key == NONE {
            return Err(builder_error(
                node_file(sess.5, expr),
                node_start(sess.5, expr),
                node_end(sess.5, expr),
                "internal: indexed value is not an array or slice",
            ));
        }
        data_ptr = slice_data(sess, base_ptr)?;
        len_val = slice_len_of(sess, base_ptr)?;
    }

    // The typechecker attaches an explicit fallibility fact to the index
    // node: INDEX_FALLIBLE for runtime and slice indices (whose result it
    // types as Result(T, IndexError)), INDEX_INFALLIBLE for a constant array
    // index proven in range (whose result is the bare element type).  The
    // element type itself cannot be the signal: an array of Result elements
    // indexed by a constant has a Result-typed result that is still
    // infallible.  Reading the attached fact keeps codegen from re-deriving
    // a decision the typechecker already made.
    if node_d(sess.5, expr) != INDEX_FALLIBLE {
        let base_ty = llvm_of(sess, base_key, span)?;
        let elem_ty = llvm_of(sess, elem_key, span)?;
        let eptr = if base_kind == TYD_ARRAY {
            offset_array_elem_ptr(sess, base_ty, data_ptr, idx_val)?
        } else {
            offset_buffer_elem_ptr(sess, elem_ty, data_ptr, idx_val)?
        };
        return Ok(eptr);
    }
    let is_oob = sess.2.build_int_compare(IntPredicate::UGE, idx_val, len_val, "").map_err(builder_fail)?;
    let ok_block = new_block(sess, ctx.0, "idx_ok");
    let err_block = new_block(sess, ctx.0, "idx_err");
    let merge = new_block(sess, ctx.0, "idx_merge");
    sess.2.build_conditional_branch(is_oob, err_block, ok_block).map_err(builder_fail)?;

    let payload_key = result_arg_key(sess, key, 0);
    let err_key = result_arg_key(sess, key, 1);
    let out = declare_local(sess, key, "idx", span)?;
    sess.2.position_at_end(err_block);
    let oob_tag = BUILTIN_INDEX_ERROR_INDEX_OOB;
    let f0 = variant_payload_key(sess, err_key, oob_tag, 0, span)?;
    let f1 = variant_payload_key(sess, err_key, oob_tag, 1, span)?;
    let e0 = declare_local(sess, f0, "iob_idx", span)?;
    store_key(sess, e0, idx_val.into())?;
    let e1 = declare_local(sess, f1, "iob_len", span)?;
    store_key(sess, e1, len_val.into())?;
    let oob_val = build_enum_value(sess, err_key, oob_tag, &[(f0, e0), (f1, e1)], span)?;
    build_enum_value_into(sess, key, BUILTIN_RESULT_ERR, &[(err_key, oob_val)], out, span)?;
    sess.2.build_unconditional_branch(merge).map_err(builder_fail)?;
    sess.2.position_at_end(ok_block);
    let base_ty = llvm_of(sess, base_key, span)?;
    let elem_ty = llvm_of(sess, elem_key, span)?;
    let eptr = if base_kind == TYD_ARRAY {
        offset_array_elem_ptr(sess, base_ty, data_ptr, idx_val)?
    } else {
        offset_buffer_elem_ptr(sess, elem_ty, data_ptr, idx_val)?
    };
    let payload_kind = em_key_kind(sess, payload_key);
    if payload_kind == TYD_REF || payload_kind == TYD_REF_MUT {
        let ref_slot = declare_local(sess, payload_key, "idx_ref", span)?;
        store_key(sess, ref_slot, eptr.into())?;
        build_enum_value_into(sess, key, BUILTIN_RESULT_OK, &[(payload_key, ref_slot)], out, span)?;
    } else {
        build_enum_value_into(sess, key, BUILTIN_RESULT_OK, &[(payload_key, eptr)], out, span)?;
    }
    sess.2.build_unconditional_branch(merge).map_err(builder_fail)?;
    sess.2.position_at_end(merge);
    Ok(out)
}

fn into_meta<'ctx>(value: BasicValueEnum<'ctx>) -> BasicMetadataValueEnum<'ctx> {
    value.into()
}

fn emit_call<'ctx, 'a>(
    sess: &mut Session<'ctx, '_, '_>,
    ctx: &mut FnCtx<'ctx, 'a>,
    expr: i64,
) -> Result<PointerValue<'ctx>, CodegenError> {
    let span = (node_file(sess.5, expr), node_start(sess.5, expr), node_end(sess.5, expr));
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
    if fn_slot == ctx.8 && mono == ctx.9 {
        if callfact_tail_of(sess.5, expr) == 1 && callfact_tail_safe_of(sess.5, expr) == 1 {
            return emit_self_tail_call(sess, ctx, expr, ret_key, span);
        }
        return Err(builder_error(span.0, span.1, span.2, "internal: self-recursive call without frontend tail certification"));
    }
    let caller_block = sess.2.get_insert_block();
    let fn_val = get_or_emit_fn(sess, fn_slot, args_list, mono, params_list, ret_key)?;
    match caller_block {
        Some(block) => sess.2.position_at_end(block),
        None => return Err(builder_error(span.0, span.1, span.2, "internal: no insertion block")),
    }
    let arg_vals = emit_call_args(sess, ctx, expr, params_list, false)?;
    let call = sess.2.build_call(fn_val, &arg_vals, "").map_err(builder_fail)?;
    let out = declare_local(sess, ret_key, "call", span)?;
    let ret_val = match call.try_as_basic_value() {
        ValueKind::Basic(bv) => bv,
        ValueKind::Instruction(inst) => {
            return Err(builder_error(
                span.0,
                span.1,
                span.2,
                &format!("internal: void return from a call ({:?})", inst.get_opcode()),
            ));
        }
    };
    store_key(sess, out, ret_val)?;
    Ok(out)
}

// A self-tail call becomes a loop jump: each argument is staged into an
// entry-block alloca via `copy_value`, copied into the parameter slots,
// then control branches to `body_header`.
fn emit_self_tail_call<'ctx, 'a>(
    sess: &mut Session<'ctx, '_, '_>,
    ctx: &mut FnCtx<'ctx, 'a>,
    expr: i64,
    ret_key: i64,
    span: (i64, i64, i64),
) -> Result<PointerValue<'ctx>, CodegenError> {
    let arg_exprs = node_d(sess.5, expr);
    let count = list_len(sess.6, arg_exprs);
    let mut staged: Vec<(i64, PointerValue<'ctx>)> = Vec::new();
    let mut idx = 0i64;
    while idx < count {
        let arg = list_get(sess.6, arg_exprs, idx);
        let akey = sub_key(sess, ctx.3, ctx.4, em_expr_ty(sess, arg));
        let ptr = emit_expr(sess, ctx, arg)?;
        let temp = declare_local(sess, akey, "arg", span)?;
        copy_value(sess, akey, temp, ptr, span)?;
        staged.push((akey, temp));
        idx += 1;
    }
    let mut idx = 0i64;
    while idx < count {
        let slot = match ctx.7.get(idx as usize) {
            Some(slot) => *slot,
            None => {
                return Err(builder_error(
                    span.0,
                    span.1,
                    span.2,
                    "internal: self-tail call has more arguments than parameters",
                ));
            }
        };
        let (akey, temp) = match staged.get(idx as usize) {
            Some(pair) => *pair,
            None => break,
        };
        copy_value(sess, akey, slot, temp, span)?;
        idx += 1;
    }
    let out = declare_local(sess, ret_key, "tail", span)?;
    sess.2.build_unconditional_branch(ctx.6).map_err(builder_fail)?;
    Ok(out)
}

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
    let span = (node_file(sess.5, expr), node_start(sess.5, expr), node_end(sess.5, expr));
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
    if fn_node == ctx.8 && mono == ctx.9 {
        if callfact_tail_of(sess.5, expr) == 1 && callfact_tail_safe_of(sess.5, expr) == 1 {
            return emit_self_tail_call(sess, ctx, expr, result, span);
        }
        return Err(builder_error(span.0, span.1, span.2, "internal: self-recursive trait call without frontend tail certification"));
    }
    let caller_block = sess.2.get_insert_block();
    let fn_val = get_or_emit_fn(sess, fn_node, NONE, mono, param_keys, result)?;
    match caller_block {
        Some(block) => sess.2.position_at_end(block),
        None => return Err(builder_error(span.0, span.1, span.2, "internal: no insertion block")),
    }
    let arg_vals = emit_call_args(sess, ctx, expr, param_keys, false)?;
    let call = sess.2.build_call(fn_val, &arg_vals, "").map_err(builder_fail)?;
    let out = declare_local(sess, result, "call", span)?;
    let ret_val = match call.try_as_basic_value() {
        ValueKind::Basic(bv) => bv,
        ValueKind::Instruction(instr) => {
            return Err(builder_error(
                span.0,
                span.1,
                span.2,
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
    params_list: i64,
    use_instance_params: bool,
) -> Result<Vec<BasicMetadataValueEnum<'ctx>>, CodegenError> {
    let span = (node_file(sess.5, expr), node_start(sess.5, expr), node_end(sess.5, expr));
    let arg_exprs = node_d(sess.5, expr);
    let count = list_len(sess.6, arg_exprs);
    let mut vals: Vec<BasicMetadataValueEnum<'ctx>> = Vec::new();
    let mut idx = 0i64;
    while idx < count {
        let arg = list_get(sess.6, arg_exprs, idx);
        let declared_key = list_get(sess.6, params_list, idx);
        let akey = if use_instance_params {
            sub_key(sess, ctx.3, ctx.4, declared_key)
        } else {
            sub_key(sess, ctx.3, ctx.4, em_expr_ty(sess, arg))
        };
        let ptr = emit_expr(sess, ctx, arg)?;
        let value = load_key(sess, akey, ptr, span)?;
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

fn build_fn_sig<'ctx>(sess: &mut Session<'ctx, '_, '_>, params_list: i64, ret_key: i64, span: (i64, i64, i64)) -> Result<FunctionType<'ctx>, CodegenError> {
    let count = list_len(sess.6, params_list);
    let mut param_tys: Vec<BasicMetadataTypeEnum<'ctx>> = Vec::new();
    let mut idx = 0i64;
    while idx < count {
        let pkey = list_get(sess.6, params_list, idx);
        param_tys.push(llvm_of(sess, pkey, span)?.into());
        idx += 1;
    }
    let ret_ty = llvm_of(sess, ret_key, span)?;
    Ok(ret_ty.fn_type(&param_tys, false))
}

fn get_or_emit_fn<'ctx>(
    sess: &mut Session<'ctx, '_, '_>,
    fn_slot: i64,
    args_list: i64,
    mono: i64,
    params_list: i64,
    ret_key: i64,
) -> Result<FunctionValue<'ctx>, CodegenError> {
    let span = (node_file(sess.5, fn_slot), node_start(sess.5, fn_slot), node_end(sess.5, fn_slot));
    if let Some(existing) = find_inst_fn(sess, mono) {
        return Ok(existing);
    }
    let fn_name = em_name(sess, node_a(sess.5, fn_slot));
    let llvm_name = format!("{}_{}", fn_name, mono);
    let sig = build_fn_sig(sess, params_list, ret_key, span)?;
    let fn_val = sess.1.add_function(&llvm_name, sig, None);
    sess.10.push((mono, fn_val));
    let from = fn_declared_param_keys(sess, fn_slot);
    let to = list_to_vec_of(sess, args_list);
    let param_decls = node_c(sess.5, fn_slot);
    let body = node_f(sess.5, fn_slot);
    // The `entry` block stores the incoming arguments into the parameter
    // slots once; the `body_header` block is the jump target of self-tail
    // calls, which overwrite the same slots before branching back.
    let entry = sess.0.append_basic_block(fn_val, "entry");
    sess.2.position_at_end(entry);
    let mut body_locals: Locals<'ctx> = Vec::new();
    let fn_loops: LoopTargets<'ctx> = Vec::new();
    let param_values = fn_val.get_params();
    let pcount = list_len(sess.6, params_list);
    let mut param_slots: Vec<PointerValue<'ctx>> = Vec::new();
    let mut idx = 0i64;
    while idx < pcount {
        let pkey = list_get(sess.6, params_list, idx);
        let pname = node_a(sess.5, list_get(sess.6, param_decls, idx));
        let ptr = declare_local(sess, pkey, &em_name(sess, pname), span)?;
        let pval = match param_values.get(idx as usize) {
            Some(value) => *value,
            None => break,
        };
        store_key(sess, ptr, pval)?;
        bind_local(&mut body_locals, pname, pkey, ptr);
        param_slots.push(ptr);
        idx += 1;
    }
    let body_header = sess.0.append_basic_block(fn_val, "body_header");
    sess.2.build_unconditional_branch(body_header).map_err(builder_fail)?;
    sess.2.position_at_end(body_header);
    let mut ctx: FnCtx<'ctx, '_> = (fn_val, body_locals, fn_loops, from.as_slice(), to.as_slice(), ret_key, body_header, param_slots, fn_slot, mono);
    let body_result = emit_stmt_list(sess, &mut ctx, body, ret_key, span);
    if let Err(err) = body_result {
        return Err(with_fn_span(err, fn_slot, sess));
    }
    if !block_terminated(sess) {
        let ret_kind = em_key_kind(sess, ret_key);
        if ret_kind == TYD_ENUM {
            let slot = declare_local(sess, ret_key, "fall", span)?;
            build_unit_value_into(sess, ret_key, slot, span)?;
            let loaded = load_key(sess, ret_key, slot, span)?;
            sess.2.build_return(Some(&loaded)).map_err(builder_fail)?;
        } else {
            sess.2.build_unreachable().map_err(builder_fail)?;
        }
    }
    Ok(fn_val)
}

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

fn extern_mmap<'ctx>(sess: &mut Session<'ctx, '_, '_>) -> FunctionValue<'ctx> {
    let i32_ty = sess.0.i32_type();
    extern_fn(sess, sess.13.abi().memory_map, ptr_ty(sess).fn_type(&[ptr_ty(sess).into(), sess.0.i64_type().into(), i32_ty.into(), i32_ty.into(), i32_ty.into(), sess.0.i64_type().into()], false))
}

fn extern_munmap<'ctx>(sess: &mut Session<'ctx, '_, '_>) -> FunctionValue<'ctx> {
    extern_fn(sess, sess.13.abi().memory_release, sess.0.i32_type().fn_type(&[ptr_ty(sess).into(), sess.0.i64_type().into()], false))
}

fn extern_open<'ctx>(sess: &mut Session<'ctx, '_, '_>) -> FunctionValue<'ctx> {
    let i32_ty = sess.0.i32_type();
    extern_fn(sess, sess.13.abi().file_open, i32_ty.fn_type(&[ptr_ty(sess).into(), i32_ty.into(), i32_ty.into()], false))
}

fn extern_read<'ctx>(sess: &mut Session<'ctx, '_, '_>) -> FunctionValue<'ctx> {
    let result = if sess.13.abi().io_result_is_32 { sess.0.i32_type() } else { sess.0.i64_type() };
    extern_fn(sess, "read", result.fn_type(&[sess.0.i32_type().into(), ptr_ty(sess).into(), sess.0.i64_type().into()], false))
}

fn extern_write<'ctx>(sess: &mut Session<'ctx, '_, '_>) -> FunctionValue<'ctx> {
    let result = if sess.13.abi().io_result_is_32 { sess.0.i32_type() } else { sess.0.i64_type() };
    extern_fn(sess, "write", result.fn_type(&[sess.0.i32_type().into(), ptr_ty(sess).into(), sess.0.i64_type().into()], false))
}

fn extern_socket<'ctx>(sess: &mut Session<'ctx, '_, '_>) -> FunctionValue<'ctx> {
    let i32_ty = sess.0.i32_type();
    let result = if sess.13.abi().socket_handle_is_64 { sess.0.i64_type() } else { i32_ty };
    extern_fn(
        sess,
        sess.13.abi().socket_create,
        result.fn_type(&[i32_ty.into(), i32_ty.into(), i32_ty.into()], false),
    )
}

fn extern_bind<'ctx>(sess: &mut Session<'ctx, '_, '_>) -> FunctionValue<'ctx> {
    let i32_ty = sess.0.i32_type();
    let socket_ty = if sess.13.abi().socket_handle_is_64 { sess.0.i64_type() } else { i32_ty };
    extern_fn(
        sess,
        sess.13.abi().socket_bind,
        i32_ty.fn_type(&[socket_ty.into(), ptr_ty(sess).into(), i32_ty.into()], false),
    )
}

fn extern_listen<'ctx>(sess: &mut Session<'ctx, '_, '_>) -> FunctionValue<'ctx> {
    let i32_ty = sess.0.i32_type();
    let socket_ty = if sess.13.abi().socket_handle_is_64 { sess.0.i64_type() } else { i32_ty };
    extern_fn(sess, sess.13.abi().socket_listen, i32_ty.fn_type(&[socket_ty.into(), i32_ty.into()], false))
}

fn extern_accept<'ctx>(sess: &mut Session<'ctx, '_, '_>) -> FunctionValue<'ctx> {
    let i32_ty = sess.0.i32_type();
    let socket_ty = if sess.13.abi().socket_handle_is_64 { sess.0.i64_type() } else { i32_ty };
    let result = if sess.13.abi().socket_handle_is_64 { sess.0.i64_type() } else { i32_ty };
    extern_fn(
        sess,
        sess.13.abi().socket_accept,
        result.fn_type(&[socket_ty.into(), ptr_ty(sess).into(), ptr_ty(sess).into()], false),
    )
}

fn extern_send<'ctx>(sess: &mut Session<'ctx, '_, '_>) -> FunctionValue<'ctx> {
    let i32_ty = sess.0.i32_type();
    let socket_ty = if sess.13.abi().socket_handle_is_64 { sess.0.i64_type() } else { i32_ty };
    let result = if sess.13.abi().io_result_is_32 { i32_ty } else { sess.0.i64_type() };
    extern_fn(
        sess,
        sess.13.abi().socket_send,
        result.fn_type(&[socket_ty.into(), ptr_ty(sess).into(), sess.0.i64_type().into(), i32_ty.into()], false),
    )
}

fn extern_close<'ctx>(sess: &mut Session<'ctx, '_, '_>) -> FunctionValue<'ctx> {
    let i32_ty = sess.0.i32_type();
    extern_fn(sess, sess.13.abi().file_close, i32_ty.fn_type(&[i32_ty.into()], false))
}

fn extern_wsa_startup<'ctx>(sess: &mut Session<'ctx, '_, '_>) -> FunctionValue<'ctx> {
    extern_fn(sess, "WSAStartup", sess.0.i32_type().fn_type(&[sess.0.i16_type().into(), ptr_ty(sess).into()], false))
}

fn extern_socket_close<'ctx>(sess: &mut Session<'ctx, '_, '_>) -> FunctionValue<'ctx> {
    let abi = sess.13.abi();
    let handle = if abi.socket_close_is_64 { sess.0.i64_type() } else { sess.0.i32_type() };
    extern_fn(sess, abi.socket_close, sess.0.i32_type().fn_type(&[handle.into()], false))
}

fn extern_fork<'ctx>(sess: &mut Session<'ctx, '_, '_>) -> FunctionValue<'ctx> {
    extern_fn(sess, "fork", sess.0.i32_type().fn_type(&[], false))
}

fn extern_execvp<'ctx>(sess: &mut Session<'ctx, '_, '_>) -> FunctionValue<'ctx> {
    let i8p = ptr_ty(sess);
    extern_fn(sess, "execvp", sess.0.i32_type().fn_type(&[i8p.into(), i8p.into()], false))
}

fn extern_waitpid<'ctx>(sess: &mut Session<'ctx, '_, '_>) -> FunctionValue<'ctx> {
    let i32_ty = sess.0.i32_type();
    extern_fn(sess, "waitpid", i32_ty.fn_type(&[i32_ty.into(), ptr_ty(sess).into(), i32_ty.into()], false))
}

fn extern_exit<'ctx>(sess: &mut Session<'ctx, '_, '_>) -> FunctionValue<'ctx> {
    extern_fn(sess, "_exit", sess.0.void_type().fn_type(&[sess.0.i32_type().into()], false))
}

fn socket_argument<'ctx>(sess: &mut Session<'ctx, '_, '_>, fd: IntValue<'ctx>) -> Result<BasicMetadataValueEnum<'ctx>, CodegenError> {
    if sess.13.abi().socket_handle_is_64 { return Ok(into_meta(fd.into())); }
    let fd32 = sess.2.build_int_truncate(fd, sess.0.i32_type(), "").map_err(builder_fail)?;
    Ok(into_meta(fd32.into()))
}

fn socket_result<'ctx>(sess: &mut Session<'ctx, '_, '_>, call: inkwell::values::CallSiteValue<'ctx>, span: (i64, i64, i64)) -> Result<IntValue<'ctx>, CodegenError> {
    let value = match call.try_as_basic_value() {
        ValueKind::Basic(value) => value.into_int_value(),
        ValueKind::Instruction(inst) => return Err(builder_error(span.0, span.1, span.2, &format!("internal: socket call returned void ({:?})", inst.get_opcode()))),
    };
    if value.get_type().get_bit_width() == 64 { return Ok(value); }
    sess.2.build_int_s_extend(value, sess.0.i64_type(), "").map_err(builder_fail)
}

fn libc_io_result<'ctx>(sess: &mut Session<'ctx, '_, '_>, call: inkwell::values::CallSiteValue<'ctx>, span: (i64, i64, i64)) -> Result<IntValue<'ctx>, CodegenError> {
    let value = match call.try_as_basic_value() {
        ValueKind::Basic(value) => value.into_int_value(),
        ValueKind::Instruction(inst) => return Err(builder_error(span.0, span.1, span.2, &format!("internal: C I/O call returned void ({:?})", inst.get_opcode()))),
    };
    if value.get_type().get_bit_width() == 64 { return Ok(value); }
    sess.2.build_int_s_extend(value, sess.0.i64_type(), "").map_err(builder_fail)
}

fn emit_native_call<'ctx, 'a>(
    sess: &mut Session<'ctx, '_, '_>,
    ctx: &mut FnCtx<'ctx, 'a>,
    expr: i64,
    inst: i64,
    sym: i64,
) -> Result<PointerValue<'ctx>, CodegenError> {
    let span = (node_file(sess.5, expr), node_start(sess.5, expr), node_end(sess.5, expr));
    let params_list = inst_params_of(sess.5, inst);
    let ret_key = sub_key(sess, ctx.3, ctx.4, inst_ret_of(sess.5, inst));
    let mono = inst_mono_of(sess.5, inst);
    let name = em_name(sess, node_b(sess.5, sym));
    let native_val = match find_inst_fn(sess, mono) {
        Some(existing) => existing,
        None => {
            let caller_block = match sess.2.get_insert_block() {
                Some(block) => block,
                None => return Err(builder_error(span.0, span.1, span.2, "internal: no insertion block")),
            };
            let sig = build_fn_sig(sess, params_list, ret_key, span)?;
            let llvm_name = format!("{}_{}", name.replace('.', "_"), mono);
            let fn_val = sess.1.add_function(&llvm_name, sig, None);
            sess.10.push((mono, fn_val));
            emit_native_body(sess, sym, params_list, ret_key, fn_val)?;
            sess.2.position_at_end(caller_block);
            fn_val
        }
    };
    let arg_vals = emit_call_args(sess, ctx, expr, params_list, false)?;
    let call = sess.2.build_call(native_val, &arg_vals, "").map_err(builder_fail)?;
    let out = declare_local(sess, ret_key, "call", span)?;
    let ret_val = match call.try_as_basic_value() {
        ValueKind::Basic(bv) => bv,
        ValueKind::Instruction(inst) => {
            return Err(builder_error(
                span.0,
                span.1,
                span.2,
                &format!("internal: void return from a native call ({:?})", inst.get_opcode()),
            ));
        }
    };
    store_key(sess, out, ret_val)?;
    Ok(out)
}

fn emit_native_body<'ctx>(
    sess: &mut Session<'ctx, '_, '_>,
    sym: i64,
    params_list: i64,
    ret_key: i64,
    fn_val: FunctionValue<'ctx>,
) -> Result<(), CodegenError> {
    let decl = em_sym_decl(sess, sym);
    let span = (node_file(sess.5, decl), node_start(sess.5, decl), node_end(sess.5, decl));
    let entry = sess.0.append_basic_block(fn_val, "entry");
    sess.2.position_at_end(entry);
    let mut body_locals: Locals<'ctx> = Vec::new();
    let param_values = fn_val.get_params();
    let pcount = list_len(sess.6, params_list);
    let mut idx = 0i64;
    while idx < pcount {
        let pkey = list_get(sess.6, params_list, idx);
        let pval = match param_values.get(idx as usize) {
            Some(value) => *value,
            None => break,
        };
        let ptr = declare_local(sess, pkey, "p", span)?;
        store_key(sess, ptr, pval)?;
        bind_local(&mut body_locals, idx, pkey, ptr);
        idx += 1;
    }
    let out = dispatch_native(sess, &mut body_locals, fn_val, sym, params_list, ret_key, span)?;
    let loaded = load_key(sess, ret_key, out, span)?;
    sess.2.build_return(Some(&loaded)).map_err(builder_fail)?;
    Ok(())
}

fn native_arg_key(sess: &Session, params_list: i64, idx: i64) -> i64 {
    list_get(sess.6, params_list, idx)
}

fn build_result_ok<'ctx>(sess: &mut Session<'ctx, '_, '_>, result_key: i64, payload_key: i64, payload_ptr: PointerValue<'ctx>, span: (i64, i64, i64)) -> Result<PointerValue<'ctx>, CodegenError> {
    build_enum_value(sess, result_key, BUILTIN_RESULT_OK, &[(payload_key, payload_ptr)], span)
}

fn build_result_err<'ctx>(sess: &mut Session<'ctx, '_, '_>, result_key: i64, payload_key: i64, payload_ptr: PointerValue<'ctx>, span: (i64, i64, i64)) -> Result<PointerValue<'ctx>, CodegenError> {
    build_enum_value(sess, result_key, BUILTIN_RESULT_ERR, &[(payload_key, payload_ptr)], span)
}

fn build_unit_value<'ctx>(sess: &mut Session<'ctx, '_, '_>, unit_key: i64, span: (i64, i64, i64)) -> Result<PointerValue<'ctx>, CodegenError> {
    let ptr = declare_local(sess, unit_key, "unit", span)?;
    build_unit_value_into(sess, unit_key, ptr, span)?;
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

fn copy_to_out<'ctx>(sess: &mut Session<'ctx, '_, '_>, key: i64, out: PointerValue<'ctx>, src: PointerValue<'ctx>, span: (i64, i64, i64)) -> Result<(), CodegenError> {
    copy_value(sess, key, out, src, span)
}

fn dispatch_native<'ctx>(
    sess: &mut Session<'ctx, '_, '_>,
    locals: &mut Locals<'ctx>,
    f: FunctionValue<'ctx>,
    sym: i64,
    params_list: i64,
    ret_key: i64,
    span: (i64, i64, i64),
) -> Result<PointerValue<'ctx>, CodegenError> {
    let out = declare_local(sess, ret_key, "ret", span)?;
    // Verb -> handler dispatch; a verb with no arm here is a compiler bug.
    match sym_native_op(sess.5, sym) {
        NAT_INT_FROM => {
            native_int_from(sess, locals, ret_key, out, span)?;
            Ok(out)
        }
        NAT_SLICE_LEN => {
            native_slice_len(sess, locals, out, span)?;
            Ok(out)
        }
        NAT_MEM_ALLOCATE => native_allocate(sess, f, locals, ret_key, out, span),
        NAT_MEM_DEALLOCATE => {
            native_deallocate(sess, locals, ret_key, out, span)?;
            Ok(out)
        }
        NAT_MEM_WRITE_U8 => {
            native_write_u8(sess, f, locals, ret_key, out, span)?;
            Ok(out)
        }
        NAT_MEM_READ_U8 => {
            native_read_u8(sess, f, locals, ret_key, out, span)?;
            Ok(out)
        }
        NAT_VEC_NEW => {
            native_vec_new(sess, ret_key, out, span)?;
            Ok(out)
        }
        NAT_VEC_PUSH => native_vec_push(sess, f, locals, params_list, ret_key, out, span),
        NAT_SLICE_VIEW => {
            native_slice_view(sess, locals, out, span)?;
            Ok(out)
        }
        NAT_VEC_FREE => {
            native_vec_free(sess, locals, ret_key, out, span)?;
            Ok(out)
        }
        NAT_VEC_POP => native_vec_pop(sess, f, locals, ret_key, out, span),
        NAT_STRING_FROM_SLICE => native_string_from_slice(sess, f, locals, ret_key, out, span),
        NAT_STRING_LEN => {
            native_string_len(sess, locals, out, span)?;
            Ok(out)
        }
        NAT_STRING_FREE => {
            native_string_free(sess, locals, ret_key, out, span)?;
            Ok(out)
        }
        NAT_HASH_MAP_NEW => {
            native_hash_map_new(sess, ret_key, out, span)?;
            Ok(out)
        }
        NAT_HASH_MAP_INSERT => native_hash_map_insert(sess, f, locals, params_list, ret_key, out, span),
        NAT_HASH_MAP_GET => native_hash_map_get(sess, f, locals, params_list, ret_key, out, span),
        NAT_HASH_MAP_FREE => {
            native_hash_map_free(sess, locals, ret_key, out, span)?;
            Ok(out)
        }
        NAT_HASH_MAP_REMOVE => native_hash_map_remove(sess, f, locals, params_list, ret_key, out, span),
        NAT_FILE_OPEN => native_file_open(sess, f, locals, ret_key, out, span),
        NAT_FILE_READ => native_file_transfer(sess, f, locals, false, ret_key, out, span),
        NAT_FILE_WRITE => native_file_transfer(sess, f, locals, true, ret_key, out, span),
        NAT_FILE_CLOSE => {
            native_file_close(sess, locals, ret_key, out, span)?;
            Ok(out)
        }
        NAT_TERM_READ_LINE => native_read_line(sess, f, ret_key, out, span),
        NAT_RUNTIME_ARGS => {
            native_runtime_args(sess, f, ret_key, out, span)?;
            Ok(out)
        }
        NAT_SELF_CHECK => {
            native_self_check(sess, ret_key, out, span)?;
            Ok(out)
        }
        NAT_TERM_PRINT => {
            native_print(sess, locals, ret_key, out, false, false, span)?;
            Ok(out)
        }
        NAT_TERM_PRINT_LINE => {
            native_print(sess, locals, ret_key, out, false, true, span)?;
            Ok(out)
        }
        NAT_TERM_EPRINT => {
            native_print(sess, locals, ret_key, out, true, false, span)?;
            Ok(out)
        }
        NAT_NET_SOCKET => native_net_socket(sess, f, ret_key, out, span),
        NAT_NET_BIND => {
            native_net_bind(sess, f, locals, ret_key, out, span)?;
            Ok(out)
        }
        NAT_NET_LISTEN => {
            native_net_listen(sess, f, locals, ret_key, out, span)?;
            Ok(out)
        }
        NAT_NET_ACCEPT => native_net_accept(sess, f, locals, ret_key, out, span),
        NAT_NET_SEND => native_net_send(sess, f, locals, ret_key, out, span),
        NAT_NET_CLOSE => {
            native_net_close(sess, locals, ret_key, out, span)?;
            Ok(out)
        }
        NAT_PROCESS_SPAWN => native_process_spawn(sess, f, locals, ret_key, out, span),
        NAT_PROCESS_WAIT => native_process_wait(sess, f, locals, ret_key, out, span),
        other => Err(builder_error(
            span.0,
            span.1,
            span.2,
            &format!("internal: native verb {} has no codegen handler", other),
        )),
    }
}

fn native_int_from<'ctx>(sess: &mut Session<'ctx, '_, '_>, locals: &Locals<'ctx>, ret_key: i64, out: PointerValue<'ctx>, span: (i64, i64, i64)) -> Result<(), CodegenError> {
    let p0 = get_local(locals, 0, span)?;
    let k0 = get_local_key(locals, 0, span)?;
    let v = load_key(sess, k0, p0, span)?.into_int_value();
    let r = coerce_int(sess, k0, v, ret_key, span)?;
    store_key(sess, out, r.into())?;
    Ok(())
}

fn native_slice_len<'ctx>(sess: &mut Session<'ctx, '_, '_>, locals: &Locals<'ctx>, out: PointerValue<'ctx>, span: (i64, i64, i64)) -> Result<(), CodegenError> {
    let p0 = get_local(locals, 0, span)?;
    let len = slice_len_of(sess, p0)?;
    store_key(sess, out, len.into())?;
    Ok(())
}

fn native_allocate<'ctx>(sess: &mut Session<'ctx, '_, '_>, f: FunctionValue<'ctx>, locals: &Locals<'ctx>, ret_key: i64, out: PointerValue<'ctx>, span: (i64, i64, i64)) -> Result<PointerValue<'ctx>, CodegenError> {
    let p0 = get_local(locals, 0, span)?;
    let k0 = get_local_key(locals, 0, span)?;
    let size = load_key(sess, k0, p0, span)?.into_int_value();
    // `mmap` directly, not libc's allocator: `Memory` is the raw-memory
    // quarantine, and a `Block` already carries the length `munmap`
    // needs to give the mapping back, so nothing about the allocator's
    // bookkeeping is required here. `Collections` keeps using the libc
    // allocator, because a growable container genuinely needs `realloc`
    // semantics rather than whole mappings.
    //
    // An anonymous private mapping of `size` bytes, rounded up to a page
    // by the kernel. The requested length is what the handle records, so
    // the bounds checks in `write_u8`/`read_u8` still reject an offset
    // past what the program asked for, not merely past the page.
    let data = if !sess.13.abi().memory_uses_mapping {
        // The target ABI selects the heap fallback when anonymous mappings
        // are unavailable.
        let call = sess.2.build_call(extern_malloc(sess), &[into_meta(size.into())], "").map_err(builder_fail)?;
        match call.try_as_basic_value() {
            ValueKind::Basic(value) => value.into_pointer_value(),
            ValueKind::Instruction(inst) => return Err(builder_error(span.0, span.1, span.2, &format!("internal: malloc returned void ({:?})", inst.get_opcode()))),
        }
    } else {
        let abi = sess.13.abi();
        let call = sess.2.build_call(extern_mmap(sess), &[
            into_meta(ptr_ty(sess).const_null().into()), into_meta(size.into()),
            into_meta(sess.0.i32_type().const_int(abi.prot_read_write, false).into()),
            into_meta(sess.0.i32_type().const_int(abi.map_private_anonymous, false).into()),
            into_meta(sess.0.i32_type().const_all_ones().into()), into_meta(sess.0.i64_type().const_zero().into()),
        ], "").map_err(builder_fail)?;
        match call.try_as_basic_value() {
            ValueKind::Basic(value) => value.into_pointer_value(),
            ValueKind::Instruction(inst) => return Err(builder_error(span.0, span.1, span.2, &format!("internal: mmap returned void ({:?})", inst.get_opcode()))),
        }
    };
    let data_word = sess.2.build_ptr_to_int(data, sess.0.i64_type(), "").map_err(builder_fail)?;
    // malloc reports failure with NULL; mmap with MAP_FAILED (all-ones).
    let null_cmp = if !sess.13.abi().memory_uses_mapping {
        sess.2.build_int_compare(IntPredicate::EQ, data_word, sess.0.i64_type().const_zero(), "").map_err(builder_fail)?
    } else {
        sess.2.build_int_compare(IntPredicate::EQ, data_word, sess.0.i64_type().const_all_ones(), "").map_err(builder_fail)?
    };
    let fail_block = new_block(sess, f, "alloc_fail");
    let ok_block = new_block(sess, f, "alloc_ok");
    let after = new_block(sess, f, "alloc_after");
    sess.2.build_conditional_branch(null_cmp, fail_block, ok_block).map_err(builder_fail)?;
    sess.2.position_at_end(fail_block);
    let err_key = result_arg_key(sess, ret_key, 1);
    let alloc_fail_tag = seeded_enum_variant_tag(sess, SEED_SYM_ALLOC_FAILED, span)?;
    let fkey = variant_payload_key(sess, err_key, alloc_fail_tag, 0, span)?;
    let fail_val = build_enum_value(sess, err_key, alloc_fail_tag, &[(fkey, p0)], span)?;
    let err_result = build_result_err(sess, ret_key, err_key, fail_val, span)?;
    copy_to_out(sess, ret_key, out, err_result, span)?;
    sess.2.build_unconditional_branch(after).map_err(builder_fail)?;
    sess.2.position_at_end(ok_block);
    let block_key = result_arg_key(sess, ret_key, 0);
    let block_val = declare_local(sess, block_key, "block", span)?;
    let bd = struct_gep(sess, block_key, block_val, 0, "", span)?;
    store_key(sess, bd, data.into())?;
    let bl = struct_gep(sess, block_key, block_val, 1, "", span)?;
    store_key(sess, bl, size.into())?;
    let ok_result = build_result_ok(sess, ret_key, block_key, block_val, span)?;
    copy_to_out(sess, ret_key, out, ok_result, span)?;
    sess.2.build_unconditional_branch(after).map_err(builder_fail)?;
    sess.2.position_at_end(after);
    Ok(out)
}

fn native_deallocate<'ctx>(sess: &mut Session<'ctx, '_, '_>, locals: &Locals<'ctx>, ret_key: i64, out: PointerValue<'ctx>, span: (i64, i64, i64)) -> Result<(), CodegenError> {
    let p0 = get_local(locals, 0, span)?;
    let block_key = deref_key_of(sess, get_local_key(locals, 0, span)?);
    let bd = struct_gep(sess, block_key, p0, 0, "", span)?;
    let data = load_ptr(sess, bd)?;
    // The allocation representation carries the requested length, so the
    // POSIX mapping path can return exactly the region it acquired.
    let bl = struct_gep(sess, block_key, p0, 1, "", span)?;
    let len = load_i64(sess, bl)?;
    if !sess.13.abi().memory_uses_mapping {
        sess.2.build_call(extern_free(sess), &[into_meta(data.into())], "").map_err(builder_fail)?;
    } else {
        sess.2.build_call(extern_munmap(sess), &[into_meta(data.into()), into_meta(len.into())], "").map_err(builder_fail)?;
    }
    build_unit_value_into(sess, ret_key, out, span)?;
    Ok(())
}

fn native_write_u8<'ctx>(sess: &mut Session<'ctx, '_, '_>, f: FunctionValue<'ctx>, locals: &Locals<'ctx>, ret_key: i64, out: PointerValue<'ctx>, span: (i64, i64, i64)) -> Result<(), CodegenError> {
    let p0 = get_local(locals, 0, span)?;
    let p1 = get_local(locals, 1, span)?;
    let p2 = get_local(locals, 2, span)?;
    let block_key = deref_key_of(sess, get_local_key(locals, 0, span)?);
    let block_ref = load_ptr(sess, p0)?;
    let bd = struct_gep(sess, block_key, block_ref, 0, "", span)?;
    let data = load_ptr(sess, bd)?;
    let bl = struct_gep(sess, block_key, block_ref, 1, "", span)?;
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
    let oob_tag = seeded_enum_variant_tag(sess, SEED_SYM_ACCESS_OOB, span)?;
    let f0 = variant_payload_key(sess, err_key, oob_tag, 0, span)?;
    let f1 = variant_payload_key(sess, err_key, oob_tag, 1, span)?;
    let e0 = declare_local(sess, f0, "o0", span)?;
    copy_value(sess, f0, e0, p1, span)?;
    let e1 = declare_local(sess, f1, "o1", span)?;
    store_key(sess, e1, len.into())?;
    let fail_val = build_enum_value(sess, err_key, oob_tag, &[(f0, e0), (f1, e1)], span)?;
    let err_result = build_result_err(sess, ret_key, err_key, fail_val, span)?;
    copy_to_out(sess, ret_key, out, err_result, span)?;
    sess.2.build_unconditional_branch(after).map_err(builder_fail)?;
    sess.2.position_at_end(ok_block);
    let target = byte_elem_ptr(sess, data, offset)?;
    store_key(sess, target, value.into())?;
    let unit_key = result_arg_key(sess, ret_key, 0);
    let unit_val = build_unit_value(sess, unit_key, span)?;
    let ok_result = build_result_ok(sess, ret_key, unit_key, unit_val, span)?;
    copy_to_out(sess, ret_key, out, ok_result, span)?;
    sess.2.build_unconditional_branch(after).map_err(builder_fail)?;
    sess.2.position_at_end(after);
    Ok(())
}

fn native_read_u8<'ctx>(sess: &mut Session<'ctx, '_, '_>, f: FunctionValue<'ctx>, locals: &Locals<'ctx>, ret_key: i64, out: PointerValue<'ctx>, span: (i64, i64, i64)) -> Result<(), CodegenError> {
    let p0 = get_local(locals, 0, span)?;
    let p1 = get_local(locals, 1, span)?;
    let block_key = deref_key_of(sess, get_local_key(locals, 0, span)?);
    let block_ref = load_ptr(sess, p0)?;
    let bd = struct_gep(sess, block_key, block_ref, 0, "", span)?;
    let data = load_ptr(sess, bd)?;
    let bl = struct_gep(sess, block_key, block_ref, 1, "", span)?;
    let len = load_i64(sess, bl)?;
    let offset = load_i64(sess, p1)?;
    let ok_cmp = sess.2.build_int_compare(IntPredicate::ULT, offset, len, "").map_err(builder_fail)?;
    let fail_block = new_block(sess, f, "r_fail");
    let ok_block = new_block(sess, f, "r_ok");
    let after = new_block(sess, f, "r_after");
    sess.2.build_conditional_branch(ok_cmp, ok_block, fail_block).map_err(builder_fail)?;
    sess.2.position_at_end(fail_block);
    let err_key = result_arg_key(sess, ret_key, 1);
    let oob_tag = seeded_enum_variant_tag(sess, SEED_SYM_ACCESS_OOB, span)?;
    let f0 = variant_payload_key(sess, err_key, oob_tag, 0, span)?;
    let f1 = variant_payload_key(sess, err_key, oob_tag, 1, span)?;
    let e0 = declare_local(sess, f0, "o0", span)?;
    copy_value(sess, f0, e0, p1, span)?;
    let e1 = declare_local(sess, f1, "o1", span)?;
    store_key(sess, e1, len.into())?;
    let fail_val = build_enum_value(sess, err_key, oob_tag, &[(f0, e0), (f1, e1)], span)?;
    let err_result = build_result_err(sess, ret_key, err_key, fail_val, span)?;
    copy_to_out(sess, ret_key, out, err_result, span)?;
    sess.2.build_unconditional_branch(after).map_err(builder_fail)?;
    sess.2.position_at_end(ok_block);
    let target = byte_elem_ptr(sess, data, offset)?;
    let byte = load_i8(sess, target)?;
    let u8_key = result_arg_key(sess, ret_key, 0);
    let u8_val = declare_local(sess, u8_key, "byte", span)?;
    store_key(sess, u8_val, byte.into())?;
    let ok_result = build_result_ok(sess, ret_key, u8_key, u8_val, span)?;
    copy_to_out(sess, ret_key, out, ok_result, span)?;
    sess.2.build_unconditional_branch(after).map_err(builder_fail)?;
    sess.2.position_at_end(after);
    Ok(())
}

// Writes a fresh growable-container handle's three declared slots — data
// pointer (null), length (0), capacity (0) — one store each.  This is what
// `vec_new` and `hash_map_new` both mean, and it is deliberately the two
// explicit stores rather than a whole-layout zero store: a constructor must
// name every slot its layout declares so that a slot added to the layout
// later is a compile-time "which constructor forgets to write it" question,
// not a silently-zeroed field nobody realized existed.
fn init_container_slots<'ctx>(sess: &mut Session<'ctx, '_, '_>, key: i64, ptr: PointerValue<'ctx>, span: (i64, i64, i64)) -> Result<(), CodegenError> {
    let d = struct_gep(sess, key, ptr, 0, "", span)?;
    store_key(sess, d, ptr_ty(sess).const_null().into())?;
    let l = struct_gep(sess, key, ptr, 1, "", span)?;
    store_key(sess, l, sess.0.i64_type().const_zero().into())?;
    let c = struct_gep(sess, key, ptr, 2, "", span)?;
    store_key(sess, c, sess.0.i64_type().const_zero().into())?;
    Ok(())
}

fn native_vec_new<'ctx>(sess: &mut Session<'ctx, '_, '_>, ret_key: i64, out: PointerValue<'ctx>, span: (i64, i64, i64)) -> Result<(), CodegenError> {
    let vec_key = result_arg_key(sess, ret_key, 0);
    let vec_val = declare_local(sess, vec_key, "vec", span)?;
    init_container_slots(sess, vec_key, vec_val, span)?;
    let ok_result = build_result_ok(sess, ret_key, vec_key, vec_val, span)?;
    copy_to_out(sess, ret_key, out, ok_result, span)?;
    Ok(())
}

fn native_vec_push<'ctx>(sess: &mut Session<'ctx, '_, '_>, f: FunctionValue<'ctx>, locals: &Locals<'ctx>, params_list: i64, ret_key: i64, out: PointerValue<'ctx>, span: (i64, i64, i64)) -> Result<PointerValue<'ctx>, CodegenError> {
    let p0 = get_local(locals, 0, span)?;
    let p1 = get_local(locals, 1, span)?;
    let t_key = native_arg_key(sess, params_list, 1);
    let vec_key = deref_key_of(sess, get_local_key(locals, 0, span)?);
    let vec_ref = load_ptr(sess, p0)?;
    let dptr = struct_gep(sess, vec_key, vec_ref, 0, "", span)?;
    let lptr = struct_gep(sess, vec_key, vec_ref, 1, "", span)?;
    let cptr = struct_gep(sess, vec_key, vec_ref, 2, "", span)?;
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
    let esize = sess.3.get_abi_size(&llvm_of(sess, t_key, span)?);
    let stride = sess.0.i64_type().const_int(esize, false);
    let needed = sess.2.build_int_mul(newcap.into_int_value(), stride, "").map_err(builder_fail)?;
    let realloc = extern_realloc(sess);
    let call = sess.2.build_call(realloc, &[into_meta(old_data.into()), into_meta(needed.into())], "").map_err(builder_fail)?;
    let new_data = match call.try_as_basic_value() {
        ValueKind::Basic(bv) => bv.into_pointer_value(),
        ValueKind::Instruction(inst) => {
            return Err(builder_error(span.0, span.1, span.2, &format!("internal: realloc returned void ({:?})", inst.get_opcode())));
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
    let elem_ty = llvm_of(sess, t_key, span)?;
    let target = offset_buffer_elem_ptr(sess, elem_ty, data2, len2)?;
    copy_value(sess, t_key, target, p1, span)?;
    let one = sess.0.i64_type().const_int(1, false);
    let len3 = sess.2.build_int_add(len2, one, "").map_err(builder_fail)?;
    store_key(sess, lptr, len3.into())?;
    let unit_key = result_arg_key(sess, ret_key, 0);
    let unit_val = build_unit_value(sess, unit_key, span)?;
    let ok_result = build_result_ok(sess, ret_key, unit_key, unit_val, span)?;
    copy_to_out(sess, ret_key, out, ok_result, span)?;
    sess.2.build_unconditional_branch(after).map_err(builder_fail)?;
    sess.2.position_at_end(fail_block);
    let err_key = result_arg_key(sess, ret_key, 1);
    let alloc_fail_tag = seeded_enum_variant_tag(sess, SEED_SYM_ALLOC_FAILED, span)?;
    let fkey = variant_payload_key(sess, err_key, alloc_fail_tag, 0, span)?;
    let fval = declare_local(sess, fkey, "need", span)?;
    store_key(sess, fval, needed.into())?;
    let fail_val = build_enum_value(sess, err_key, alloc_fail_tag, &[(fkey, fval)], span)?;
    let err_result = build_result_err(sess, ret_key, err_key, fail_val, span)?;
    copy_to_out(sess, ret_key, out, err_result, span)?;
    sess.2.build_unconditional_branch(after).map_err(builder_fail)?;
    sess.2.position_at_end(after);
    Ok(out)
}

fn native_slice_view<'ctx>(sess: &mut Session<'ctx, '_, '_>, locals: &Locals<'ctx>, out: PointerValue<'ctx>, span: (i64, i64, i64)) -> Result<(), CodegenError> {
    let p0 = get_local(locals, 0, span)?;
    let handle_key = deref_key_of(sess, get_local_key(locals, 0, span)?);
    let handle_ref = load_ptr(sess, p0)?;
    let data_slot = struct_gep(sess, handle_key, handle_ref, 0, "", span)?;
    let data_ptr = load_ptr(sess, data_slot)?;
    let len_slot = struct_gep(sess, handle_key, handle_ref, 1, "", span)?;
    let len = load_i64(sess, len_slot)?;
    let out_data = slice_gep(sess, out, 0, "")?;
    store_key(sess, out_data, data_ptr.into())?;
    let out_len = slice_gep(sess, out, 1, "")?;
    store_key(sess, out_len, len.into())?;
    Ok(())
}

fn native_vec_free<'ctx>(sess: &mut Session<'ctx, '_, '_>, locals: &Locals<'ctx>, ret_key: i64, out: PointerValue<'ctx>, span: (i64, i64, i64)) -> Result<(), CodegenError> {
    let p0 = get_local(locals, 0, span)?;
    let vec_key = deref_key_of(sess, get_local_key(locals, 0, span)?);
    let d = struct_gep(sess, vec_key, p0, 0, "", span)?;
    let data = load_ptr(sess, d)?;
    let free = extern_free(sess);
    sess.2.build_call(free, &[into_meta(data.into())], "").map_err(builder_fail)?;
    build_unit_value_into(sess, ret_key, out, span)?;
    Ok(())
}

fn native_vec_pop<'ctx>(sess: &mut Session<'ctx, '_, '_>, f: FunctionValue<'ctx>, locals: &Locals<'ctx>, ret_key: i64, out: PointerValue<'ctx>, span: (i64, i64, i64)) -> Result<PointerValue<'ctx>, CodegenError> {
    let p0 = get_local(locals, 0, span)?;
    let t_key = result_arg_key(sess, ret_key, 0);
    let vec_key = deref_key_of(sess, get_local_key(locals, 0, span)?);
    let vec_ref = load_ptr(sess, p0)?;
    let dptr = struct_gep(sess, vec_key, vec_ref, 0, "", span)?;
    let lptr = struct_gep(sess, vec_key, vec_ref, 1, "", span)?;
    let len = load_i64(sess, lptr)?;
    let zero = sess.0.i64_type().const_zero();
    let empty = sess.2.build_int_compare(IntPredicate::EQ, len, zero, "").map_err(builder_fail)?;
    let empty_block = new_block(sess, f, "pop_empty");
    let ok_block = new_block(sess, f, "pop_ok");
    let after = new_block(sess, f, "pop_after");
    sess.2.build_conditional_branch(empty, empty_block, ok_block).map_err(builder_fail)?;
    sess.2.position_at_end(empty_block);
    let err_key = result_arg_key(sess, ret_key, 1);
    let oob_tag = seeded_enum_variant_tag(sess, SEED_SYM_COLLECTIONS_INDEX_OOB, span)?;
    let f0 = variant_payload_key(sess, err_key, oob_tag, 0, span)?;
    let f1 = variant_payload_key(sess, err_key, oob_tag, 1, span)?;
    let idx0 = declare_local(sess, f0, "oob_idx", span)?;
    store_key(sess, idx0, zero.into())?;
    let len0 = declare_local(sess, f1, "oob_len", span)?;
    store_key(sess, len0, zero.into())?;
    let fail_val = build_enum_value(sess, err_key, oob_tag, &[(f0, idx0), (f1, len0)], span)?;
    let err_result = build_result_err(sess, ret_key, err_key, fail_val, span)?;
    copy_to_out(sess, ret_key, out, err_result, span)?;
    sess.2.build_unconditional_branch(after).map_err(builder_fail)?;
    sess.2.position_at_end(ok_block);
    let data = load_ptr(sess, dptr)?;
    let one = sess.0.i64_type().const_int(1, false);
    let pop_idx = sess.2.build_int_sub(len, one, "").map_err(builder_fail)?;
    let elem_ty = llvm_of(sess, t_key, span)?;
    let elem_ptr = offset_buffer_elem_ptr(sess, elem_ty, data, pop_idx)?;
    let elem_val = declare_local(sess, t_key, "popped", span)?;
    copy_value(sess, t_key, elem_val, elem_ptr, span)?;
    store_key(sess, lptr, pop_idx.into())?;
    let ok_result = build_result_ok(sess, ret_key, t_key, elem_val, span)?;
    copy_to_out(sess, ret_key, out, ok_result, span)?;
    sess.2.build_unconditional_branch(after).map_err(builder_fail)?;
    sess.2.position_at_end(after);
    Ok(out)
}

fn native_string_len<'ctx>(sess: &mut Session<'ctx, '_, '_>, locals: &Locals<'ctx>, out: PointerValue<'ctx>, span: (i64, i64, i64)) -> Result<(), CodegenError> {
    let p0 = get_local(locals, 0, span)?;
    let str_key = deref_key_of(sess, get_local_key(locals, 0, span)?);
    let str_ref = load_ptr(sess, p0)?;
    let l = struct_gep(sess, str_key, str_ref, 1, "", span)?;
    let len = load_i64(sess, l)?;
    store_key(sess, out, len.into())?;
    Ok(())
}

// The (data pointer, byte length) pair of whichever byte-sequence view a
// native's first parameter names.
//
// The language has no overloading, so `Terminal.print` accepts either
// `&Collections.String` or `&[U8]` by having its *emitted body* read the
// pair out of whichever representation the program declared — never by
// matching the parameter's spelling. The two differ only in where the pair
// lives: a `&String` parameter is a pointer to a heap-owning handle whose
// fields must be loaded through it, while a `&[U8]` parameter *is* the
// `{ ptr, i64 }` view, held directly in the parameter slot. The decision
// comes from the canonical type descriptor the typechecker already
// attached to the parameter.
fn byte_view_of<'ctx>(sess: &mut Session<'ctx, '_, '_>, locals: &Locals<'ctx>, span: (i64, i64, i64)) -> Result<(PointerValue<'ctx>, IntValue<'ctx>), CodegenError> {
    let p0 = get_local(locals, 0, span)?;
    let viewed = deref_key_of(sess, get_local_key(locals, 0, span)?);
    if em_key_kind(sess, viewed) == TYD_SLICE {
        return Ok((slice_data(sess, p0)?, slice_len_of(sess, p0)?));
    }
    let handle = load_ptr(sess, p0)?;
    let data_ptr = struct_gep(sess, viewed, handle, 0, "", span)?;
    let data = load_ptr(sess, data_ptr)?;
    let len_ptr = struct_gep(sess, viewed, handle, 1, "", span)?;
    let len = load_i64(sess, len_ptr)?;
    Ok((data, len))
}

// A private constant global holding the single byte 0x0A, shared by every
// newline-terminated print.
fn newline_global<'ctx>(sess: &mut Session<'ctx, '_, '_>) -> PointerValue<'ctx> {
    let name = ".cnb.newline";
    match sess.1.get_global(name) {
        Some(existing) => existing.as_pointer_value(),
        None => {
            let byte = sess.0.i8_type().const_int(10, false);
            let created = sess.1.add_global(sess.0.i8_type(), Some(AddressSpace::from(0u16)), name);
            created.set_initializer(&byte);
            created.set_constant(true);
            created.set_linkage(inkwell::module::Linkage::Private);
            created.as_pointer_value()
        }
    }
}

fn native_print<'ctx>(
    sess: &mut Session<'ctx, '_, '_>,
    locals: &Locals<'ctx>,
    ret_key: i64,
    out: PointerValue<'ctx>,
    stderr: bool,
    newline: bool,
    span: (i64, i64, i64),
) -> Result<(), CodegenError> {
    // `write` is emitted directly, not through libc's wrapper: the kernel
    // entry point sits in the emitted IR for the Terminal surface, so
    // nothing sits between the Cinnabar declaration and the system call.
    let (data, len) = byte_view_of(sess, locals, span)?;
    let fd = sess.0.i64_type().const_int(if stderr { 2 } else { 1 }, false);
    write_all(sess, fd, data, len, span)?;
    if newline {
        let nl = newline_global(sess);
        let one = sess.0.i64_type().const_int(1, false);
        write_all(sess, fd, nl, one, span)?;
    }
    build_unit_value_into(sess, ret_key, out, span)?;
    Ok(())
}

// The out-of-line `write_all` runtime helper, emitted once per module with
// private linkage; retries a partial `write` with the cursor advanced, and
// retries unchanged on `EINTR`.
fn get_or_emit_write_all<'ctx>(sess: &mut Session<'ctx, '_, '_>, span: (i64, i64, i64)) -> Result<FunctionValue<'ctx>, CodegenError> {
    let name = ".cnb.write_all";
    if let Some(existing) = sess.1.get_function(name) {
        return Ok(existing);
    }
    let sig = sess.0.void_type().fn_type(
        &[sess.0.i64_type().into(), ptr_ty(sess).into(), sess.0.i64_type().into()],
        false,
    );
    let function = sess.1.add_function(name, sig, Some(inkwell::module::Linkage::Private));
    let caller_block = sess.2.get_insert_block();
    let entry = sess.0.append_basic_block(function, "entry");
    sess.2.position_at_end(entry);
    let i64_ty = sess.0.i64_type();
    let done_slot = alloca_raw(sess, i64_ty.into(), "written", span)?;
    store_key(sess, done_slot, i64_ty.const_zero().into())?;
    let fd = match function.get_nth_param(0) {
        Some(value) => value.into_int_value(),
        None => return Err(builder_error(span.0, span.1, span.2, "internal: write_all helper missing fd parameter")),
    };
    let data = match function.get_nth_param(1) {
        Some(value) => value.into_pointer_value(),
        None => return Err(builder_error(span.0, span.1, span.2, "internal: write_all helper missing data parameter")),
    };
    let len = match function.get_nth_param(2) {
        Some(value) => value.into_int_value(),
        None => return Err(builder_error(span.0, span.1, span.2, "internal: write_all helper missing len parameter")),
    };
    let cond = new_block(sess, function, "write_cond");
    let body = new_block(sess, function, "write_body");
    let after = new_block(sess, function, "write_done");
    sess.2.build_unconditional_branch(cond).map_err(builder_fail)?;
    sess.2.position_at_end(cond);
    let done = load_i64(sess, done_slot)?;
    let more = sess.2.build_int_compare(IntPredicate::ULT, done, len, "").map_err(builder_fail)?;
    sess.2.build_conditional_branch(more, body, after).map_err(builder_fail)?;
    sess.2.position_at_end(body);
    let cursor = byte_elem_ptr(sess, data, done)?;
    let remaining = sess.2.build_int_sub(len, done, "").map_err(builder_fail)?;
    let fd32 = sess.2.build_int_truncate(fd, sess.0.i32_type(), "").map_err(builder_fail)?;
    let call = sess.2.build_call(extern_write(sess), &[into_meta(fd32.into()), into_meta(cursor.into()), into_meta(remaining.into())], "").map_err(builder_fail)?;
    let wrote = libc_io_result(sess, call, span)?;
    let progressed = sess.2.build_int_compare(IntPredicate::SGT, wrote, i64_ty.const_zero(), "").map_err(builder_fail)?;
    let advance = new_block(sess, function, "write_advance");
    let retry = new_block(sess, function, "write_retry");
    let stopped = new_block(sess, function, "write_stopped");
    sess.2.build_conditional_branch(progressed, advance, stopped).map_err(builder_fail)?;
    sess.2.position_at_end(stopped);
    let failed = sess.2.build_int_compare(IntPredicate::EQ, wrote, i64_ty.const_all_ones(), "").map_err(builder_fail)?;
    let failure = new_block(sess, function, "write_failure");
    sess.2.build_conditional_branch(failed, failure, after).map_err(builder_fail)?;
    sess.2.position_at_end(failure);
    let interrupted = is_eintr(sess, wrote, span)?;
    sess.2.build_conditional_branch(interrupted, retry, after).map_err(builder_fail)?;
    sess.2.position_at_end(retry);
    sess.2.build_unconditional_branch(cond).map_err(builder_fail)?;
    sess.2.position_at_end(advance);
    let next = sess.2.build_int_add(done, wrote, "").map_err(builder_fail)?;
    store_key(sess, done_slot, next.into())?;
    sess.2.build_unconditional_branch(cond).map_err(builder_fail)?;
    sess.2.position_at_end(after);
    sess.2.build_return(None).map_err(builder_fail)?;
    if let Some(block) = caller_block {
        sess.2.position_at_end(block);
    }
    Ok(function)
}

// Dispatches the full write through the shared `.cnb.write_all` helper.
fn write_all<'ctx>(
    sess: &mut Session<'ctx, '_, '_>,
    fd: IntValue<'ctx>,
    data: PointerValue<'ctx>,
    len: IntValue<'ctx>,
    span: (i64, i64, i64),
) -> Result<(), CodegenError> {
    if sess.2.get_insert_block().is_none() {
        return Err(builder_error(span.0, span.1, span.2, "internal: write outside a function body"));
    }
    let helper = get_or_emit_write_all(sess, span)?;
    sess.2.build_call(helper, &[into_meta(fd.into()), into_meta(data.into()), into_meta(len.into())], "").map_err(builder_fail)?;
    Ok(())
}

fn native_string_free<'ctx>(sess: &mut Session<'ctx, '_, '_>, locals: &Locals<'ctx>, ret_key: i64, out: PointerValue<'ctx>, span: (i64, i64, i64)) -> Result<(), CodegenError> {
    let p0 = get_local(locals, 0, span)?;
    let str_key = deref_key_of(sess, get_local_key(locals, 0, span)?);
    let d = struct_gep(sess, str_key, p0, 0, "", span)?;
    let data = load_ptr(sess, d)?;
    let free = extern_free(sess);
    sess.2.build_call(free, &[into_meta(data.into())], "").map_err(builder_fail)?;
    build_unit_value_into(sess, ret_key, out, span)?;
    Ok(())
}

// === HashMap: structural keys over an open-addressed table ===
//
// A map is an open-addressed table of slots, each slot a `{ flag, key,
// value }` triple whose stride and field offsets derive from the slot's own
// LLVM struct type (declared K and V, never a hand-counted byte size). The
// flag marks a slot empty, occupied, or tombstoned (left by `remove` so a
// probe past a removed key still reaches later keys that collided behind
// it). Capacity is a power of two, so `(hash + step) & (capacity - 1)` maps
// a hash to a probe sequence, and the table grows by rehashing before the
// load factor reaches half.
//
// A key is hashed and compared *structurally*, field by field, not by its
// raw ABI bytes: two keys are equal exactly when their declared fields are,
// read recursively through nested structs, enums, arrays, and slice
// contents. Padding bytes are never read, so no construction-time zeroing
// is needed for correctness. The hash folds the same field values the
// comparison reads -- the property a probe relies on -- and both are
// emitted inline into the monomorphized native body, one implementation per
// concrete K. Bucketing appears only as the final `& (capacity - 1)` after
// hashing, never as a substitute for it.

const HASH_SEED: u64 = 14695981039346656037;
const HASH_PRIME: u64 = 1099511628211;
const SLOT_EMPTY: u64 = 0;
const SLOT_OCCUPIED: u64 = 1;
const SLOT_TOMBSTONE: u64 = 2;
const INITIAL_CAPACITY: u64 = 8;

// The LLVM type of one slot: a flag word, the key, and the value.
fn hash_map_slot_ty<'ctx>(sess: &mut Session<'ctx, '_, '_>, k_key: i64, v_key: i64, span: (i64, i64, i64)) -> Result<BasicTypeEnum<'ctx>, CodegenError> {
    let flag = sess.0.i64_type().into();
    let kty = llvm_of(sess, k_key, span)?;
    let vty = llvm_of(sess, v_key, span)?;
    Ok(sess.0.struct_type(&[flag, kty, vty], false).into())
}

// GEP to one field of a slot.
fn slot_gep<'ctx>(sess: &mut Session<'ctx, '_, '_>, slot_ty: BasicTypeEnum<'ctx>, slot_ptr: PointerValue<'ctx>, index: u32) -> Result<PointerValue<'ctx>, CodegenError> {
    sess.2.build_struct_gep(slot_ty, slot_ptr, index, "").map_err(builder_fail)
}

// The slot a probe step lands on: `(hash + step) & (capacity - 1)` indexes
// the slot array directly through the slot type.
fn probe_slot<'ctx>(sess: &mut Session<'ctx, '_, '_>, slot_ty: BasicTypeEnum<'ctx>, data: PointerValue<'ctx>, hash: IntValue<'ctx>, step: IntValue<'ctx>, cap: IntValue<'ctx>) -> Result<PointerValue<'ctx>, CodegenError> {
    let sum = sess.2.build_int_add(hash, step, "").map_err(builder_fail)?;
    let mask = sess.2.build_int_sub(cap, sess.0.i64_type().const_int(1, false), "").map_err(builder_fail)?;
    let index = sess.2.build_and(sum, mask, "").map_err(builder_fail)?;
    // `index` is masked to `cap - 1`, inside the caller's table allocation.
    offset_buffer_elem_ptr(sess, slot_ty, data, index)
}

// One FNV-1a fold: `state = (state ^ word) * prime`. A real bit mixer, not
// a raw field value or a byte-range checksum.
fn fold_hash_word<'ctx>(sess: &mut Session<'ctx, '_, '_>, hash_slot: PointerValue<'ctx>, word: IntValue<'ctx>) -> Result<(), CodegenError> {
    let current = load_i64(sess, hash_slot)?;
    let xored = sess.2.build_xor(current, word, "").map_err(builder_fail)?;
    let mixed = sess.2.build_int_mul(xored, sess.0.i64_type().const_int(HASH_PRIME, false), "").map_err(builder_fail)?;
    store_key(sess, hash_slot, mixed.into())?;
    Ok(())
}

// A scalar's bits as a full machine word. Zero-extension (never
// sign-extension) so two equal values of any width or signedness fold the
// same word.
fn scalar_word<'ctx>(sess: &mut Session<'ctx, '_, '_>, value: IntValue<'ctx>) -> Result<IntValue<'ctx>, CodegenError> {
    if value.get_type().get_bit_width() >= 64 {
        Ok(value)
    } else {
        sess.2.build_int_z_extend(value, sess.0.i64_type(), "").map_err(builder_fail)
    }
}

fn key_kind_label(kind: i64) -> &'static str {
    if kind == TYD_REF || kind == TYD_REF_MUT {
        "a reference"
    } else if kind == TYD_SLICE {
        "a slice"
    } else {
        "a native handle"
    }
}

fn no_structural_equality<'ctx>(sess: &Session<'ctx, '_, '_>, key: i64, span: (i64, i64, i64)) -> CodegenError {
    builder_error(
        span.0,
        span.1,
        span.2,
        &format!(
            "a HashMap key cannot contain {}: only scalar, struct, enum, array, and slice-of-value keys have structural equality",
            key_kind_label(key_kind_of(sess.5, key))
        ),
    )
}

// Structural key equality, emitted inline into the monomorphized native
// body: two keys are equal exactly when their declared fields are, read
// recursively through nested structs, enums, arrays, and slice contents.
// Padding bytes are never read, and a reference-typed field compares what
// it points to, not the address it holds.
fn emit_key_eq<'ctx>(sess: &mut Session<'ctx, '_, '_>, f: FunctionValue<'ctx>, key: i64, a: PointerValue<'ctx>, b: PointerValue<'ctx>, span: (i64, i64, i64)) -> Result<IntValue<'ctx>, CodegenError> {
    let kind = key_kind_of(sess.5, key);
    if kind == TYD_BUILTIN {
        let va = load_key(sess, key, a, span)?.into_int_value();
        let vb = load_key(sess, key, b, span)?.into_int_value();
        return sess.2.build_int_compare(IntPredicate::EQ, va, vb, "").map_err(builder_fail);
    }
    if kind == TYD_STRUCT {
        let rows = fieldkey_rows_of(sess.5, key);
        let mut acc = sess.0.bool_type().const_all_ones();
        let mut idx = 0usize;
        while idx < rows.len() {
            let fkey = match rows.get(idx) {
                Some(row) => row.1,
                None => break,
            };
            let fa = struct_gep(sess, key, a, idx as u32, "", span)?;
            let fb = struct_gep(sess, key, b, idx as u32, "", span)?;
            let feq = emit_key_eq(sess, f, fkey, fa, fb, span)?;
            acc = sess.2.build_and(acc, feq, "").map_err(builder_fail)?;
            idx += 1;
        }
        return Ok(acc);
    }
    if kind == TYD_ENUM {
        let ta_ptr = struct_gep(sess, key, a, 0, "", span)?;
        let ta = load_i64(sess, ta_ptr)?;
        let tb_ptr = struct_gep(sess, key, b, 0, "", span)?;
        let tb = load_i64(sess, tb_ptr)?;
        let tag_eq = sess.2.build_int_compare(IntPredicate::EQ, ta, tb, "").map_err(builder_fail)?;
        let result_slot = alloca_raw(sess, sess.0.bool_type().into(), "enum_eq", span)?;
        store_key(sess, result_slot, sess.0.bool_type().const_zero().into())?;
        let compare = new_block(sess, f, "enum_eq_compare");
        let done = new_block(sess, f, "enum_eq_done");
        sess.2.build_conditional_branch(tag_eq, compare, done).map_err(builder_fail)?;
        sess.2.position_at_end(compare);
        let item = em_sym_decl(sess, em_key_sym(sess, key));
        let variants = node_e(sess.5, item);
        let vcount = list_len(sess.6, variants);
        let mut vi = 0i64;
        while vi < vcount {
            let arm = new_block(sess, f, "enum_eq_arm");
            let next = if vi + 1 < vcount {
                new_block(sess, f, "enum_eq_next")
            } else {
                done
            };
            let is_vi = sess.2.build_int_compare(IntPredicate::EQ, ta, sess.0.i64_type().const_int(vi as u64, false), "").map_err(builder_fail)?;
            sess.2.build_conditional_branch(is_vi, arm, next).map_err(builder_fail)?;
            sess.2.position_at_end(arm);
            let mut variant_eq = sess.0.bool_type().const_all_ones();
            let pcount = variant_payload_count(sess, key, vi);
            if pcount > 0 {
                let (pa, pty) = enum_payload_ptr(sess, a, key, vi, span)?;
                let pb = enum_payload_ptr(sess, b, key, vi, span)?.0;
                let mut fi = 0i64;
                while fi < pcount {
                    let fkey = variant_payload_key(sess, key, vi, fi, span)?;
                    let fa = sess.2.build_struct_gep(pty, pa, fi as u32, "").map_err(builder_fail)?;
                    let fb = sess.2.build_struct_gep(pty, pb, fi as u32, "").map_err(builder_fail)?;
                    let feq = emit_key_eq(sess, f, fkey, fa, fb, span)?;
                    variant_eq = sess.2.build_and(variant_eq, feq, "").map_err(builder_fail)?;
                    fi += 1;
                }
            }
            store_key(sess, result_slot, variant_eq.into())?;
            sess.2.build_unconditional_branch(done).map_err(builder_fail)?;
            sess.2.position_at_end(next);
            vi += 1;
        }
        sess.2.position_at_end(done);
        let result = sess.2.build_load(sess.0.bool_type(), result_slot, "").map_err(builder_fail)?.into_int_value();
        return Ok(result);
    }
    if kind == TYD_ARRAY {
        let elem = key_elem_of(sess.5, key);
        let len = key_len_of(sess, key);
        let mut acc = sess.0.bool_type().const_all_ones();
        let mut idx = 0i64;
        while idx < len {
            let ival = sess.0.i64_type().const_int(idx as u64, false);
            let array_ty = llvm_of(sess, key, span)?;
            let ea = offset_array_elem_ptr(sess, array_ty, a, ival)?;
            let eb = offset_array_elem_ptr(sess, array_ty, b, ival)?;
            let feq = emit_key_eq(sess, f, elem, ea, eb, span)?;
            acc = sess.2.build_and(acc, feq, "").map_err(builder_fail)?;
            idx += 1;
        }
        return Ok(acc);
    }
    if kind == TYD_REF || kind == TYD_REF_MUT {
        let elem = key_elem_of(sess.5, key);
        if key_kind_of(sess.5, elem) == TYD_SLICE {
            return emit_slice_eq(sess, f, elem, a, b, span);
        }
        let aref = load_ptr(sess, a)?;
        let bref = load_ptr(sess, b)?;
        return emit_key_eq(sess, f, elem, aref, bref, span);
    }
    Err(no_structural_equality(sess, key, span))
}

// Content equality of two `&[T]` slice views: equal length, then every
// element equal, as a short-circuiting runtime loop. Two slices with equal
// bytes but different backing addresses therefore compare equal.
fn emit_slice_eq<'ctx>(sess: &mut Session<'ctx, '_, '_>, f: FunctionValue<'ctx>, slice_key: i64, a: PointerValue<'ctx>, b: PointerValue<'ctx>, span: (i64, i64, i64)) -> Result<IntValue<'ctx>, CodegenError> {
    let elem = key_elem_of(sess.5, slice_key);
    let a_data = slice_data(sess, a)?;
    let b_data = slice_data(sess, b)?;
    let a_len = slice_len_of(sess, a)?;
    let b_len = slice_len_of(sess, b)?;
    let len_eq = sess.2.build_int_compare(IntPredicate::EQ, a_len, b_len, "").map_err(builder_fail)?;
    let result_slot = alloca_raw(sess, sess.0.bool_type().into(), "slice_eq", span)?;
    store_key(sess, result_slot, len_eq.into())?;
    let i_slot = alloca_raw(sess, sess.0.i64_type().into(), "slice_i", span)?;
    store_key(sess, i_slot, sess.0.i64_type().const_zero().into())?;
    let cond = new_block(sess, f, "slice_eq_cond");
    let body = new_block(sess, f, "slice_eq_body");
    let mismatch = new_block(sess, f, "slice_eq_mismatch");
    let done = new_block(sess, f, "slice_eq_done");
    sess.2.build_conditional_branch(len_eq, cond, done).map_err(builder_fail)?;
    sess.2.position_at_end(cond);
    let i = load_i64(sess, i_slot)?;
    let more = sess.2.build_int_compare(IntPredicate::ULT, i, a_len, "").map_err(builder_fail)?;
    sess.2.build_conditional_branch(more, body, done).map_err(builder_fail)?;
    sess.2.position_at_end(body);
    let elem_ty = llvm_of(sess, elem, span)?;
    let ea = offset_buffer_elem_ptr(sess, elem_ty, a_data, i)?;
    let eb = offset_buffer_elem_ptr(sess, elem_ty, b_data, i)?;
    let elem_eq = emit_key_eq(sess, f, elem, ea, eb, span)?;
    let next = new_block(sess, f, "slice_eq_next");
    sess.2.build_conditional_branch(elem_eq, next, mismatch).map_err(builder_fail)?;
    sess.2.position_at_end(mismatch);
    store_key(sess, result_slot, sess.0.bool_type().const_zero().into())?;
    sess.2.build_unconditional_branch(done).map_err(builder_fail)?;
    sess.2.position_at_end(next);
    let i2 = sess.2.build_int_add(i, sess.0.i64_type().const_int(1, false), "").map_err(builder_fail)?;
    store_key(sess, i_slot, i2.into())?;
    sess.2.build_unconditional_branch(cond).map_err(builder_fail)?;
    sess.2.position_at_end(done);
    let result = sess.2.build_load(sess.0.bool_type(), result_slot, "").map_err(builder_fail)?.into_int_value();
    Ok(result)
}

// Folds a key's structural fields into `hash_slot`, so hash and equality
// read the same values and cannot disagree. An enum's tag is folded, then
// control dispatches on the tag so only the active variant's payload is
// hashed -- an inactive variant's bytes are uninitialized, and reading them
// as this variant's fields (a reference in particular) would dereference a
// garbage pointer.
fn emit_key_hash_into<'ctx>(sess: &mut Session<'ctx, '_, '_>, f: FunctionValue<'ctx>, key: i64, ptr: PointerValue<'ctx>, hash_slot: PointerValue<'ctx>, span: (i64, i64, i64)) -> Result<(), CodegenError> {
    let kind = key_kind_of(sess.5, key);
    if kind == TYD_BUILTIN {
        let value = load_key(sess, key, ptr, span)?.into_int_value();
        let word = scalar_word(sess, value)?;
        fold_hash_word(sess, hash_slot, word)?;
        return Ok(());
    }
    if kind == TYD_STRUCT {
        let rows = fieldkey_rows_of(sess.5, key);
        let mut idx = 0usize;
        while idx < rows.len() {
            let fkey = match rows.get(idx) {
                Some(row) => row.1,
                None => break,
            };
            let fptr = struct_gep(sess, key, ptr, idx as u32, "", span)?;
            emit_key_hash_into(sess, f, fkey, fptr, hash_slot, span)?;
            idx += 1;
        }
        return Ok(());
    }
    if kind == TYD_ENUM {
        let tag_ptr = struct_gep(sess, key, ptr, 0, "", span)?;
        let tag = load_i64(sess, tag_ptr)?;
        fold_hash_word(sess, hash_slot, tag)?;
        let item = em_sym_decl(sess, em_key_sym(sess, key));
        let variants = node_e(sess.5, item);
        let vcount = list_len(sess.6, variants);
        let done = new_block(sess, f, "enum_hash_done");
        let mut vi = 0i64;
        while vi < vcount {
            let arm = new_block(sess, f, "enum_hash_arm");
            let next = if vi + 1 < vcount {
                new_block(sess, f, "enum_hash_next")
            } else {
                done
            };
            let is_vi = sess.2.build_int_compare(IntPredicate::EQ, tag, sess.0.i64_type().const_int(vi as u64, false), "").map_err(builder_fail)?;
            sess.2.build_conditional_branch(is_vi, arm, next).map_err(builder_fail)?;
            sess.2.position_at_end(arm);
            let pcount = variant_payload_count(sess, key, vi);
            if pcount > 0 {
                let (region, pty) = enum_payload_ptr(sess, ptr, key, vi, span)?;
                let mut fi = 0i64;
                while fi < pcount {
                    let fkey = variant_payload_key(sess, key, vi, fi, span)?;
                    let fptr = sess.2.build_struct_gep(pty, region, fi as u32, "").map_err(builder_fail)?;
                    emit_key_hash_into(sess, f, fkey, fptr, hash_slot, span)?;
                    fi += 1;
                }
            }
            sess.2.build_unconditional_branch(done).map_err(builder_fail)?;
            sess.2.position_at_end(next);
            vi += 1;
        }
        sess.2.position_at_end(done);
        return Ok(());
    }
    if kind == TYD_ARRAY {
        let elem = key_elem_of(sess.5, key);
        let len = key_len_of(sess, key);
        let mut idx = 0i64;
        while idx < len {
            let array_ty = llvm_of(sess, key, span)?;
            let eptr = offset_array_elem_ptr(sess, array_ty, ptr, sess.0.i64_type().const_int(idx as u64, false))?;
            emit_key_hash_into(sess, f, elem, eptr, hash_slot, span)?;
            idx += 1;
        }
        return Ok(());
    }
    if kind == TYD_REF || kind == TYD_REF_MUT {
        let elem = key_elem_of(sess.5, key);
        if key_kind_of(sess.5, elem) == TYD_SLICE {
            emit_slice_hash_into(sess, f, elem, ptr, hash_slot, span)?;
            return Ok(());
        }
        let referent = load_ptr(sess, ptr)?;
        emit_key_hash_into(sess, f, elem, referent, hash_slot, span)?;
        return Ok(());
    }
    Err(no_structural_equality(sess, key, span))
}

// Folds a `&[T]` slice's *contents* into the hash: the length, then every
// element, so two equal-content slices hash equal wherever their bytes live.
fn emit_slice_hash_into<'ctx>(sess: &mut Session<'ctx, '_, '_>, f: FunctionValue<'ctx>, slice_key: i64, view: PointerValue<'ctx>, hash_slot: PointerValue<'ctx>, span: (i64, i64, i64)) -> Result<(), CodegenError> {
    let elem = key_elem_of(sess.5, slice_key);
    let data = slice_data(sess, view)?;
    let len = slice_len_of(sess, view)?;
    fold_hash_word(sess, hash_slot, len)?;
    let i_slot = alloca_raw(sess, sess.0.i64_type().into(), "slice_i", span)?;
    store_key(sess, i_slot, sess.0.i64_type().const_zero().into())?;
    let cond = new_block(sess, f, "slice_hash_cond");
    let body = new_block(sess, f, "slice_hash_body");
    let done = new_block(sess, f, "slice_hash_done");
    sess.2.build_unconditional_branch(cond).map_err(builder_fail)?;
    sess.2.position_at_end(cond);
    let i = load_i64(sess, i_slot)?;
    let more = sess.2.build_int_compare(IntPredicate::ULT, i, len, "").map_err(builder_fail)?;
    sess.2.build_conditional_branch(more, body, done).map_err(builder_fail)?;
    sess.2.position_at_end(body);
    let elem_ty = llvm_of(sess, elem, span)?;
    let eptr = offset_buffer_elem_ptr(sess, elem_ty, data, i)?;
    emit_key_hash_into(sess, f, elem, eptr, hash_slot, span)?;
    let i2 = sess.2.build_int_add(i, sess.0.i64_type().const_int(1, false), "").map_err(builder_fail)?;
    store_key(sess, i_slot, i2.into())?;
    sess.2.build_unconditional_branch(cond).map_err(builder_fail)?;
    sess.2.position_at_end(done);
    Ok(())
}

// Moves every occupied slot of a table into a fresh, zeroed one, probing
// the new table for an empty slot per key. Called only on grow, so the new
// table always has room; this drops tombstones, keeping a grown table at
// most half occupied and free of stale markers.
fn rehash_into<'ctx>(
    sess: &mut Session<'ctx, '_, '_>,
    f: FunctionValue<'ctx>,
    old: (PointerValue<'ctx>, IntValue<'ctx>),
    new: (PointerValue<'ctx>, IntValue<'ctx>),
    k_key: i64,
    v_key: i64,
    span: (i64, i64, i64),
) -> Result<(), CodegenError> {
    let (old_data, old_cap) = old;
    let (new_data, new_cap) = new;
    let slot_ty = hash_map_slot_ty(sess, k_key, v_key, span)?;
    let i_slot = alloca_raw(sess, sess.0.i64_type().into(), "old", span)?;
    store_key(sess, i_slot, sess.0.i64_type().const_zero().into())?;
    let cond = new_block(sess, f, "rehash_cond");
    let body = new_block(sess, f, "rehash_body");
    let done = new_block(sess, f, "rehash_done");
    sess.2.build_unconditional_branch(cond).map_err(builder_fail)?;
    sess.2.position_at_end(cond);
    let i = load_i64(sess, i_slot)?;
    let more = sess.2.build_int_compare(IntPredicate::ULT, i, old_cap, "").map_err(builder_fail)?;
    sess.2.build_conditional_branch(more, body, done).map_err(builder_fail)?;
    sess.2.position_at_end(body);
    let old_slot = offset_buffer_elem_ptr(sess, slot_ty, old_data, i)?;
    let flag_ptr = slot_gep(sess, slot_ty, old_slot, 0)?;
    let flag = load_i64(sess, flag_ptr)?;
    let occupied = sess.2.build_int_compare(IntPredicate::EQ, flag, sess.0.i64_type().const_int(SLOT_OCCUPIED, false), "").map_err(builder_fail)?;
    let move_it = new_block(sess, f, "rehash_move");
    let skip = new_block(sess, f, "rehash_skip");
    sess.2.build_conditional_branch(occupied, move_it, skip).map_err(builder_fail)?;
    sess.2.position_at_end(move_it);
    let old_key = slot_gep(sess, slot_ty, old_slot, 1)?;
    let old_val = slot_gep(sess, slot_ty, old_slot, 2)?;
    let hash_slot = alloca_raw(sess, sess.0.i64_type().into(), "rehash_key", span)?;
    store_key(sess, hash_slot, sess.0.i64_type().const_int(HASH_SEED, false).into())?;
    emit_key_hash_into(sess, f, k_key, old_key, hash_slot, span)?;
    let hash = load_i64(sess, hash_slot)?;
    let step_slot = alloca_raw(sess, sess.0.i64_type().into(), "rehash_step", span)?;
    store_key(sess, step_slot, sess.0.i64_type().const_zero().into())?;
    let probe_body = new_block(sess, f, "rehash_probe");
    let place = new_block(sess, f, "rehash_place");
    sess.2.build_unconditional_branch(probe_body).map_err(builder_fail)?;
    sess.2.position_at_end(probe_body);
    let step = load_i64(sess, step_slot)?;
    let new_slot = probe_slot(sess, slot_ty, new_data, hash, step, new_cap)?;
    let new_flag_ptr = slot_gep(sess, slot_ty, new_slot, 0)?;
    let new_flag = load_i64(sess, new_flag_ptr)?;
    let empty = sess.2.build_int_compare(IntPredicate::EQ, new_flag, sess.0.i64_type().const_int(SLOT_EMPTY, false), "").map_err(builder_fail)?;
    let next_probe = new_block(sess, f, "rehash_next");
    sess.2.build_conditional_branch(empty, place, next_probe).map_err(builder_fail)?;
    sess.2.position_at_end(next_probe);
    let next_step = sess.2.build_int_add(step, sess.0.i64_type().const_int(1, false), "").map_err(builder_fail)?;
    store_key(sess, step_slot, next_step.into())?;
    sess.2.build_unconditional_branch(probe_body).map_err(builder_fail)?;
    sess.2.position_at_end(place);
    let new_key_ptr = slot_gep(sess, slot_ty, new_slot, 1)?;
    let new_val_ptr = slot_gep(sess, slot_ty, new_slot, 2)?;
    copy_value(sess, k_key, new_key_ptr, old_key, span)?;
    copy_value(sess, v_key, new_val_ptr, old_val, span)?;
    store_key(sess, new_flag_ptr, sess.0.i64_type().const_int(SLOT_OCCUPIED, false).into())?;
    sess.2.build_unconditional_branch(skip).map_err(builder_fail)?;
    sess.2.position_at_end(skip);
    let next_i = sess.2.build_int_add(i, sess.0.i64_type().const_int(1, false), "").map_err(builder_fail)?;
    store_key(sess, i_slot, next_i.into())?;
    sess.2.build_unconditional_branch(cond).map_err(builder_fail)?;
    sess.2.position_at_end(done);
    Ok(())
}

fn native_hash_map_new<'ctx>(sess: &mut Session<'ctx, '_, '_>, ret_key: i64, out: PointerValue<'ctx>, span: (i64, i64, i64)) -> Result<(), CodegenError> {
    let map_key = result_arg_key(sess, ret_key, 0);
    let map_val = declare_local(sess, map_key, "map", span)?;
    init_container_slots(sess, map_key, map_val, span)?;
    let ok_result = build_result_ok(sess, ret_key, map_key, map_val, span)?;
    copy_to_out(sess, ret_key, out, ok_result, span)?;
    Ok(())
}

fn native_hash_map_insert<'ctx>(sess: &mut Session<'ctx, '_, '_>, f: FunctionValue<'ctx>, locals: &Locals<'ctx>, params_list: i64, ret_key: i64, out: PointerValue<'ctx>, span: (i64, i64, i64)) -> Result<PointerValue<'ctx>, CodegenError> {
    let p0 = get_local(locals, 0, span)?;
    let p1 = get_local(locals, 1, span)?;
    let p2 = get_local(locals, 2, span)?;
    let k_key = native_arg_key(sess, params_list, 1);
    let v_key = native_arg_key(sess, params_list, 2);
    let slot_ty = hash_map_slot_ty(sess, k_key, v_key, span)?;
    let slot_size = sess.3.get_abi_size(&slot_ty);
    let slot_size_const = sess.0.i64_type().const_int(slot_size, false);
    let map_key = deref_key_of(sess, get_local_key(locals, 0, span)?);
    let map_ref = load_ptr(sess, p0)?;
    let dptr = struct_gep(sess, map_key, map_ref, 0, "", span)?;
    let lptr = struct_gep(sess, map_key, map_ref, 1, "", span)?;
    let cptr = struct_gep(sess, map_key, map_ref, 2, "", span)?;

    // Grow (rehash) up front when the table is empty or already half full,
    // so the probe below always has an empty or tombstoned slot to land in.
    let zero = sess.0.i64_type().const_zero();
    let len0 = load_i64(sess, lptr)?;
    let cap0 = load_i64(sess, cptr)?;
    let cap0_empty = sess.2.build_int_compare(IntPredicate::EQ, cap0, zero, "").map_err(builder_fail)?;
    let len_doubled = sess.2.build_int_mul(len0, sess.0.i64_type().const_int(2, false), "").map_err(builder_fail)?;
    let half_full = sess.2.build_int_compare(IntPredicate::SGE, len_doubled, cap0, "").map_err(builder_fail)?;
    let need_grow = sess.2.build_or(cap0_empty, half_full, "").map_err(builder_fail)?;
    let grow_block = new_block(sess, f, "map_grow");
    let probe_prep = new_block(sess, f, "map_probe_prep");
    let fail_block = new_block(sess, f, "map_fail");
    let after = new_block(sess, f, "map_after");
    sess.2.build_conditional_branch(need_grow, grow_block, probe_prep).map_err(builder_fail)?;

    sess.2.position_at_end(grow_block);
    let old_data = load_ptr(sess, dptr)?;
    let old_cap = load_i64(sess, cptr)?;
    let old_cap_empty = sess.2.build_int_compare(IntPredicate::EQ, old_cap, zero, "").map_err(builder_fail)?;
    let doubled_cap = sess.2.build_int_mul(old_cap, sess.0.i64_type().const_int(2, false), "").map_err(builder_fail)?;
    let initial = sess.0.i64_type().const_int(INITIAL_CAPACITY, false);
    let new_cap = sess.2.build_select(old_cap_empty, initial, doubled_cap, "").map_err(builder_fail)?.into_int_value();
    let needed = sess.2.build_int_mul(new_cap, slot_size_const, "").map_err(builder_fail)?;
    let malloc = extern_malloc(sess);
    let call = sess.2.build_call(malloc, &[into_meta(needed.into())], "").map_err(builder_fail)?;
    let new_data = match call.try_as_basic_value() {
        ValueKind::Basic(value) => value.into_pointer_value(),
        ValueKind::Instruction(inst) => return Err(builder_error(span.0, span.1, span.2, &format!("internal: malloc returned void ({:?})", inst.get_opcode()))),
    };
    let null_cmp = is_null_ptr(sess, new_data)?;
    let grow_ok = new_block(sess, f, "map_grow_ok");
    sess.2.build_conditional_branch(null_cmp, fail_block, grow_ok).map_err(builder_fail)?;
    sess.2.position_at_end(grow_ok);
    let zero8 = sess.0.i8_type().const_zero();
    sess.2.build_memset(new_data, 1, zero8, needed).map_err(builder_fail)?;
    rehash_into(sess, f, (old_data, old_cap), (new_data, new_cap), k_key, v_key, span)?;
    let old_null = is_null_ptr(sess, old_data)?;
    let free_old = new_block(sess, f, "map_free_old");
    let grow_done = new_block(sess, f, "map_grow_done");
    sess.2.build_conditional_branch(old_null, grow_done, free_old).map_err(builder_fail)?;
    sess.2.position_at_end(free_old);
    sess.2.build_call(extern_free(sess), &[into_meta(old_data.into())], "").map_err(builder_fail)?;
    sess.2.build_unconditional_branch(grow_done).map_err(builder_fail)?;
    sess.2.position_at_end(grow_done);
    store_key(sess, dptr, new_data.into())?;
    store_key(sess, cptr, new_cap.into())?;
    sess.2.build_unconditional_branch(probe_prep).map_err(builder_fail)?;

    sess.2.position_at_end(probe_prep);
    let data = load_ptr(sess, dptr)?;
    let cap = load_i64(sess, cptr)?;
    let hash_slot = alloca_raw(sess, sess.0.i64_type().into(), "key_hash", span)?;
    store_key(sess, hash_slot, sess.0.i64_type().const_int(HASH_SEED, false).into())?;
    emit_key_hash_into(sess, f, k_key, p1, hash_slot, span)?;
    let hash = load_i64(sess, hash_slot)?;
    let step_slot = alloca_raw(sess, sess.0.i64_type().into(), "step", span)?;
    store_key(sess, step_slot, zero.into())?;
    let insert_slot = alloca_raw(sess, sess.0.i64_type().into(), "insert_at", span)?;
    let minus_one = sess.0.i64_type().const_int(u64::MAX, false);
    store_key(sess, insert_slot, minus_one.into())?;
    let probe_cond = new_block(sess, f, "map_probe_cond");
    let probe_body = new_block(sess, f, "map_probe_body");
    let exhausted = new_block(sess, f, "map_probe_exhausted");
    sess.2.build_unconditional_branch(probe_cond).map_err(builder_fail)?;
    sess.2.position_at_end(probe_cond);
    let step = load_i64(sess, step_slot)?;
    let more = sess.2.build_int_compare(IntPredicate::ULT, step, cap, "").map_err(builder_fail)?;
    sess.2.build_conditional_branch(more, probe_body, exhausted).map_err(builder_fail)?;
    sess.2.position_at_end(probe_body);
    let slot = probe_slot(sess, slot_ty, data, hash, step, cap)?;
    let flag_ptr = slot_gep(sess, slot_ty, slot, 0)?;
    let flag = load_i64(sess, flag_ptr)?;
    let empty_const = sess.0.i64_type().const_int(SLOT_EMPTY, false);
    let occupied_const = sess.0.i64_type().const_int(SLOT_OCCUPIED, false);
    let is_empty = sess.2.build_int_compare(IntPredicate::EQ, flag, empty_const, "").map_err(builder_fail)?;
    let is_occupied = sess.2.build_int_compare(IntPredicate::EQ, flag, occupied_const, "").map_err(builder_fail)?;
    let empty_case = new_block(sess, f, "map_slot_empty");
    let occupied_case = new_block(sess, f, "map_slot_occupied");
    let tombstone_case = new_block(sess, f, "map_slot_tombstone");
    sess.2.build_conditional_branch(is_empty, empty_case, occupied_case).map_err(builder_fail)?;
    let compare_block = new_block(sess, f, "map_compare");
    let replace = new_block(sess, f, "map_replace");
    let remember = new_block(sess, f, "map_remember");
    let advance = new_block(sess, f, "map_advance");
    sess.2.position_at_end(occupied_case);
    sess.2.build_conditional_branch(is_occupied, compare_block, tombstone_case).map_err(builder_fail)?;
    sess.2.position_at_end(compare_block);
    let slot_key = slot_gep(sess, slot_ty, slot, 1)?;
    let eq = emit_key_eq(sess, f, k_key, slot_key, p1, span)?;
    sess.2.build_conditional_branch(eq, replace, advance).map_err(builder_fail)?;
    sess.2.position_at_end(tombstone_case);
    let have_slot = sess.2.build_int_compare(IntPredicate::EQ, load_i64(sess, insert_slot)?, minus_one, "").map_err(builder_fail)?;
    sess.2.build_conditional_branch(have_slot, remember, advance).map_err(builder_fail)?;
    sess.2.position_at_end(remember);
    store_key(sess, insert_slot, step.into())?;
    sess.2.build_unconditional_branch(advance).map_err(builder_fail)?;
    sess.2.position_at_end(empty_case);
    let have_slot2 = sess.2.build_int_compare(IntPredicate::EQ, load_i64(sess, insert_slot)?, minus_one, "").map_err(builder_fail)?;
    let remember2 = new_block(sess, f, "map_remember2");
    let do_insert = new_block(sess, f, "map_insert");
    sess.2.build_conditional_branch(have_slot2, remember2, do_insert).map_err(builder_fail)?;
    sess.2.position_at_end(remember2);
    store_key(sess, insert_slot, step.into())?;
    sess.2.build_unconditional_branch(do_insert).map_err(builder_fail)?;
    sess.2.position_at_end(advance);
    let next_step = sess.2.build_int_add(step, sess.0.i64_type().const_int(1, false), "").map_err(builder_fail)?;
    store_key(sess, step_slot, next_step.into())?;
    sess.2.build_unconditional_branch(probe_cond).map_err(builder_fail)?;

    sess.2.position_at_end(replace);
    let slot_val = slot_gep(sess, slot_ty, slot, 2)?;
    copy_value(sess, v_key, slot_val, p2, span)?;
    let unit_key = result_arg_key(sess, ret_key, 0);
    let unit_val = build_unit_value(sess, unit_key, span)?;
    let ok_result = build_result_ok(sess, ret_key, unit_key, unit_val, span)?;
    copy_to_out(sess, ret_key, out, ok_result, span)?;
    sess.2.build_unconditional_branch(after).map_err(builder_fail)?;

    sess.2.position_at_end(exhausted);
    // Defensive: a table kept below half occupancy always has an empty or
    // tombstoned slot, so this is unreachable in practice. If it is ever
    // reached, rehash into a larger table (which drops tombstones and
    // guarantees room) and retry, rather than writing a slot that does not
    // exist.
    sess.2.build_unconditional_branch(grow_block).map_err(builder_fail)?;

    sess.2.position_at_end(do_insert);
    let at = load_i64(sess, insert_slot)?;
    let target = probe_slot(sess, slot_ty, data, hash, at, cap)?;
    let target_flag = slot_gep(sess, slot_ty, target, 0)?;
    let target_key = slot_gep(sess, slot_ty, target, 1)?;
    let target_val = slot_gep(sess, slot_ty, target, 2)?;
    copy_value(sess, k_key, target_key, p1, span)?;
    copy_value(sess, v_key, target_val, p2, span)?;
    store_key(sess, target_flag, occupied_const.into())?;
    let len_now = load_i64(sess, lptr)?;
    let len_next = sess.2.build_int_add(len_now, sess.0.i64_type().const_int(1, false), "").map_err(builder_fail)?;
    store_key(sess, lptr, len_next.into())?;
    let unit_key2 = result_arg_key(sess, ret_key, 0);
    let unit_val2 = build_unit_value(sess, unit_key2, span)?;
    let ok_result2 = build_result_ok(sess, ret_key, unit_key2, unit_val2, span)?;
    copy_to_out(sess, ret_key, out, ok_result2, span)?;
    sess.2.build_unconditional_branch(after).map_err(builder_fail)?;

    sess.2.position_at_end(fail_block);
    let err_key = result_arg_key(sess, ret_key, 1);
    let alloc_fail_tag = seeded_enum_variant_tag(sess, SEED_SYM_ALLOC_FAILED, span)?;
    let fkey = variant_payload_key(sess, err_key, alloc_fail_tag, 0, span)?;
    let fval = declare_local(sess, fkey, "need", span)?;
    store_key(sess, fval, needed.into())?;
    let fail_val = build_enum_value(sess, err_key, alloc_fail_tag, &[(fkey, fval)], span)?;
    let err_result = build_result_err(sess, ret_key, err_key, fail_val, span)?;
    copy_to_out(sess, ret_key, out, err_result, span)?;
    sess.2.build_unconditional_branch(after).map_err(builder_fail)?;
    sess.2.position_at_end(after);
    Ok(out)
}


fn native_hash_map_get<'ctx>(sess: &mut Session<'ctx, '_, '_>, f: FunctionValue<'ctx>, locals: &Locals<'ctx>, params_list: i64, ret_key: i64, out: PointerValue<'ctx>, span: (i64, i64, i64)) -> Result<PointerValue<'ctx>, CodegenError> {
    let p0 = get_local(locals, 0, span)?;
    let p1 = get_local(locals, 1, span)?;
    let k_key = native_arg_key(sess, params_list, 1);
    let v_key = result_arg_key(sess, ret_key, 0);
    let slot_ty = hash_map_slot_ty(sess, k_key, v_key, span)?;
    let map_key = deref_key_of(sess, get_local_key(locals, 0, span)?);
    let map_ref = load_ptr(sess, p0)?;
    let dptr = struct_gep(sess, map_key, map_ref, 0, "", span)?;
    let cptr = struct_gep(sess, map_key, map_ref, 2, "", span)?;
    let data = load_ptr(sess, dptr)?;
    let cap = load_i64(sess, cptr)?;
    let hash_slot = alloca_raw(sess, sess.0.i64_type().into(), "key_hash", span)?;
    store_key(sess, hash_slot, sess.0.i64_type().const_int(HASH_SEED, false).into())?;
    emit_key_hash_into(sess, f, k_key, p1, hash_slot, span)?;
    let hash = load_i64(sess, hash_slot)?;
    let step_slot = alloca_raw(sess, sess.0.i64_type().into(), "step", span)?;
    store_key(sess, step_slot, sess.0.i64_type().const_zero().into())?;
    let probe_cond = new_block(sess, f, "g_cond");
    let probe_body = new_block(sess, f, "g_body");
    let found_block = new_block(sess, f, "g_found");
    let missing_block = new_block(sess, f, "g_missing");
    let after = new_block(sess, f, "g_after");
    sess.2.build_unconditional_branch(probe_cond).map_err(builder_fail)?;
    sess.2.position_at_end(probe_cond);
    let step = load_i64(sess, step_slot)?;
    let more = sess.2.build_int_compare(IntPredicate::ULT, step, cap, "").map_err(builder_fail)?;
    sess.2.build_conditional_branch(more, probe_body, missing_block).map_err(builder_fail)?;
    sess.2.position_at_end(probe_body);
    let slot = probe_slot(sess, slot_ty, data, hash, step, cap)?;
    let flag_ptr = slot_gep(sess, slot_ty, slot, 0)?;
    let flag = load_i64(sess, flag_ptr)?;
    let empty_const = sess.0.i64_type().const_int(SLOT_EMPTY, false);
    let occupied_const = sess.0.i64_type().const_int(SLOT_OCCUPIED, false);
    let is_empty = sess.2.build_int_compare(IntPredicate::EQ, flag, empty_const, "").map_err(builder_fail)?;
    let is_occupied = sess.2.build_int_compare(IntPredicate::EQ, flag, occupied_const, "").map_err(builder_fail)?;
    let occupied_or_tombstone = new_block(sess, f, "g_occupied_or_tombstone");
    let compare_block = new_block(sess, f, "g_compare");
    let advance = new_block(sess, f, "g_advance");
    sess.2.build_conditional_branch(is_empty, missing_block, occupied_or_tombstone).map_err(builder_fail)?;
    sess.2.position_at_end(occupied_or_tombstone);
    sess.2.build_conditional_branch(is_occupied, compare_block, advance).map_err(builder_fail)?;
    sess.2.position_at_end(compare_block);
    let slot_key = slot_gep(sess, slot_ty, slot, 1)?;
    let eq = emit_key_eq(sess, f, k_key, slot_key, p1, span)?;
    sess.2.build_conditional_branch(eq, found_block, advance).map_err(builder_fail)?;
    sess.2.position_at_end(advance);
    let next_step = sess.2.build_int_add(step, sess.0.i64_type().const_int(1, false), "").map_err(builder_fail)?;
    store_key(sess, step_slot, next_step.into())?;
    sess.2.build_unconditional_branch(probe_cond).map_err(builder_fail)?;
    sess.2.position_at_end(found_block);
    let slot_val = slot_gep(sess, slot_ty, slot, 2)?;
    let v_val = declare_local(sess, v_key, "got", span)?;
    copy_value(sess, v_key, v_val, slot_val, span)?;
    let ok_result = build_result_ok(sess, ret_key, v_key, v_val, span)?;
    copy_to_out(sess, ret_key, out, ok_result, span)?;
    sess.2.build_unconditional_branch(after).map_err(builder_fail)?;
    sess.2.position_at_end(missing_block);
    let err_key = result_arg_key(sess, ret_key, 1);
    let key_missing_tag = seeded_enum_variant_tag(sess, SEED_SYM_KEY_NOT_FOUND, span)?;
    let fail_val = build_enum_value(sess, err_key, key_missing_tag, &[], span)?;
    let err_result = build_result_err(sess, ret_key, err_key, fail_val, span)?;
    copy_to_out(sess, ret_key, out, err_result, span)?;
    sess.2.build_unconditional_branch(after).map_err(builder_fail)?;
    sess.2.position_at_end(after);
    Ok(out)
}

fn native_hash_map_free<'ctx>(sess: &mut Session<'ctx, '_, '_>, locals: &Locals<'ctx>, ret_key: i64, out: PointerValue<'ctx>, span: (i64, i64, i64)) -> Result<(), CodegenError> {
    let p0 = get_local(locals, 0, span)?;
    let map_key = deref_key_of(sess, get_local_key(locals, 0, span)?);
    let d = struct_gep(sess, map_key, p0, 0, "", span)?;
    let data = load_ptr(sess, d)?;
    let free = extern_free(sess);
    sess.2.build_call(free, &[into_meta(data.into())], "").map_err(builder_fail)?;
    build_unit_value_into(sess, ret_key, out, span)?;
    Ok(())
}

fn native_hash_map_remove<'ctx>(sess: &mut Session<'ctx, '_, '_>, f: FunctionValue<'ctx>, locals: &Locals<'ctx>, params_list: i64, ret_key: i64, out: PointerValue<'ctx>, span: (i64, i64, i64)) -> Result<PointerValue<'ctx>, CodegenError> {
    let p0 = get_local(locals, 0, span)?;
    let p1 = get_local(locals, 1, span)?;
    let k_key = native_arg_key(sess, params_list, 1);
    let v_key = result_arg_key(sess, ret_key, 0);
    let slot_ty = hash_map_slot_ty(sess, k_key, v_key, span)?;
    let map_key = deref_key_of(sess, get_local_key(locals, 0, span)?);
    let map_ref = load_ptr(sess, p0)?;
    let dptr = struct_gep(sess, map_key, map_ref, 0, "", span)?;
    let lptr = struct_gep(sess, map_key, map_ref, 1, "", span)?;
    let cptr = struct_gep(sess, map_key, map_ref, 2, "", span)?;
    let data = load_ptr(sess, dptr)?;
    let cap = load_i64(sess, cptr)?;
    let hash_slot = alloca_raw(sess, sess.0.i64_type().into(), "key_hash", span)?;
    store_key(sess, hash_slot, sess.0.i64_type().const_int(HASH_SEED, false).into())?;
    emit_key_hash_into(sess, f, k_key, p1, hash_slot, span)?;
    let hash = load_i64(sess, hash_slot)?;
    let step_slot = alloca_raw(sess, sess.0.i64_type().into(), "step", span)?;
    store_key(sess, step_slot, sess.0.i64_type().const_zero().into())?;
    let probe_cond = new_block(sess, f, "rm_cond");
    let probe_body = new_block(sess, f, "rm_body");
    let found_block = new_block(sess, f, "rm_found");
    let missing_block = new_block(sess, f, "rm_missing");
    let after = new_block(sess, f, "rm_after");
    sess.2.build_unconditional_branch(probe_cond).map_err(builder_fail)?;
    sess.2.position_at_end(probe_cond);
    let step = load_i64(sess, step_slot)?;
    let more = sess.2.build_int_compare(IntPredicate::ULT, step, cap, "").map_err(builder_fail)?;
    sess.2.build_conditional_branch(more, probe_body, missing_block).map_err(builder_fail)?;
    sess.2.position_at_end(probe_body);
    let slot = probe_slot(sess, slot_ty, data, hash, step, cap)?;
    let flag_ptr = slot_gep(sess, slot_ty, slot, 0)?;
    let flag = load_i64(sess, flag_ptr)?;
    let empty_const = sess.0.i64_type().const_int(SLOT_EMPTY, false);
    let occupied_const = sess.0.i64_type().const_int(SLOT_OCCUPIED, false);
    let is_empty = sess.2.build_int_compare(IntPredicate::EQ, flag, empty_const, "").map_err(builder_fail)?;
    let is_occupied = sess.2.build_int_compare(IntPredicate::EQ, flag, occupied_const, "").map_err(builder_fail)?;
    let occupied_or_tombstone = new_block(sess, f, "rm_occupied_or_tombstone");
    let compare_block = new_block(sess, f, "rm_compare");
    let advance = new_block(sess, f, "rm_advance");
    sess.2.build_conditional_branch(is_empty, missing_block, occupied_or_tombstone).map_err(builder_fail)?;
    sess.2.position_at_end(occupied_or_tombstone);
    sess.2.build_conditional_branch(is_occupied, compare_block, advance).map_err(builder_fail)?;
    sess.2.position_at_end(compare_block);
    let slot_key = slot_gep(sess, slot_ty, slot, 1)?;
    let eq = emit_key_eq(sess, f, k_key, slot_key, p1, span)?;
    sess.2.build_conditional_branch(eq, found_block, advance).map_err(builder_fail)?;
    sess.2.position_at_end(advance);
    let next_step = sess.2.build_int_add(step, sess.0.i64_type().const_int(1, false), "").map_err(builder_fail)?;
    store_key(sess, step_slot, next_step.into())?;
    sess.2.build_unconditional_branch(probe_cond).map_err(builder_fail)?;
    sess.2.position_at_end(found_block);
    let slot_val = slot_gep(sess, slot_ty, slot, 2)?;
    let v_val = declare_local(sess, v_key, "removed", span)?;
    copy_value(sess, v_key, v_val, slot_val, span)?;
    store_key(sess, flag_ptr, sess.0.i64_type().const_int(SLOT_TOMBSTONE, false).into())?;
    let len_now = load_i64(sess, lptr)?;
    let len_next = sess.2.build_int_sub(len_now, sess.0.i64_type().const_int(1, false), "").map_err(builder_fail)?;
    store_key(sess, lptr, len_next.into())?;
    let ok_result = build_result_ok(sess, ret_key, v_key, v_val, span)?;
    copy_to_out(sess, ret_key, out, ok_result, span)?;
    sess.2.build_unconditional_branch(after).map_err(builder_fail)?;
    sess.2.position_at_end(missing_block);
    let err_key = result_arg_key(sess, ret_key, 1);
    let key_missing_tag = seeded_enum_variant_tag(sess, SEED_SYM_KEY_NOT_FOUND, span)?;
    let fail_val = build_enum_value(sess, err_key, key_missing_tag, &[], span)?;
    let err_result = build_result_err(sess, ret_key, err_key, fail_val, span)?;
    copy_to_out(sess, ret_key, out, err_result, span)?;
    sess.2.build_unconditional_branch(after).map_err(builder_fail)?;
    sess.2.position_at_end(after);
    Ok(out)
}

fn native_self_check<'ctx>(sess: &mut Session<'ctx, '_, '_>, ret_key: i64, out: PointerValue<'ctx>, span: (i64, i64, i64)) -> Result<(), CodegenError> {
    let unit_key = result_arg_key(sess, ret_key, 0);
    let unit_val = build_unit_value(sess, unit_key, span)?;
    let ok_result = build_result_ok(sess, ret_key, unit_key, unit_val, span)?;
    copy_to_out(sess, ret_key, out, ok_result, span)?;
    Ok(())
}

// The largest path staged in the compiler-generated C string buffer.
//
// A path arrives as a `&[U8]` carrying its own length, but `openat` wants a
// NUL-terminated string, so the path is copied into a stack buffer with a
// terminator appended. Bounding it at the kernel's own limit means the copy
// needs no allocation and a longer path is rejected with the same error the
// kernel would have produced.
const PATH_MAX: u64 = 4096;

// The platform's errno accessor comes from the target's typed ABI row; the
// emitter never names a platform to pick one.
fn runtime_errno<'ctx>(sess: &mut Session<'ctx, '_, '_>, span: (i64, i64, i64)) -> Result<IntValue<'ctx>, CodegenError> {
    let name = sess.13.abi().errno_accessor;
    let loc_fn = extern_fn(sess, name, ptr_ty(sess).fn_type(&[], false));
    let call = sess.2.build_call(loc_fn, &[], "").map_err(builder_fail)?;
    let loc = match call.try_as_basic_value() {
        ValueKind::Basic(value) => value.into_pointer_value(),
        ValueKind::Instruction(inst) => return Err(builder_error(span.0, span.1, span.2, &format!("internal: errno accessor returned void ({:?})", inst.get_opcode()))),
    };
    let err32 = sess.2.build_load(sess.0.i32_type(), loc, "").map_err(builder_fail)?.into_int_value();
    sess.2.build_int_s_extend(err32, sess.0.i64_type(), "").map_err(builder_fail)
}

fn is_eintr<'ctx>(sess: &mut Session<'ctx, '_, '_>, result: IntValue<'ctx>, span: (i64, i64, i64)) -> Result<inkwell::values::IntValue<'ctx>, CodegenError> {
    let failed = sess.2.build_int_compare(IntPredicate::EQ, result, result.get_type().const_all_ones(), "").map_err(builder_fail)?;
    let errno = runtime_errno(sess, span)?;
    let interrupted = sess.0.i64_type().const_int(sess.13.abi().interrupted, false);
    let matches = sess.2.build_int_compare(IntPredicate::EQ, errno, interrupted, "").map_err(builder_fail)?;
    sess.2.build_and(failed, matches, "").map_err(builder_fail)
}

// Builds `Err(SystemFault(code))` into `out`.
//
// Shared by every native surface that reports a platform error, so the
// mapping from an errno to a Cinnabar value is stated once. Each caller
// reads the code from the platform's errno accessor (`runtime_errno`) and
// passes it here to be written into the `SystemFault` payload.
fn system_fault_result<'ctx>(sess: &mut Session<'ctx, '_, '_>, ret_key: i64, out: PointerValue<'ctx>, code: IntValue<'ctx>, span: (i64, i64, i64)) -> Result<(), CodegenError> {
    let err_key = result_arg_key(sess, ret_key, 1);
    let tag = seeded_enum_variant_tag(sess, SEED_SYM_SYSTEM_FAULT, span)?;
    let f0 = variant_payload_key(sess, err_key, tag, 0, span)?;
    let slot = declare_local(sess, f0, "errno", span)?;
    store_key(sess, slot, code.into())?;
    let fail_val = build_enum_value(sess, err_key, tag, &[(f0, slot)], span)?;
    let err_result = build_result_err(sess, ret_key, err_key, fail_val, span)?;
    copy_to_out(sess, ret_key, out, err_result, span)
}

// Splits control flow on a libc result: negative is a failure that
// writes `Err(SystemFault(errno))` into `out` and jumps to the join block,
// non-negative continues in the block this returns positioned at.
//
// The caller resumes emitting the success path immediately and branches to
// the returned join block when done.
fn libc_result_branch<'ctx>(
    sess: &mut Session<'ctx, '_, '_>,
    f: FunctionValue<'ctx>,
    ret_key: i64,
    out: PointerValue<'ctx>,
    raw: IntValue<'ctx>,
    span: (i64, i64, i64),
) -> Result<BasicBlockId<'ctx>, CodegenError> {
    let zero = raw.get_type().const_zero();
    let ok_cmp = sess.2.build_int_compare(IntPredicate::SGE, raw, zero, "").map_err(builder_fail)?;
    let fail_block = new_block(sess, f, "sys_fail");
    let ok_block = new_block(sess, f, "sys_ok");
    let after = new_block(sess, f, "sys_after");
    sess.2.build_conditional_branch(ok_cmp, ok_block, fail_block).map_err(builder_fail)?;
    sess.2.position_at_end(fail_block);
    let code = runtime_errno(sess, span)?;
    system_fault_result(sess, ret_key, out, code, span)?;
    sess.2.build_unconditional_branch(after).map_err(builder_fail)?;
    sess.2.position_at_end(ok_block);
    Ok(after)
}

// The `openat` flags a `File.Mode` variant selects.
//
// The mode is a Cinnabar enum rather than an integer parameter, so a
// program never writes a Linux flag constant: `File.open(path,
// WriteTruncate)` says what it means, and the numbers stay inside the
// compiler. The tags come from the program's own declaration order through
// the seeded variant symbol, so the mapping does not depend on
// how the enum happens to be written.
fn open_flags_of<'ctx>(
    sess: &mut Session<'ctx, '_, '_>,
    mode_key: i64,
    mode_ptr: PointerValue<'ctx>,
    span: (i64, i64, i64),
) -> Result<IntValue<'ctx>, CodegenError> {
    let i64_ty = sess.0.i64_type();
    let tag_ptr = struct_gep(sess, mode_key, mode_ptr, 0, "", span)?;
    let tag = load_i64(sess, tag_ptr)?;
    let read_only = seeded_enum_variant_tag(sess, SEED_SYM_READ_ONLY, span)?;
    let truncate = seeded_enum_variant_tag(sess, SEED_SYM_WRITE_TRUNCATE, span)?;
    let abi = sess.13.abi();
    let read_flags = i64_ty.const_int(abi.open_binary, false);
    let truncate_flags = i64_ty.const_int(abi.open_write | abi.open_create | abi.open_truncate | abi.open_binary, false);
    let append_flags = i64_ty.const_int(abi.open_write | abi.open_create | abi.open_append | abi.open_binary, false);
    let is_read = sess.2.build_int_compare(IntPredicate::EQ, tag, i64_ty.const_int(read_only as u64, false), "").map_err(builder_fail)?;
    let is_truncate = sess.2.build_int_compare(IntPredicate::EQ, tag, i64_ty.const_int(truncate as u64, false), "").map_err(builder_fail)?;
    // Append is the remaining variant: the enum is exhaustive, so anything
    // that is neither read nor truncate is the append mode.
    let write_flags = sess.2.build_select(is_truncate, truncate_flags, append_flags, "").map_err(builder_fail)?;
    let chosen = sess.2.build_select(is_read, read_flags.into(), write_flags, "").map_err(builder_fail)?;
    Ok(chosen.into_int_value())
}

// Copies a `&[U8]` path into a stack buffer and appends the NUL the kernel
// expects, returning the buffer and whether the path fitted.
fn nul_terminated_path<'ctx>(
    sess: &mut Session<'ctx, '_, '_>,
    data: PointerValue<'ctx>,
    len: IntValue<'ctx>,
    span: (i64, i64, i64),
) -> Result<(PointerValue<'ctx>, IntValue<'ctx>), CodegenError> {
    let i64_ty = sess.0.i64_type();
    let buffer = alloca_raw(sess, sess.0.i8_type().array_type(PATH_MAX as u32 + 1).into(), "path", span)?;
    let fits = sess.2.build_int_compare(IntPredicate::ULT, len, i64_ty.const_int(PATH_MAX, false), "").map_err(builder_fail)?;
    // Clamp before copying so an over-long path cannot overrun the buffer
    // between the test and the copy; the caller rejects it on `fits`.
    let clamped = sess.2.build_select(fits, len, i64_ty.const_zero(), "").map_err(builder_fail)?.into_int_value();
    sess.2.build_memcpy(buffer, 1, data, 1, clamped).map_err(builder_fail)?;
    // NUL at `[0, clamped]` against the buffer's `[PATH_MAX + 1 x i8]`.
    let array_ty = sess.0.i8_type().array_type(PATH_MAX as u32 + 1);
    let terminator = offset_array_elem_ptr(sess, array_ty.into(), buffer, clamped)?;
    sess.2.build_store(terminator, sess.0.i8_type().const_zero()).map_err(builder_fail)?;
    Ok((buffer, fits))
}

fn native_file_open<'ctx>(sess: &mut Session<'ctx, '_, '_>, f: FunctionValue<'ctx>, locals: &Locals<'ctx>, ret_key: i64, out: PointerValue<'ctx>, span: (i64, i64, i64)) -> Result<PointerValue<'ctx>, CodegenError> {
    let p0 = get_local(locals, 0, span)?;
    let p1 = get_local(locals, 1, span)?;
    let mode_key = get_local_key(locals, 1, span)?;
    let data = slice_data(sess, p0)?;
    let len = slice_len_of(sess, p0)?;
    let (path, fits) = nul_terminated_path(sess, data, len, span)?;
    let too_long = new_block(sess, f, "open_too_long");
    let attempt = new_block(sess, f, "open_attempt");
    let after = new_block(sess, f, "open_after");
    sess.2.build_conditional_branch(fits, attempt, too_long).map_err(builder_fail)?;
    sess.2.position_at_end(too_long);
    // A path the buffer cannot hold is reported with the code the kernel
    // itself would have returned, rather than a Cinnabar-specific one.
    let name_too_long = sess.0.i64_type().const_int(sess.13.abi().name_too_long, false);
    system_fault_result(sess, ret_key, out, name_too_long, span)?;
    sess.2.build_unconditional_branch(after).map_err(builder_fail)?;
    sess.2.position_at_end(attempt);
    let flags = open_flags_of(sess, mode_key, p1, span)?;
    let flags32 = sess.2.build_int_truncate(flags, sess.0.i32_type(), "").map_err(builder_fail)?;
    let call = sess.2.build_call(extern_open(sess), &[into_meta(path.into()), into_meta(flags32.into()), into_meta(sess.0.i32_type().const_int(0o644, false).into())], "").map_err(builder_fail)?;
    let raw = match call.try_as_basic_value() {
        ValueKind::Basic(value) => value.into_int_value(),
        ValueKind::Instruction(inst) => return Err(builder_error(span.0, span.1, span.2, &format!("internal: open returned void ({:?})", inst.get_opcode()))),
    };
    let join = libc_result_branch(sess, f, ret_key, out, raw, span)?;
    let handle_key = result_arg_key(sess, ret_key, 0);
    let handle = declare_local(sess, handle_key, "file", span)?;
    // `File.Handle` is a scalar handle: it is the descriptor integer, not a
    // struct wrapping one.
    let raw64 = sess.2.build_int_s_extend(raw, sess.0.i64_type(), "").map_err(builder_fail)?;
    store_key(sess, handle, raw64.into())?;
    let ok_result = build_result_ok(sess, ret_key, handle_key, handle, span)?;
    copy_to_out(sess, ret_key, out, ok_result, span)?;
    sess.2.build_unconditional_branch(join).map_err(builder_fail)?;
    // Both `open` outcomes meet at `join`; that joins in turn with the
    // path-too-long branch, which never reached the open call.
    sess.2.position_at_end(join);
    sess.2.build_unconditional_branch(after).map_err(builder_fail)?;
    sess.2.position_at_end(after);
    Ok(out)
}

// `read` and `write` differ only in their direction and which C entry point
// direction the bytes travel, so one emitter serves both: the buffer is a
// `&mut [U8]` to fill or a `&[U8]` to send, and the result is the count the
// kernel reports.
//
// The count is returned rather than looped over, because a short read is
// information the caller needs — it is how end-of-file is observed (a zero
// count) and how a partial record is detected. `Terminal.print` loops
// because it has no way to report a short write; `File.write` does.
fn native_file_transfer<'ctx>(
    sess: &mut Session<'ctx, '_, '_>,
    f: FunctionValue<'ctx>,
    locals: &Locals<'ctx>,
    write: bool,
    ret_key: i64,
    out: PointerValue<'ctx>,
    span: (i64, i64, i64),
) -> Result<PointerValue<'ctx>, CodegenError> {
    let p0 = get_local(locals, 0, span)?;
    let p1 = get_local(locals, 1, span)?;
    let handle = load_ptr(sess, p0)?;
    let fd = net_fd_of_handle(sess, handle)?;
    let data = slice_data(sess, p1)?;
    let len = slice_len_of(sess, p1)?;
    let fd32 = sess.2.build_int_truncate(fd, sess.0.i32_type(), "").map_err(builder_fail)?;
    let attempt = new_block(sess, f, "file_transfer_attempt");
    let retry = new_block(sess, f, "file_transfer_retry");
    let failed = new_block(sess, f, "file_transfer_failed");
    sess.2.build_unconditional_branch(attempt).map_err(builder_fail)?;
    sess.2.position_at_end(attempt);
    let call = if write { extern_write(sess) } else { extern_read(sess) };
    let emitted = sess.2.build_call(call, &[into_meta(fd32.into()), into_meta(data.into()), into_meta(len.into())], "").map_err(builder_fail)?;
    let raw = libc_io_result(sess, emitted, span)?;
    let raw_failed = sess.2.build_int_compare(IntPredicate::EQ, raw, raw.get_type().const_all_ones(), "").map_err(builder_fail)?;
    let success = new_block(sess, f, "file_transfer_success");
    let decide_failure = new_block(sess, f, "file_transfer_decide_failure");
    sess.2.build_conditional_branch(raw_failed, decide_failure, success).map_err(builder_fail)?;
    sess.2.position_at_end(decide_failure);
    let interrupted = is_eintr(sess, raw, span)?;
    sess.2.build_conditional_branch(interrupted, retry, failed).map_err(builder_fail)?;
    sess.2.position_at_end(retry);
    sess.2.build_unconditional_branch(attempt).map_err(builder_fail)?;
    sess.2.position_at_end(failed);
    let code = runtime_errno(sess, span)?;
    system_fault_result(sess, ret_key, out, code, span)?;
    let join = new_block(sess, f, "file_transfer_after");
    sess.2.build_unconditional_branch(join).map_err(builder_fail)?;
    sess.2.position_at_end(success);
    let count_key = result_arg_key(sess, ret_key, 0);
    let count = declare_local(sess, count_key, "count", span)?;
    store_key(sess, count, raw.into())?;
    let ok_result = build_result_ok(sess, ret_key, count_key, count, span)?;
    copy_to_out(sess, ret_key, out, ok_result, span)?;
    sess.2.build_unconditional_branch(join).map_err(builder_fail)?;
    sess.2.position_at_end(join);
    Ok(out)
}

// Closing consumes the handle, which is why `File.Handle` is linear: a
// descriptor left open is a leak the borrow checker can catch, and closing
// twice would release a descriptor another part of the program has since
// been handed.
//
// `close` returns `Unit`, so a failing close is not reported. That is the
// honest surface: the descriptor is gone either way, and the errors `close`
// can report are about flushing that the program can no longer act on.
fn native_file_close<'ctx>(sess: &mut Session<'ctx, '_, '_>, locals: &Locals<'ctx>, ret_key: i64, out: PointerValue<'ctx>, span: (i64, i64, i64)) -> Result<(), CodegenError> {
    let p0 = get_local(locals, 0, span)?;
    let fd = net_fd_of_handle(sess, p0)?;
    let fd32 = sess.2.build_int_truncate(fd, sess.0.i32_type(), "").map_err(builder_fail)?;
    sess.2.build_call(extern_close(sess), &[into_meta(fd32.into())], "").map_err(builder_fail)?;
    build_unit_value_into(sess, ret_key, out, span)?;
    Ok(())
}

// Names of the module globals holding the command line and the built
// argument slice.  Derived from one place so the entry-point writer and
// `Runtime.args` cannot disagree about where the values live.
const ARGC_GLOBAL: &str = ".cnb.argc";
const ARGV_GLOBAL: &str = ".cnb.argv";
const ARGS_VIEW_GLOBAL: &str = ".cnb.args.view";
const ARGS_BUILT_GLOBAL: &str = ".cnb.args.built";

// A private mutable global, created on first use.
fn runtime_global<'ctx>(sess: &mut Session<'ctx, '_, '_>, name: &str, ty: BasicTypeEnum<'ctx>) -> inkwell::values::GlobalValue<'ctx> {
    match sess.1.get_global(name) {
        Some(existing) => existing,
        None => {
            let created = sess.1.add_global(ty, Some(AddressSpace::from(0u16)), name);
            created.set_initializer(&ty.const_zero());
            created.set_linkage(inkwell::module::Linkage::Private);
            created
        }
    }
}

// Stores `argc` and `argv` where `Runtime.args` can find them.
//
// The C runtime hands the command line to `main` and to nothing else, so
// this is the only point at which it can be captured. Two stores in the
// entry block, unconditionally: a program that never asks for its
// arguments pays those and nothing more.
fn capture_command_line<'ctx>(sess: &mut Session<'ctx, '_, '_>, wrapper: FunctionValue<'ctx>, span: (i64, i64, i64)) -> Result<(), CodegenError> {
    let argc = match wrapper.get_nth_param(0) {
        Some(value) => value.into_int_value(),
        None => return Err(builder_error(span.0, span.1, span.2, "internal: entry point has no argc parameter")),
    };
    let argv = match wrapper.get_nth_param(1) {
        Some(value) => value.into_pointer_value(),
        None => return Err(builder_error(span.0, span.1, span.2, "internal: entry point has no argv parameter")),
    };
    let argc_global = runtime_global(sess, ARGC_GLOBAL, sess.0.i64_type().into());
    let widened = sess.2.build_int_s_extend(argc, sess.0.i64_type(), "").map_err(builder_fail)?;
    store_key(sess, argc_global.as_pointer_value(), widened.into())?;
    let argv_global = runtime_global(sess, ARGV_GLOBAL, ptr_ty(sess).into());
    store_key(sess, argv_global.as_pointer_value(), argv.into())?;
    Ok(())
}

// The length of a NUL-terminated C string, as a loop over its bytes.
//
// `argv` entries are C strings, and a Cinnabar `String` carries an explicit
// length instead of a terminator, so the length has to be measured once at
// the boundary. This is emitted rather than calling libc's `strlen` because
// the argument surface does not route through libc.
fn emit_strlen<'ctx>(sess: &mut Session<'ctx, '_, '_>, f: FunctionValue<'ctx>, text: PointerValue<'ctx>, span: (i64, i64, i64)) -> Result<IntValue<'ctx>, CodegenError> {
    let i64_ty = sess.0.i64_type();
    let cursor = alloca_raw(sess, i64_ty.into(), "len", span)?;
    store_key(sess, cursor, i64_ty.const_zero().into())?;
    let cond = new_block(sess, f, "strlen_cond");
    let body = new_block(sess, f, "strlen_body");
    let done = new_block(sess, f, "strlen_done");
    sess.2.build_unconditional_branch(cond).map_err(builder_fail)?;
    sess.2.position_at_end(cond);
    let idx = load_i64(sess, cursor)?;
    let byte_ptr = byte_elem_ptr(sess, text, idx)?;
    let byte = load_i8(sess, byte_ptr)?;
    let more = sess.2.build_int_compare(IntPredicate::NE, byte, sess.0.i8_type().const_zero(), "").map_err(builder_fail)?;
    sess.2.build_conditional_branch(more, body, done).map_err(builder_fail)?;
    sess.2.position_at_end(body);
    let next = sess.2.build_int_add(idx, i64_ty.const_int(1, false), "").map_err(builder_fail)?;
    store_key(sess, cursor, next.into())?;
    sess.2.build_unconditional_branch(cond).map_err(builder_fail)?;
    sess.2.position_at_end(done);
    load_i64(sess, cursor)
}

// Builds the `&[Collections.String]` view over the process's arguments,
// once, and returns it on every later call.
//
// The `String` handles point *into* `argv` rather than copying it. The
// process's argument strings live for the whole run, which is what makes
// that safe, and it is also why the view is a shared borrow: a `String`
// cannot be moved out of a slice (the element is linear, and moving a
// linear element out of a container by index is a compile error), so a
// program can read an argument but can never hand one to `string_free`.
// The borrow checker enforces on its own that these are never freed.
//
// The handle array itself is allocated once and deliberately never
// released: it is process-lifetime data, exactly like the `argv` it points
// into, so there is no moment at which freeing it would be correct.
fn native_runtime_args<'ctx>(sess: &mut Session<'ctx, '_, '_>, f: FunctionValue<'ctx>, ret_key: i64, out: PointerValue<'ctx>, span: (i64, i64, i64)) -> Result<(), CodegenError> {
    let i64_ty = sess.0.i64_type();
    let view_ty = slice_view_ty(sess.0);
    let view_global = runtime_global(sess, ARGS_VIEW_GLOBAL, view_ty);
    let built_global = runtime_global(sess, ARGS_BUILT_GLOBAL, i64_ty.into());
    let built = load_i64(sess, built_global.as_pointer_value())?;
    let already = sess.2.build_int_compare(IntPredicate::NE, built, i64_ty.const_zero(), "").map_err(builder_fail)?;
    let build = new_block(sess, f, "args_build");
    let ready = new_block(sess, f, "args_ready");
    sess.2.build_conditional_branch(already, ready, build).map_err(builder_fail)?;

    sess.2.position_at_end(build);
    let argc_slot = runtime_global(sess, ARGC_GLOBAL, i64_ty.into()).as_pointer_value();
    let argc = load_i64(sess, argc_slot)?;
    let argv_ty = ptr_ty(sess).into();
    let argv_slot = runtime_global(sess, ARGV_GLOBAL, argv_ty).as_pointer_value();
    let argv = load_ptr(sess, argv_slot)?;
    let elem_key = em_key_elem(sess, em_key_elem(sess, ret_key));
    let elem_ty = llvm_of(sess, elem_key, span)?;
    let stride = i64_ty.const_int(sess.3.get_abi_size(&elem_ty), false);
    let bytes = sess.2.build_int_mul(argc, stride, "").map_err(builder_fail)?;
    let malloc = extern_malloc(sess);
    let call = sess.2.build_call(malloc, &[into_meta(bytes.into())], "").map_err(builder_fail)?;
    let table = match call.try_as_basic_value() {
        ValueKind::Basic(value) => value.into_pointer_value(),
        ValueKind::Instruction(inst) => {
            return Err(builder_error(span.0, span.1, span.2, &format!("internal: malloc returned void ({:?})", inst.get_opcode())));
        }
    };
    // A failed allocation yields an empty argument list rather than a
    // failure the surface cannot report: `Runtime.args` returns a slice,
    // not a Result, and a program that reads no arguments is a better
    // outcome than one that reads a null pointer.
    let null = is_null_ptr(sess, table)?;
    let fill = new_block(sess, f, "args_fill");
    let empty = new_block(sess, f, "args_empty");
    sess.2.build_conditional_branch(null, empty, fill).map_err(builder_fail)?;

    sess.2.position_at_end(empty);
    let empty_data = slice_gep(sess, view_global.as_pointer_value(), 0, "")?;
    store_key(sess, empty_data, ptr_ty(sess).const_null().into())?;
    let empty_len = slice_gep(sess, view_global.as_pointer_value(), 1, "")?;
    store_key(sess, empty_len, i64_ty.const_zero().into())?;
    sess.2.build_unconditional_branch(ready).map_err(builder_fail)?;

    sess.2.position_at_end(fill);
    let index = alloca_raw(sess, i64_ty.into(), "i", span)?;
    store_key(sess, index, i64_ty.const_zero().into())?;
    let loop_cond = new_block(sess, f, "args_cond");
    let loop_body = new_block(sess, f, "args_body");
    let loop_done = new_block(sess, f, "args_done");
    sess.2.build_unconditional_branch(loop_cond).map_err(builder_fail)?;
    sess.2.position_at_end(loop_cond);
    let i = load_i64(sess, index)?;
    let more = sess.2.build_int_compare(IntPredicate::ULT, i, argc, "").map_err(builder_fail)?;
    sess.2.build_conditional_branch(more, loop_body, loop_done).map_err(builder_fail)?;
    sess.2.position_at_end(loop_body);
    let slot_ptr = offset_buffer_elem_ptr(sess, ptr_ty(sess).into(), argv, i)?;
    let text = load_ptr(sess, slot_ptr)?;
    let length = emit_strlen(sess, f, text, span)?;
    let entry = offset_buffer_elem_ptr(sess, elem_ty, table, i)?;
    let entry_data = struct_gep(sess, elem_key, entry, 0, "", span)?;
    store_key(sess, entry_data, text.into())?;
    let entry_len = struct_gep(sess, elem_key, entry, 1, "", span)?;
    store_key(sess, entry_len, length.into())?;
    let stepped = sess.2.build_int_add(i, i64_ty.const_int(1, false), "").map_err(builder_fail)?;
    store_key(sess, index, stepped.into())?;
    sess.2.build_unconditional_branch(loop_cond).map_err(builder_fail)?;
    sess.2.position_at_end(loop_done);
    let data_slot = slice_gep(sess, view_global.as_pointer_value(), 0, "")?;
    store_key(sess, data_slot, table.into())?;
    let len_slot = slice_gep(sess, view_global.as_pointer_value(), 1, "")?;
    store_key(sess, len_slot, argc.into())?;
    sess.2.build_unconditional_branch(ready).map_err(builder_fail)?;

    sess.2.position_at_end(ready);
    store_key(sess, built_global.as_pointer_value(), i64_ty.const_int(1, false).into())?;
    copy_value(sess, ret_key, out, view_global.as_pointer_value(), span)?;
    Ok(())
}

// Reads one line from standard input into a fresh `Collections.String`.
//
// Byte at a time, which is the point rather than an oversight. A larger
// read would consume bytes past the newline, and an unbuffered file
// descriptor has nowhere to put them back — the next `read_line` would
// silently lose them, and so would anything else reading the same
// descriptor. Buffering belongs to a reader the program owns, not to a
// primitive that hands the descriptor back after every call.
//
// The newline is consumed but not included: a line's content is what the
// caller wants, and the terminator is an artifact of the encoding. End of
// input with nothing read is `Err(EndOfInput)` rather than an empty
// string, so a program can tell "a blank line" from "no more lines". Bytes
// already read followed by end of input are returned as a final line.
//
// The bytes are validated as UTF-8 before they become a `String`, through
// the same scan `string_from_slice` runs. Standard input is the least
// controlled source a string can have, and the language validates string
// construction from any slice whose contents are not settled at compile
// time; a line is one of those.
fn native_read_line<'ctx>(sess: &mut Session<'ctx, '_, '_>, f: FunctionValue<'ctx>, ret_key: i64, out: PointerValue<'ctx>, span: (i64, i64, i64)) -> Result<PointerValue<'ctx>, CodegenError> {
    let i64_ty = sess.0.i64_type();
    let str_key = result_arg_key(sess, ret_key, 0);
    let err_key = result_arg_key(sess, ret_key, 1);
    let capacity = alloca_raw(sess, i64_ty.into(), "cap", span)?;
    let length = alloca_raw(sess, i64_ty.into(), "len", span)?;
    let buffer = alloca_raw(sess, ptr_ty(sess).into(), "buf", span)?;
    let byte_slot = alloca_raw(sess, sess.0.i8_type().into(), "byte", span)?;
    let start_capacity = i64_ty.const_int(READ_LINE_CAPACITY, false);
    store_key(sess, capacity, start_capacity.into())?;
    store_key(sess, length, i64_ty.const_zero().into())?;
    let malloc = extern_malloc(sess);
    let first = sess.2.build_call(malloc, &[into_meta(start_capacity.into())], "").map_err(builder_fail)?;
    let initial = match first.try_as_basic_value() {
        ValueKind::Basic(value) => value.into_pointer_value(),
        ValueKind::Instruction(inst) => {
            return Err(builder_error(span.0, span.1, span.2, &format!("internal: malloc returned void ({:?})", inst.get_opcode())));
        }
    };
    store_key(sess, buffer, initial.into())?;
    let after = new_block(sess, f, "line_after");
    let alloc_failed = new_block(sess, f, "line_alloc_fail");
    let scan = new_block(sess, f, "line_scan");
    let null = is_null_ptr(sess, initial)?;
    sess.2.build_conditional_branch(null, alloc_failed, scan).map_err(builder_fail)?;

    sess.2.position_at_end(alloc_failed);
    emit_payload_error(sess, ret_key, err_key, seeded_enum_variant_tag(sess, SEED_SYM_ALLOC_FAILED, span)?, start_capacity, out, span)?;
    sess.2.build_unconditional_branch(after).map_err(builder_fail)?;

    sess.2.position_at_end(scan);
    let cond = new_block(sess, f, "line_cond");
    let finish = new_block(sess, f, "line_finish");
    sess.2.build_unconditional_branch(cond).map_err(builder_fail)?;
    sess.2.position_at_end(cond);
    let read_call = sess.2.build_call(extern_read(sess), &[into_meta(sess.0.i32_type().const_zero().into()), into_meta(byte_slot.into()), into_meta(i64_ty.const_int(1, false).into())], "").map_err(builder_fail)?;
    let got = libc_io_result(sess, read_call, span)?;
    // A non-positive result ends the line: zero is end of input, negative
    // is a read error. Both stop here, and `finish` decides between
    // returning what was read and reporting end of input.
    let progressed = sess.2.build_int_compare(IntPredicate::SGT, got, i64_ty.const_zero(), "").map_err(builder_fail)?;
    let keep = new_block(sess, f, "line_keep");
    let retry = new_block(sess, f, "line_retry");
    let stopped = new_block(sess, f, "line_stopped");
    sess.2.build_conditional_branch(progressed, keep, stopped).map_err(builder_fail)?;
    sess.2.position_at_end(stopped);
    let failed = sess.2.build_int_compare(IntPredicate::EQ, got, i64_ty.const_all_ones(), "").map_err(builder_fail)?;
    let failure = new_block(sess, f, "line_failure");
    sess.2.build_conditional_branch(failed, failure, finish).map_err(builder_fail)?;
    sess.2.position_at_end(failure);
    let interrupted = is_eintr(sess, got, span)?;
    sess.2.build_conditional_branch(interrupted, retry, finish).map_err(builder_fail)?;
    sess.2.position_at_end(retry);
    sess.2.build_unconditional_branch(cond).map_err(builder_fail)?;
    sess.2.position_at_end(keep);
    let byte = load_i8(sess, byte_slot)?;
    let is_newline = sess.2.build_int_compare(IntPredicate::EQ, byte, sess.0.i8_type().const_int(10, false), "").map_err(builder_fail)?;
    let store_byte = new_block(sess, f, "line_store");
    sess.2.build_conditional_branch(is_newline, finish, store_byte).map_err(builder_fail)?;
    sess.2.position_at_end(store_byte);
    let used = load_i64(sess, length)?;
    let room = load_i64(sess, capacity)?;
    let full = sess.2.build_int_compare(IntPredicate::UGE, used, room, "").map_err(builder_fail)?;
    let grow = new_block(sess, f, "line_grow");
    let place = new_block(sess, f, "line_place");
    sess.2.build_conditional_branch(full, grow, place).map_err(builder_fail)?;
    sess.2.position_at_end(grow);
    let doubled = sess.2.build_int_mul(room, i64_ty.const_int(2, false), "").map_err(builder_fail)?;
    let old = load_ptr(sess, buffer)?;
    let realloc = extern_realloc(sess);
    let grown_call = sess.2.build_call(realloc, &[into_meta(old.into()), into_meta(doubled.into())], "").map_err(builder_fail)?;
    let grown = match grown_call.try_as_basic_value() {
        ValueKind::Basic(value) => value.into_pointer_value(),
        ValueKind::Instruction(inst) => {
            return Err(builder_error(span.0, span.1, span.2, &format!("internal: realloc returned void ({:?})", inst.get_opcode())));
        }
    };
    // `realloc` returning null leaves the old block valid, so it is freed
    // here rather than leaked.
    //
    // The line so far is *not* returned. The byte that triggered the growth
    // has already been taken off the descriptor and cannot be put back, so
    // there is no line left to hand over: returning `Ok` with the bytes that
    // happened to fit would drop that byte and every byte after it while
    // reporting success, and no caller could tell that truncated line from a
    // complete one. An allocation that failed is reported as one, exactly as
    // the initial allocation is.
    let grow_failed = is_null_ptr(sess, grown)?;
    let grow_fail_block = new_block(sess, f, "line_grow_fail");
    let grow_ok = new_block(sess, f, "line_grow_ok");
    sess.2.build_conditional_branch(grow_failed, grow_fail_block, grow_ok).map_err(builder_fail)?;

    sess.2.position_at_end(grow_fail_block);
    let free_partial = extern_free(sess);
    sess.2.build_call(free_partial, &[into_meta(old.into())], "").map_err(builder_fail)?;
    emit_payload_error(sess, ret_key, err_key, seeded_enum_variant_tag(sess, SEED_SYM_ALLOC_FAILED, span)?, doubled, out, span)?;
    sess.2.build_unconditional_branch(after).map_err(builder_fail)?;

    sess.2.position_at_end(grow_ok);
    store_key(sess, buffer, grown.into())?;
    store_key(sess, capacity, doubled.into())?;
    sess.2.build_unconditional_branch(place).map_err(builder_fail)?;
    sess.2.position_at_end(place);
    let target_base = load_ptr(sess, buffer)?;
    let at = load_i64(sess, length)?;
    let target = byte_elem_ptr(sess, target_base, at)?;
    sess.2.build_store(target, byte).map_err(builder_fail)?;
    let advanced = sess.2.build_int_add(at, i64_ty.const_int(1, false), "").map_err(builder_fail)?;
    store_key(sess, length, advanced.into())?;
    sess.2.build_unconditional_branch(cond).map_err(builder_fail)?;

    sess.2.position_at_end(finish);
    let final_len = load_i64(sess, length)?;
    let final_buf = load_ptr(sess, buffer)?;

    // A failed read is a failed read, not the end of the stream.
    //
    // `finish` is reached three ways: the read returned zero, the read
    // failed, or a newline arrived. Reporting the middle one as
    // `EndOfInput` told a caller the stream had ended cleanly when the
    // device had in fact errored, and returning the bytes read so far as
    // `Ok` handed back a line that was never terminated — the same defect
    // the allocation path was fixed for, one screen up.
    //
    let read_failed = sess.2.build_int_compare(IntPredicate::EQ, got, i64_ty.const_all_ones(), "").map_err(builder_fail)?;
    let failed_block = new_block(sess, f, "line_read_failed");
    let ended_block = new_block(sess, f, "line_ended");
    sess.2.build_conditional_branch(read_failed, failed_block, ended_block).map_err(builder_fail)?;

    sess.2.position_at_end(failed_block);
    let free_failed = extern_free(sess);
    sess.2.build_call(free_failed, &[into_meta(final_buf.into())], "").map_err(builder_fail)?;
    let errno = runtime_errno(sess, span)?;
    emit_payload_error(sess, ret_key, err_key, seeded_enum_variant_tag(sess, SEED_SYM_READ_FAILED, span)?, errno, out, span)?;
    sess.2.build_unconditional_branch(after).map_err(builder_fail)?;

    sess.2.position_at_end(ended_block);
    let ended = sess.2.build_int_compare(IntPredicate::SLE, got, i64_ty.const_zero(), "").map_err(builder_fail)?;
    let empty = sess.2.build_int_compare(IntPredicate::EQ, final_len, i64_ty.const_zero(), "").map_err(builder_fail)?;
    let at_end = sess.2.build_and(ended, empty, "").map_err(builder_fail)?;
    let end_block = new_block(sess, f, "line_end_of_input");
    let line_block = new_block(sess, f, "line_value");
    sess.2.build_conditional_branch(at_end, end_block, line_block).map_err(builder_fail)?;

    sess.2.position_at_end(end_block);
    // Nothing was read and the stream is over: the buffer is released here
    // rather than handed back inside a `String` nobody asked for.
    let free = extern_free(sess);
    sess.2.build_call(free, &[into_meta(final_buf.into())], "").map_err(builder_fail)?;
    let end_tag = seeded_enum_variant_tag(sess, SEED_SYM_END_OF_INPUT, span)?;
    let end_val = build_enum_value(sess, err_key, end_tag, &[], span)?;
    let end_result = build_result_err(sess, ret_key, err_key, end_val, span)?;
    copy_to_out(sess, ret_key, out, end_result, span)?;
    sess.2.build_unconditional_branch(after).map_err(builder_fail)?;

    sess.2.position_at_end(line_block);
    // A line arrives from outside the process, so its bytes are precisely the
    // "a slice can come from anywhere" case the language validates on string
    // construction — no different from `string_from_slice`, and rather more
    // exposed. Storing them unchecked would let `read_line` be the one
    // constructor able to hand back a `String` holding malformed UTF-8, and
    // every reader of that string would inherit a guarantee the language
    // states but this path never established.
    let line_valid = new_block(sess, f, "line_utf8_ok");
    let line_invalid = new_block(sess, f, "line_utf8_bad");
    emit_utf8_scan(sess, f, final_buf, final_len, line_valid, line_invalid, span)?;

    sess.2.position_at_end(line_invalid);
    let free_malformed = extern_free(sess);
    sess.2.build_call(free_malformed, &[into_meta(final_buf.into())], "").map_err(builder_fail)?;
    let invalid_tag = seeded_enum_variant_tag(sess, SEED_SYM_INVALID_UTF8, span)?;
    let invalid_val = build_enum_value(sess, err_key, invalid_tag, &[], span)?;
    let invalid_result = build_result_err(sess, ret_key, err_key, invalid_val, span)?;
    copy_to_out(sess, ret_key, out, invalid_result, span)?;
    sess.2.build_unconditional_branch(after).map_err(builder_fail)?;

    sess.2.position_at_end(line_valid);
    let line = declare_local(sess, str_key, "line", span)?;
    let line_data = struct_gep(sess, str_key, line, 0, "", span)?;
    store_key(sess, line_data, final_buf.into())?;
    let line_len = struct_gep(sess, str_key, line, 1, "", span)?;
    store_key(sess, line_len, final_len.into())?;
    let ok_result = build_result_ok(sess, ret_key, str_key, line, span)?;
    copy_to_out(sess, ret_key, out, ok_result, span)?;
    sess.2.build_unconditional_branch(after).map_err(builder_fail)?;
    sess.2.position_at_end(after);
    Ok(out)
}

/// Starting capacity for a `read_line` buffer, doubled as needed.
const READ_LINE_CAPACITY: u64 = 128;

// Writes `Err(<variant>(payload))` into the caller's return slot, for the
// error variants that carry exactly one integer.
//
// `read_line` has two allocation sites — the initial buffer and every
// doubling — and both report failure the same way. Building that error in
// one place keeps the variant and its payload derived from the declared
// surface once: both sites consume the sealed registry tag.
fn emit_payload_error<'ctx>(
    sess: &mut Session<'ctx, '_, '_>,
    ret_key: i64,
    err_key: i64,
    variant_tag: i64,
    payload: IntValue<'ctx>,
    out: PointerValue<'ctx>,
    span: (i64, i64, i64),
) -> Result<(), CodegenError> {
    let payload_key = variant_payload_key(sess, err_key, variant_tag, 0, span)?;
    let slot = declare_local(sess, payload_key, "payload", span)?;
    store_key(sess, slot, payload.into())?;
    let value = build_enum_value(sess, err_key, variant_tag, &[(payload_key, slot)], span)?;
    let result = build_result_err(sess, ret_key, err_key, value, span)?;
    copy_to_out(sess, ret_key, out, result, span)
}

// Reads the descriptor out of a scalar-layout handle (`File.Handle` or
// `Net.Socket`).  A scalar handle *is* its integer — there is no struct to
// GEP through — so this is the plain load the layout declares, not a field
// read from a shared envelope.
fn net_fd_of_handle<'ctx>(sess: &mut Session<'ctx, '_, '_>, handle: PointerValue<'ctx>) -> Result<IntValue<'ctx>, CodegenError> {
    load_i64(sess, handle)
}

fn net_errno<'ctx>(sess: &mut Session<'ctx, '_, '_>, span: (i64, i64, i64)) -> Result<IntValue<'ctx>, CodegenError> {
    let abi = sess.13.abi();
    if abi.socket_error_is_value {
        let error_fn = extern_fn(sess, abi.socket_error_accessor, sess.0.i32_type().fn_type(&[], false));
        let call = sess.2.build_call(error_fn, &[], "").map_err(builder_fail)?;
        let code = match call.try_as_basic_value() {
            ValueKind::Basic(value) => value.into_int_value(),
            ValueKind::Instruction(inst) => return Err(builder_error(span.0, span.1, span.2, &format!("internal: socket error accessor returned void ({:?})", inst.get_opcode()))),
        };
        return sess.2.build_int_s_extend(code, sess.0.i64_type(), "").map_err(builder_fail);
    }
    runtime_errno(sess, span)
}

fn net_fault_result<'ctx>(sess: &mut Session<'ctx, '_, '_>, ret_key: i64, out: PointerValue<'ctx>, span: (i64, i64, i64)) -> Result<(), CodegenError> {
    let err_key = result_arg_key(sess, ret_key, 1);
    let tag = seeded_enum_variant_tag(sess, SEED_SYM_SYSTEM_FAULT, span)?;
    let f0 = variant_payload_key(sess, err_key, tag, 0, span)?;
    let code = declare_local(sess, f0, "errno", span)?;
    let err = net_errno(sess, span)?;
    store_key(sess, code, err.into())?;
    let fail_val = build_enum_value(sess, err_key, tag, &[(f0, code)], span)?;
    let err_result = build_result_err(sess, ret_key, err_key, fail_val, span)?;
    copy_to_out(sess, ret_key, out, err_result, span)
}

fn net_rc_branch<'ctx>(sess: &mut Session<'ctx, '_, '_>, f: FunctionValue<'ctx>, ret_key: i64, out: PointerValue<'ctx>, rc: IntValue<'ctx>, span: (i64, i64, i64)) -> Result<BasicBlockId<'ctx>, CodegenError> {
    let zero = rc.get_type().const_zero();
    let ok_cmp = sess.2.build_int_compare(IntPredicate::SGE, rc, zero, "").map_err(builder_fail)?;
    let fail_block = new_block(sess, f, "net_fail");
    let ok_block = new_block(sess, f, "net_ok");
    let after = new_block(sess, f, "net_after");
    sess.2.build_conditional_branch(ok_cmp, ok_block, fail_block).map_err(builder_fail)?;
    sess.2.position_at_end(fail_block);
    net_fault_result(sess, ret_key, out, span)?;
    sess.2.build_unconditional_branch(after).map_err(builder_fail)?;
    sess.2.position_at_end(ok_block);
    Ok(after)
}

fn build_sockaddr_in<'ctx>(sess: &mut Session<'ctx, '_, '_>, port: IntValue<'ctx>, span: (i64, i64, i64)) -> Result<PointerValue<'ctx>, CodegenError> {
    // `sockaddr_in` as a typed struct: i16 family, i16 port, i32 addr, [8 x i8] zero.
    let sa_ty = sess.0.struct_type(
        &[
            sess.0.i16_type().into(),
            sess.0.i16_type().into(),
            sess.0.i32_type().into(),
            sess.0.i8_type().array_type(8).into(),
        ],
        false,
    );
    let sa = alloca_raw(sess, sa_ty.into(), "sa", span)?;
    let fam = sess.2.build_struct_gep(sa_ty, sa, 0, "").map_err(builder_fail)?;
    sess.2.build_store(fam, sess.0.i16_type().const_int(2, false)).map_err(builder_fail)?;
    let port_field = sess.2.build_struct_gep(sa_ty, sa, 1, "").map_err(builder_fail)?;
    let port16 = sess.2.build_int_truncate(port, sess.0.i16_type(), "").map_err(builder_fail)?;
    let eight = sess.0.i16_type().const_int(8, false);
    let hi = sess.2.build_right_shift(port16, eight, false, "").map_err(builder_fail)?;
    let lo = sess.2.build_left_shift(port16, eight, "").map_err(builder_fail)?;
    let swapped = sess.2.build_or(lo, hi, "").map_err(builder_fail)?;
    sess.2.build_store(port_field, swapped).map_err(builder_fail)?;
    let addr_field = sess.2.build_struct_gep(sa_ty, sa, 2, "").map_err(builder_fail)?;
    sess.2.build_store(addr_field, sess.0.i32_type().const_zero()).map_err(builder_fail)?;
    let zero_field = sess.2.build_struct_gep(sa_ty, sa, 3, "").map_err(builder_fail)?;
    let zero8 = sess.0.i8_type().const_zero();
    sess.2.build_memset(zero_field, 1, zero8, sess.0.i64_type().const_int(8, false)).map_err(builder_fail)?;
    Ok(sa)
}

fn build_net_sock_ok<'ctx>(sess: &mut Session<'ctx, '_, '_>, ret_key: i64, out: PointerValue<'ctx>, fd: IntValue<'ctx>, span: (i64, i64, i64)) -> Result<(), CodegenError> {
    let sock_key = result_arg_key(sess, ret_key, 0);
    let sock_val = declare_local(sess, sock_key, "sock", span)?;
    // `Net.Socket` is a scalar handle: it is the descriptor integer.
    store_key(sess, sock_val, fd.into())?;
    let ok_result = build_result_ok(sess, ret_key, sock_key, sock_val, span)?;
    copy_to_out(sess, ret_key, out, ok_result, span)
}

fn native_net_socket<'ctx>(sess: &mut Session<'ctx, '_, '_>, f: FunctionValue<'ctx>, ret_key: i64, out: PointerValue<'ctx>, span: (i64, i64, i64)) -> Result<PointerValue<'ctx>, CodegenError> {
    let after = new_block(sess, f, "socket_after");
    if sess.13.abi().needs_winsock_init {
        let started_global = runtime_global(sess, ".cnb.wsa.started", sess.0.i64_type().into());
        let started = load_i64(sess, started_global.as_pointer_value())?;
        let already_started = sess.2.build_int_compare(IntPredicate::NE, started, sess.0.i64_type().const_zero(), "").map_err(builder_fail)?;
        let initialize = new_block(sess, f, "socket_initialize");
        let socket_call = new_block(sess, f, "socket_call");
        sess.2.build_conditional_branch(already_started, socket_call, initialize).map_err(builder_fail)?;
        sess.2.position_at_end(initialize);
        let data_ty = sess.0.i64_type().array_type(64);
        let data = alloca_raw(sess, data_ty.into(), "wsa_data", span)?;
        let startup = sess.2.build_call(extern_wsa_startup(sess), &[into_meta(sess.0.i16_type().const_int(0x202, false).into()), into_meta(data.into())], "").map_err(builder_fail)?;
        let status = match startup.try_as_basic_value() {
            ValueKind::Basic(value) => value.into_int_value(),
            ValueKind::Instruction(inst) => return Err(builder_error(span.0, span.1, span.2, &format!("internal: WSAStartup returned void ({:?})", inst.get_opcode()))),
        };
        let initialized = sess.2.build_int_compare(IntPredicate::EQ, status, sess.0.i32_type().const_zero(), "").map_err(builder_fail)?;
        let startup_failed = new_block(sess, f, "socket_startup_failed");
        let startup_ready = new_block(sess, f, "socket_startup_ready");
        sess.2.build_conditional_branch(initialized, startup_ready, startup_failed).map_err(builder_fail)?;
        sess.2.position_at_end(startup_failed);
        let code = sess.2.build_int_s_extend(status, sess.0.i64_type(), "").map_err(builder_fail)?;
        system_fault_result(sess, ret_key, out, code, span)?;
        sess.2.build_unconditional_branch(after).map_err(builder_fail)?;
        sess.2.position_at_end(startup_ready);
        store_key(sess, started_global.as_pointer_value(), sess.0.i64_type().const_int(1, false).into())?;
        sess.2.build_unconditional_branch(socket_call).map_err(builder_fail)?;
        sess.2.position_at_end(socket_call);
    }
    let domain = sess.0.i32_type().const_int(2, false);
    let stype = sess.0.i32_type().const_int(1, false);
    let proto = sess.0.i32_type().const_zero();
    let call = sess.2.build_call(extern_socket(sess), &[into_meta(domain.into()), into_meta(stype.into()), into_meta(proto.into())], "").map_err(builder_fail)?;
    let rc = socket_result(sess, call, span)?;
    let socket_ok = sess.2.build_int_compare(IntPredicate::SGE, rc, sess.0.i64_type().const_zero(), "").map_err(builder_fail)?;
    let socket_failed = new_block(sess, f, "socket_failed");
    let socket_ready = new_block(sess, f, "socket_ready");
    sess.2.build_conditional_branch(socket_ok, socket_ready, socket_failed).map_err(builder_fail)?;
    sess.2.position_at_end(socket_failed);
    net_fault_result(sess, ret_key, out, span)?;
    sess.2.build_unconditional_branch(after).map_err(builder_fail)?;
    sess.2.position_at_end(socket_ready);
    build_net_sock_ok(sess, ret_key, out, rc, span)?;
    sess.2.build_unconditional_branch(after).map_err(builder_fail)?;
    sess.2.position_at_end(after);
    Ok(out)
}

fn native_net_bind<'ctx>(sess: &mut Session<'ctx, '_, '_>, f: FunctionValue<'ctx>, locals: &Locals<'ctx>, ret_key: i64, out: PointerValue<'ctx>, span: (i64, i64, i64)) -> Result<(), CodegenError> {
    let p0 = get_local(locals, 0, span)?;
    let p1 = get_local(locals, 1, span)?;
    let handle = load_ptr(sess, p0)?;
    let fd = net_fd_of_handle(sess, handle)?;
    let port = load_i64(sess, p1)?;
    let sa = build_sockaddr_in(sess, port, span)?;
    let addr_len = sess.0.i32_type().const_int(16, false);
    let fd_arg = socket_argument(sess, fd)?;
    let call = sess.2.build_call(extern_bind(sess), &[fd_arg, into_meta(sa.into()), into_meta(addr_len.into())], "").map_err(builder_fail)?;
    let rc = match call.try_as_basic_value() {
        ValueKind::Basic(bv) => bv.into_int_value(),
        ValueKind::Instruction(inst) => {
            return Err(builder_error(span.0, span.1, span.2, &format!("internal: bind returned void ({:?})", inst.get_opcode())));
        }
    };
    let after = net_rc_branch(sess, f, ret_key, out, rc, span)?;
    let unit_key = result_arg_key(sess, ret_key, 0);
    let unit_val = build_unit_value(sess, unit_key, span)?;
    let ok_result = build_result_ok(sess, ret_key, unit_key, unit_val, span)?;
    copy_to_out(sess, ret_key, out, ok_result, span)?;
    sess.2.build_unconditional_branch(after).map_err(builder_fail)?;
    sess.2.position_at_end(after);
    Ok(())
}

fn native_net_listen<'ctx>(sess: &mut Session<'ctx, '_, '_>, f: FunctionValue<'ctx>, locals: &Locals<'ctx>, ret_key: i64, out: PointerValue<'ctx>, span: (i64, i64, i64)) -> Result<(), CodegenError> {
    let p0 = get_local(locals, 0, span)?;
    let p1 = get_local(locals, 1, span)?;
    let handle = load_ptr(sess, p0)?;
    let fd = net_fd_of_handle(sess, handle)?;
    let backlog = load_i64(sess, p1)?;
    let backlog32 = sess.2.build_int_truncate(backlog, sess.0.i32_type(), "").map_err(builder_fail)?;
    let fd_arg = socket_argument(sess, fd)?;
    let call = sess.2.build_call(extern_listen(sess), &[fd_arg, into_meta(backlog32.into())], "").map_err(builder_fail)?;
    let rc = match call.try_as_basic_value() {
        ValueKind::Basic(bv) => bv.into_int_value(),
        ValueKind::Instruction(inst) => {
            return Err(builder_error(span.0, span.1, span.2, &format!("internal: listen returned void ({:?})", inst.get_opcode())));
        }
    };
    let after = net_rc_branch(sess, f, ret_key, out, rc, span)?;
    let unit_key = result_arg_key(sess, ret_key, 0);
    let unit_val = build_unit_value(sess, unit_key, span)?;
    let ok_result = build_result_ok(sess, ret_key, unit_key, unit_val, span)?;
    copy_to_out(sess, ret_key, out, ok_result, span)?;
    sess.2.build_unconditional_branch(after).map_err(builder_fail)?;
    sess.2.position_at_end(after);
    Ok(())
}

fn native_net_accept<'ctx>(sess: &mut Session<'ctx, '_, '_>, f: FunctionValue<'ctx>, locals: &Locals<'ctx>, ret_key: i64, out: PointerValue<'ctx>, span: (i64, i64, i64)) -> Result<PointerValue<'ctx>, CodegenError> {
    let p0 = get_local(locals, 0, span)?;
    let handle = load_ptr(sess, p0)?;
    let fd = net_fd_of_handle(sess, handle)?;
    let null_ptr = ptr_ty(sess).const_null();
    let fd_arg = socket_argument(sess, fd)?;
    let call = sess.2.build_call(extern_accept(sess), &[fd_arg, into_meta(null_ptr.into()), into_meta(null_ptr.into())], "").map_err(builder_fail)?;
    let rc = socket_result(sess, call, span)?;
    let after = net_rc_branch(sess, f, ret_key, out, rc, span)?;
    build_net_sock_ok(sess, ret_key, out, rc, span)?;
    sess.2.build_unconditional_branch(after).map_err(builder_fail)?;
    sess.2.position_at_end(after);
    Ok(out)
}

fn native_net_send<'ctx>(sess: &mut Session<'ctx, '_, '_>, f: FunctionValue<'ctx>, locals: &Locals<'ctx>, ret_key: i64, out: PointerValue<'ctx>, span: (i64, i64, i64)) -> Result<PointerValue<'ctx>, CodegenError> {
    let p0 = get_local(locals, 0, span)?;
    let p1 = get_local(locals, 1, span)?;
    let handle = load_ptr(sess, p0)?;
    let fd = net_fd_of_handle(sess, handle)?;
    let data = slice_data(sess, p1)?;
    let len = slice_len_of(sess, p1)?;
    let flags = sess.0.i32_type().const_zero();
    let fd_arg = socket_argument(sess, fd)?;
    let call = sess.2.build_call(extern_send(sess), &[fd_arg, into_meta(data.into()), into_meta(len.into()), into_meta(flags.into())], "").map_err(builder_fail)?;
    let rc = socket_result(sess, call, span)?;
    let after = net_rc_branch(sess, f, ret_key, out, rc, span)?;
    let usize_key = result_arg_key(sess, ret_key, 0);
    let sent = declare_local(sess, usize_key, "sent", span)?;
    store_key(sess, sent, rc.into())?;
    let ok_result = build_result_ok(sess, ret_key, usize_key, sent, span)?;
    copy_to_out(sess, ret_key, out, ok_result, span)?;
    sess.2.build_unconditional_branch(after).map_err(builder_fail)?;
    sess.2.position_at_end(after);
    Ok(out)
}

fn native_net_close<'ctx>(sess: &mut Session<'ctx, '_, '_>, locals: &Locals<'ctx>, ret_key: i64, out: PointerValue<'ctx>, span: (i64, i64, i64)) -> Result<(), CodegenError> {
    let p0 = get_local(locals, 0, span)?;
    let fd = net_fd_of_handle(sess, p0)?;
    let abi = sess.13.abi();
    let argument = if abi.socket_close_is_64 {
        into_meta(fd.into())
    } else {
        let fd32 = sess.2.build_int_truncate(fd, sess.0.i32_type(), "").map_err(builder_fail)?;
        into_meta(fd32.into())
    };
    sess.2.build_call(extern_socket_close(sess), &[argument], "").map_err(builder_fail)?;
    build_unit_value_into(sess, ret_key, out, span)
}

/// `ENOMEM`, reported when an allocation this native needs (as opposed to
/// the child's own memory, which the kernel is responsible for) fails.
const ENOMEM: u64 = 12;

/// Builds a fresh, NUL-terminated heap copy of a Cinnabar `String`'s bytes
/// -- what `execvp`'s `argv` entries need and a length-prefixed `String`
/// does not provide on its own. Unlike `nul_terminated_path`'s
/// fixed stack buffer (fine for the one path a single `File.open` call
/// needs), this runs once per element of a runtime-sized argv list: an
/// `alloca` emitted inside that loop's body would be one stack slot shared
/// by every iteration, silently aliasing every earlier argument's storage
/// once the loop moved on. A fresh `malloc` per element is what makes each
/// argument's buffer outlive the loop that built it.
///
/// Returns a null pointer if the allocation failed, exactly what
/// `is_null_ptr` on the result reports -- the caller branches on that
/// before ever reading the buffer, so the copy below never runs against a
/// null destination.
fn heap_nul_terminated<'ctx>(
    sess: &mut Session<'ctx, '_, '_>,
    f: FunctionValue<'ctx>,
    data: PointerValue<'ctx>,
    len: IntValue<'ctx>,
    span: (i64, i64, i64),
) -> Result<PointerValue<'ctx>, CodegenError> {
    let one = sess.0.i64_type().const_int(1, false);
    let buf_size = sess.2.build_int_add(len, one, "").map_err(builder_fail)?;
    let malloc = extern_malloc(sess);
    let call = sess.2.build_call(malloc, &[into_meta(buf_size.into())], "").map_err(builder_fail)?;
    let buf = match call.try_as_basic_value() {
        ValueKind::Basic(bv) => bv.into_pointer_value(),
        ValueKind::Instruction(inst) => {
            return Err(builder_error(span.0, span.1, span.2, &format!("internal: malloc returned void ({:?})", inst.get_opcode())));
        }
    };
    let result_slot = alloca_raw(sess, ptr_ty(sess).into(), "argbuf", span)?;
    store_key(sess, result_slot, buf.into())?;
    let failed = is_null_ptr(sess, buf)?;
    let copy_block = new_block(sess, f, "argbuf_copy");
    let after = new_block(sess, f, "argbuf_after");
    sess.2.build_conditional_branch(failed, after, copy_block).map_err(builder_fail)?;
    sess.2.position_at_end(copy_block);
    sess.2.build_memcpy(buf, 1, data, 1, len).map_err(builder_fail)?;
    let nul_off = byte_elem_ptr(sess, buf, len)?;
    sess.2.build_store(nul_off, sess.0.i8_type().const_zero()).map_err(builder_fail)?;
    sess.2.build_unconditional_branch(after).map_err(builder_fail)?;
    sess.2.position_at_end(after);
    load_ptr(sess, result_slot)
}

fn native_process_spawn<'ctx>(
    sess: &mut Session<'ctx, '_, '_>,
    f: FunctionValue<'ctx>,
    locals: &Locals<'ctx>,
    ret_key: i64,
    out: PointerValue<'ctx>,
    span: (i64, i64, i64),
) -> Result<PointerValue<'ctx>, CodegenError> {
    let p0 = get_local(locals, 0, span)?;
    // Windows has no fork/exec; `Process.spawn` lowers through
    // CreateProcessW with a quoted command line instead.
    if sess.13.abi().process_is_windows {
        return native_process_spawn_windows(sess, f, locals, ret_key, out, span);
    }
    let argv_key = deref_key_of(sess, get_local_key(locals, 0, span)?);
    let elem_key = list_get(sess.6, key_args_of(sess, argv_key), 0);
    let argv_ref = load_ptr(sess, p0)?;
    let dptr = struct_gep(sess, argv_key, argv_ref, 0, "", span)?;
    let lptr = struct_gep(sess, argv_key, argv_ref, 1, "", span)?;
    let data = load_ptr(sess, dptr)?;
    let len = load_i64(sess, lptr)?;

    let merge = new_block(sess, f, "spawn_merge");
    let enomem = sess.0.i64_type().const_int(ENOMEM, false);

    // The `char*[]` argv for execvp needs: `len` entries plus one trailing NULL,
    // eight bytes each regardless of target word size since this compiler
    // only targets LP64 triples.
    let one = sess.0.i64_type().const_int(1, false);
    let eight = sess.0.i64_type().const_int(8, false);
    let n_plus_1 = sess.2.build_int_add(len, one, "").map_err(builder_fail)?;
    let argv_bytes = sess.2.build_int_mul(n_plus_1, eight, "").map_err(builder_fail)?;
    let malloc = extern_malloc(sess);
    let argv_call = sess.2.build_call(malloc, &[into_meta(argv_bytes.into())], "").map_err(builder_fail)?;
    let argv_arr = match argv_call.try_as_basic_value() {
        ValueKind::Basic(bv) => bv.into_pointer_value(),
        ValueKind::Instruction(inst) => {
            return Err(builder_error(span.0, span.1, span.2, &format!("internal: malloc returned void ({:?})", inst.get_opcode())));
        }
    };
    let argv_oom = new_block(sess, f, "spawn_argv_oom");
    let build_argv = new_block(sess, f, "spawn_build_argv");
    let argv_null = is_null_ptr(sess, argv_arr)?;
    sess.2.build_conditional_branch(argv_null, argv_oom, build_argv).map_err(builder_fail)?;
    sess.2.position_at_end(argv_oom);
    system_fault_result(sess, ret_key, out, enomem, span)?;
    sess.2.build_unconditional_branch(merge).map_err(builder_fail)?;
    sess.2.position_at_end(build_argv);

    let i_slot = alloca_raw(sess, sess.0.i64_type().into(), "i", span)?;
    store_key(sess, i_slot, sess.0.i64_type().const_zero().into())?;
    let cond = new_block(sess, f, "spawn_cond");
    let body = new_block(sess, f, "spawn_body");
    let elem_oom = new_block(sess, f, "spawn_elem_oom");
    let next = new_block(sess, f, "spawn_next");
    let after_loop = new_block(sess, f, "spawn_after_loop");
    sess.2.build_unconditional_branch(cond).map_err(builder_fail)?;
    sess.2.position_at_end(cond);
    let i = load_i64(sess, i_slot)?;
    let done = sess.2.build_int_compare(IntPredicate::ULT, i, len, "").map_err(builder_fail)?;
    sess.2.build_conditional_branch(done, body, after_loop).map_err(builder_fail)?;
    sess.2.position_at_end(body);
    let elem_ty = llvm_of(sess, elem_key, span)?;
    let str_ptr = offset_buffer_elem_ptr(sess, elem_ty, data, i)?;
    let s_data_slot = struct_gep(sess, elem_key, str_ptr, 0, "", span)?;
    let s_data = load_ptr(sess, s_data_slot)?;
    let s_len_slot = struct_gep(sess, elem_key, str_ptr, 1, "", span)?;
    let s_len = load_i64(sess, s_len_slot)?;
    let buf = heap_nul_terminated(sess, f, s_data, s_len, span)?;
    let buf_null = is_null_ptr(sess, buf)?;
    sess.2.build_conditional_branch(buf_null, elem_oom, next).map_err(builder_fail)?;
    sess.2.position_at_end(elem_oom);
    system_fault_result(sess, ret_key, out, enomem, span)?;
    sess.2.build_unconditional_branch(merge).map_err(builder_fail)?;
    sess.2.position_at_end(next);
    let slot = offset_buffer_elem_ptr(sess, ptr_ty(sess).into(), argv_arr, i)?;
    store_key(sess, slot, buf.into())?;
    let i2 = sess.2.build_int_add(i, one, "").map_err(builder_fail)?;
    store_key(sess, i_slot, i2.into())?;
    sess.2.build_unconditional_branch(cond).map_err(builder_fail)?;
    sess.2.position_at_end(after_loop);
    let term_slot = offset_buffer_elem_ptr(sess, ptr_ty(sess).into(), argv_arr, len)?;
    store_key(sess, term_slot, ptr_ty(sess).const_null().into())?;

    let fork_call = sess.2.build_call(extern_fork(sess), &[], "").map_err(builder_fail)?;
    let fork_res = match fork_call.try_as_basic_value() {
        ValueKind::Basic(value) => value.into_int_value(),
        ValueKind::Instruction(inst) => return Err(builder_error(span.0, span.1, span.2, &format!("internal: fork returned void ({:?})", inst.get_opcode()))),
    };
    let zero32 = sess.0.i32_type().const_zero();
    let is_child = sess.2.build_int_compare(IntPredicate::EQ, fork_res, zero32, "").map_err(builder_fail)?;
    let child_block = new_block(sess, f, "spawn_child");
    let parent_check = new_block(sess, f, "spawn_parent_check");
    sess.2.build_conditional_branch(is_child, child_block, parent_check).map_err(builder_fail)?;

    sess.2.position_at_end(child_block);
    // `execvp` searches PATH when argv[0] has no slash and uses an explicit
    // path unchanged when it does.
    let path_ptr = load_ptr(sess, argv_arr)?;
    sess.2.build_call(extern_execvp(sess), &[into_meta(path_ptr.into()), into_meta(argv_arr.into())], "").map_err(builder_fail)?;
    sess.2.build_call(extern_exit(sess), &[into_meta(sess.0.i32_type().const_int(127, false).into())], "").map_err(builder_fail)?;
    // Not `unreachable`: that the kernel never returns from `exit_group`
    // is a fact about the kernel, not something this compiler's type
    // checker proved the way it proves a `match` exhaustive -- exactly the
    // "cannot happen with nothing proving it" `undefined_behaviour.rs`
    // exists to catch. If it somehow did return, falling through into the
    // parent's own continuation would run the rest of this native, then
    // the caller's own code, a second time in what only started as the
    // child; spinning in place is the honest terminator for a branch this
    // compiler cannot actually rule out.
    let trap = new_block(sess, f, "spawn_child_trap");
    sess.2.build_unconditional_branch(trap).map_err(builder_fail)?;
    sess.2.position_at_end(trap);
    sess.2.build_unconditional_branch(trap).map_err(builder_fail)?;

    sess.2.position_at_end(parent_check);
    // The child either replaces its image through execvp or exits after
    // execvp fails; the parent frees its argv storage after fork.
    let free = extern_free(sess);
    let free_i_slot = alloca_raw(sess, sess.0.i64_type().into(), "free_i", span)?;
    store_key(sess, free_i_slot, sess.0.i64_type().const_zero().into())?;
    let free_cond = new_block(sess, f, "spawn_free_cond");
    let free_body = new_block(sess, f, "spawn_free_body");
    let free_after = new_block(sess, f, "spawn_free_after");
    sess.2.build_unconditional_branch(free_cond).map_err(builder_fail)?;
    sess.2.position_at_end(free_cond);
    let free_i = load_i64(sess, free_i_slot)?;
    let free_more = sess.2.build_int_compare(IntPredicate::ULT, free_i, len, "").map_err(builder_fail)?;
    sess.2.build_conditional_branch(free_more, free_body, free_after).map_err(builder_fail)?;
    sess.2.position_at_end(free_body);
    let entry_slot = offset_buffer_elem_ptr(sess, ptr_ty(sess).into(), argv_arr, free_i)?;
    let entry_ptr = load_ptr(sess, entry_slot)?;
    sess.2.build_call(free, &[into_meta(entry_ptr.into())], "").map_err(builder_fail)?;
    let free_i2 = sess.2.build_int_add(free_i, one, "").map_err(builder_fail)?;
    store_key(sess, free_i_slot, free_i2.into())?;
    sess.2.build_unconditional_branch(free_cond).map_err(builder_fail)?;
    sess.2.position_at_end(free_after);
    sess.2.build_call(free, &[into_meta(argv_arr.into())], "").map_err(builder_fail)?;

    let spawned = sess.2.build_int_compare(IntPredicate::SGT, fork_res, zero32, "").map_err(builder_fail)?;
    let parent_ok = new_block(sess, f, "spawn_parent_ok");
    let clone_failed = new_block(sess, f, "spawn_clone_failed");
    sess.2.build_conditional_branch(spawned, parent_ok, clone_failed).map_err(builder_fail)?;
    sess.2.position_at_end(clone_failed);
    let code = runtime_errno(sess, span)?;
    system_fault_result(sess, ret_key, out, code, span)?;
    sess.2.build_unconditional_branch(merge).map_err(builder_fail)?;
    sess.2.position_at_end(parent_ok);
    let child_key = result_arg_key(sess, ret_key, 0);
    let child_val = declare_local(sess, child_key, "child", span)?;
    // `Process.Child` is a scalar handle: it is the child's pid.
    let pid64 = sess.2.build_int_s_extend(fork_res, sess.0.i64_type(), "").map_err(builder_fail)?;
    store_key(sess, child_val, pid64.into())?;
    let ok_result = build_result_ok(sess, ret_key, child_key, child_val, span)?;
    copy_to_out(sess, ret_key, out, ok_result, span)?;
    sess.2.build_unconditional_branch(merge).map_err(builder_fail)?;

    sess.2.position_at_end(merge);
    Ok(out)
}

fn native_process_wait<'ctx>(
    sess: &mut Session<'ctx, '_, '_>,
    f: FunctionValue<'ctx>,
    locals: &Locals<'ctx>,
    ret_key: i64,
    out: PointerValue<'ctx>,
    span: (i64, i64, i64),
) -> Result<PointerValue<'ctx>, CodegenError> {
    let p0 = get_local(locals, 0, span)?;
    // Windows has no waitpid; `Process.wait` lowers through
    // WaitForSingleObject and GetExitCodeProcess instead.
    if sess.13.abi().process_is_windows {
        return native_process_wait_windows(sess, f, locals, ret_key, out, span);
    }
    let pid = load_i64(sess, p0)?;
    let status_slot = alloca_raw(sess, sess.0.i32_type().into(), "status", span)?;
    let pid32 = sess.2.build_int_truncate(pid, sess.0.i32_type(), "").map_err(builder_fail)?;
    let wait_call = sess.2.build_call(extern_waitpid(sess), &[into_meta(pid32.into()), into_meta(status_slot.into()), into_meta(sess.0.i32_type().const_zero().into())], "").map_err(builder_fail)?;
    let raw = match wait_call.try_as_basic_value() {
        ValueKind::Basic(value) => value.into_int_value(),
        ValueKind::Instruction(inst) => return Err(builder_error(span.0, span.1, span.2, &format!("internal: waitpid returned void ({:?})", inst.get_opcode()))),
    };
    let join = libc_result_branch(sess, f, ret_key, out, raw, span)?;
    // The kernel packs the child's status as `((exit_code & 0xff) << 8) |
    // termination_signal`: a clean exit has the low byte zero. Reading a
    // signaled child's exit code as if it had one is the one case this
    // first slice does not distinguish -- the low byte can be checked by a
    // caller that cares, since `wait`'s own contract is "the encoded status
    // word `waitpid` returned", not a codegen-imposed simplification of it.
    let status = sess.2.build_load(sess.0.i32_type(), status_slot, "").map_err(builder_fail)?.into_int_value();
    let status64 = sess.2.build_int_z_extend(status, sess.0.i64_type(), "").map_err(builder_fail)?;
    let eight = sess.0.i64_type().const_int(8, false);
    let shifted = sess.2.build_right_shift(status64, eight, false, "").map_err(builder_fail)?;
    let mask = sess.0.i64_type().const_int(0xff, false);
    let exit_code = sess.2.build_and(shifted, mask, "").map_err(builder_fail)?;
    let code_key = result_arg_key(sess, ret_key, 0);
    let code_val = declare_local(sess, code_key, "code", span)?;
    store_key(sess, code_val, exit_code.into())?;
    let ok_result = build_result_ok(sess, ret_key, code_key, code_val, span)?;
    copy_to_out(sess, ret_key, out, ok_result, span)?;
    sess.2.build_unconditional_branch(join).map_err(builder_fail)?;
    sess.2.position_at_end(join);
    Ok(out)
}

// The Win32 `CloseHandle` entry point: takes a HANDLE pointer and returns
// a 32-bit success flag, so a kernel object is released when its handle is
// consumed.
fn extern_close_handle<'ctx>(sess: &mut Session<'ctx, '_, '_>) -> FunctionValue<'ctx> {
    extern_fn(sess, "CloseHandle", sess.0.i32_type().fn_type(&[ptr_ty(sess).into()], false))
}

// Reads `GetLastError()` as a sign-extended i64, for Windows error paths.
fn windows_last_error<'ctx>(sess: &mut Session<'ctx, '_, '_>, span: (i64, i64, i64)) -> Result<IntValue<'ctx>, CodegenError> {
    let error_fn = extern_fn(sess, "GetLastError", sess.0.i32_type().fn_type(&[], false));
    let call = sess.2.build_call(error_fn, &[], "").map_err(builder_fail)?;
    let code = match call.try_as_basic_value() {
        ValueKind::Basic(value) => value.into_int_value(),
        ValueKind::Instruction(inst) => {
            return Err(builder_error(span.0, span.1, span.2, &format!("internal: GetLastError returned void ({:?})", inst.get_opcode())));
        }
    };
    sess.2.build_int_s_extend(code, sess.0.i64_type(), "").map_err(builder_fail)
}

// Loads the byte pointer and length of argv element `i` (a Cinnabar
// `String` = { data, len }) from the slice at `data`.
fn arg_bytes<'ctx>(
    sess: &mut Session<'ctx, '_, '_>,
    data: PointerValue<'ctx>,
    elem_key: i64,
    i: IntValue<'ctx>,
    span: (i64, i64, i64),
) -> Result<(PointerValue<'ctx>, IntValue<'ctx>), CodegenError> {
    let elem_ty = llvm_of(sess, elem_key, span)?;
    let elem_ptr = offset_buffer_elem_ptr(sess, elem_ty, data, i)?;
    let sd = struct_gep(sess, elem_key, elem_ptr, 0, "", span)?;
    let sdata = load_ptr(sess, sd)?;
    let sl = struct_gep(sess, elem_key, elem_ptr, 1, "", span)?;
    let slen = load_i64(sess, sl)?;
    Ok((sdata, slen))
}

fn i8_eq<'ctx>(sess: &mut Session<'ctx, '_, '_>, a: IntValue<'ctx>, v: u64) -> Result<IntValue<'ctx>, CodegenError> {
    sess.2.build_int_compare(IntPredicate::EQ, a, sess.0.i8_type().const_int(v, false), "").map_err(builder_fail)
}

fn i1_or<'ctx>(sess: &mut Session<'ctx, '_, '_>, a: IntValue<'ctx>, b: IntValue<'ctx>) -> Result<IntValue<'ctx>, CodegenError> {
    sess.2.build_or(a, b, "").map_err(builder_fail)
}

fn i1_and<'ctx>(sess: &mut Session<'ctx, '_, '_>, a: IntValue<'ctx>, b: IntValue<'ctx>) -> Result<IntValue<'ctx>, CodegenError> {
    sess.2.build_and(a, b, "").map_err(builder_fail)
}

// Lowercases an ASCII letter byte (OR 0x20); non-letters pass through
// unchanged, and the caller only compares the result against lowercase
// letters or punctuation that OR 0x20 leaves alone.
fn i8_lower<'ctx>(sess: &mut Session<'ctx, '_, '_>, a: IntValue<'ctx>) -> Result<IntValue<'ctx>, CodegenError> {
    sess.2.build_or(a, sess.0.i8_type().const_int(0x20, false), "").map_err(builder_fail)
}

// True when `b` is a cmd.exe shell metacharacter that would be
// reinterpreted if the command line were re-parsed by a batch file.
fn is_shell_metachar<'ctx>(sess: &mut Session<'ctx, '_, '_>, b: IntValue<'ctx>) -> Result<IntValue<'ctx>, CodegenError> {
    let amp = i8_eq(sess, b, 0x26)?;
    let pipe = i8_eq(sess, b, 0x7C)?;
    let lt = i8_eq(sess, b, 0x3C)?;
    let gt = i8_eq(sess, b, 0x3E)?;
    let caret = i8_eq(sess, b, 0x5E)?;
    let lpar = i8_eq(sess, b, 0x28)?;
    let rpar = i8_eq(sess, b, 0x29)?;
    let pct = i8_eq(sess, b, 0x25)?;
    let bang = i8_eq(sess, b, 0x21)?;
    let a = i1_or(sess, amp, pipe)?;
    let b1 = i1_or(sess, lt, gt)?;
    let c1 = i1_or(sess, caret, lpar)?;
    let d1 = i1_or(sess, rpar, pct)?;
    let e1 = i1_or(sess, a, b1)?;
    let f1 = i1_or(sess, c1, d1)?;
    let g1 = i1_or(sess, e1, f1)?;
    i1_or(sess, g1, bang)
}

// Writes `count` backslash bytes into `buf` at the running offset held in
// `w_slot`, advancing the offset.  Used by the Windows command-line
// quoting pass, where backslashes before quotes and at the end of an
// argument must be doubled.
fn emit_backslash_run<'ctx>(
    sess: &mut Session<'ctx, '_, '_>,
    f: FunctionValue<'ctx>,
    buf: PointerValue<'ctx>,
    w_slot: PointerValue<'ctx>,
    count: IntValue<'ctx>,
    span: (i64, i64, i64),
) -> Result<(), CodegenError> {
    let bs_slot = alloca_raw(sess, sess.0.i64_type().into(), "bs_count", span)?;
    store_key(sess, bs_slot, count.into())?;
    let cond = new_block(sess, f, "bs_cond");
    let body = new_block(sess, f, "bs_body");
    let done = new_block(sess, f, "bs_done");
    sess.2.build_unconditional_branch(cond).map_err(builder_fail)?;
    sess.2.position_at_end(cond);
    let n = load_i64(sess, bs_slot)?;
    let more = sess.2.build_int_compare(IntPredicate::SGT, n, sess.0.i64_type().const_zero(), "").map_err(builder_fail)?;
    sess.2.build_conditional_branch(more, body, done).map_err(builder_fail)?;
    sess.2.position_at_end(body);
    let w = load_i64(sess, w_slot)?;
    let slot = byte_elem_ptr(sess, buf, w)?;
    sess.2.build_store(slot, sess.0.i8_type().const_int(0x5C, false)).map_err(builder_fail)?;
    let w2 = sess.2.build_int_add(w, sess.0.i64_type().const_int(1, false), "").map_err(builder_fail)?;
    store_key(sess, w_slot, w2.into())?;
    let n2 = sess.2.build_int_sub(n, sess.0.i64_type().const_int(1, false), "").map_err(builder_fail)?;
    store_key(sess, bs_slot, n2.into())?;
    sess.2.build_unconditional_branch(cond).map_err(builder_fail)?;
    sess.2.position_at_end(done);
    Ok(())
}

// The Windows `Process.wait` lowering: WaitForSingleObject on the stored
// handle (infinite timeout), then GetExitCodeProcess for the status.  Both
// failures report `Err(SystemFault(GetLastError()))`.
fn native_process_wait_windows<'ctx>(
    sess: &mut Session<'ctx, '_, '_>,
    f: FunctionValue<'ctx>,
    locals: &Locals<'ctx>,
    ret_key: i64,
    out: PointerValue<'ctx>,
    span: (i64, i64, i64),
) -> Result<PointerValue<'ctx>, CodegenError> {
    let p0 = get_local(locals, 0, span)?;
    let handle = load_i64(sess, p0)?;
    let handle_ptr = sess.2.build_int_to_ptr(handle, ptr_ty(sess), "").map_err(builder_fail)?;
    let close_process = extern_close_handle(sess);
    let wait = extern_fn(sess, "WaitForSingleObject", sess.0.i32_type().fn_type(&[ptr_ty(sess).into(), sess.0.i32_type().into()], false));
    let wait_call = sess.2.build_call(wait, &[into_meta(handle_ptr.into()), into_meta(sess.0.i32_type().const_int(0xFFFFFFFF, false).into())], "").map_err(builder_fail)?;
    let wait_raw = match wait_call.try_as_basic_value() {
        ValueKind::Basic(value) => value.into_int_value(),
        ValueKind::Instruction(inst) => {
            return Err(builder_error(span.0, span.1, span.2, &format!("internal: WaitForSingleObject returned void ({:?})", inst.get_opcode())));
        }
    };
    let merge = new_block(sess, f, "wait_merge");
    let wait_ok = sess.2.build_int_compare(IntPredicate::EQ, wait_raw, sess.0.i32_type().const_zero(), "").map_err(builder_fail)?;
    let wait_fail_block = new_block(sess, f, "wait_fail");
    let wait_ok_block = new_block(sess, f, "wait_ok");
    sess.2.build_conditional_branch(wait_ok, wait_ok_block, wait_fail_block).map_err(builder_fail)?;
    sess.2.position_at_end(wait_fail_block);
    let err_code = windows_last_error(sess, span)?;
    system_fault_result(sess, ret_key, out, err_code, span)?;
    sess.2.build_call(close_process, &[into_meta(handle_ptr.into())], "").map_err(builder_fail)?;
    sess.2.build_unconditional_branch(merge).map_err(builder_fail)?;
    sess.2.position_at_end(wait_ok_block);
    let code_slot = alloca_raw(sess, sess.0.i32_type().into(), "exit_code", span)?;
    let get_code = extern_fn(sess, "GetExitCodeProcess", sess.0.i32_type().fn_type(&[ptr_ty(sess).into(), ptr_ty(sess).into()], false));
    let code_call = sess.2.build_call(get_code, &[into_meta(handle_ptr.into()), into_meta(code_slot.into())], "").map_err(builder_fail)?;
    let code_raw = match code_call.try_as_basic_value() {
        ValueKind::Basic(value) => value.into_int_value(),
        ValueKind::Instruction(inst) => {
            return Err(builder_error(span.0, span.1, span.2, &format!("internal: GetExitCodeProcess returned void ({:?})", inst.get_opcode())));
        }
    };
    let code_ok = sess.2.build_int_compare(IntPredicate::NE, code_raw, sess.0.i32_type().const_zero(), "").map_err(builder_fail)?;
    let code_fail_block = new_block(sess, f, "wait_code_fail");
    let code_ok_block = new_block(sess, f, "wait_code_ok");
    sess.2.build_conditional_branch(code_ok, code_ok_block, code_fail_block).map_err(builder_fail)?;
    sess.2.position_at_end(code_fail_block);
    let err_code2 = windows_last_error(sess, span)?;
    system_fault_result(sess, ret_key, out, err_code2, span)?;
    sess.2.build_call(close_process, &[into_meta(handle_ptr.into())], "").map_err(builder_fail)?;
    sess.2.build_unconditional_branch(merge).map_err(builder_fail)?;
    sess.2.position_at_end(code_ok_block);
    let code = sess.2.build_load(sess.0.i32_type(), code_slot, "").map_err(builder_fail)?.into_int_value();
    let code64 = sess.2.build_int_z_extend(code, sess.0.i64_type(), "").map_err(builder_fail)?;
    sess.2.build_call(close_process, &[into_meta(handle_ptr.into())], "").map_err(builder_fail)?;
    let code_key = result_arg_key(sess, ret_key, 0);
    let code_val = declare_local(sess, code_key, "code", span)?;
    store_key(sess, code_val, code64.into())?;
    let ok_result = build_result_ok(sess, ret_key, code_key, code_val, span)?;
    copy_to_out(sess, ret_key, out, ok_result, span)?;
    sess.2.build_unconditional_branch(merge).map_err(builder_fail)?;
    sess.2.position_at_end(merge);
    Ok(out)
}

// The Windows `Process.spawn` lowering: builds one quoted command line
// from the argv slice (the Win32 convention), converts it to UTF-16, and
// hands it to CreateProcessW with an explicit empty environment.  Standard
// argument quoting doubles backslashes before quotes and at the end of an
// argument; when the program is a batch file (whose command line cmd.exe
// re-parses), an argument containing a shell metacharacter is rejected
// rather than silently reinterpreted.
fn native_process_spawn_windows<'ctx>(
    sess: &mut Session<'ctx, '_, '_>,
    f: FunctionValue<'ctx>,
    locals: &Locals<'ctx>,
    ret_key: i64,
    out: PointerValue<'ctx>,
    span: (i64, i64, i64),
) -> Result<PointerValue<'ctx>, CodegenError> {
    let p0 = get_local(locals, 0, span)?;
    let argv_key = deref_key_of(sess, get_local_key(locals, 0, span)?);
    let elem_key = list_get(sess.6, key_args_of(sess, argv_key), 0);
    let argv_ref = load_ptr(sess, p0)?;
    let dptr = struct_gep(sess, argv_key, argv_ref, 0, "", span)?;
    let lptr = struct_gep(sess, argv_key, argv_ref, 1, "", span)?;
    let data = load_ptr(sess, dptr)?;
    let len = load_i64(sess, lptr)?;
    let i64_ty = sess.0.i64_type();
    let zero = i64_ty.const_zero();
    let one = i64_ty.const_int(1, false);
    let two = i64_ty.const_int(2, false);
    let three = i64_ty.const_int(3, false);
    let four = i64_ty.const_int(4, false);
    let enomem = i64_ty.const_int(ENOMEM, false);
    let invalid_param = i64_ty.const_int(87, false);
    let malloc = extern_malloc(sess);
    let free = extern_free(sess);

    let scan_i = alloca_raw(sess, i64_ty.into(), "scan_i", span)?;
    let scan_j = alloca_raw(sess, i64_ty.into(), "scan_j", span)?;
    let total_slot = alloca_raw(sess, i64_ty.into(), "total", span)?;
    let len_i = alloca_raw(sess, i64_ty.into(), "len_i", span)?;
    let need_slot = alloca_raw(sess, i64_ty.into(), "need", span)?;
    let qlen_slot = alloca_raw(sess, i64_ty.into(), "qlen", span)?;
    let len_j = alloca_raw(sess, i64_ty.into(), "len_j", span)?;
    let k_slot = alloca_raw(sess, i64_ty.into(), "k", span)?;
    let followed_slot = alloca_raw(sess, i64_ty.into(), "followed", span)?;
    let w_slot = alloca_raw(sess, i64_ty.into(), "w", span)?;
    let f_i = alloca_raw(sess, i64_ty.into(), "f_i", span)?;
    let fq_j = alloca_raw(sess, i64_ty.into(), "fq_j", span)?;
    let fk_slot = alloca_raw(sess, i64_ty.into(), "fk", span)?;
    let ffollowed_slot = alloca_raw(sess, i64_ty.into(), "ffollowed", span)?;
    let fr_j = alloca_raw(sess, i64_ty.into(), "fr_j", span)?;
    let units_slot = alloca_raw(sess, i64_ty.into(), "units", span)?;
    let u_i = alloca_raw(sess, i64_ty.into(), "u_i", span)?;
    let w16_slot = alloca_raw(sess, i64_ty.into(), "w16", span)?;
    let v_i = alloca_raw(sess, i64_ty.into(), "v_i", span)?;

    let merge = new_block(sess, f, "wspawn_merge");
    let length_pass = new_block(sess, f, "wspawn_length");

    // ---- is argv[0] a batch file? ----
    let has_first = sess.2.build_int_compare(IntPredicate::SGT, len, zero, "").map_err(builder_fail)?;
    let check_batch = new_block(sess, f, "wspawn_check_batch");
    let not_batch = new_block(sess, f, "wspawn_not_batch");
    sess.2.build_conditional_branch(has_first, check_batch, not_batch).map_err(builder_fail)?;
    sess.2.position_at_end(check_batch);
    let (s0_data, s0_len) = arg_bytes(sess, data, elem_key, zero, span)?;
    let has_ext = sess.2.build_int_compare(IntPredicate::UGE, s0_len, four, "").map_err(builder_fail)?;
    let safe_len = sess.2.build_select(has_ext, s0_len, four, "").map_err(builder_fail)?.into_int_value();
    let ext_check = new_block(sess, f, "wspawn_ext_check");
    sess.2.build_conditional_branch(has_ext, ext_check, not_batch).map_err(builder_fail)?;
    sess.2.position_at_end(ext_check);
    let start = sess.2.build_int_sub(safe_len, four, "").map_err(builder_fail)?;
    let dot_slot = byte_elem_ptr(sess, s0_data, start)?;
    let dot_raw = load_i8(sess, dot_slot)?;
    let dot = i8_lower(sess, dot_raw)?;
    let p1 = sess.2.build_int_add(start, one, "").map_err(builder_fail)?;
    let b1_slot = byte_elem_ptr(sess, s0_data, p1)?;
    let b1_raw = load_i8(sess, b1_slot)?;
    let b1 = i8_lower(sess, b1_raw)?;
    let p2 = sess.2.build_int_add(start, two, "").map_err(builder_fail)?;
    let b2_slot = byte_elem_ptr(sess, s0_data, p2)?;
    let b2_raw = load_i8(sess, b2_slot)?;
    let b2 = i8_lower(sess, b2_raw)?;
    let p3 = sess.2.build_int_add(start, three, "").map_err(builder_fail)?;
    let b3_slot = byte_elem_ptr(sess, s0_data, p3)?;
    let b3_raw = load_i8(sess, b3_slot)?;
    let b3 = i8_lower(sess, b3_raw)?;
    let is_dot = i8_eq(sess, dot, 0x2E)?;
    let is_b = i8_eq(sess, b1, 0x62)?;
    let is_c = i8_eq(sess, b1, 0x63)?;
    let is_a = i8_eq(sess, b2, 0x61)?;
    let is_m = i8_eq(sess, b2, 0x6D)?;
    let is_t = i8_eq(sess, b3, 0x74)?;
    let is_d = i8_eq(sess, b3, 0x64)?;
    let bat_ba = i1_and(sess, is_b, is_a)?;
    let bat = i1_and(sess, bat_ba, is_t)?;
    let cmd_cm = i1_and(sess, is_c, is_m)?;
    let cmd = i1_and(sess, cmd_cm, is_d)?;
    let bat_or_cmd = i1_or(sess, bat, cmd)?;
    let is_batch = i1_and(sess, is_dot, bat_or_cmd)?;
    let metachar_scan = new_block(sess, f, "wspawn_meta_scan");
    sess.2.build_conditional_branch(is_batch, metachar_scan, not_batch).map_err(builder_fail)?;

    // ---- reject shell metacharacters when the program is a batch file ----
    sess.2.position_at_end(metachar_scan);
    store_key(sess, scan_i, zero.into())?;
    let scan_cond = new_block(sess, f, "wspawn_scan_cond");
    let scan_body = new_block(sess, f, "wspawn_scan_body");
    let scan_done = new_block(sess, f, "wspawn_scan_done");
    sess.2.build_unconditional_branch(scan_cond).map_err(builder_fail)?;
    sess.2.position_at_end(scan_cond);
    let si = load_i64(sess, scan_i)?;
    let scan_more = sess.2.build_int_compare(IntPredicate::ULT, si, len, "").map_err(builder_fail)?;
    sess.2.build_conditional_branch(scan_more, scan_body, scan_done).map_err(builder_fail)?;
    sess.2.position_at_end(scan_body);
    let (sdata, slen) = arg_bytes(sess, data, elem_key, si, span)?;
    store_key(sess, scan_j, zero.into())?;
    let scan_jcond = new_block(sess, f, "wspawn_scan_jcond");
    let scan_jbody = new_block(sess, f, "wspawn_scan_jbody");
    let scan_jdone = new_block(sess, f, "wspawn_scan_jdone");
    sess.2.build_unconditional_branch(scan_jcond).map_err(builder_fail)?;
    sess.2.position_at_end(scan_jcond);
    let sj = load_i64(sess, scan_j)?;
    let jmore = sess.2.build_int_compare(IntPredicate::ULT, sj, slen, "").map_err(builder_fail)?;
    sess.2.build_conditional_branch(jmore, scan_jbody, scan_jdone).map_err(builder_fail)?;
    sess.2.position_at_end(scan_jbody);
    let byte_slot = byte_elem_ptr(sess, sdata, sj)?;
    let byte = load_i8(sess, byte_slot)?;
    let is_meta = is_shell_metachar(sess, byte)?;
    let meta_fail = new_block(sess, f, "wspawn_meta_fail");
    let meta_next = new_block(sess, f, "wspawn_meta_next");
    sess.2.build_conditional_branch(is_meta, meta_fail, meta_next).map_err(builder_fail)?;
    sess.2.position_at_end(meta_fail);
    system_fault_result(sess, ret_key, out, invalid_param, span)?;
    sess.2.build_unconditional_branch(merge).map_err(builder_fail)?;
    sess.2.position_at_end(meta_next);
    let sj2 = sess.2.build_int_add(sj, one, "").map_err(builder_fail)?;
    store_key(sess, scan_j, sj2.into())?;
    sess.2.build_unconditional_branch(scan_jcond).map_err(builder_fail)?;
    sess.2.position_at_end(scan_jdone);
    let si2 = sess.2.build_int_add(si, one, "").map_err(builder_fail)?;
    store_key(sess, scan_i, si2.into())?;
    sess.2.build_unconditional_branch(scan_cond).map_err(builder_fail)?;
    sess.2.position_at_end(scan_done);
    sess.2.build_unconditional_branch(length_pass).map_err(builder_fail)?;
    sess.2.position_at_end(not_batch);
    sess.2.build_unconditional_branch(length_pass).map_err(builder_fail)?;

    // ---- pass 1: per-arg quoted byte length and need-quote flag ----
    // The per-argument need-quote flags live in a heap array of `len`
    // bytes (one per argument) so the fill pass can read them back.
    sess.2.position_at_end(length_pass);
    let need_alloc = sess.2.build_select(sess.2.build_int_compare(IntPredicate::EQ, len, zero, "").map_err(builder_fail)?, one, len, "").map_err(builder_fail)?.into_int_value();
    let need_call = sess.2.build_call(malloc, &[into_meta(need_alloc.into())], "").map_err(builder_fail)?;
    let need_arr = match need_call.try_as_basic_value() {
        ValueKind::Basic(bv) => bv.into_pointer_value(),
        ValueKind::Instruction(inst) => {
            return Err(builder_error(span.0, span.1, span.2, &format!("internal: malloc returned void ({:?})", inst.get_opcode())));
        }
    };
    let need_oom = new_block(sess, f, "wspawn_need_oom");
    let length_loop = new_block(sess, f, "wspawn_length_loop");
    let need_null = is_null_ptr(sess, need_arr)?;
    sess.2.build_conditional_branch(need_null, need_oom, length_loop).map_err(builder_fail)?;
    sess.2.position_at_end(need_oom);
    system_fault_result(sess, ret_key, out, enomem, span)?;
    sess.2.build_unconditional_branch(merge).map_err(builder_fail)?;
    sess.2.position_at_end(length_loop);
    store_key(sess, total_slot, zero.into())?;
    store_key(sess, len_i, zero.into())?;
    let len_cond = new_block(sess, f, "wspawn_len_cond");
    let len_body = new_block(sess, f, "wspawn_len_body");
    let len_done = new_block(sess, f, "wspawn_len_done");
    sess.2.build_unconditional_branch(len_cond).map_err(builder_fail)?;
    sess.2.position_at_end(len_cond);
    let li = load_i64(sess, len_i)?;
    let lmore = sess.2.build_int_compare(IntPredicate::ULT, li, len, "").map_err(builder_fail)?;
    sess.2.build_conditional_branch(lmore, len_body, len_done).map_err(builder_fail)?;
    sess.2.position_at_end(len_body);
    let (sdata, slen) = arg_bytes(sess, data, elem_key, li, span)?;
    let is_empty = sess.2.build_int_compare(IntPredicate::EQ, slen, zero, "").map_err(builder_fail)?;
    let empty_need = sess.2.build_select(is_empty, one, zero, "").map_err(builder_fail)?.into_int_value();
    store_key(sess, need_slot, empty_need.into())?;
    store_key(sess, qlen_slot, two.into())?;
    store_key(sess, len_j, zero.into())?;
    let len_jcond = new_block(sess, f, "wspawn_len_jcond");
    let len_jbody = new_block(sess, f, "wspawn_len_jbody");
    let len_jdone = new_block(sess, f, "wspawn_len_jdone");
    sess.2.build_unconditional_branch(len_jcond).map_err(builder_fail)?;
    sess.2.position_at_end(len_jcond);
    let lj = load_i64(sess, len_j)?;
    let jmore = sess.2.build_int_compare(IntPredicate::ULT, lj, slen, "").map_err(builder_fail)?;
    sess.2.build_conditional_branch(jmore, len_jbody, len_jdone).map_err(builder_fail)?;
    sess.2.position_at_end(len_jbody);
    let byte_slot = byte_elem_ptr(sess, sdata, lj)?;
    let byte = load_i8(sess, byte_slot)?;
    let is_sp = i8_eq(sess, byte, 0x20)?;
    let is_tab = i8_eq(sess, byte, 0x09)?;
    let is_dq = i8_eq(sess, byte, 0x22)?;
    let is_ws = i1_or(sess, is_sp, is_tab)?;
    let is_wsq = i1_or(sess, is_ws, is_dq)?;
    let cur_need = load_i64(sess, need_slot)?;
    let new_need = sess.2.build_select(is_wsq, one, cur_need, "").map_err(builder_fail)?.into_int_value();
    store_key(sess, need_slot, new_need.into())?;
    let is_bs = i8_eq(sess, byte, 0x5C)?;
    let len_bs_run = new_block(sess, f, "wspawn_len_bs");
    let len_not_bs = new_block(sess, f, "wspawn_len_notbs");
    sess.2.build_conditional_branch(is_bs, len_bs_run, len_not_bs).map_err(builder_fail)?;
    sess.2.position_at_end(len_not_bs);
    let is_quote = i8_eq(sess, byte, 0x22)?;
    let qlen_val = load_i64(sess, qlen_slot)?;
    let inc2 = sess.2.build_int_add(qlen_val, two, "").map_err(builder_fail)?;
    let inc1 = sess.2.build_int_add(qlen_val, one, "").map_err(builder_fail)?;
    let stepped = sess.2.build_select(is_quote, inc2, inc1, "").map_err(builder_fail)?.into_int_value();
    store_key(sess, qlen_slot, stepped.into())?;
    let lj2 = sess.2.build_int_add(lj, one, "").map_err(builder_fail)?;
    store_key(sess, len_j, lj2.into())?;
    sess.2.build_unconditional_branch(len_jcond).map_err(builder_fail)?;
    sess.2.position_at_end(len_bs_run);
    store_key(sess, k_slot, lj.into())?;
    let bs_cond = new_block(sess, f, "wspawn_bs_cond");
    let bs_check = new_block(sess, f, "wspawn_bs_check");
    let bs_done = new_block(sess, f, "wspawn_bs_done");
    sess.2.build_unconditional_branch(bs_cond).map_err(builder_fail)?;
    sess.2.position_at_end(bs_cond);
    let k = load_i64(sess, k_slot)?;
    let k_in = sess.2.build_int_compare(IntPredicate::ULT, k, slen, "").map_err(builder_fail)?;
    let bs_at_end = new_block(sess, f, "wspawn_bs_atend");
    sess.2.build_conditional_branch(k_in, bs_check, bs_at_end).map_err(builder_fail)?;
    sess.2.position_at_end(bs_check);
    let kb_slot = byte_elem_ptr(sess, sdata, k)?;
    let kb = load_i8(sess, kb_slot)?;
    let is_kbs = i8_eq(sess, kb, 0x5C)?;
    let is_kq = i8_eq(sess, kb, 0x22)?;
    let fq = sess.2.build_select(is_kq, one, zero, "").map_err(builder_fail)?.into_int_value();
    store_key(sess, followed_slot, fq.into())?;
    let bs_adv = new_block(sess, f, "wspawn_bs_adv");
    sess.2.build_conditional_branch(is_kbs, bs_adv, bs_done).map_err(builder_fail)?;
    sess.2.position_at_end(bs_adv);
    let k2 = sess.2.build_int_add(k, one, "").map_err(builder_fail)?;
    store_key(sess, k_slot, k2.into())?;
    sess.2.build_unconditional_branch(bs_cond).map_err(builder_fail)?;
    sess.2.position_at_end(bs_at_end);
    store_key(sess, followed_slot, zero.into())?;
    sess.2.build_unconditional_branch(bs_done).map_err(builder_fail)?;
    sess.2.position_at_end(bs_done);
    let k = load_i64(sess, k_slot)?;
    let followed = load_i64(sess, followed_slot)?;
    let bs = sess.2.build_int_sub(k, lj, "").map_err(builder_fail)?;
    let bs2 = sess.2.build_int_mul(bs, two, "").map_err(builder_fail)?;
    let at_end = sess.2.build_int_compare(IntPredicate::UGE, k, slen, "").map_err(builder_fail)?;
    let qlen_val = load_i64(sess, qlen_slot)?;
    let tail_add = sess.2.build_int_add(qlen_val, bs2, "").map_err(builder_fail)?;
    let plus_quote = sess.2.build_int_add(tail_add, two, "").map_err(builder_fail)?;
    let bs_plus_char = sess.2.build_int_add(sess.2.build_int_add(qlen_val, bs, "").map_err(builder_fail)?, one, "").map_err(builder_fail)?;
    let followed_cond = sess.2.build_int_compare(IntPredicate::NE, followed, zero, "").map_err(builder_fail)?;
    let fq_add = sess.2.build_select(followed_cond, plus_quote, bs_plus_char, "").map_err(builder_fail)?.into_int_value();
    let stepped = sess.2.build_select(at_end, tail_add, fq_add, "").map_err(builder_fail)?.into_int_value();
    store_key(sess, qlen_slot, stepped.into())?;
    let kp1 = sess.2.build_int_add(k, one, "").map_err(builder_fail)?;
    store_key(sess, len_j, kp1.into())?;
    sess.2.build_unconditional_branch(len_jcond).map_err(builder_fail)?;
    sess.2.position_at_end(len_jdone);
    let need = load_i64(sess, need_slot)?;
    let qlen = load_i64(sess, qlen_slot)?;
    let need_cond = sess.2.build_int_compare(IntPredicate::NE, need, zero, "").map_err(builder_fail)?;
    let final_len = sess.2.build_select(need_cond, qlen, slen, "").map_err(builder_fail)?.into_int_value();
    let need_byte = sess.2.build_int_truncate(need, sess.0.i8_type(), "").map_err(builder_fail)?;
    sess.2.build_store(byte_elem_ptr(sess, need_arr, li)?, need_byte).map_err(builder_fail)?;
    let total = load_i64(sess, total_slot)?;
    let total2 = sess.2.build_int_add(total, sess.2.build_int_add(final_len, one, "").map_err(builder_fail)?, "").map_err(builder_fail)?;
    store_key(sess, total_slot, total2.into())?;
    let li2 = sess.2.build_int_add(li, one, "").map_err(builder_fail)?;
    store_key(sess, len_i, li2.into())?;
    sess.2.build_unconditional_branch(len_cond).map_err(builder_fail)?;
    sess.2.position_at_end(len_done);
    let total = load_i64(sess, total_slot)?;
    let alloc_bytes = sess.2.build_select(sess.2.build_int_compare(IntPredicate::EQ, total, zero, "").map_err(builder_fail)?, one, total, "").map_err(builder_fail)?.into_int_value();
    let buf_call = sess.2.build_call(malloc, &[into_meta(alloc_bytes.into())], "").map_err(builder_fail)?;
    let buf8 = match buf_call.try_as_basic_value() {
        ValueKind::Basic(bv) => bv.into_pointer_value(),
        ValueKind::Instruction(inst) => {
            return Err(builder_error(span.0, span.1, span.2, &format!("internal: malloc returned void ({:?})", inst.get_opcode())));
        }
    };
    let buf_oom = new_block(sess, f, "wspawn_buf_oom");
    let fill_pass = new_block(sess, f, "wspawn_fill");
    let buf_null = is_null_ptr(sess, buf8)?;
    sess.2.build_conditional_branch(buf_null, buf_oom, fill_pass).map_err(builder_fail)?;
    sess.2.position_at_end(buf_oom);
    sess.2.build_call(free, &[into_meta(need_arr.into())], "").map_err(builder_fail)?;
    system_fault_result(sess, ret_key, out, enomem, span)?;
    sess.2.build_unconditional_branch(merge).map_err(builder_fail)?;

    // ---- pass 2: fill the quoted UTF-8 command line ----
    sess.2.position_at_end(fill_pass);
    store_key(sess, w_slot, zero.into())?;
    store_key(sess, f_i, zero.into())?;
    let f_cond = new_block(sess, f, "wspawn_f_cond");
    let f_body = new_block(sess, f, "wspawn_f_body");
    let f_done = new_block(sess, f, "wspawn_f_done");
    sess.2.build_unconditional_branch(f_cond).map_err(builder_fail)?;
    sess.2.position_at_end(f_cond);
    let fi = load_i64(sess, f_i)?;
    let fmore = sess.2.build_int_compare(IntPredicate::ULT, fi, len, "").map_err(builder_fail)?;
    sess.2.build_conditional_branch(fmore, f_body, f_done).map_err(builder_fail)?;
    sess.2.position_at_end(f_body);
    let (sdata, slen) = arg_bytes(sess, data, elem_key, fi, span)?;
    let need_byte_slot = byte_elem_ptr(sess, need_arr, fi)?;
    let need_byte = load_i8(sess, need_byte_slot)?;
    let need64 = sess.2.build_int_z_extend(need_byte, i64_ty, "").map_err(builder_fail)?;
    let need_cond = sess.2.build_int_compare(IntPredicate::NE, need64, zero, "").map_err(builder_fail)?;
    let f_quoted = new_block(sess, f, "wspawn_f_quoted");
    let f_raw = new_block(sess, f, "wspawn_f_raw");
    sess.2.build_conditional_branch(need_cond, f_quoted, f_raw).map_err(builder_fail)?;
    sess.2.position_at_end(f_quoted);
    let w = load_i64(sess, w_slot)?;
    sess.2.build_store(byte_elem_ptr(sess, buf8, w)?, sess.0.i8_type().const_int(0x22, false)).map_err(builder_fail)?;
    let w1 = sess.2.build_int_add(w, one, "").map_err(builder_fail)?;
    store_key(sess, w_slot, w1.into())?;
    store_key(sess, fq_j, zero.into())?;
    let fq_cond = new_block(sess, f, "wspawn_fq_cond");
    let fq_body = new_block(sess, f, "wspawn_fq_body");
    let fq_done = new_block(sess, f, "wspawn_fq_done");
    sess.2.build_unconditional_branch(fq_cond).map_err(builder_fail)?;
    sess.2.position_at_end(fq_cond);
    let fj = load_i64(sess, fq_j)?;
    let fjmore = sess.2.build_int_compare(IntPredicate::ULT, fj, slen, "").map_err(builder_fail)?;
    sess.2.build_conditional_branch(fjmore, fq_body, fq_done).map_err(builder_fail)?;
    sess.2.position_at_end(fq_body);
    let byte_slot = byte_elem_ptr(sess, sdata, fj)?;
    let byte = load_i8(sess, byte_slot)?;
    let is_bs = i8_eq(sess, byte, 0x5C)?;
    let fq_bs = new_block(sess, f, "wspawn_fq_bs");
    let fq_not_bs = new_block(sess, f, "wspawn_fq_notbs");
    sess.2.build_conditional_branch(is_bs, fq_bs, fq_not_bs).map_err(builder_fail)?;
    sess.2.position_at_end(fq_not_bs);
    let is_quote = i8_eq(sess, byte, 0x22)?;
    let notbs_quote = new_block(sess, f, "wspawn_fq_quote");
    let notbs_plain = new_block(sess, f, "wspawn_fq_plain");
    let notbs_after = new_block(sess, f, "wspawn_fq_notbs_after");
    sess.2.build_conditional_branch(is_quote, notbs_quote, notbs_plain).map_err(builder_fail)?;
    sess.2.position_at_end(notbs_quote);
    // A literal quote inside a quoted argument is escaped as backslash-quote.
    let w = load_i64(sess, w_slot)?;
    sess.2.build_store(byte_elem_ptr(sess, buf8, w)?, sess.0.i8_type().const_int(0x5C, false)).map_err(builder_fail)?;
    let w1 = sess.2.build_int_add(w, one, "").map_err(builder_fail)?;
    sess.2.build_store(byte_elem_ptr(sess, buf8, w1)?, sess.0.i8_type().const_int(0x22, false)).map_err(builder_fail)?;
    let w2 = sess.2.build_int_add(w1, one, "").map_err(builder_fail)?;
    store_key(sess, w_slot, w2.into())?;
    sess.2.build_unconditional_branch(notbs_after).map_err(builder_fail)?;
    sess.2.position_at_end(notbs_plain);
    let w = load_i64(sess, w_slot)?;
    sess.2.build_store(byte_elem_ptr(sess, buf8, w)?, byte).map_err(builder_fail)?;
    let w1 = sess.2.build_int_add(w, one, "").map_err(builder_fail)?;
    store_key(sess, w_slot, w1.into())?;
    sess.2.build_unconditional_branch(notbs_after).map_err(builder_fail)?;
    sess.2.position_at_end(notbs_after);
    let fj2 = sess.2.build_int_add(fj, one, "").map_err(builder_fail)?;
    store_key(sess, fq_j, fj2.into())?;
    sess.2.build_unconditional_branch(fq_cond).map_err(builder_fail)?;
    sess.2.position_at_end(fq_bs);
    store_key(sess, fk_slot, fj.into())?;
    let fbs_cond = new_block(sess, f, "wspawn_fbs_cond");
    let fbs_check = new_block(sess, f, "wspawn_fbs_check");
    let fbs_done = new_block(sess, f, "wspawn_fbs_done");
    sess.2.build_unconditional_branch(fbs_cond).map_err(builder_fail)?;
    sess.2.position_at_end(fbs_cond);
    let k = load_i64(sess, fk_slot)?;
    let k_in = sess.2.build_int_compare(IntPredicate::ULT, k, slen, "").map_err(builder_fail)?;
    let fbs_at_end = new_block(sess, f, "wspawn_fbs_atend");
    sess.2.build_conditional_branch(k_in, fbs_check, fbs_at_end).map_err(builder_fail)?;
    sess.2.position_at_end(fbs_check);
    let kb_slot = byte_elem_ptr(sess, sdata, k)?;
    let kb = load_i8(sess, kb_slot)?;
    let is_kbs = i8_eq(sess, kb, 0x5C)?;
    let is_kq = i8_eq(sess, kb, 0x22)?;
    let fq = sess.2.build_select(is_kq, one, zero, "").map_err(builder_fail)?.into_int_value();
    store_key(sess, ffollowed_slot, fq.into())?;
    let fbs_adv = new_block(sess, f, "wspawn_fbs_adv");
    sess.2.build_conditional_branch(is_kbs, fbs_adv, fbs_done).map_err(builder_fail)?;
    sess.2.position_at_end(fbs_adv);
    let k2 = sess.2.build_int_add(k, one, "").map_err(builder_fail)?;
    store_key(sess, fk_slot, k2.into())?;
    sess.2.build_unconditional_branch(fbs_cond).map_err(builder_fail)?;
    sess.2.position_at_end(fbs_at_end);
    store_key(sess, ffollowed_slot, zero.into())?;
    sess.2.build_unconditional_branch(fbs_done).map_err(builder_fail)?;
    sess.2.position_at_end(fbs_done);
    let k = load_i64(sess, fk_slot)?;
    let followed = load_i64(sess, ffollowed_slot)?;
    let bs = sess.2.build_int_sub(k, fj, "").map_err(builder_fail)?;
    let bs2 = sess.2.build_int_mul(bs, two, "").map_err(builder_fail)?;
    let at_end = sess.2.build_int_compare(IntPredicate::UGE, k, slen, "").map_err(builder_fail)?;
    let fbs_tail = new_block(sess, f, "wspawn_fbs_tail");
    let fbs_fq = new_block(sess, f, "wspawn_fbs_fq");
    let fbs_plain = new_block(sess, f, "wspawn_fbs_plain");
    let fbs_after = new_block(sess, f, "wspawn_fbs_after");
    sess.2.build_conditional_branch(at_end, fbs_tail, fbs_fq).map_err(builder_fail)?;
    sess.2.position_at_end(fbs_tail);
    emit_backslash_run(sess, f, buf8, w_slot, bs2, span)?;
    sess.2.build_unconditional_branch(fbs_after).map_err(builder_fail)?;
    sess.2.position_at_end(fbs_fq);
    let followed_cond = sess.2.build_int_compare(IntPredicate::NE, followed, zero, "").map_err(builder_fail)?;
    let fq_branch = new_block(sess, f, "wspawn_fbs_fq_branch");
    sess.2.build_conditional_branch(followed_cond, fq_branch, fbs_plain).map_err(builder_fail)?;
    sess.2.position_at_end(fq_branch);
    let bs2p1 = sess.2.build_int_add(bs2, one, "").map_err(builder_fail)?;
    emit_backslash_run(sess, f, buf8, w_slot, bs2p1, span)?;
    let w = load_i64(sess, w_slot)?;
    sess.2.build_store(byte_elem_ptr(sess, buf8, w)?, sess.0.i8_type().const_int(0x22, false)).map_err(builder_fail)?;
    let w1 = sess.2.build_int_add(w, one, "").map_err(builder_fail)?;
    store_key(sess, w_slot, w1.into())?;
    sess.2.build_unconditional_branch(fbs_after).map_err(builder_fail)?;
    sess.2.position_at_end(fbs_plain);
    emit_backslash_run(sess, f, buf8, w_slot, bs, span)?;
    let w = load_i64(sess, w_slot)?;
    let kb_here_slot = byte_elem_ptr(sess, sdata, k)?;
    let kb_here = load_i8(sess, kb_here_slot)?;
    sess.2.build_store(byte_elem_ptr(sess, buf8, w)?, kb_here).map_err(builder_fail)?;
    let w1 = sess.2.build_int_add(w, one, "").map_err(builder_fail)?;
    store_key(sess, w_slot, w1.into())?;
    sess.2.build_unconditional_branch(fbs_after).map_err(builder_fail)?;
    sess.2.position_at_end(fbs_after);
    let kp1 = sess.2.build_int_add(k, one, "").map_err(builder_fail)?;
    store_key(sess, fq_j, kp1.into())?;
    sess.2.build_unconditional_branch(fq_cond).map_err(builder_fail)?;
    sess.2.position_at_end(fq_done);
    let w = load_i64(sess, w_slot)?;
    sess.2.build_store(byte_elem_ptr(sess, buf8, w)?, sess.0.i8_type().const_int(0x22, false)).map_err(builder_fail)?;
    let w1 = sess.2.build_int_add(w, one, "").map_err(builder_fail)?;
    store_key(sess, w_slot, w1.into())?;
    let f_next = new_block(sess, f, "wspawn_f_next");
    sess.2.build_unconditional_branch(f_next).map_err(builder_fail)?;
    sess.2.position_at_end(f_raw);
    store_key(sess, fr_j, zero.into())?;
    let fr_cond = new_block(sess, f, "wspawn_fr_cond");
    let fr_body = new_block(sess, f, "wspawn_fr_body");
    let fr_done = new_block(sess, f, "wspawn_fr_done");
    sess.2.build_unconditional_branch(fr_cond).map_err(builder_fail)?;
    sess.2.position_at_end(fr_cond);
    let rj = load_i64(sess, fr_j)?;
    let rjmore = sess.2.build_int_compare(IntPredicate::ULT, rj, slen, "").map_err(builder_fail)?;
    sess.2.build_conditional_branch(rjmore, fr_body, fr_done).map_err(builder_fail)?;
    sess.2.position_at_end(fr_body);
    let w = load_i64(sess, w_slot)?;
    let rb_slot = byte_elem_ptr(sess, sdata, rj)?;
    let rb = load_i8(sess, rb_slot)?;
    sess.2.build_store(byte_elem_ptr(sess, buf8, w)?, rb).map_err(builder_fail)?;
    let w1 = sess.2.build_int_add(w, one, "").map_err(builder_fail)?;
    store_key(sess, w_slot, w1.into())?;
    let rj2 = sess.2.build_int_add(rj, one, "").map_err(builder_fail)?;
    store_key(sess, fr_j, rj2.into())?;
    sess.2.build_unconditional_branch(fr_cond).map_err(builder_fail)?;
    sess.2.position_at_end(fr_done);
    sess.2.build_unconditional_branch(f_next).map_err(builder_fail)?;
    sess.2.position_at_end(f_next);
    let fi2 = sess.2.build_int_add(fi, one, "").map_err(builder_fail)?;
    let has_next = sess.2.build_int_compare(IntPredicate::ULT, fi2, len, "").map_err(builder_fail)?;
    let f_space = new_block(sess, f, "wspawn_f_space");
    let f_no_space = new_block(sess, f, "wspawn_f_no_space");
    let f_advance = new_block(sess, f, "wspawn_f_advance");
    sess.2.build_conditional_branch(has_next, f_space, f_no_space).map_err(builder_fail)?;
    sess.2.position_at_end(f_space);
    let w = load_i64(sess, w_slot)?;
    sess.2.build_store(byte_elem_ptr(sess, buf8, w)?, sess.0.i8_type().const_int(0x20, false)).map_err(builder_fail)?;
    let w1 = sess.2.build_int_add(w, one, "").map_err(builder_fail)?;
    store_key(sess, w_slot, w1.into())?;
    sess.2.build_unconditional_branch(f_advance).map_err(builder_fail)?;
    sess.2.position_at_end(f_no_space);
    sess.2.build_unconditional_branch(f_advance).map_err(builder_fail)?;
    sess.2.position_at_end(f_advance);
    store_key(sess, f_i, fi2.into())?;
    sess.2.build_unconditional_branch(f_cond).map_err(builder_fail)?;
    sess.2.position_at_end(f_done);
    sess.2.build_call(free, &[into_meta(need_arr.into())], "").map_err(builder_fail)?;

    // ---- pass 3: UTF-8 -> UTF-16 length of the assembled command line ----
    store_key(sess, units_slot, zero.into())?;
    store_key(sess, u_i, zero.into())?;
    let u_cond = new_block(sess, f, "wspawn_u_cond");
    let u_body = new_block(sess, f, "wspawn_u_body");
    let u_done = new_block(sess, f, "wspawn_u_done");
    sess.2.build_unconditional_branch(u_cond).map_err(builder_fail)?;
    sess.2.position_at_end(u_cond);
    let ui = load_i64(sess, u_i)?;
    let umore = sess.2.build_int_compare(IntPredicate::ULT, ui, total, "").map_err(builder_fail)?;
    sess.2.build_conditional_branch(umore, u_body, u_done).map_err(builder_fail)?;
    sess.2.position_at_end(u_body);
    let ub_slot = byte_elem_ptr(sess, buf8, ui)?;
    let ub = load_i8(sess, ub_slot)?;
    let ub64 = sess.2.build_int_z_extend(ub, i64_ty, "").map_err(builder_fail)?;
    let u1 = sess.2.build_int_compare(IntPredicate::ULT, ub64, i64_ty.const_int(0x80, false), "").map_err(builder_fail)?;
    let u2 = sess.2.build_int_compare(IntPredicate::ULT, ub64, i64_ty.const_int(0xE0, false), "").map_err(builder_fail)?;
    let u3 = sess.2.build_int_compare(IntPredicate::ULT, ub64, i64_ty.const_int(0xF0, false), "").map_err(builder_fail)?;
    let units = load_i64(sess, units_slot)?;
    let units1 = sess.2.build_int_add(units, one, "").map_err(builder_fail)?;
    let units2 = sess.2.build_int_add(units, two, "").map_err(builder_fail)?;
    let step1 = sess.2.build_select(u1, units1, units2, "").map_err(builder_fail)?.into_int_value();
    let step2 = sess.2.build_select(u2, units1, step1, "").map_err(builder_fail)?.into_int_value();
    let step3 = sess.2.build_select(u3, units1, step2, "").map_err(builder_fail)?.into_int_value();
    store_key(sess, units_slot, step3.into())?;
    let adv = sess.2.build_select(u1, one, sess.2.build_select(u2, two, sess.2.build_select(u3, three, four, "").map_err(builder_fail)?.into_int_value(), "").map_err(builder_fail)?.into_int_value(), "").map_err(builder_fail)?.into_int_value();
    let ui2 = sess.2.build_int_add(ui, adv, "").map_err(builder_fail)?;
    store_key(sess, u_i, ui2.into())?;
    sess.2.build_unconditional_branch(u_cond).map_err(builder_fail)?;
    sess.2.position_at_end(u_done);
    let units = load_i64(sess, units_slot)?;
    let units_p1 = sess.2.build_int_add(units, one, "").map_err(builder_fail)?;
    let utf16_bytes = sess.2.build_int_mul(units_p1, two, "").map_err(builder_fail)?;
    let u16_call = sess.2.build_call(malloc, &[into_meta(utf16_bytes.into())], "").map_err(builder_fail)?;
    let buf16 = match u16_call.try_as_basic_value() {
        ValueKind::Basic(bv) => bv.into_pointer_value(),
        ValueKind::Instruction(inst) => {
            return Err(builder_error(span.0, span.1, span.2, &format!("internal: malloc returned void ({:?})", inst.get_opcode())));
        }
    };
    let u16_oom = new_block(sess, f, "wspawn_u16_oom");
    let utf16_fill = new_block(sess, f, "wspawn_utf16_fill");
    let u16_null = is_null_ptr(sess, buf16)?;
    sess.2.build_conditional_branch(u16_null, u16_oom, utf16_fill).map_err(builder_fail)?;
    sess.2.position_at_end(u16_oom);
    sess.2.build_call(free, &[into_meta(buf8.into())], "").map_err(builder_fail)?;
    system_fault_result(sess, ret_key, out, enomem, span)?;
    sess.2.build_unconditional_branch(merge).map_err(builder_fail)?;

    // ---- pass 4: UTF-8 -> UTF-16 fill + NUL ----
    sess.2.position_at_end(utf16_fill);
    store_key(sess, w16_slot, zero.into())?;
    store_key(sess, v_i, zero.into())?;
    let v_cond = new_block(sess, f, "wspawn_v_cond");
    let v_body = new_block(sess, f, "wspawn_v_body");
    let v_done = new_block(sess, f, "wspawn_v_done");
    sess.2.build_unconditional_branch(v_cond).map_err(builder_fail)?;
    sess.2.position_at_end(v_cond);
    let vi = load_i64(sess, v_i)?;
    let vmore = sess.2.build_int_compare(IntPredicate::ULT, vi, total, "").map_err(builder_fail)?;
    sess.2.build_conditional_branch(vmore, v_body, v_done).map_err(builder_fail)?;
    sess.2.position_at_end(v_body);
    let vb_slot = byte_elem_ptr(sess, buf8, vi)?;
    let vb = load_i8(sess, vb_slot)?;
    let vb64 = sess.2.build_int_z_extend(vb, i64_ty, "").map_err(builder_fail)?;
    let v1 = sess.2.build_int_compare(IntPredicate::ULT, vb64, i64_ty.const_int(0x80, false), "").map_err(builder_fail)?;
    let v2 = sess.2.build_int_compare(IntPredicate::ULT, vb64, i64_ty.const_int(0xE0, false), "").map_err(builder_fail)?;
    let v3 = sess.2.build_int_compare(IntPredicate::ULT, vb64, i64_ty.const_int(0xF0, false), "").map_err(builder_fail)?;
    let v1b = new_block(sess, f, "wspawn_v1");
    let v2b = new_block(sess, f, "wspawn_v2");
    let v3b = new_block(sess, f, "wspawn_v3");
    let v4b = new_block(sess, f, "wspawn_v4");
    let v2_dispatch = new_block(sess, f, "wspawn_v2_dispatch");
    let v3_dispatch = new_block(sess, f, "wspawn_v3_dispatch");
    let v_after = new_block(sess, f, "wspawn_v_after");
    sess.2.build_conditional_branch(v1, v1b, v2_dispatch).map_err(builder_fail)?;
    sess.2.position_at_end(v2_dispatch);
    sess.2.build_conditional_branch(v2, v2b, v3_dispatch).map_err(builder_fail)?;
    sess.2.position_at_end(v3_dispatch);
    sess.2.build_conditional_branch(v3, v3b, v4b).map_err(builder_fail)?;
    sess.2.position_at_end(v1b);
    store_utf16(sess, buf16, w16_slot, vb64)?;
    let adv1 = one;
    store_key(sess, v_i, sess.2.build_int_add(vi, adv1, "").map_err(builder_fail)?.into())?;
    sess.2.build_unconditional_branch(v_after).map_err(builder_fail)?;
    sess.2.position_at_end(v2b);
    let b1_slot = byte_elem_ptr(sess, buf8, sess.2.build_int_add(vi, one, "").map_err(builder_fail)?)?;
    let b1 = load_i8(sess, b1_slot)?;
    let b1_64 = sess.2.build_int_z_extend(b1, i64_ty, "").map_err(builder_fail)?;
    let lo6 = sess.2.build_and(b1_64, i64_ty.const_int(0x3F, false), "").map_err(builder_fail)?;
    let hi5 = sess.2.build_and(vb64, i64_ty.const_int(0x1F, false), "").map_err(builder_fail)?;
    let cp = sess.2.build_or(sess.2.build_left_shift(hi5, i64_ty.const_int(6, false), "").map_err(builder_fail)?, lo6, "").map_err(builder_fail)?;
    store_utf16(sess, buf16, w16_slot, cp)?;
    store_key(sess, v_i, sess.2.build_int_add(vi, two, "").map_err(builder_fail)?.into())?;
    sess.2.build_unconditional_branch(v_after).map_err(builder_fail)?;
    sess.2.position_at_end(v3b);
    let b1_slot = byte_elem_ptr(sess, buf8, sess.2.build_int_add(vi, one, "").map_err(builder_fail)?)?;
    let b1 = load_i8(sess, b1_slot)?;
    let b2_slot = byte_elem_ptr(sess, buf8, sess.2.build_int_add(vi, two, "").map_err(builder_fail)?)?;
    let b2 = load_i8(sess, b2_slot)?;
    let b1_64 = sess.2.build_int_z_extend(b1, i64_ty, "").map_err(builder_fail)?;
    let b2_64 = sess.2.build_int_z_extend(b2, i64_ty, "").map_err(builder_fail)?;
    let lo12 = sess.2.build_or(sess.2.build_left_shift(sess.2.build_and(b1_64, i64_ty.const_int(0x3F, false), "").map_err(builder_fail)?, i64_ty.const_int(6, false), "").map_err(builder_fail)?, sess.2.build_and(b2_64, i64_ty.const_int(0x3F, false), "").map_err(builder_fail)?, "").map_err(builder_fail)?;
    let hi4 = sess.2.build_and(vb64, i64_ty.const_int(0x0F, false), "").map_err(builder_fail)?;
    let cp = sess.2.build_or(sess.2.build_left_shift(hi4, i64_ty.const_int(12, false), "").map_err(builder_fail)?, lo12, "").map_err(builder_fail)?;
    store_utf16(sess, buf16, w16_slot, cp)?;
    store_key(sess, v_i, sess.2.build_int_add(vi, three, "").map_err(builder_fail)?.into())?;
    sess.2.build_unconditional_branch(v_after).map_err(builder_fail)?;
    sess.2.position_at_end(v4b);
    let b1_slot = byte_elem_ptr(sess, buf8, sess.2.build_int_add(vi, one, "").map_err(builder_fail)?)?;
    let b1 = load_i8(sess, b1_slot)?;
    let b2_slot = byte_elem_ptr(sess, buf8, sess.2.build_int_add(vi, two, "").map_err(builder_fail)?)?;
    let b2 = load_i8(sess, b2_slot)?;
    let b3_slot = byte_elem_ptr(sess, buf8, sess.2.build_int_add(vi, three, "").map_err(builder_fail)?)?;
    let b3 = load_i8(sess, b3_slot)?;
    let b1_64 = sess.2.build_int_z_extend(b1, i64_ty, "").map_err(builder_fail)?;
    let b2_64 = sess.2.build_int_z_extend(b2, i64_ty, "").map_err(builder_fail)?;
    let b3_64 = sess.2.build_int_z_extend(b3, i64_ty, "").map_err(builder_fail)?;
    let lo18 = sess.2.build_or(sess.2.build_left_shift(sess.2.build_and(b1_64, i64_ty.const_int(0x3F, false), "").map_err(builder_fail)?, i64_ty.const_int(12, false), "").map_err(builder_fail)?, sess.2.build_or(sess.2.build_left_shift(sess.2.build_and(b2_64, i64_ty.const_int(0x3F, false), "").map_err(builder_fail)?, i64_ty.const_int(6, false), "").map_err(builder_fail)?, sess.2.build_and(b3_64, i64_ty.const_int(0x3F, false), "").map_err(builder_fail)?, "").map_err(builder_fail)?, "").map_err(builder_fail)?;
    let hi3 = sess.2.build_and(vb64, i64_ty.const_int(0x07, false), "").map_err(builder_fail)?;
    let cp = sess.2.build_or(sess.2.build_left_shift(hi3, i64_ty.const_int(18, false), "").map_err(builder_fail)?, lo18, "").map_err(builder_fail)?;
    let cp_adj = sess.2.build_int_sub(cp, i64_ty.const_int(0x10000, false), "").map_err(builder_fail)?;
    let high = sess.2.build_or(i64_ty.const_int(0xD800, false), sess.2.build_right_shift(cp_adj, i64_ty.const_int(10, false), true, "").map_err(builder_fail)?, "").map_err(builder_fail)?;
    let low = sess.2.build_or(i64_ty.const_int(0xDC00, false), sess.2.build_and(cp_adj, i64_ty.const_int(0x3FF, false), "").map_err(builder_fail)?, "").map_err(builder_fail)?;
    store_utf16(sess, buf16, w16_slot, high)?;
    store_utf16(sess, buf16, w16_slot, low)?;
    store_key(sess, v_i, sess.2.build_int_add(vi, four, "").map_err(builder_fail)?.into())?;
    sess.2.build_unconditional_branch(v_after).map_err(builder_fail)?;
    sess.2.position_at_end(v_after);
    sess.2.build_unconditional_branch(v_cond).map_err(builder_fail)?;
    sess.2.position_at_end(v_done);
    let w16 = load_i64(sess, w16_slot)?;
    let nul_off = offset_buffer_elem_ptr(sess, sess.0.i16_type().into(), buf16, w16)?;
    sess.2.build_store(nul_off, sess.0.i16_type().const_zero()).map_err(builder_fail)?;
    // The UTF-8 command line is no longer needed once the wide buffer is
    // complete (the per-argument need array was already freed).
    sess.2.build_call(free, &[into_meta(buf8.into())], "").map_err(builder_fail)?;

    // ---- CreateProcessW ----
    // `si`, `pi`, `env` slots are reached through their declared array types.
    let si = alloca_raw(sess, sess.0.i64_type().array_type(12).into(), "si", span)?;
    let zero8 = sess.0.i8_type().const_zero();
    sess.2.build_memset(si, 8, zero8, i64_ty.const_int(96, false)).map_err(builder_fail)?;
    let si_ty = sess.0.i64_type().array_type(12);
    let cb_slot = offset_array_elem_ptr(sess, si_ty.into(), si, zero)?;
    sess.2.build_store(cb_slot, sess.0.i32_type().const_int(96, false)).map_err(builder_fail)?;
    let pi = alloca_raw(sess, sess.0.i64_type().array_type(3).into(), "pi", span)?;
    let env = alloca_raw(sess, sess.0.i16_type().array_type(2).into(), "env", span)?;
    let env_ty = sess.0.i16_type().array_type(2);
    let env_zero = offset_array_elem_ptr(sess, env_ty.into(), env, zero)?;
    sess.2.build_store(env_zero, sess.0.i16_type().const_zero()).map_err(builder_fail)?;
    let env_one = offset_array_elem_ptr(sess, env_ty.into(), env, one)?;
    sess.2.build_store(env_one, sess.0.i16_type().const_zero()).map_err(builder_fail)?;
    let create = extern_fn(sess, "CreateProcessW", sess.0.i32_type().fn_type(&[
        ptr_ty(sess).into(),
        ptr_ty(sess).into(),
        ptr_ty(sess).into(),
        ptr_ty(sess).into(),
        sess.0.i32_type().into(),
        sess.0.i32_type().into(),
        ptr_ty(sess).into(),
        ptr_ty(sess).into(),
        ptr_ty(sess).into(),
        ptr_ty(sess).into(),
    ], false));
    let create_call = sess.2.build_call(create, &[
        into_meta(ptr_ty(sess).const_null().into()),
        into_meta(buf16.into()),
        into_meta(ptr_ty(sess).const_null().into()),
        into_meta(ptr_ty(sess).const_null().into()),
        into_meta(sess.0.i32_type().const_zero().into()),
        into_meta(sess.0.i32_type().const_int(0x400, false).into()),
        into_meta(env.into()),
        into_meta(ptr_ty(sess).const_null().into()),
        into_meta(si.into()),
        into_meta(pi.into()),
    ], "").map_err(builder_fail)?;
    let created = match create_call.try_as_basic_value() {
        ValueKind::Basic(value) => value.into_int_value(),
        ValueKind::Instruction(inst) => {
            return Err(builder_error(span.0, span.1, span.2, &format!("internal: CreateProcessW returned void ({:?})", inst.get_opcode())));
        }
    };
    // CreateProcessW copies the command line into the child's address
    // space before returning, so the wide buffer is spent on both the
    // success and failure paths.
    sess.2.build_call(free, &[into_meta(buf16.into())], "").map_err(builder_fail)?;
    let create_ok = new_block(sess, f, "wspawn_create_ok");
    let create_fail = new_block(sess, f, "wspawn_create_fail");
    sess.2.build_conditional_branch(created, create_ok, create_fail).map_err(builder_fail)?;
    sess.2.position_at_end(create_fail);
    let err_code = windows_last_error(sess, span)?;
    system_fault_result(sess, ret_key, out, err_code, span)?;
    sess.2.build_unconditional_branch(merge).map_err(builder_fail)?;
    sess.2.position_at_end(create_ok);
    let pi_ty = sess.0.i64_type().array_type(3);
    let handle_slot = offset_array_elem_ptr(sess, pi_ty.into(), pi, zero)?;
    let handle = load_i64(sess, handle_slot)?;
    let thread_slot = offset_array_elem_ptr(sess, pi_ty.into(), pi, one)?;
    let thread_handle = load_i64(sess, thread_slot)?;
    let thread_ptr = sess.2.build_int_to_ptr(thread_handle, ptr_ty(sess), "").map_err(builder_fail)?;
    let close_thread = extern_close_handle(sess);
    sess.2.build_call(close_thread, &[into_meta(thread_ptr.into())], "").map_err(builder_fail)?;
    let child_key = result_arg_key(sess, ret_key, 0);
    let child_val = declare_local(sess, child_key, "child", span)?;
    store_key(sess, child_val, handle.into())?;
    let ok_result = build_result_ok(sess, ret_key, child_key, child_val, span)?;
    copy_to_out(sess, ret_key, out, ok_result, span)?;
    sess.2.build_unconditional_branch(merge).map_err(builder_fail)?;
    sess.2.position_at_end(merge);
    Ok(out)
}

// Writes one UTF-16 code unit into `buf16` at the running unit offset held
// in `w_slot`, advancing the offset by one unit (two bytes).
fn store_utf16<'ctx>(
    sess: &mut Session<'ctx, '_, '_>,
    buf16: PointerValue<'ctx>,
    w_slot: PointerValue<'ctx>,
    unit: IntValue<'ctx>,
) -> Result<(), CodegenError> {
    let w = load_i64(sess, w_slot)?;
    let slot = offset_buffer_elem_ptr(sess, sess.0.i16_type().into(), buf16, w)?;
    let unit16 = sess.2.build_int_truncate(unit, sess.0.i16_type(), "").map_err(builder_fail)?;
    sess.2.build_store(slot, unit16).map_err(builder_fail)?;
    let w2 = sess.2.build_int_add(w, sess.0.i64_type().const_int(1, false), "").map_err(builder_fail)?;
    store_key(sess, w_slot, w2.into())?;
    Ok(())
}

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
    let bptr = byte_elem_ptr(sess, data, idx)?;
    let b = load_i8(sess, bptr)?;
    let byte = utf8_byte_value(sess, b)?;
    let top2 = sess.2.build_right_shift(byte, sess.0.i64_type().const_int(6, false), false, "").map_err(builder_fail)?;
    let is_cont = sess.2.build_int_compare(IntPredicate::EQ, top2, sess.0.i64_type().const_int(2, false), "").map_err(builder_fail)?;
    let ok2 = new_block(sess, f, "utf8_okc");
    sess.2.build_conditional_branch(is_cont, ok2, bad).map_err(builder_fail)?;
    sess.2.position_at_end(ok2);
    Ok(ok2)
}

// Emits the UTF-8 well-formedness scan over the `len` bytes at `data`.
//
// Control leaves through exactly one of the two blocks the caller supplies:
// `valid_block` once every sequence in the range has been accepted, and
// `invalid_block` at the first one that is not. The caller positions the
// builder at each and decides what a well-formed or malformed buffer means
// for it; the scan has no opinion about where the bytes came from.
//
// Every construction of a `Collections.String` from bytes that are not
// settled at compile time runs this one scan, which is what makes "a String
// holds well-formed UTF-8" a single fact rather than one per constructor.
// A second copy would be free to drift — accepting an overlong encoding
// here and rejecting it there — while the language promises one answer.
fn emit_utf8_scan<'ctx>(
    sess: &mut Session<'ctx, '_, '_>,
    f: FunctionValue<'ctx>,
    data: PointerValue<'ctx>,
    len: IntValue<'ctx>,
    valid_block: BasicBlockId<'ctx>,
    invalid_block: BasicBlockId<'ctx>,
    span: (i64, i64, i64),
) -> Result<(), CodegenError> {
    let i_slot = alloca_raw(sess, sess.0.i64_type().into(), "i", span)?;
    store_key(sess, i_slot, sess.0.i64_type().const_zero().into())?;
    let loop_cond = new_block(sess, f, "utf8_cond");
    let loop_body = new_block(sess, f, "utf8_body");
    sess.2.build_unconditional_branch(loop_cond).map_err(builder_fail)?;
    sess.2.position_at_end(loop_cond);
    let i = load_i64(sess, i_slot)?;
    let done = sess.2.build_int_compare(IntPredicate::ULT, i, len, "").map_err(builder_fail)?;
    sess.2.build_conditional_branch(done, loop_body, valid_block).map_err(builder_fail)?;
    sess.2.position_at_end(loop_body);
    let bptr = byte_elem_ptr(sess, data, i)?;
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
    // A 2-byte sequence from C2..DF decodes to U+0080..U+07FF: never
    // overlong, never a surrogate, never above U+10FFFF, so no range check.
    let two = sess.0.i64_type().const_int(2, false);
    let i3 = sess.2.build_int_add(i, two, "").map_err(builder_fail)?;
    store_key(sess, i_slot, i3.into())?;
    sess.2.build_unconditional_branch(loop_cond).map_err(builder_fail)?;
    sess.2.position_at_end(chk2);
    let c2a = emit_cont_step(sess, f, i, len, data, 1, bad)?;
    sess.2.position_at_end(c2a);
    let c2b = emit_cont_step(sess, f, i, len, data, 2, bad)?;
    sess.2.position_at_end(c2b);
    // Decode the 3-byte code point
    // ((lead & 0x0F) << 12) | ((b1 & 0x3F) << 6) | (b2 & 0x3F) and reject
    // overlong encodings (cp < U+0800) and surrogates (U+D800..U+DFFF).
    let one = sess.0.i64_type().const_int(1, false);
    let i1 = sess.2.build_int_add(i, one, "").map_err(builder_fail)?;
    let i2 = sess.2.build_int_add(i, two, "").map_err(builder_fail)?;
    let b1p = byte_elem_ptr(sess, data, i1)?;
    let b1r = load_i8(sess, b1p)?;
    let b1 = utf8_byte_value(sess, b1r)?;
    let b2p = byte_elem_ptr(sess, data, i2)?;
    let b2r = load_i8(sess, b2p)?;
    let b2 = utf8_byte_value(sess, b2r)?;
    let lead_lo = sess.2.build_and(byte, sess.0.i64_type().const_int(0x0F, false), "").map_err(builder_fail)?;
    let lead_sh = sess.2.build_left_shift(lead_lo, sess.0.i64_type().const_int(12, false), "").map_err(builder_fail)?;
    let b1_lo = sess.2.build_and(b1, sess.0.i64_type().const_int(0x3F, false), "").map_err(builder_fail)?;
    let b1_sh = sess.2.build_left_shift(b1_lo, sess.0.i64_type().const_int(6, false), "").map_err(builder_fail)?;
    let b2_lo = sess.2.build_and(b2, sess.0.i64_type().const_int(0x3F, false), "").map_err(builder_fail)?;
    let cp_mid = sess.2.build_or(lead_sh, b1_sh, "").map_err(builder_fail)?;
    let cp3 = sess.2.build_or(cp_mid, b2_lo, "").map_err(builder_fail)?;
    let ge_min3 = sess.2.build_int_compare(IntPredicate::UGE, cp3, sess.0.i64_type().const_int(UTF8_CP_3_MIN, false), "").map_err(builder_fail)?;
    let lt_surr = sess.2.build_int_compare(IntPredicate::ULT, cp3, sess.0.i64_type().const_int(UTF8_SURROGATE_MIN, false), "").map_err(builder_fail)?;
    let gt_surr = sess.2.build_int_compare(IntPredicate::UGT, cp3, sess.0.i64_type().const_int(UTF8_SURROGATE_MAX, false), "").map_err(builder_fail)?;
    let not_surr = sess.2.build_or(lt_surr, gt_surr, "").map_err(builder_fail)?;
    let ok3 = sess.2.build_and(ge_min3, not_surr, "").map_err(builder_fail)?;
    let chk2_valid = new_block(sess, f, "utf8_3ok");
    sess.2.build_conditional_branch(ok3, chk2_valid, bad).map_err(builder_fail)?;
    sess.2.position_at_end(chk2_valid);
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
    // Decode the 4-byte code point
    // ((lead & 0x07) << 18) | ((b1 & 0x3F) << 12) | ((b2 & 0x3F) << 6) |
    // (b3 & 0x3F) and reject overlong encodings (cp < U+10000) and code
    // points above U+10FFFF.  A 4-byte sequence can never be a surrogate.
    let three_c = sess.0.i64_type().const_int(3, false);
    let i1b = sess.2.build_int_add(i, one, "").map_err(builder_fail)?;
    let i2b = sess.2.build_int_add(i, two, "").map_err(builder_fail)?;
    let i3b = sess.2.build_int_add(i, three_c, "").map_err(builder_fail)?;
    let b1p = byte_elem_ptr(sess, data, i1b)?;
    let b1r = load_i8(sess, b1p)?;
    let b1b = utf8_byte_value(sess, b1r)?;
    let b2p = byte_elem_ptr(sess, data, i2b)?;
    let b2r = load_i8(sess, b2p)?;
    let b2b = utf8_byte_value(sess, b2r)?;
    let b3p = byte_elem_ptr(sess, data, i3b)?;
    let b3r = load_i8(sess, b3p)?;
    let b3b = utf8_byte_value(sess, b3r)?;
    let lead4_lo = sess.2.build_and(byte, sess.0.i64_type().const_int(0x07, false), "").map_err(builder_fail)?;
    let lead4_sh = sess.2.build_left_shift(lead4_lo, sess.0.i64_type().const_int(18, false), "").map_err(builder_fail)?;
    let b1_lo2 = sess.2.build_and(b1b, sess.0.i64_type().const_int(0x3F, false), "").map_err(builder_fail)?;
    let b1_sh2 = sess.2.build_left_shift(b1_lo2, sess.0.i64_type().const_int(12, false), "").map_err(builder_fail)?;
    let b2_lo2 = sess.2.build_and(b2b, sess.0.i64_type().const_int(0x3F, false), "").map_err(builder_fail)?;
    let b2_sh2 = sess.2.build_left_shift(b2_lo2, sess.0.i64_type().const_int(6, false), "").map_err(builder_fail)?;
    let b3_lo2 = sess.2.build_and(b3b, sess.0.i64_type().const_int(0x3F, false), "").map_err(builder_fail)?;
    let cp_mid2 = sess.2.build_or(lead4_sh, b1_sh2, "").map_err(builder_fail)?;
    let cp_mid3 = sess.2.build_or(cp_mid2, b2_sh2, "").map_err(builder_fail)?;
    let cp4 = sess.2.build_or(cp_mid3, b3_lo2, "").map_err(builder_fail)?;
    let ge_min4 = sess.2.build_int_compare(IntPredicate::UGE, cp4, sess.0.i64_type().const_int(UTF8_CP_4_MIN, false), "").map_err(builder_fail)?;
    let le_max = sess.2.build_int_compare(IntPredicate::ULE, cp4, sess.0.i64_type().const_int(UTF8_CP_MAX, false), "").map_err(builder_fail)?;
    let ok4 = sess.2.build_and(ge_min4, le_max, "").map_err(builder_fail)?;
    let chk3_valid = new_block(sess, f, "utf8_4ok");
    sess.2.build_conditional_branch(ok4, chk3_valid, bad).map_err(builder_fail)?;
    sess.2.position_at_end(chk3_valid);
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
    Ok(())
}

fn native_string_from_slice<'ctx>(sess: &mut Session<'ctx, '_, '_>, f: FunctionValue<'ctx>, locals: &Locals<'ctx>, ret_key: i64, out: PointerValue<'ctx>, span: (i64, i64, i64)) -> Result<PointerValue<'ctx>, CodegenError> {
    let p0 = get_local(locals, 0, span)?;
    let data = slice_data(sess, p0)?;
    let len = slice_len_of(sess, p0)?;
    let valid_block = new_block(sess, f, "utf8_valid");
    let invalid_block = new_block(sess, f, "utf8_invalid");
    emit_utf8_scan(sess, f, data, len, valid_block, invalid_block, span)?;
    sess.2.position_at_end(invalid_block);
    let err_key = result_arg_key(sess, ret_key, 1);
    let invalid_tag = seeded_enum_variant_tag(sess, SEED_SYM_INVALID_UTF8, span)?;
    let fail_val = build_enum_value(sess, err_key, invalid_tag, &[], span)?;
    let err_result = build_result_err(sess, ret_key, err_key, fail_val, span)?;
    copy_to_out(sess, ret_key, out, err_result, span)?;
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
            return Err(builder_error(span.0, span.1, span.2, &format!("internal: malloc returned void ({:?})", inst.get_opcode())));
        }
    };
    let null_cmp = is_null_ptr(sess, raw)?;
    let fail_alloc = new_block(sess, f, "str_alloc_fail");
    let copy_block = new_block(sess, f, "str_copy");
    sess.2.build_conditional_branch(null_cmp, fail_alloc, copy_block).map_err(builder_fail)?;
    sess.2.position_at_end(fail_alloc);
    let err_key2 = result_arg_key(sess, ret_key, 1);
    let alloc_fail_tag = seeded_enum_variant_tag(sess, SEED_SYM_ALLOC_FAILED, span)?;
    let fkey = variant_payload_key(sess, err_key2, alloc_fail_tag, 0, span)?;
    let fval = declare_local(sess, fkey, "need", span)?;
    store_key(sess, fval, len.into())?;
    let fail_val2 = build_enum_value(sess, err_key2, alloc_fail_tag, &[(fkey, fval)], span)?;
    let err_result2 = build_result_err(sess, ret_key, err_key2, fail_val2, span)?;
    copy_to_out(sess, ret_key, out, err_result2, span)?;
    sess.2.build_unconditional_branch(after).map_err(builder_fail)?;
    sess.2.position_at_end(copy_block);
    sess.2.build_memcpy(raw, 1, data, 1, len).map_err(builder_fail)?;
    let str_key = result_arg_key(sess, ret_key, 0);
    let str_val = declare_local(sess, str_key, "str", span)?;
    let sd = struct_gep(sess, str_key, str_val, 0, "", span)?;
    store_key(sess, sd, raw.into())?;
    let sl = struct_gep(sess, str_key, str_val, 1, "", span)?;
    store_key(sess, sl, len.into())?;
    let ok_result = build_result_ok(sess, ret_key, str_key, str_val, span)?;
    copy_to_out(sess, ret_key, out, ok_result, span)?;
    sess.2.build_unconditional_branch(after).map_err(builder_fail)?;
    sess.2.position_at_end(after);
    Ok(out)
}

fn find_main_fn(sess: &Session) -> i64 {
    let mut idx = 0i64;
    while idx < sess.5.len() as i64 / NODE_STRIDE {
        if node_tag(sess.5, idx) == NODE_SYM
            && node_a(sess.5, idx) == SYM_FUN
            && node_f(sess.5, idx) == SYM_FUN_MAIN
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

pub fn emit_program<'ctx>(sess: &mut Session<'ctx, '_, '_>, entry_span: (i64, i64, i64)) -> Result<(), CodegenError> {
    let main_fn = find_main_fn(sess);
    if main_fn == NONE {
        return Err(builder_error(entry_span.0, entry_span.1, entry_span.2, "program has no main function"));
    }
    let main_span = (node_file(sess.5, main_fn), node_start(sess.5, main_fn), node_end(sess.5, main_fn));
    let params_list = fn_param_key_list(sess, main_fn)?;
    let ret_key = ty_key_of(sess.5, node_d(sess.5, main_fn));
    let nodes = &mut sess.5;
    let lists = &mut sess.6;
    let mono = canon_tyinfo(nodes, lists, TYD_MONO, main_fn, NONE, NONE, NONE);
    let main_val = get_or_emit_fn(sess, main_fn, NONE, mono, params_list, ret_key)?;
    let exit_key = ret_key;
    let i32_ty = sess.0.i32_type();
    // `main(int argc, char **argv)` rather than `main(void)`: the C runtime
    // passes the command line here and nowhere else, so `Runtime.args` can
    // only see it if the entry point accepts it. The two values are stashed
    // in module globals on entry and read back when a program asks for
    // them — a program that never calls `Runtime.args` pays two stores and
    // allocates nothing.
    let sig = i32_ty.fn_type(&[i32_ty.into(), ptr_ty(sess).into()], false);
    let main_wrapper = sess.1.add_function("main", sig, None);
    let entry = sess.0.append_basic_block(main_wrapper, "entry");
    sess.2.position_at_end(entry);
    capture_command_line(sess, main_wrapper, main_span)?;
    let call = sess.2.build_call(main_val, &[], "").map_err(builder_fail)?;
    let exit_val = match call.try_as_basic_value() {
        ValueKind::Basic(bv) => bv,
        ValueKind::Instruction(inst) => {
            return Err(builder_error(
                node_file(sess.5, main_fn),
                node_start(sess.5, main_fn),
                node_end(sess.5, main_fn),
                &format!("internal: main returned void ({:?})", inst.get_opcode()),
            ));
        }
    };
    let exit_alloca = declare_local(sess, exit_key, "exit", main_span)?;
    store_key(sess, exit_alloca, exit_val)?;
    let exit_kind = em_key_kind(sess, exit_key);
    if exit_kind == TYD_BUILTIN {
        let code_val = load_key(sess, exit_key, exit_alloca, main_span)?.into_int_value();
        let code = sess.2.build_int_cast(code_val, i32_ty, "").map_err(builder_fail)?;
        sess.2.build_return(Some(&code)).map_err(builder_fail)?;
        return Ok(());
    }
    if exit_kind == TYD_ENUM {
        let tag_ptr = struct_gep(sess, exit_key, exit_alloca, 0, "", main_span)?;
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
        let diag_tag = exit_diag_tag_of(sess, exit_key, main_span)?;
        sess.2.position_at_end(diag_block);
        if diag_tag != NONE {
            let (region, pty) = enum_payload_ptr(sess, exit_alloca, exit_key, diag_tag, main_span)?;
            let payload = sess.2.build_struct_gep(pty, region, 0, "").map_err(builder_fail)?;
            let diag_key = variant_payload_key(sess, exit_key, diag_tag, 0, main_span)?;
            let diag = load_key(sess, diag_key, payload, main_span)?.into_int_value();
            let code = sess.2.build_int_cast(diag, i32_ty, "").map_err(builder_fail)?;
            sess.2.build_return(Some(&code)).map_err(builder_fail)?;
        } else {
            let code1 = i32_ty.const_int(1, false);
            sess.2.build_return(Some(&code1)).map_err(builder_fail)?;
        }
        return Ok(());
    }
    // Any other return layout is rejected by the typechecker (main must
    // return a builtin scalar, Unit, or an exit-status enum), so this is
    // defensive: exit with code 1 instead of misreading a non-tag layout.
    let code1 = i32_ty.const_int(1, false);
    sess.2.build_return(Some(&code1)).map_err(builder_fail)?;
    Ok(())
}
