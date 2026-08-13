use crate::ast::*;
use crate::codegen::error::*;
use crate::codegen::syscall;
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
    Protocol,
);

#[derive(Clone, Copy)]
pub struct Protocol {
    pub ok: i64,
    pub err: i64,
    pub some: i64,
    pub none: i64,
    pub div_by_zero: i64,
    pub alloc_failed: i64,
    pub oob: i64,
    pub index_oob: i64,
    pub key_not_found: i64,
    pub invalid_utf8: i64,
    pub exit_diag: i64,
    pub system_fault: i64,
    pub read_only: i64,
    pub write_truncate: i64,
    pub end_of_input: i64,
}

pub fn protocol_of(names: &[String]) -> Protocol {
    Protocol {
        ok: find_name(names, "Ok"),
        err: find_name(names, "Err"),
        some: find_name(names, "Some"),
        none: find_name(names, "None"),
        div_by_zero: find_name(names, "DivByZero"),
        alloc_failed: find_name(names, "AllocationFailed"),
        oob: find_name(names, "AccessOutOfBounds"),
        index_oob: find_name(names, "IndexOutOfBounds"),
        key_not_found: find_name(names, "KeyNotFound"),
        invalid_utf8: find_name(names, "InvalidUtf8"),
        exit_diag: find_name(names, "ExitDiagnostic"),
        system_fault: find_name(names, "SystemFault"),
        read_only: find_name(names, "ReadOnly"),
        write_truncate: find_name(names, "WriteTruncate"),
        end_of_input: find_name(names, "EndOfInput"),
    }
}

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
    bool,
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

// The attached fact row for `name` on the canonical struct key, filled by
// the typechecker; no ITEM_STRUCT re-walk and no re-run of generic
// substitution here (Single-Fact Rule).  Callers read the index and key
// slots they consume.
fn struct_field_fact_row(sess: &Session, struct_key: i64, name: i64, span: (i64, i64, i64)) -> Result<i64, CodegenError> {
    let row = find_fieldkey(sess.5, struct_key, name);
    if row == NONE {
        return Err(builder_error(span.0, span.1, span.2, "internal: struct field fact not found"));
    }
    Ok(row)
}

// The declared-order tag was attached to the varfact row by the
// typechecker; no ITEM_ENUM variant-list re-search (Single-Fact Rule).
fn variant_index_of_raw(sess: &Session, enum_key: i64, variant_sym: i64) -> i64 {
    if variant_sym == NONE {
        return NONE;
    }
    let vdecl = node_c(sess.5, variant_sym);
    let name = node_a(sess.5, vdecl);
    if name == NONE {
        return NONE;
    }
    varfact_index_of(sess.5, enum_key, name)
}

fn variant_index_of(sess: &Session, enum_key: i64, variant_sym: i64, span: (i64, i64, i64)) -> Result<i64, CodegenError> {
    let idx = variant_index_of_raw(sess, enum_key, variant_sym);
    if idx == NONE {
        return Err(builder_error(span.0, span.1, span.2, "internal: variant not found in its enum"));
    }
    Ok(idx)
}

fn variant_tag_of(sess: &Session, key: i64, name_id: i64, span: (i64, i64, i64)) -> Result<i64, CodegenError> {
    if name_id == NONE {
        return Err(builder_error(span.0, span.1, span.2, "internal: protocol variant name not interned"));
    }
    let vsym = find_varfact(sess.5, key, name_id);
    if vsym == NONE {
        return Err(builder_error(span.0, span.1, span.2, &format!("internal: variant '{}' not found in its enum", em_name(sess, name_id))));
    }
    variant_index_of(sess, key, vsym, span)
}

fn variant_tag_of_opt(sess: &Session, key: i64, name_id: i64) -> i64 {
    if name_id == NONE {
        return NONE;
    }
    varfact_index_of(sess.5, key, name_id)
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
    let enum_sym = em_key_sym(sess, enum_key);
    if enum_sym == NONE {
        return Err(builder_error(span.0, span.1, span.2, "internal: enum key without a symbol"));
    }
    let item = em_sym_decl(sess, enum_sym);
    let variants = node_e(sess.5, item);
    let variant = list_get(sess.6, variants, variant_idx);
    if variant == NONE {
        return Err(builder_error(span.0, span.1, span.2, "internal: variant index out of range"));
    }
    let payload_decl = node_b(sess.5, variant);
    let declared = ty_key_of(sess.5, list_get(sess.6, payload_decl, field_idx));
    let from = declared_param_keys_of_item(sess, item);
    let to = list_to_vec_of(sess, key_args_of(sess, enum_key));
    let nodes = &mut sess.5;
    let lists = &mut sess.6;
    Ok(subst_key(nodes, lists, declared, &from, &to))
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

fn offset_elem_ptr<'ctx>(sess: &mut Session<'ctx, '_, '_>, elem_key: i64, base: PointerValue<'ctx>, idx: IntValue<'ctx>, span: (i64, i64, i64)) -> Result<PointerValue<'ctx>, CodegenError> {
    let elem_ty = llvm_of(sess, elem_key, span)?;
    let gep = unsafe { sess.2.build_gep(elem_ty, base, &[idx], "") }.map_err(builder_fail)?;
    Ok(gep)
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
    let saved_tail = ctx.6;
    ctx.6 = false;
    emit_stmt_list(sess, ctx, body, key, span)?;
    ctx.6 = saved_tail;
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
    let saved_tail = ctx.6;
    ctx.6 = false;
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
    ctx.6 = saved_tail;
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
        let saved_tail = ctx.6;
        ctx.6 = true;
        let expr_ptr = emit_expr(sess, ctx, value);
        ctx.6 = saved_tail;
        expr_ptr?
    };
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
    let saved_tail = ctx.6;
    if kind == EXPR_MATCH {
        // A match in tail position forwards its non-diverging arm values
        // to the tail, so tailness propagates into the arm bodies;
        // emit_match clears it around the scrutinee and pattern work.
        emit_match(sess, ctx, expr)
    } else {
        ctx.6 = false;
        let r = if kind == EXPR_LIT {
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
        };
        ctx.6 = saved_tail;
        r
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
            let idx = variant_index_of(sess, key, sym, span)?;
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
        let fld_idx = fieldkey_idx_of(sess.5, row);
        let fkey = fieldkey_key_of(sess.5, row);
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
        if em_key_kind(sess, key) == TYD_ENUM {
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
    let proto = sess.12;
    let err_key = result_arg_key(sess, result_key, 1);
    let err_tag = variant_tag_of(sess, err_key, proto.div_by_zero, span)?;
    let div_error = declare_local(sess, err_key, "div_err_val", span)?;
    build_enum_value_into(sess, err_key, err_tag, &[], div_error, span)?;
    let err_variant = variant_tag_of(sess, result_key, proto.err, span)?;
    build_enum_value_into(sess, result_key, err_variant, &[(err_key, div_error)], out, span)?;
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
    let ok_tag = variant_tag_of(sess, result_key, proto.ok, span)?;
    build_enum_value_into(sess, result_key, ok_tag, &[(lkey, quotient)], out, span)?;
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
        let eptr = offset_elem_ptr(sess, elem_key, ptr, sess.0.i64_type().const_int(idx as u64, false), span)?;
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
            let fld_idx = fieldkey_idx_of(sess.5, row);
            let fkey = fieldkey_key_of(sess.5, row);
            let fptr = struct_gep(sess, key, ptr, fld_idx as u32, "", span)?;
            let vptr = emit_expr(sess, ctx, list_get(sess.6, values, idx))?;
            copy_value(sess, fkey, fptr, vptr, span)?;
            idx += 1;
        }
        return Ok(ptr);
    }
    if kind == SYM_VARIANT {
        let idx = variant_index_of(sess, key, sym, span)?;
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
    let saved_tail = ctx.6;
    ctx.6 = false;
    let scrut_ptr = emit_expr(sess, ctx, scrutinee)?;
    ctx.6 = saved_tail;
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
        let idx = variant_index_of(sess, pat_key, sym, span)?;
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
        let idx = variant_index_of(sess, pat_key, sym, span)?;
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
    let mut i = 0i64;
    while i < fixed {
        let eptr = offset_elem_ptr(sess, elem_key, data, sess.0.i64_type().const_int(i as u64, false), span)?;
        let subpat = list_get(sess.6, elems, i);
        let sub_scrut: MatchScrut<'ctx> = (elem_key, eptr, fail_block);
        emit_pattern(sess, ctx, subpat, sub_scrut, NONE, continuation)?;
        i += 1;
    }
    if rest != NONE {
        let rest_key = sub_key(sess, ctx.3, ctx.4, pat_rest_key_of(sess.5, pat));
        let rptr = declare_local(sess, rest_key, "rest", span)?;
        let rdata = slice_gep(sess, rptr, 0, "")?;
        let rest_base = offset_elem_ptr(sess, elem_key, data, sess.0.i64_type().const_int(fixed as u64, false), span)?;
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
    let proto = sess.12;
    let inner_sym = em_key_sym(sess, inner_key);
    let is_result = sym_prim_kind(sess.5, inner_sym) == PRIM_RESULT;
    let ok_name = if is_result { proto.ok } else { proto.some };
    let err_name = if is_result { proto.err } else { proto.none };
    let ok_tag = variant_tag_of(sess, inner_key, ok_name, span)?;
    let err_tag = variant_tag_of(sess, inner_key, err_name, span)?;
    let ret_err_tag = variant_tag_of(sess, ret_key, err_name, span)?;
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

    // The typechecker attaches the element type directly to constant array
    // indices (proven in range at compile time) and Result(T, IndexError)
    // only to dynamic array and slice indices.  An element type that is
    // itself an enum is not a fallible Result; the seeded Result primitive
    // kind is the single source of truth for the fallible path.
    let key_sym = em_key_sym(sess, key);
    if key_sym == NONE || sym_prim_kind(sess.5, key_sym) != PRIM_RESULT {
        let eptr = offset_elem_ptr(sess, elem_key, data_ptr, idx_val, span)?;
        return Ok(eptr);
    }
    let is_oob = sess.2.build_int_compare(IntPredicate::UGE, idx_val, len_val, "").map_err(builder_fail)?;
    let ok_block = new_block(sess, ctx.0, "idx_ok");
    let err_block = new_block(sess, ctx.0, "idx_err");
    let merge = new_block(sess, ctx.0, "idx_merge");
    sess.2.build_conditional_branch(is_oob, err_block, ok_block).map_err(builder_fail)?;

    let proto = sess.12;
    let payload_key = result_arg_key(sess, key, 0);
    let err_key = result_arg_key(sess, key, 1);
    let out = declare_local(sess, key, "idx", span)?;
    sess.2.position_at_end(err_block);
    let oob_tag = variant_tag_of(sess, err_key, proto.index_oob, span)?;
    let f0 = variant_payload_key(sess, err_key, oob_tag, 0, span)?;
    let f1 = variant_payload_key(sess, err_key, oob_tag, 1, span)?;
    let e0 = declare_local(sess, f0, "iob_idx", span)?;
    store_key(sess, e0, idx_val.into())?;
    let e1 = declare_local(sess, f1, "iob_len", span)?;
    store_key(sess, e1, len_val.into())?;
    let oob_val = build_enum_value(sess, err_key, oob_tag, &[(f0, e0), (f1, e1)], span)?;
    let err_variant = variant_tag_of(sess, key, proto.err, span)?;
    build_enum_value_into(sess, key, err_variant, &[(err_key, oob_val)], out, span)?;
    sess.2.build_unconditional_branch(merge).map_err(builder_fail)?;
    sess.2.position_at_end(ok_block);
    let eptr = offset_elem_ptr(sess, elem_key, data_ptr, idx_val, span)?;
    let ok_tag = variant_tag_of(sess, key, proto.ok, span)?;
    let payload_kind = em_key_kind(sess, payload_key);
    if payload_kind == TYD_REF || payload_kind == TYD_REF_MUT {
        let ref_slot = declare_local(sess, payload_key, "idx_ref", span)?;
        store_key(sess, ref_slot, eptr.into())?;
        build_enum_value_into(sess, key, ok_tag, &[(payload_key, ref_slot)], out, span)?;
    } else {
        build_enum_value_into(sess, key, ok_tag, &[(payload_key, eptr)], out, span)?;
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
    let is_tail = ctx.6;
    let args_list = inst_args_of(sess.5, inst);
    let mono = inst_mono_of(sess.5, inst);
    let params_list = inst_params_of(sess.5, inst);
    let ret_key = sub_key(sess, ctx.3, ctx.4, inst_ret_of(sess.5, inst));
    let caller_block = sess.2.get_insert_block();
    let fn_val = get_or_emit_fn(sess, fn_slot, args_list, mono, params_list, ret_key)?;
    match caller_block {
        Some(block) => sess.2.position_at_end(block),
        None => return Err(builder_error(span.0, span.1, span.2, "internal: no insertion block")),
    }
    let saved_tail = ctx.6;
    ctx.6 = false;
    let arg_vals = emit_call_args(sess, ctx, expr);
    ctx.6 = saved_tail;
    let arg_vals = arg_vals?;
    let call = sess.2.build_call(fn_val, &arg_vals, "").map_err(builder_fail)?;
    if is_tail {
        call.set_tail_call(true);
    }
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

fn emit_deferred_trait_call<'ctx, 'a>(
    sess: &mut Session<'ctx, '_, '_>,
    ctx: &mut FnCtx<'ctx, 'a>,
    expr: i64,
    trow: i64,
) -> Result<PointerValue<'ctx>, CodegenError> {
    let is_tail = ctx.6;
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
    let caller_block = sess.2.get_insert_block();
    let fn_val = get_or_emit_fn(sess, fn_node, NONE, mono, param_keys, result)?;
    match caller_block {
        Some(block) => sess.2.position_at_end(block),
        None => return Err(builder_error(span.0, span.1, span.2, "internal: no insertion block")),
    }
    let saved_tail = ctx.6;
    ctx.6 = false;
    let arg_vals = emit_call_args(sess, ctx, expr);
    ctx.6 = saved_tail;
    let arg_vals = arg_vals?;
    let call = sess.2.build_call(fn_val, &arg_vals, "").map_err(builder_fail)?;
    if is_tail {
        call.set_tail_call(true);
    }
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
) -> Result<Vec<BasicMetadataValueEnum<'ctx>>, CodegenError> {
    let span = (node_file(sess.5, expr), node_start(sess.5, expr), node_end(sess.5, expr));
    let arg_exprs = node_d(sess.5, expr);
    let count = list_len(sess.6, arg_exprs);
    let mut vals: Vec<BasicMetadataValueEnum<'ctx>> = Vec::new();
    let mut idx = 0i64;
    while idx < count {
        let arg = list_get(sess.6, arg_exprs, idx);
        let akey = sub_key(sess, ctx.3, ctx.4, em_expr_ty(sess, arg));
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
        let ty = llvm_of(sess, pkey, span)?;
        param_tys.push(ty.into());
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
        let ptr = declare_local(sess, pkey, &em_name(sess, pname), span)?;
        let pval = match param_values.get(idx as usize) {
            Some(value) => *value,
            None => break,
        };
        store_key(sess, ptr, pval)?;
        bind_local(&mut body_locals, pname, pkey, ptr);
        idx += 1;
    }
    let mut ctx: FnCtx<'ctx, '_> = (fn_val, body_locals, fn_loops, from.as_slice(), to.as_slice(), ret_key, false);
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

fn extern_memcmp<'ctx>(sess: &mut Session<'ctx, '_, '_>) -> FunctionValue<'ctx> {
    let i8p = ptr_ty(sess);
    extern_fn(
        sess,
        "memcmp",
        sess.0.i32_type().fn_type(&[i8p.into(), i8p.into(), sess.0.i64_type().into()], false),
    )
}

// The architecture whose syscall ABI this module targets.
//
// Read from the module's own triple rather than carried in `Session`, so
// there is one source for it and no chance of the emitter and the linker
// disagreeing about what is being built. An architecture with no
// implemented ABI is a compile error naming the triple, never a guess: a
// syscall number is meaningless on an architecture whose table is absent,
// and emitting one anyway would call an arbitrary kernel entry point.
fn target_arch(sess: &Session, span: (i64, i64, i64)) -> Result<syscall::Arch, CodegenError> {
    let triple = sess.1.get_triple();
    let text = triple.as_str().to_string_lossy().to_string();
    match syscall::arch_of(&text) {
        Some(arch) => Ok(arch),
        None => Err(builder_error(
            span.0,
            span.1,
            span.2,
            &format!("no system-call ABI is implemented for target '{}': Memory, Terminal, and File issue Linux system calls directly on x86_64 and AArch64", text),
        )),
    }
}

// Issues one system call, widening every argument to the machine word the
// ABI passes it in.
fn emit_syscall<'ctx>(
    sess: &mut Session<'ctx, '_, '_>,
    call: syscall::Sys,
    args: &[IntValue<'ctx>],
    span: (i64, i64, i64),
) -> Result<IntValue<'ctx>, CodegenError> {
    let arch = target_arch(sess, span)?;
    let mut widened: Vec<IntValue<'ctx>> = Vec::new();
    let mut idx = 0usize;
    while idx < args.len() {
        let arg = match args.get(idx) {
            Some(value) => *value,
            None => break,
        };
        widened.push(word_of(sess, arg, span)?);
        idx += 1;
    }
    syscall::emit(sess.0, sess.2, arch, call, &widened, span)
}

// Sign-extends a value to the machine word a syscall argument register
// holds.  Sign-extension rather than zero-extension because several
// arguments are signed: `AT_FDCWD` is -100 and `mmap`'s file descriptor is
// -1 for an anonymous mapping.
fn word_of<'ctx>(sess: &mut Session<'ctx, '_, '_>, value: IntValue<'ctx>, span: (i64, i64, i64)) -> Result<IntValue<'ctx>, CodegenError> {
    let i64_ty = sess.0.i64_type();
    if value.get_type().get_bit_width() >= 64 {
        return Ok(value);
    }
    sess.2
        .build_int_s_extend(value, i64_ty, "")
        .map_err(|err| builder_error(span.0, span.1, span.2, &format!("internal: cannot widen a system-call argument: {}", err)))
}

// A pointer as the integer a syscall argument register holds.
fn ptr_word<'ctx>(sess: &mut Session<'ctx, '_, '_>, ptr: PointerValue<'ctx>, span: (i64, i64, i64)) -> Result<IntValue<'ctx>, CodegenError> {
    sess.2
        .build_ptr_to_int(ptr, sess.0.i64_type(), "")
        .map_err(|err| builder_error(span.0, span.1, span.2, &format!("internal: cannot pass a pointer to a system call: {}", err)))
}

// The integer a syscall returned, as a pointer.
fn word_ptr<'ctx>(sess: &mut Session<'ctx, '_, '_>, value: IntValue<'ctx>, span: (i64, i64, i64)) -> Result<PointerValue<'ctx>, CodegenError> {
    sess.2
        .build_int_to_ptr(value, ptr_ty(sess), "")
        .map_err(|err| builder_error(span.0, span.1, span.2, &format!("internal: cannot read a pointer out of a system-call result: {}", err)))
}

fn extern_socket<'ctx>(sess: &mut Session<'ctx, '_, '_>) -> FunctionValue<'ctx> {
    let i32_ty = sess.0.i32_type();
    extern_fn(
        sess,
        "socket",
        i32_ty.fn_type(&[i32_ty.into(), i32_ty.into(), i32_ty.into()], false),
    )
}

fn extern_bind<'ctx>(sess: &mut Session<'ctx, '_, '_>) -> FunctionValue<'ctx> {
    let i32_ty = sess.0.i32_type();
    extern_fn(
        sess,
        "bind",
        i32_ty.fn_type(&[i32_ty.into(), ptr_ty(sess).into(), i32_ty.into()], false),
    )
}

fn extern_listen<'ctx>(sess: &mut Session<'ctx, '_, '_>) -> FunctionValue<'ctx> {
    let i32_ty = sess.0.i32_type();
    extern_fn(sess, "listen", i32_ty.fn_type(&[i32_ty.into(), i32_ty.into()], false))
}

fn extern_accept<'ctx>(sess: &mut Session<'ctx, '_, '_>) -> FunctionValue<'ctx> {
    let i32_ty = sess.0.i32_type();
    extern_fn(
        sess,
        "accept",
        i32_ty.fn_type(&[i32_ty.into(), ptr_ty(sess).into(), ptr_ty(sess).into()], false),
    )
}

fn extern_send<'ctx>(sess: &mut Session<'ctx, '_, '_>) -> FunctionValue<'ctx> {
    let i32_ty = sess.0.i32_type();
    extern_fn(
        sess,
        "send",
        sess.0
            .i64_type()
            .fn_type(&[i32_ty.into(), ptr_ty(sess).into(), sess.0.i64_type().into(), i32_ty.into()], false),
    )
}

fn extern_close<'ctx>(sess: &mut Session<'ctx, '_, '_>) -> FunctionValue<'ctx> {
    let i32_ty = sess.0.i32_type();
    extern_fn(sess, "close", i32_ty.fn_type(&[i32_ty.into()], false))
}

fn emit_native_call<'ctx, 'a>(
    sess: &mut Session<'ctx, '_, '_>,
    ctx: &mut FnCtx<'ctx, 'a>,
    expr: i64,
    inst: i64,
    sym: i64,
) -> Result<PointerValue<'ctx>, CodegenError> {
    let span = (node_file(sess.5, expr), node_start(sess.5, expr), node_end(sess.5, expr));
    let is_tail = ctx.6;
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
    let saved_tail = ctx.6;
    ctx.6 = false;
    let arg_vals = emit_call_args(sess, ctx, expr);
    ctx.6 = saved_tail;
    let arg_vals = arg_vals?;
    let call = sess.2.build_call(native_val, &arg_vals, "").map_err(builder_fail)?;
    if is_tail {
        call.set_tail_call(true);
    }
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
        let ptr = declare_local(sess, pkey, "p", span)?;
        let pval = match param_values.get(idx as usize) {
            Some(value) => *value,
            None => break,
        };
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

fn result_ok_tag(sess: &Session, key: i64, span: (i64, i64, i64)) -> Result<i64, CodegenError> {
    variant_tag_of(sess, key, sess.12.ok, span)
}

fn result_err_tag(sess: &Session, key: i64, span: (i64, i64, i64)) -> Result<i64, CodegenError> {
    variant_tag_of(sess, key, sess.12.err, span)
}

fn build_result_ok<'ctx>(sess: &mut Session<'ctx, '_, '_>, result_key: i64, payload_key: i64, payload_ptr: PointerValue<'ctx>, span: (i64, i64, i64)) -> Result<PointerValue<'ctx>, CodegenError> {
    let ok_tag = result_ok_tag(sess, result_key, span)?;
    build_enum_value(sess, result_key, ok_tag, &[(payload_key, payload_ptr)], span)
}

fn build_result_err<'ctx>(sess: &mut Session<'ctx, '_, '_>, result_key: i64, payload_key: i64, payload_ptr: PointerValue<'ctx>, span: (i64, i64, i64)) -> Result<PointerValue<'ctx>, CodegenError> {
    let err_tag = result_err_tag(sess, result_key, span)?;
    build_enum_value(sess, result_key, err_tag, &[(payload_key, payload_ptr)], span)
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

fn byte_offset<'ctx>(sess: &mut Session<'ctx, '_, '_>, base: PointerValue<'ctx>, offset: IntValue<'ctx>) -> Result<PointerValue<'ctx>, CodegenError> {
    let i8_ty = sess.0.i8_type();
    let cast = sess.2.build_pointer_cast(base, ptr_ty(sess), "").map_err(builder_fail)?;
    let gep = unsafe { sess.2.build_gep(i8_ty, cast, &[offset], "") }.map_err(builder_fail)?;
    Ok(gep)
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
    let op = sym_native_op(sess.5, sym);
    if op == NAT_INT_FROM {
        native_int_from(sess, locals, ret_key, out, span)?;
        return Ok(out);
    }
    if op == NAT_SLICE_LEN {
        native_slice_len(sess, locals, out, span)?;
        return Ok(out);
    }
    if op == NAT_MEM_ALLOCATE {
        return native_allocate(sess, f, locals, ret_key, out, span);
    }
    if op == NAT_MEM_DEALLOCATE {
        native_deallocate(sess, locals, ret_key, out, span)?;
        return Ok(out);
    }
    if op == NAT_MEM_WRITE_U8 {
        native_write_u8(sess, f, locals, ret_key, out, span)?;
        return Ok(out);
    }
    if op == NAT_MEM_READ_U8 {
        native_read_u8(sess, f, locals, ret_key, out, span)?;
        return Ok(out);
    }
    if op == NAT_VEC_NEW {
        native_vec_new(sess, ret_key, out, span)?;
        return Ok(out);
    }
    if op == NAT_VEC_PUSH {
        return native_vec_push(sess, f, locals, params_list, ret_key, out, span);
    }
    if op == NAT_VEC_VIEW {
        native_vec_view(sess, locals, out, span)?;
        return Ok(out);
    }
    if op == NAT_VEC_FREE {
        native_vec_free(sess, locals, ret_key, out, span)?;
        return Ok(out);
    }
    if op == NAT_VEC_POP {
        return native_vec_pop(sess, f, locals, ret_key, out, span);
    }
    if op == NAT_STRING_FROM_SLICE {
        return native_string_from_slice(sess, f, locals, ret_key, out, span);
    }
    if op == NAT_STRING_LEN {
        native_string_len(sess, locals, out, span)?;
        return Ok(out);
    }
    if op == NAT_STRING_FREE {
        native_string_free(sess, locals, ret_key, out, span)?;
        return Ok(out);
    }
    if op == NAT_HASH_MAP_NEW {
        native_hash_map_new(sess, ret_key, out, span)?;
        return Ok(out);
    }
    if op == NAT_HASH_MAP_INSERT {
        return native_hash_map_insert(sess, f, locals, params_list, ret_key, out, span);
    }
    if op == NAT_HASH_MAP_GET {
        return native_hash_map_get(sess, f, locals, params_list, ret_key, out, span);
    }
    if op == NAT_HASH_MAP_FREE {
        native_hash_map_free(sess, locals, ret_key, out, span)?;
        return Ok(out);
    }
    if op == NAT_HASH_MAP_REMOVE {
        return native_hash_map_remove(sess, f, locals, params_list, ret_key, out, span);
    }
    if op == NAT_FILE_OPEN {
        return native_file_open(sess, f, locals, ret_key, out, span);
    }
    if op == NAT_FILE_READ {
        return native_file_transfer(sess, f, locals, syscall::Sys::Read, ret_key, out, span);
    }
    if op == NAT_FILE_WRITE {
        return native_file_transfer(sess, f, locals, syscall::Sys::Write, ret_key, out, span);
    }
    if op == NAT_FILE_CLOSE {
        native_file_close(sess, locals, ret_key, out, span)?;
        return Ok(out);
    }
    if op == NAT_TERM_READ_LINE {
        return native_read_line(sess, f, ret_key, out, span);
    }
    if op == NAT_RUNTIME_ARGS {
        native_runtime_args(sess, f, ret_key, out, span)?;
        return Ok(out);
    }
    if op == NAT_SELF_CHECK {
        native_self_check(sess, ret_key, out, span)?;
        return Ok(out);
    }
    if op == NAT_TERM_PRINT {
        native_print(sess, locals, ret_key, out, false, false, span)?;
        return Ok(out);
    }
    if op == NAT_TERM_PRINT_LINE {
        native_print(sess, locals, ret_key, out, false, true, span)?;
        return Ok(out);
    }
    if op == NAT_TERM_EPRINT {
        native_print(sess, locals, ret_key, out, true, false, span)?;
        return Ok(out);
    }
    if op == NAT_NET_SOCKET {
        return native_net_socket(sess, f, ret_key, out, span);
    }
    if op == NAT_NET_BIND {
        native_net_bind(sess, f, locals, ret_key, out, span)?;
        return Ok(out);
    }
    if op == NAT_NET_LISTEN {
        native_net_listen(sess, f, locals, ret_key, out, span)?;
        return Ok(out);
    }
    if op == NAT_NET_ACCEPT {
        return native_net_accept(sess, f, locals, ret_key, out, span);
    }
    if op == NAT_NET_SEND {
        return native_net_send(sess, f, locals, ret_key, out, span);
    }
    if op == NAT_NET_CLOSE {
        native_net_close(sess, locals, ret_key, out, span)?;
        return Ok(out);
    }
    let name = em_name(sess, node_b(sess.5, sym));
    let llvm_name = name.replace('.', "_");
    let sig = build_fn_sig(sess, params_list, ret_key, span)?;
    let fn_val = extern_fn(sess, &llvm_name, sig);
    let pcount = list_len(sess.6, params_list);
    let mut arg_vals: Vec<BasicMetadataValueEnum<'ctx>> = Vec::new();
    let mut idx = 0i64;
    while idx < pcount {
        let p = get_local(locals, idx, span)?;
        let k = get_local_key(locals, idx, span)?;
        let v = load_key(sess, k, p, span)?;
        arg_vals.push(v.into());
        idx += 1;
    }
    let call = sess.2.build_call(fn_val, &arg_vals, "").map_err(builder_fail)?;
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
    let zero = sess.0.i64_type().const_zero();
    let prot = sess.0.i64_type().const_int(syscall::PROT_READ_WRITE as u64, false);
    let flags = sess.0.i64_type().const_int(syscall::MAP_PRIVATE_ANONYMOUS as u64, false);
    let no_fd = sess.0.i64_type().const_all_ones();
    let raw = emit_syscall(sess, syscall::Sys::Mmap, &[zero, size, prot, flags, no_fd, zero], span)?;
    // `mmap` reports failure as a small negative errno rather than as a
    // null pointer, so the failure test is `raw < 0` — a returned address
    // is never in that range for a user-space mapping.
    let failed = sess.2.build_int_compare(IntPredicate::SLT, raw, sess.0.i64_type().const_zero(), "").map_err(builder_fail)?;
    let data = word_ptr(sess, raw, span)?;
    let null_cmp = failed;
    let fail_block = new_block(sess, f, "alloc_fail");
    let ok_block = new_block(sess, f, "alloc_ok");
    let after = new_block(sess, f, "alloc_after");
    sess.2.build_conditional_branch(null_cmp, fail_block, ok_block).map_err(builder_fail)?;
    sess.2.position_at_end(fail_block);
    let err_key = result_arg_key(sess, ret_key, 1);
    let alloc_fail_tag = variant_tag_of(sess, err_key, sess.12.alloc_failed, span)?;
    let fkey = variant_payload_key(sess, err_key, alloc_fail_tag, 0, span)?;
    let fail_val = build_enum_value(sess, err_key, alloc_fail_tag, &[(fkey, p0)], span)?;
    let err_result = build_result_err(sess, ret_key, err_key, fail_val, span)?;
    copy_to_out(sess, ret_key, out, err_result, span)?;
    sess.2.build_unconditional_branch(after).map_err(builder_fail)?;
    sess.2.position_at_end(ok_block);
    let block_key = result_arg_key(sess, ret_key, 0);
    let block_val = declare_local(sess, block_key, "block", span)?;
    // `Block` uses only the data and length fields of the shared handle
    // layout; the rest is zeroed so moving the block by value never reads
    // uninitialized stack.
    init_native_handle(sess, block_key, block_val, span)?;
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
    // `munmap` needs the mapping's length as well as its address, which is
    // exactly the field `allocate` recorded in the handle. This is the
    // second reason the handle must be fully initialized (Milestone 3): a
    // garbage length here would unmap memory the program still owns.
    let bl = struct_gep(sess, block_key, p0, 1, "", span)?;
    let len = load_i64(sess, bl)?;
    let addr = ptr_word(sess, data, span)?;
    emit_syscall(sess, syscall::Sys::Munmap, &[addr, len], span)?;
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
    let oob_tag = variant_tag_of(sess, err_key, sess.12.oob, span)?;
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
    let target = byte_offset(sess, data, offset)?;
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
    let oob_tag = variant_tag_of(sess, err_key, sess.12.oob, span)?;
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
    let target = byte_offset(sess, data, offset)?;
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

// The LLVM struct a native handle key lowers to.  Every native handle
// shares one layout (`native_llvm`), so this is the single place the
// emitter recovers it as a struct rather than assuming a field count.
fn handle_struct_of<'ctx>(sess: &mut Session<'ctx, '_, '_>, key: i64, span: (i64, i64, i64)) -> Result<inkwell::types::StructType<'ctx>, CodegenError> {
    let ty = llvm_of(sess, key, span)?;
    match ty {
        BasicTypeEnum::StructType(st) => Ok(st),
        BasicTypeEnum::ArrayType(other) => Err(builder_error(span.0, span.1, span.2, &format!("native handle key {} lowered to non-struct type {:?}", key, other))),
        BasicTypeEnum::FloatType(other) => Err(builder_error(span.0, span.1, span.2, &format!("native handle key {} lowered to non-struct type {:?}", key, other))),
        BasicTypeEnum::IntType(other) => Err(builder_error(span.0, span.1, span.2, &format!("native handle key {} lowered to non-struct type {:?}", key, other))),
        BasicTypeEnum::PointerType(other) => Err(builder_error(span.0, span.1, span.2, &format!("native handle key {} lowered to non-struct type {:?}", key, other))),
        BasicTypeEnum::VectorType(other) => Err(builder_error(span.0, span.1, span.2, &format!("native handle key {} lowered to non-struct type {:?}", key, other))),
        BasicTypeEnum::ScalableVectorType(other) => Err(builder_error(span.0, span.1, span.2, &format!("native handle key {} lowered to non-struct type {:?}", key, other))),
    }
}
// Zero-fills a native handle across its whole lowered layout before any
// field is stored into it.
//
// A native handle is moved, passed, and returned *by value*: `deallocate`
// receives a `Block` as `{ ptr, i64, i64 }`, so the caller loads every byte
// of the layout whether or not that particular native surface uses every
// field.  A constructor that writes only the fields its own surface cares
// about therefore leaves the rest as whatever was on the stack, and that
// garbage is read at the first move.  Zeroing from the layout itself — one
// aggregate store of `StructType::const_zero()`, not a hand-counted run of
// per-field stores — makes it impossible for a constructor to miss a field
// when the handle layout grows.
fn init_native_handle<'ctx>(sess: &mut Session<'ctx, '_, '_>, key: i64, ptr: PointerValue<'ctx>, span: (i64, i64, i64)) -> Result<(), CodegenError> {
    let zero = handle_struct_of(sess, key, span)?.const_zero();
    store_key(sess, ptr, zero.into())?;
    Ok(())
}

fn native_vec_new<'ctx>(sess: &mut Session<'ctx, '_, '_>, ret_key: i64, out: PointerValue<'ctx>, span: (i64, i64, i64)) -> Result<(), CodegenError> {
    let vec_key = result_arg_key(sess, ret_key, 0);
    let vec_val = declare_local(sess, vec_key, "vec", span)?;
    init_native_handle(sess, vec_key, vec_val, span)?;
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
    let target = offset_elem_ptr(sess, t_key, data2, len2, span)?;
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
    let alloc_fail_tag = variant_tag_of(sess, err_key, sess.12.alloc_failed, span)?;
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

fn native_vec_view<'ctx>(sess: &mut Session<'ctx, '_, '_>, locals: &Locals<'ctx>, out: PointerValue<'ctx>, span: (i64, i64, i64)) -> Result<(), CodegenError> {
    let p0 = get_local(locals, 0, span)?;
    let vec_key = deref_key_of(sess, get_local_key(locals, 0, span)?);
    let vec_ref = load_ptr(sess, p0)?;
    let d = struct_gep(sess, vec_key, vec_ref, 0, "", span)?;
    let data = load_ptr(sess, d)?;
    let l = struct_gep(sess, vec_key, vec_ref, 1, "", span)?;
    let len = load_i64(sess, l)?;
    let od = slice_gep(sess, out, 0, "")?;
    store_key(sess, od, data.into())?;
    let ol = slice_gep(sess, out, 1, "")?;
    store_key(sess, ol, len.into())?;
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
    let oob_tag = variant_tag_of(sess, err_key, sess.12.index_oob, span)?;
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
    let elem_ptr = offset_elem_ptr(sess, t_key, data, pop_idx, span)?;
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

fn native_print<'ctx>(
    sess: &mut Session<'ctx, '_, '_>,
    locals: &Locals<'ctx>,
    ret_key: i64,
    out: PointerValue<'ctx>,
    stderr: bool,
    newline: bool,
    span: (i64, i64, i64),
) -> Result<(), CodegenError> {
    // `write` directly, not through libc's wrapper: Milestone 4 puts the
    // kernel entry point in the emitted IR for the Terminal surface, so
    // nothing sits between the Cinnabar declaration and the system call.
    let (data, len) = byte_view_of(sess, locals, span)?;
    let fd = sess.0.i64_type().const_int(if stderr { 2 } else { 1 }, false);
    write_all(sess, fd, data, len, span)?;
    if newline {
        let nl_slot = alloca_raw(sess, sess.0.i8_type().into(), "nl", span)?;
        let nl = sess.0.i8_type().const_int(10, false);
        sess.2.build_store(nl_slot, nl).map_err(builder_fail)?;
        let one = sess.0.i64_type().const_int(1, false);
        write_all(sess, fd, nl_slot, one, span)?;
    }
    build_unit_value_into(sess, ret_key, out, span)?;
    Ok(())
}

// Writes exactly `len` bytes, looping until they are all gone.
//
// A single `write` is allowed to transfer fewer bytes than asked — that is
// the documented contract, not an error condition — so issuing one and
// walking away silently truncates output on a pipe or a slow terminal. The
// libc wrapper does not loop either; the loop has to be here.
//
// The loop also ends on a non-positive result, which covers both an error
// (negative errno) and a zero-byte write. `Terminal.print` returns `Unit`
// and so has nowhere to report a failure: this is the surface the reference
// specification declares, and giving it an error channel would change the
// language surface rather than implement it. Stopping rather than spinning
// is the honest behaviour available here — an infinite loop on a closed
// stdout would be worse than a short write.
fn write_all<'ctx>(
    sess: &mut Session<'ctx, '_, '_>,
    fd: IntValue<'ctx>,
    data: PointerValue<'ctx>,
    len: IntValue<'ctx>,
    span: (i64, i64, i64),
) -> Result<(), CodegenError> {
    let function = match sess.2.get_insert_block().and_then(|block| block.get_parent()) {
        Some(found) => found,
        None => return Err(builder_error(span.0, span.1, span.2, "internal: write outside a function body")),
    };
    let i64_ty = sess.0.i64_type();
    let done_slot = alloca_raw(sess, i64_ty.into(), "written", span)?;
    store_key(sess, done_slot, i64_ty.const_zero().into())?;
    let cond = new_block(sess, function, "write_cond");
    let body = new_block(sess, function, "write_body");
    let after = new_block(sess, function, "write_done");
    sess.2.build_unconditional_branch(cond).map_err(builder_fail)?;
    sess.2.position_at_end(cond);
    let done = load_i64(sess, done_slot)?;
    let more = sess.2.build_int_compare(IntPredicate::ULT, done, len, "").map_err(builder_fail)?;
    sess.2.build_conditional_branch(more, body, after).map_err(builder_fail)?;
    sess.2.position_at_end(body);
    let cursor = byte_offset(sess, data, done)?;
    let remaining = sess.2.build_int_sub(len, done, "").map_err(builder_fail)?;
    let cursor_word = ptr_word(sess, cursor, span)?;
    let wrote = emit_syscall(sess, syscall::Sys::Write, &[fd, cursor_word, remaining], span)?;
    let progressed = sess.2.build_int_compare(IntPredicate::SGT, wrote, i64_ty.const_zero(), "").map_err(builder_fail)?;
    let advance = new_block(sess, function, "write_advance");
    sess.2.build_conditional_branch(progressed, advance, after).map_err(builder_fail)?;
    sess.2.position_at_end(advance);
    let next = sess.2.build_int_add(done, wrote, "").map_err(builder_fail)?;
    store_key(sess, done_slot, next.into())?;
    sess.2.build_unconditional_branch(cond).map_err(builder_fail)?;
    sess.2.position_at_end(after);
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

fn native_hash_map_new<'ctx>(sess: &mut Session<'ctx, '_, '_>, ret_key: i64, out: PointerValue<'ctx>, span: (i64, i64, i64)) -> Result<(), CodegenError> {
    let map_key = result_arg_key(sess, ret_key, 0);
    let map_val = declare_local(sess, map_key, "map", span)?;
    init_native_handle(sess, map_key, map_val, span)?;
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
    let ksize = sess.3.get_abi_size(&llvm_of(sess, k_key, span)?);
    let vsize = sess.3.get_abi_size(&llvm_of(sess, v_key, span)?);
    let stride_const = sess.0.i64_type().const_int(ksize + vsize, false);
    let ksize_const = sess.0.i64_type().const_int(ksize, false);
    let vsize_const = sess.0.i64_type().const_int(vsize, false);
    let map_key = deref_key_of(sess, get_local_key(locals, 0, span)?);
    let map_ref = load_ptr(sess, p0)?;
    let dptr = struct_gep(sess, map_key, map_ref, 0, "", span)?;
    let lptr = struct_gep(sess, map_key, map_ref, 1, "", span)?;
    let cptr = struct_gep(sess, map_key, map_ref, 2, "", span)?;
    let data = load_ptr(sess, dptr)?;
    let len = load_i64(sess, lptr)?;
    let key_base = sess.2.build_pointer_cast(p1, ptr_ty(sess), "").map_err(builder_fail)?;
    let i_slot = alloca_raw(sess, sess.0.i64_type().into(), "i", span)?;
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
            return Err(builder_error(span.0, span.1, span.2, &format!("internal: memcmp returned void ({:?})", inst.get_opcode())));
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
    copy_value(sess, v_key, valueptr, p2, span)?;
    let unit_key = result_arg_key(sess, ret_key, 0);
    let unit_val = build_unit_value(sess, unit_key, span)?;
    let ok_result = build_result_ok(sess, ret_key, unit_key, unit_val, span)?;
    copy_to_out(sess, ret_key, out, ok_result, span)?;
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
            return Err(builder_error(span.0, span.1, span.2, &format!("internal: realloc returned void ({:?})", inst.get_opcode())));
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
    // Zero-fill the new key and value slots before copying: struct and
    // enum keys lowered with ABI layout carry padding bytes, and the scan
    // compares whole keys with memcmp over ksize bytes.  Deterministic
    // padding keeps that comparison well-defined (interim measure; the
    // long-term fix is structural key equality).
    let zero8 = sess.0.i8_type().const_zero();
    sess.2.build_memset(keyptr2, 1, zero8, ksize_const).map_err(builder_fail)?;
    copy_value(sess, k_key, keyptr2, p1, span)?;
    let voff2 = sess.2.build_int_add(entry_off, ksize_const, "").map_err(builder_fail)?;
    let valueptr2 = byte_offset(sess, data2, voff2)?;
    sess.2.build_memset(valueptr2, 1, zero8, vsize_const).map_err(builder_fail)?;
    copy_value(sess, v_key, valueptr2, p2, span)?;
    let len3 = sess.2.build_int_add(len2, one, "").map_err(builder_fail)?;
    store_key(sess, lptr, len3.into())?;
    let unit_key2 = result_arg_key(sess, ret_key, 0);
    let unit_val2 = build_unit_value(sess, unit_key2, span)?;
    let ok_result2 = build_result_ok(sess, ret_key, unit_key2, unit_val2, span)?;
    copy_to_out(sess, ret_key, out, ok_result2, span)?;
    sess.2.build_unconditional_branch(after).map_err(builder_fail)?;
    sess.2.position_at_end(fail_block);
    let err_key = result_arg_key(sess, ret_key, 1);
    let alloc_fail_tag = variant_tag_of(sess, err_key, sess.12.alloc_failed, span)?;
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
    let ksize = sess.3.get_abi_size(&llvm_of(sess, k_key, span)?);
    let vsize = sess.3.get_abi_size(&llvm_of(sess, v_key, span)?);
    let stride_const = sess.0.i64_type().const_int(ksize + vsize, false);
    let ksize_const = sess.0.i64_type().const_int(ksize, false);
    let map_key = deref_key_of(sess, get_local_key(locals, 0, span)?);
    let map_ref = load_ptr(sess, p0)?;
    let dptr = struct_gep(sess, map_key, map_ref, 0, "", span)?;
    let data = load_ptr(sess, dptr)?;
    let lptr = struct_gep(sess, map_key, map_ref, 1, "", span)?;
    let len = load_i64(sess, lptr)?;
    let key_base = sess.2.build_pointer_cast(p1, ptr_ty(sess), "").map_err(builder_fail)?;
    let i_slot = alloca_raw(sess, sess.0.i64_type().into(), "i", span)?;
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
            return Err(builder_error(span.0, span.1, span.2, &format!("internal: memcmp returned void ({:?})", inst.get_opcode())));
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
    let v_val = declare_local(sess, v_key, "got", span)?;
    copy_value(sess, v_key, v_val, valueptr, span)?;
    let ok_result = build_result_ok(sess, ret_key, v_key, v_val, span)?;
    copy_to_out(sess, ret_key, out, ok_result, span)?;
    sess.2.build_unconditional_branch(after).map_err(builder_fail)?;
    sess.2.position_at_end(missing_block);
    let err_key = result_arg_key(sess, ret_key, 1);
    let key_missing_tag = variant_tag_of(sess, err_key, sess.12.key_not_found, span)?;
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
    let ksize = sess.3.get_abi_size(&llvm_of(sess, k_key, span)?);
    let vsize = sess.3.get_abi_size(&llvm_of(sess, v_key, span)?);
    let stride_const = sess.0.i64_type().const_int(ksize + vsize, false);
    let ksize_const = sess.0.i64_type().const_int(ksize, false);
    let map_key = deref_key_of(sess, get_local_key(locals, 0, span)?);
    let map_ref = load_ptr(sess, p0)?;
    let dptr = struct_gep(sess, map_key, map_ref, 0, "", span)?;
    let data = load_ptr(sess, dptr)?;
    let lptr = struct_gep(sess, map_key, map_ref, 1, "", span)?;
    let len = load_i64(sess, lptr)?;
    let key_base = sess.2.build_pointer_cast(p1, ptr_ty(sess), "").map_err(builder_fail)?;
    let i_slot = alloca_raw(sess, sess.0.i64_type().into(), "i", span)?;
    store_key(sess, i_slot, sess.0.i64_type().const_zero().into())?;
    let scan_cond = new_block(sess, f, "rm_cond");
    let scan_body = new_block(sess, f, "rm_body");
    let found_block = new_block(sess, f, "rm_found");
    let missing_block = new_block(sess, f, "rm_missing");
    let after = new_block(sess, f, "rm_after");
    sess.2.build_unconditional_branch(scan_cond).map_err(builder_fail)?;
    sess.2.position_at_end(scan_cond);
    let i = load_i64(sess, i_slot)?;
    let done = sess.2.build_int_compare(IntPredicate::ULT, i, len, "").map_err(builder_fail)?;
    let next_i = new_block(sess, f, "rm_next");
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
            return Err(builder_error(span.0, span.1, span.2, &format!("internal: memcmp returned void ({:?})", inst.get_opcode())));
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
    let v_val = declare_local(sess, v_key, "removed", span)?;
    copy_value(sess, v_key, v_val, valueptr, span)?;
    let i_found = load_i64(sess, i_slot)?;
    let one_more = sess.2.build_int_add(i_found, one, "").map_err(builder_fail)?;
    let remain = sess.2.build_int_sub(len, one_more, "").map_err(builder_fail)?;
    let shift_bytes = sess.2.build_int_mul(remain, stride_const, "").map_err(builder_fail)?;
    let dst_off = sess.2.build_int_mul(i_found, stride_const, "").map_err(builder_fail)?;
    let src_off = sess.2.build_int_mul(one_more, stride_const, "").map_err(builder_fail)?;
    let dst = byte_offset(sess, data, dst_off)?;
    let src = byte_offset(sess, data, src_off)?;
    sess.2.build_memmove(dst, 1, src, 1, shift_bytes).map_err(builder_fail)?;
    let len2 = sess.2.build_int_sub(len, one, "").map_err(builder_fail)?;
    store_key(sess, lptr, len2.into())?;
    let ok_result = build_result_ok(sess, ret_key, v_key, v_val, span)?;
    copy_to_out(sess, ret_key, out, ok_result, span)?;
    sess.2.build_unconditional_branch(after).map_err(builder_fail)?;
    sess.2.position_at_end(missing_block);
    let err_key = result_arg_key(sess, ret_key, 1);
    let key_missing_tag = variant_tag_of(sess, err_key, sess.12.key_not_found, span)?;
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

// The largest path `File.open` accepts, matching Linux's own `PATH_MAX`.
//
// A path arrives as a `&[U8]` carrying its own length, but `openat` wants a
// NUL-terminated string, so the path is copied into a stack buffer with a
// terminator appended. Bounding it at the kernel's own limit means the copy
// needs no allocation and a longer path is rejected with the same error the
// kernel would have produced.
const PATH_MAX: u64 = 4096;

/// `ENAMETOOLONG`, reported for a path at or over `PATH_MAX`.
const ENAMETOOLONG: u64 = 36;

// A Linux system call reports failure as a negative errno in its result
// register.  This turns that into the positive code a `SystemFault` payload
// carries, so a Cinnabar program sees the same number `errno` would hold.
fn errno_of<'ctx>(sess: &mut Session<'ctx, '_, '_>, raw: IntValue<'ctx>, span: (i64, i64, i64)) -> Result<IntValue<'ctx>, CodegenError> {
    sess.2
        .build_int_neg(raw, "")
        .map_err(|err| builder_error(span.0, span.1, span.2, &format!("internal: cannot read an error code out of a system-call result: {}", err)))
}

// Builds `Err(SystemFault(code))` into `out`.
//
// Shared by every syscall-backed surface, so the mapping from a kernel
// error to a Cinnabar value is stated once. `Net` reaches the same variant
// through `__errno_location`, because its libc wrappers report that way;
// the syscall path has the code in hand already.
fn system_fault_result<'ctx>(sess: &mut Session<'ctx, '_, '_>, ret_key: i64, out: PointerValue<'ctx>, code: IntValue<'ctx>, span: (i64, i64, i64)) -> Result<(), CodegenError> {
    let err_key = result_arg_key(sess, ret_key, 1);
    let tag = variant_tag_of(sess, err_key, sess.12.system_fault, span)?;
    let f0 = variant_payload_key(sess, err_key, tag, 0, span)?;
    let slot = declare_local(sess, f0, "errno", span)?;
    let widened = word_of(sess, code, span)?;
    store_key(sess, slot, widened.into())?;
    let fail_val = build_enum_value(sess, err_key, tag, &[(f0, slot)], span)?;
    let err_result = build_result_err(sess, ret_key, err_key, fail_val, span)?;
    copy_to_out(sess, ret_key, out, err_result, span)
}

// Splits control flow on a raw syscall result: negative is a failure that
// writes `Err(SystemFault(errno))` into `out` and jumps to the join block,
// non-negative continues in the block this returns positioned at.
//
// The caller resumes emitting the success path immediately and branches to
// the returned join block when done.
fn syscall_result_branch<'ctx>(
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
    let code = errno_of(sess, raw, span)?;
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
// `variant_tag_of`, keyed by variant name, so the mapping does not depend
// on how the enum happens to be written.
fn open_flags_of<'ctx>(
    sess: &mut Session<'ctx, '_, '_>,
    mode_key: i64,
    mode_ptr: PointerValue<'ctx>,
    span: (i64, i64, i64),
) -> Result<IntValue<'ctx>, CodegenError> {
    let i64_ty = sess.0.i64_type();
    let tag_ptr = struct_gep(sess, mode_key, mode_ptr, 0, "", span)?;
    let tag = load_i64(sess, tag_ptr)?;
    let read_only = variant_tag_of(sess, mode_key, sess.12.read_only, span)?;
    let truncate = variant_tag_of(sess, mode_key, sess.12.write_truncate, span)?;
    let read_flags = i64_ty.const_int(syscall::O_RDONLY as u64, false);
    let truncate_flags = i64_ty.const_int((syscall::O_WRONLY | syscall::O_CREAT | syscall::O_TRUNC) as u64, false);
    let append_flags = i64_ty.const_int((syscall::O_WRONLY | syscall::O_CREAT | syscall::O_APPEND) as u64, false);
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
    let terminator = byte_offset(sess, buffer, clamped)?;
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
    let name_too_long = sess.0.i64_type().const_int(ENAMETOOLONG, false);
    system_fault_result(sess, ret_key, out, name_too_long, span)?;
    sess.2.build_unconditional_branch(after).map_err(builder_fail)?;
    sess.2.position_at_end(attempt);
    let flags = open_flags_of(sess, mode_key, p1, span)?;
    let dir = sess.0.i64_type().const_int(syscall::AT_FDCWD as u64, false);
    let mode_bits = sess.0.i64_type().const_int(syscall::CREATE_MODE as u64, false);
    let path_word = ptr_word(sess, path, span)?;
    // `openat` rather than `open`: AArch64's Linux ABI has no `open`, so
    // this is one code path on both architectures instead of two.
    let raw = emit_syscall(sess, syscall::Sys::OpenAt, &[dir, path_word, flags, mode_bits], span)?;
    let join = syscall_result_branch(sess, f, ret_key, out, raw, span)?;
    let handle_key = result_arg_key(sess, ret_key, 0);
    let handle = declare_local(sess, handle_key, "file", span)?;
    init_native_handle(sess, handle_key, handle, span)?;
    let fd_slot = struct_gep(sess, handle_key, handle, 1, "", span)?;
    store_key(sess, fd_slot, raw.into())?;
    let ok_result = build_result_ok(sess, ret_key, handle_key, handle, span)?;
    copy_to_out(sess, ret_key, out, ok_result, span)?;
    sess.2.build_unconditional_branch(join).map_err(builder_fail)?;
    // Both syscall outcomes meet at `join`; that joins in turn with the
    // path-too-long branch, which never reached the system call.
    sess.2.position_at_end(join);
    sess.2.build_unconditional_branch(after).map_err(builder_fail)?;
    sess.2.position_at_end(after);
    Ok(out)
}

// `read` and `write` differ only in which system call they issue and which
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
    call: syscall::Sys,
    ret_key: i64,
    out: PointerValue<'ctx>,
    span: (i64, i64, i64),
) -> Result<PointerValue<'ctx>, CodegenError> {
    let p0 = get_local(locals, 0, span)?;
    let p1 = get_local(locals, 1, span)?;
    let handle_key = deref_key_of(sess, get_local_key(locals, 0, span)?);
    let handle = load_ptr(sess, p0)?;
    let fd = net_fd_of_handle(sess, handle_key, handle, span)?;
    let data = slice_data(sess, p1)?;
    let len = slice_len_of(sess, p1)?;
    let buffer = ptr_word(sess, data, span)?;
    let raw = emit_syscall(sess, call, &[fd, buffer, len], span)?;
    let join = syscall_result_branch(sess, f, ret_key, out, raw, span)?;
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
    let handle_key = get_local_key(locals, 0, span)?;
    let fd = net_fd_of_handle(sess, handle_key, p0, span)?;
    emit_syscall(sess, syscall::Sys::Close, &[fd], span)?;
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
    let widened = word_of(sess, argc, span)?;
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
// the argument surface, like the rest of Milestone 4, does not route
// through libc.
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
    let byte_ptr = byte_offset(sess, text, idx)?;
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
    let slot_ptr = unsafe { sess.2.build_gep(ptr_ty(sess), argv, &[i], "") }.map_err(builder_fail)?;
    let text = load_ptr(sess, slot_ptr)?;
    let length = emit_strlen(sess, f, text, span)?;
    let entry = offset_elem_ptr(sess, elem_key, table, i, span)?;
    init_native_handle(sess, elem_key, entry, span)?;
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
    emit_alloc_failed(sess, ret_key, err_key, start_capacity, out, span)?;
    sess.2.build_unconditional_branch(after).map_err(builder_fail)?;

    sess.2.position_at_end(scan);
    let cond = new_block(sess, f, "line_cond");
    let finish = new_block(sess, f, "line_finish");
    sess.2.build_unconditional_branch(cond).map_err(builder_fail)?;
    sess.2.position_at_end(cond);
    let byte_word = ptr_word(sess, byte_slot, span)?;
    let got = emit_syscall(sess, syscall::Sys::Read, &[i64_ty.const_zero(), byte_word, i64_ty.const_int(1, false)], span)?;
    // A non-positive result ends the line: zero is end of input, negative
    // is a read error. Both stop here, and `finish` decides between
    // returning what was read and reporting end of input.
    let progressed = sess.2.build_int_compare(IntPredicate::SGT, got, i64_ty.const_zero(), "").map_err(builder_fail)?;
    let keep = new_block(sess, f, "line_keep");
    sess.2.build_conditional_branch(progressed, keep, finish).map_err(builder_fail)?;
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
    emit_alloc_failed(sess, ret_key, err_key, doubled, out, span)?;
    sess.2.build_unconditional_branch(after).map_err(builder_fail)?;

    sess.2.position_at_end(grow_ok);
    store_key(sess, buffer, grown.into())?;
    store_key(sess, capacity, doubled.into())?;
    sess.2.build_unconditional_branch(place).map_err(builder_fail)?;
    sess.2.position_at_end(place);
    let target_base = load_ptr(sess, buffer)?;
    let at = load_i64(sess, length)?;
    let target = byte_offset(sess, target_base, at)?;
    sess.2.build_store(target, byte).map_err(builder_fail)?;
    let advanced = sess.2.build_int_add(at, i64_ty.const_int(1, false), "").map_err(builder_fail)?;
    store_key(sess, length, advanced.into())?;
    sess.2.build_unconditional_branch(cond).map_err(builder_fail)?;

    sess.2.position_at_end(finish);
    let final_len = load_i64(sess, length)?;
    let final_buf = load_ptr(sess, buffer)?;
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
    let end_tag = variant_tag_of(sess, err_key, sess.12.end_of_input, span)?;
    let end_val = build_enum_value(sess, err_key, end_tag, &[], span)?;
    let end_result = build_result_err(sess, ret_key, err_key, end_val, span)?;
    copy_to_out(sess, ret_key, out, end_result, span)?;
    sess.2.build_unconditional_branch(after).map_err(builder_fail)?;

    sess.2.position_at_end(line_block);
    let line = declare_local(sess, str_key, "line", span)?;
    init_native_handle(sess, str_key, line, span)?;
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

// Writes `Err(AllocFailed { need })` into the caller's return slot.
//
// `read_line` has two allocation sites — the initial buffer and every
// doubling — and both report failure the same way. Building that error in
// one place keeps the variant and its payload derived from the declared
// surface once: two copies could drift into naming different variants, or
// into one site carrying the byte count it asked for while the other carried
// the count it already had.
fn emit_alloc_failed<'ctx>(
    sess: &mut Session<'ctx, '_, '_>,
    ret_key: i64,
    err_key: i64,
    need: IntValue<'ctx>,
    out: PointerValue<'ctx>,
    span: (i64, i64, i64),
) -> Result<(), CodegenError> {
    let tag = variant_tag_of(sess, err_key, sess.12.alloc_failed, span)?;
    let payload_key = variant_payload_key(sess, err_key, tag, 0, span)?;
    let slot = declare_local(sess, payload_key, "need", span)?;
    store_key(sess, slot, need.into())?;
    let value = build_enum_value(sess, err_key, tag, &[(payload_key, slot)], span)?;
    let result = build_result_err(sess, ret_key, err_key, value, span)?;
    copy_to_out(sess, ret_key, out, result, span)
}

fn net_fd_of_handle<'ctx>(sess: &mut Session<'ctx, '_, '_>, sock_key: i64, handle: PointerValue<'ctx>, span: (i64, i64, i64)) -> Result<IntValue<'ctx>, CodegenError> {
    let fd_slot = struct_gep(sess, sock_key, handle, 1, "", span)?;
    load_i64(sess, fd_slot)
}

fn net_errno<'ctx>(sess: &mut Session<'ctx, '_, '_>, span: (i64, i64, i64)) -> Result<IntValue<'ctx>, CodegenError> {
    let loc_fn = extern_fn(sess, "__errno_location", ptr_ty(sess).fn_type(&[], false));
    let call = sess.2.build_call(loc_fn, &[], "").map_err(builder_fail)?;
    let loc = match call.try_as_basic_value() {
        ValueKind::Basic(bv) => bv.into_pointer_value(),
        ValueKind::Instruction(inst) => {
            return Err(builder_error(span.0, span.1, span.2, &format!("internal: __errno_location returned void ({:?})", inst.get_opcode())));
        }
    };
    let err32 = sess.2.build_load(sess.0.i32_type(), loc, "").map_err(builder_fail)?.into_int_value();
    sess.2.build_int_s_extend(err32, sess.0.i64_type(), "").map_err(builder_fail)
}

fn net_fault_result<'ctx>(sess: &mut Session<'ctx, '_, '_>, ret_key: i64, out: PointerValue<'ctx>, span: (i64, i64, i64)) -> Result<(), CodegenError> {
    let err_key = result_arg_key(sess, ret_key, 1);
    let tag = variant_tag_of(sess, err_key, sess.12.system_fault, span)?;
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
    let sa_ty = sess.0.i8_type().array_type(16);
    let sa = alloca_raw(sess, sa_ty.into(), "sa", span)?;
    let zero = sess.0.i64_type().const_zero();
    let fam_off = byte_offset(sess, sa, zero)?;
    sess.2.build_store(fam_off, sess.0.i16_type().const_int(2, false)).map_err(builder_fail)?;
    let two = sess.0.i64_type().const_int(2, false);
    let port_off = byte_offset(sess, sa, two)?;
    let port16 = sess.2.build_int_truncate(port, sess.0.i16_type(), "").map_err(builder_fail)?;
    let eight = sess.0.i16_type().const_int(8, false);
    let hi = sess.2.build_right_shift(port16, eight, false, "").map_err(builder_fail)?;
    let lo = sess.2.build_left_shift(port16, eight, "").map_err(builder_fail)?;
    let swapped = sess.2.build_or(lo, hi, "").map_err(builder_fail)?;
    sess.2.build_store(port_off, swapped).map_err(builder_fail)?;
    let four = sess.0.i64_type().const_int(4, false);
    let addr_off = byte_offset(sess, sa, four)?;
    sess.2.build_store(addr_off, sess.0.i32_type().const_zero()).map_err(builder_fail)?;
    let eight64 = sess.0.i64_type().const_int(8, false);
    let pad_off = byte_offset(sess, sa, eight64)?;
    sess.2.build_store(pad_off, zero).map_err(builder_fail)?;
    Ok(sa)
}

fn build_net_sock_ok<'ctx>(sess: &mut Session<'ctx, '_, '_>, ret_key: i64, out: PointerValue<'ctx>, fd: IntValue<'ctx>, span: (i64, i64, i64)) -> Result<(), CodegenError> {
    let sock_key = result_arg_key(sess, ret_key, 0);
    let sock_val = declare_local(sess, sock_key, "sock", span)?;
    let d = struct_gep(sess, sock_key, sock_val, 0, "", span)?;
    store_key(sess, d, ptr_ty(sess).const_null().into())?;
    let l = struct_gep(sess, sock_key, sock_val, 1, "", span)?;
    let fd64 = sess.2.build_int_s_extend(fd, sess.0.i64_type(), "").map_err(builder_fail)?;
    store_key(sess, l, fd64.into())?;
    let c = struct_gep(sess, sock_key, sock_val, 2, "", span)?;
    store_key(sess, c, sess.0.i64_type().const_zero().into())?;
    let ok_result = build_result_ok(sess, ret_key, sock_key, sock_val, span)?;
    copy_to_out(sess, ret_key, out, ok_result, span)
}

fn native_net_socket<'ctx>(sess: &mut Session<'ctx, '_, '_>, f: FunctionValue<'ctx>, ret_key: i64, out: PointerValue<'ctx>, span: (i64, i64, i64)) -> Result<PointerValue<'ctx>, CodegenError> {
    let domain = sess.0.i32_type().const_int(2, false);
    let stype = sess.0.i32_type().const_int(1, false);
    let proto = sess.0.i32_type().const_zero();
    let call = sess.2.build_call(extern_socket(sess), &[into_meta(domain.into()), into_meta(stype.into()), into_meta(proto.into())], "").map_err(builder_fail)?;
    let rc = match call.try_as_basic_value() {
        ValueKind::Basic(bv) => bv.into_int_value(),
        ValueKind::Instruction(inst) => {
            return Err(builder_error(span.0, span.1, span.2, &format!("internal: socket returned void ({:?})", inst.get_opcode())));
        }
    };
    let after = net_rc_branch(sess, f, ret_key, out, rc, span)?;
    build_net_sock_ok(sess, ret_key, out, rc, span)?;
    sess.2.build_unconditional_branch(after).map_err(builder_fail)?;
    sess.2.position_at_end(after);
    Ok(out)
}

fn native_net_bind<'ctx>(sess: &mut Session<'ctx, '_, '_>, f: FunctionValue<'ctx>, locals: &Locals<'ctx>, ret_key: i64, out: PointerValue<'ctx>, span: (i64, i64, i64)) -> Result<(), CodegenError> {
    let p0 = get_local(locals, 0, span)?;
    let p1 = get_local(locals, 1, span)?;
    let sock_key = deref_key_of(sess, get_local_key(locals, 0, span)?);
    let handle = load_ptr(sess, p0)?;
    let fd = net_fd_of_handle(sess, sock_key, handle, span)?;
    let fd32 = sess.2.build_int_truncate(fd, sess.0.i32_type(), "").map_err(builder_fail)?;
    let port = load_i64(sess, p1)?;
    let sa = build_sockaddr_in(sess, port, span)?;
    let addr_len = sess.0.i32_type().const_int(16, false);
    let call = sess.2.build_call(extern_bind(sess), &[into_meta(fd32.into()), into_meta(sa.into()), into_meta(addr_len.into())], "").map_err(builder_fail)?;
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
    let sock_key = deref_key_of(sess, get_local_key(locals, 0, span)?);
    let handle = load_ptr(sess, p0)?;
    let fd = net_fd_of_handle(sess, sock_key, handle, span)?;
    let fd32 = sess.2.build_int_truncate(fd, sess.0.i32_type(), "").map_err(builder_fail)?;
    let backlog = load_i64(sess, p1)?;
    let backlog32 = sess.2.build_int_truncate(backlog, sess.0.i32_type(), "").map_err(builder_fail)?;
    let call = sess.2.build_call(extern_listen(sess), &[into_meta(fd32.into()), into_meta(backlog32.into())], "").map_err(builder_fail)?;
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
    let sock_key = deref_key_of(sess, get_local_key(locals, 0, span)?);
    let handle = load_ptr(sess, p0)?;
    let fd = net_fd_of_handle(sess, sock_key, handle, span)?;
    let fd32 = sess.2.build_int_truncate(fd, sess.0.i32_type(), "").map_err(builder_fail)?;
    let null_ptr = ptr_ty(sess).const_null();
    let call = sess.2.build_call(extern_accept(sess), &[into_meta(fd32.into()), into_meta(null_ptr.into()), into_meta(null_ptr.into())], "").map_err(builder_fail)?;
    let rc = match call.try_as_basic_value() {
        ValueKind::Basic(bv) => bv.into_int_value(),
        ValueKind::Instruction(inst) => {
            return Err(builder_error(span.0, span.1, span.2, &format!("internal: accept returned void ({:?})", inst.get_opcode())));
        }
    };
    let after = net_rc_branch(sess, f, ret_key, out, rc, span)?;
    build_net_sock_ok(sess, ret_key, out, rc, span)?;
    sess.2.build_unconditional_branch(after).map_err(builder_fail)?;
    sess.2.position_at_end(after);
    Ok(out)
}

fn native_net_send<'ctx>(sess: &mut Session<'ctx, '_, '_>, f: FunctionValue<'ctx>, locals: &Locals<'ctx>, ret_key: i64, out: PointerValue<'ctx>, span: (i64, i64, i64)) -> Result<PointerValue<'ctx>, CodegenError> {
    let p0 = get_local(locals, 0, span)?;
    let p1 = get_local(locals, 1, span)?;
    let sock_key = deref_key_of(sess, get_local_key(locals, 0, span)?);
    let handle = load_ptr(sess, p0)?;
    let fd = net_fd_of_handle(sess, sock_key, handle, span)?;
    let fd32 = sess.2.build_int_truncate(fd, sess.0.i32_type(), "").map_err(builder_fail)?;
    let data = slice_data(sess, p1)?;
    let len = slice_len_of(sess, p1)?;
    let flags = sess.0.i32_type().const_zero();
    let call = sess.2.build_call(extern_send(sess), &[into_meta(fd32.into()), into_meta(data.into()), into_meta(len.into()), into_meta(flags.into())], "").map_err(builder_fail)?;
    let rc = match call.try_as_basic_value() {
        ValueKind::Basic(bv) => bv.into_int_value(),
        ValueKind::Instruction(inst) => {
            return Err(builder_error(span.0, span.1, span.2, &format!("internal: send returned void ({:?})", inst.get_opcode())));
        }
    };
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
    let sock_key = get_local_key(locals, 0, span)?;
    let fd = net_fd_of_handle(sess, sock_key, p0, span)?;
    let fd32 = sess.2.build_int_truncate(fd, sess.0.i32_type(), "").map_err(builder_fail)?;
    sess.2.build_call(extern_close(sess), &[into_meta(fd32.into())], "").map_err(builder_fail)?;
    build_unit_value_into(sess, ret_key, out, span)
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

fn native_string_from_slice<'ctx>(sess: &mut Session<'ctx, '_, '_>, f: FunctionValue<'ctx>, locals: &Locals<'ctx>, ret_key: i64, out: PointerValue<'ctx>, span: (i64, i64, i64)) -> Result<PointerValue<'ctx>, CodegenError> {
    let p0 = get_local(locals, 0, span)?;
    let data = slice_data(sess, p0)?;
    let len = slice_len_of(sess, p0)?;
    let i_slot = alloca_raw(sess, sess.0.i64_type().into(), "i", span)?;
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
    let b1p = byte_offset(sess, data, i1)?;
    let b1r = load_i8(sess, b1p)?;
    let b1 = utf8_byte_value(sess, b1r)?;
    let b2p = byte_offset(sess, data, i2)?;
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
    let b1p = byte_offset(sess, data, i1b)?;
    let b1r = load_i8(sess, b1p)?;
    let b1b = utf8_byte_value(sess, b1r)?;
    let b2p = byte_offset(sess, data, i2b)?;
    let b2r = load_i8(sess, b2p)?;
    let b2b = utf8_byte_value(sess, b2r)?;
    let b3p = byte_offset(sess, data, i3b)?;
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
    sess.2.position_at_end(invalid_block);
    let err_key = result_arg_key(sess, ret_key, 1);
    let invalid_tag = variant_tag_of(sess, err_key, sess.12.invalid_utf8, span)?;
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
    let alloc_fail_tag = variant_tag_of(sess, err_key2, sess.12.alloc_failed, span)?;
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
    // `String` uses only the data and length fields of the shared handle
    // layout; the rest is zeroed so moving the string by value never reads
    // uninitialized stack.
    init_native_handle(sess, str_key, str_val, span)?;
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
        let diag_tag = variant_tag_of_opt(sess, exit_key, sess.12.exit_diag);
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
