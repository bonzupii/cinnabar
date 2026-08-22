//! Lowering of canonical type keys to LLVM types.
//!
//! `llvm_type`/`build_type` turn the typechecker's canonical keys into
//! inkwell `BasicTypeEnum`s: builtins to LLVM integer and `i1` types,
//! structs to LLVM structs, enums to tagged unions whose payload layout
//! follows the enum's own declared variant order, and native handles to
//! their opaque pointer-shaped representation. Lowering is memoized by
//! type-key index, so a key lowers once per module.
//!
//! **Invariants:**
//! - Layout is derived from the declared type every time. There is no
//!   hand-maintained, name-keyed table of sizes or variant tags that could
//!   drift out of step with the language.
//! - A generic argument is lowered recursively: a `Vec(Int)` is not a
//!   `Vec(String)`, and the element type drives both GEP and allocation
//!   size.

use crate::ast::*;
use crate::codegen::error::*;
use inkwell::context::Context;
use inkwell::targets::TargetData;
use inkwell::types::{BasicType, BasicTypeEnum};

pub type KeyTypes<'ctx> = Vec<Option<BasicTypeEnum<'ctx>>>;

pub type EnumInfos = Vec<(i64, i64, i64, i64)>;

pub type PayloadStructs<'ctx> = Vec<(i64, i64, BasicTypeEnum<'ctx>)>;

pub type TyEnv<'ctx, 'a> = (
    &'ctx Context,
    &'a TargetData,
    &'a [String],
    &'a mut Vec<i64>,
    &'a mut Vec<Vec<i64>>,
    &'a mut KeyTypes<'ctx>,
    &'a mut EnumInfos,
    &'a mut PayloadStructs<'ctx>,
);

fn round_up(size: i64, align: i64) -> i64 {
    if align <= 1 {
        return size;
    }
    let rem = size % align;
    if rem == 0 {
        size
    } else {
        size + (align - rem)
    }
}

fn row_of(nodes: &[i64], key: i64, span: (i64, i64, i64)) -> Result<i64, CodegenError> {
    let row = find_tyinfo(nodes, key);
    if row == NONE {
        return Err(builder_error(span.0, span.1, span.2, &format!("cannot lower type key {}", key)));
    }
    Ok(row)
}

pub fn llvm_type<'ctx, 'a>(env: &mut TyEnv<'ctx, 'a>, key: i64, span: (i64, i64, i64)) -> Result<BasicTypeEnum<'ctx>, CodegenError> {
    let index = key as usize;
    if let Some(Some(ty)) = env.5.get(index) {
        return Ok(*ty);
    }
    let ty = build_type(env, key, span)?;
    if env.5.len() <= index {
        env.5.resize(index + 1, None);
    }
    if let Some(slot) = env.5.get_mut(index) {
        *slot = Some(ty);
    }
    Ok(ty)
}

fn build_type<'ctx, 'a>(env: &mut TyEnv<'ctx, 'a>, key: i64, span: (i64, i64, i64)) -> Result<BasicTypeEnum<'ctx>, CodegenError> {
    let row = row_of(env.3, key, span)?;
    let kind = node_b(env.3, row);
    let sym = node_c(env.3, row);
    let elem = node_e(env.3, row);
    let len = node_f(env.3, row);
    if kind == TYD_BUILTIN {
        return builtin_llvm(env.0, node_f(env.3, row), span);
    }
    if kind == TYD_STRUCT {
        return struct_llvm(env, key, span);
    }
    if kind == TYD_ENUM {
        let ty = enum_llvm(env, key, span)?;
        return Ok(ty);
    }
    if kind == TYD_NATIVE {
        return native_llvm(env.0, nattype_layout_of(env.3, sym), span);
    }
    if kind == TYD_REF || kind == TYD_REF_MUT {
        if key_kind_of(env.3, elem) == TYD_SLICE {
            return slice_llvm(env.0);
        }
        return Ok(env.0.ptr_type(inkwell::AddressSpace::from(0u16)).into());
    }
    if kind == TYD_SLICE {
        return slice_llvm(env.0);
    }
    if kind == TYD_ARRAY {
        let elem_ty = llvm_type(env, elem, span)?;
        let count = if len < 0 { 0 } else { len };
        return Ok(elem_ty.array_type(count as u32).into());
    }
    Err(builder_error(
        span.0,
        span.1,
        span.2,
        "attempted to lower a non-runtime type key",
    ))
}

fn key_kind_of(nodes: &[i64], key: i64) -> i64 {
    let row = find_tyinfo(nodes, key);
    if row == NONE {
        TYD_UNKNOWN
    } else {
        node_b(nodes, row)
    }
}

fn builtin_llvm<'ctx>(context: &'ctx Context, sub: i64, span: (i64, i64, i64)) -> Result<BasicTypeEnum<'ctx>, CodegenError> {
    if sub == BUILTIN_BOOL {
        return Ok(context.bool_type().into());
    }
    let width = builtin_int_width(sub);
    if width == 8 {
        return Ok(context.i8_type().into());
    }
    if width == 16 {
        return Ok(context.i16_type().into());
    }
    if width == 32 {
        return Ok(context.i32_type().into());
    }
    if width == 64 {
        return Ok(context.i64_type().into());
    }
    Err(builder_error(span.0, span.1, span.2, "unsupported builtin type"))
}

pub fn slice_view_ty<'ctx>(context: &'ctx Context) -> BasicTypeEnum<'ctx> {
    let ptr = context.ptr_type(inkwell::AddressSpace::from(0u16));
    context.struct_type(&[ptr.into(), context.i64_type().into()], false).into()
}

fn slice_llvm<'ctx>(context: &'ctx Context) -> Result<BasicTypeEnum<'ctx>, CodegenError> {
    Ok(slice_view_ty(context))
}

// A native handle lowers to its registry-declared layout kind
// (`nattype_layout_of`): scalar i64, pair { ptr, i64 }, triple { ptr, i64, i64 }.
fn native_llvm<'ctx>(context: &'ctx Context, layout: i64, span: (i64, i64, i64)) -> Result<BasicTypeEnum<'ctx>, CodegenError> {
    let ptr = context.ptr_type(inkwell::AddressSpace::from(0u16));
    let i64_ty = context.i64_type();
    if layout == NATIVE_LAYOUT_SCALAR {
        return Ok(i64_ty.into());
    }
    if layout == NATIVE_LAYOUT_PAIR {
        return Ok(context.struct_type(&[ptr.into(), i64_ty.into()], false).into());
    }
    if layout == NATIVE_LAYOUT_TRIPLE {
        return Ok(context.struct_type(&[ptr.into(), i64_ty.into(), i64_ty.into()], false).into());
    }
    Err(builder_error(span.0, span.1, span.2, "native type has no declared layout kind"))
}

// The LLVM struct of a canonical struct key, built from the typechecker's
// precomputed NODE_FIELDKEY facts, in declared order.
fn struct_llvm<'ctx, 'a>(env: &mut TyEnv<'ctx, 'a>, key: i64, span: (i64, i64, i64)) -> Result<BasicTypeEnum<'ctx>, CodegenError> {
    let rows = fieldkey_rows_of(env.3, key);
    let mut field_tys: Vec<BasicTypeEnum<'ctx>> = Vec::new();
    let mut idx = 0usize;
    while idx < rows.len() {
        let fkey = match rows.get(idx) {
            Some(row) => row.1,
            None => break,
        };
        let ty = llvm_type(env, fkey, span)?;
        field_tys.push(ty);
        idx += 1;
    }
    Ok(env.0.struct_type(&field_tys, false).into())
}

fn enum_llvm<'ctx, 'a>(env: &mut TyEnv<'ctx, 'a>, key: i64, span: (i64, i64, i64)) -> Result<BasicTypeEnum<'ctx>, CodegenError> {
    let (size, align, count) = enum_payload_bounds(env, key, span)?;
    let ty = if size == 0 {
        env.0.struct_type(&[env.0.i64_type().into()], false).into()
    } else {
        // Payload storage is `[words x i64]`; `size` is a multiple of 8, so
        // `words = size / 8` words hold every variant's payload.
        let words = (size + 7) / 8;
        let payload = env.0.i64_type().array_type(words as u32);
        env.0
            .struct_type(&[env.0.i64_type().into(), payload.into()], false)
            .into()
    };
    push_enum_info(env.6, key, size, align, count);
    Ok(ty)
}

fn push_enum_info(enum_infos: &mut EnumInfos, key: i64, size: i64, align: i64, count: i64) {
    let mut idx = 0usize;
    while idx < enum_infos.len() {
        match enum_infos.get(idx) {
            Some(info) => {
                if info.0 == key {
                    return;
                }
            }
            None => break,
        }
        idx += 1;
    }
    enum_infos.push((key, size, align, count));
}

fn enum_payload_bounds<'ctx, 'a>(env: &mut TyEnv<'ctx, 'a>, key: i64, span: (i64, i64, i64)) -> Result<(i64, i64, i64), CodegenError> {
    // Payload bounds from NODE_PAYLOADKEY rows; variant count is the
    // declared list length (payload rows cannot express unit variants).
    let row = row_of(env.3, key, span)?;
    let sym = node_c(env.3, row);
    let decl = node_c(env.3, sym);
    if decl == NONE || node_tag(env.3, decl) != NODE_ITEM || node_a(env.3, decl) != ITEM_ENUM {
        return Err(builder_error(span.0, span.1, span.2, "internal: enum key has no declaration"));
    }
    let count = list_len(env.4, node_e(env.3, decl));
    let rows = payloadkey_rows_of(env.3, key);
    let mut max_size = 0i64;
    let mut max_align = 1i64;
    let mut idx = 0usize;
    while idx < rows.len() {
        let variant = match rows.get(idx) {
            Some(row) => row.0,
            None => break,
        };
        let mut field_tys: Vec<BasicTypeEnum<'ctx>> = Vec::new();
        let mut j = idx;
        while j < rows.len() {
            let row = match rows.get(j) {
                Some(row) => row,
                None => break,
            };
            if row.0 != variant {
                break;
            }
            field_tys.push(llvm_type(env, row.1, span)?);
            j += 1;
        }
        let payload_ty = env.0.struct_type(&field_tys, false);
        let size = env.1.get_abi_size(&payload_ty) as i64;
        let align = env.1.get_abi_alignment(&payload_ty) as i64;
        if size > max_size {
            max_size = size;
        }
        if align > max_align {
            max_align = align;
        }
        idx = j;
    }
    // The payload region is sized in whole 64-bit words; the maximum
    // payload size is rounded up to an 8-byte boundary.
    Ok((round_up(max_size, 8), max_align, count))
}

pub fn payload_struct_of<'ctx, 'a>(env: &mut TyEnv<'ctx, 'a>, enum_key: i64, variant_idx: i64, span: (i64, i64, i64)) -> Result<BasicTypeEnum<'ctx>, CodegenError> {
    let mut idx = 0usize;
    while idx < env.7.len() {
        match env.7.get(idx) {
            Some(entry) => {
                if entry.0 == enum_key && entry.1 == variant_idx {
                    return Ok(entry.2);
                }
            }
            None => break,
        }
        idx += 1;
    }
    // The variant payload struct comes from NODE_PAYLOADKEY rows; a variant
    // with no rows is a unit variant and lowers to an empty struct.
    let rows = payloadkey_rows_of(env.3, enum_key);
    let mut field_tys: Vec<BasicTypeEnum<'ctx>> = Vec::new();
    let mut idx = 0usize;
    while idx < rows.len() {
        let row = match rows.get(idx) {
            Some(row) => row,
            None => break,
        };
        if row.0 == variant_idx {
            field_tys.push(llvm_type(env, row.1, span)?);
        }
        idx += 1;
    }
    let ty = if field_tys.is_empty() {
        env.0.struct_type(&[], false).into()
    } else {
        env.0.struct_type(&field_tys, false).into()
    };
    env.7.push((enum_key, variant_idx, ty));
    Ok(ty)
}

