//! LLVM type lowering.
//!
//! Every layout is derived from the program's declarations: a struct key
//! lowers to a struct of its declared fields (with the typechecker's
//! argument substitution), an enum key lowers to a `{i64 tag, [N x i8]
//! payload}` whose payload byte count is the largest declared variant
//! payload sized through the target data layout, and native keys lower to
//! the runtime representations of the declared native surfaces.  The
//! lowering is cached per canonical key so each layout fact is computed
//! once and reused by every construction, extraction, and match.

use crate::ast::*;
use crate::codegen::error::*;
use inkwell::context::Context;
use inkwell::targets::TargetData;
use inkwell::types::{BasicType, BasicTypeEnum};

pub type KeyTypes<'ctx> = Vec<Option<BasicTypeEnum<'ctx>>>;

/// `(enum key, payload size, payload align, variant count)`.
pub type EnumInfos = Vec<(i64, i64, i64, i64)>;

/// `(enum key, variant index, payload struct type)`.
pub type PayloadStructs<'ctx> = Vec<(i64, i64, BasicTypeEnum<'ctx>)>;

/// The type-lowering environment: the LLVM context, the host target
/// data, the arenas, and the lowering caches.  Threaded as one argument,
/// the same shape as the emitter's `Session`.
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

/// The interned name of a symbol row.
fn sym_name(nodes: &[i64], sym: i64) -> i64 {
    node_b(nodes, sym)
}

/// The cached descriptor row for `key`, or an error when a key was never
/// built (an internal invariant failure: codegen only lowers keys the
/// typechecker attached).
fn row_of(nodes: &[i64], key: i64) -> Result<i64, CodegenError> {
    let row = find_tyinfo(nodes, key);
    if row == NONE {
        return Err(builder_error(-1, 0, 0, &format!("cannot lower type key {}", key)));
    }
    Ok(row)
}

/// The LLVM type of a canonical key, cached per key.
pub fn llvm_type<'ctx, 'a>(env: &mut TyEnv<'ctx, 'a>, key: i64) -> Result<BasicTypeEnum<'ctx>, CodegenError> {
    let index = key as usize;
    if let Some(Some(ty)) = env.5.get(index) {
        return Ok(*ty);
    }
    let ty = build_type(env, key)?;
    if env.5.len() <= index {
        env.5.resize(index + 1, None);
    }
    if let Some(slot) = env.5.get_mut(index) {
        *slot = Some(ty);
    }
    Ok(ty)
}

fn build_type<'ctx, 'a>(env: &mut TyEnv<'ctx, 'a>, key: i64) -> Result<BasicTypeEnum<'ctx>, CodegenError> {
    let row = row_of(env.3, key)?;
    let kind = node_b(env.3, row);
    let sym = node_c(env.3, row);
    let args = node_d(env.3, row);
    let elem = node_e(env.3, row);
    let len = node_f(env.3, row);
    if kind == TYD_BUILTIN {
        let name = name_text(env.2, sym_name(env.3, sym));
        return builtin_llvm(env.0, &name);
    }
    if kind == TYD_STRUCT {
        let item = node_c(env.3, sym);
        return struct_llvm(env, item, args);
    }
    if kind == TYD_ENUM {
        let item = node_c(env.3, sym);
        let ty = enum_llvm(env, key, item, args)?;
        return Ok(ty);
    }
    if kind == TYD_NATIVE {
        let name = name_text(env.2, sym_name(env.3, sym));
        return native_llvm(env.0, &name);
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
        let elem_ty = llvm_type(env, elem)?;
        let count = if len < 0 { 0 } else { len };
        return Ok(elem_ty.array_type(count as u32).into());
    }
    Err(builder_error(
        -1,
        0,
        0,
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

/// The scalar representation of a builtin type, keyed by its name.
fn builtin_llvm<'ctx>(context: &'ctx Context, name: &str) -> Result<BasicTypeEnum<'ctx>, CodegenError> {
    if name == "U8" {
        return Ok(context.i8_type().into());
    }
    if name == "U32" {
        return Ok(context.i32_type().into());
    }
    if name == "Bool" {
        return Ok(context.bool_type().into());
    }
    if name == "Int" || name == "Usize" {
        return Ok(context.i64_type().into());
    }
    Err(builder_error(
        -1,
        0,
        0,
        &format!("unsupported builtin type '{}'", name),
    ))
}

/// The slice view type `{data, len}` shared by `[T]` and `&[T]`.
pub fn slice_view_ty<'ctx>(context: &'ctx Context) -> BasicTypeEnum<'ctx> {
    let ptr = context.ptr_type(inkwell::AddressSpace::from(0u16));
    context.struct_type(&[ptr.into(), context.i64_type().into()], false).into()
}

/// The slice view `{data, len}` shared by `[T]` and `&[T]`.
fn slice_llvm<'ctx>(context: &'ctx Context) -> Result<BasicTypeEnum<'ctx>, CodegenError> {
    Ok(slice_view_ty(context))
}

/// The declared name of a native surface: the segment after the last
/// `.` of its qualified symbol name (`Memory.Block` -> `Block`).  The
/// runtime representations are keyed by the declared surface name, not
/// the module path.
fn native_surface_name(name: &str) -> &str {
    match name.rfind('.') {
        Some(idx) => &name[idx + 1..],
        None => name,
    }
}

/// The runtime representation of a declared native surface.
fn native_llvm<'ctx>(context: &'ctx Context, name: &str) -> Result<BasicTypeEnum<'ctx>, CodegenError> {
    let ptr = context.ptr_type(inkwell::AddressSpace::from(0u16));
    let i64_ty = context.i64_type();
    let surface = native_surface_name(name);
    if surface == "Block" {
        return Ok(context.struct_type(&[ptr.into(), i64_ty.into()], false).into());
    }
    if surface == "Vec" {
        // { data, len, cap }; the element type drives allocation size,
        // which the runtime bodies compute from the argument
        // substitution.
        return Ok(context.struct_type(&[ptr.into(), i64_ty.into(), i64_ty.into()], false).into());
    }
    if surface == "String" {
        return Ok(context.struct_type(&[ptr.into(), i64_ty.into()], false).into());
    }
    if surface == "HashMap" {
        return Ok(context.struct_type(&[ptr.into(), i64_ty.into(), i64_ty.into()], false).into());
    }
    Err(builder_error(
        -1,
        0,
        0,
        &format!("native type '{}' has no runtime representation", name),
    ))
}

/// The struct type of a declared struct, with the typechecker's argument
/// substitution applied to every field.
fn struct_llvm<'ctx, 'a>(env: &mut TyEnv<'ctx, 'a>, item: i64, args: i64) -> Result<BasicTypeEnum<'ctx>, CodegenError> {
    let from = declared_param_keys(env.3, env.4, item);
    let to = list_to_vec(env.4, args);
    let fields = node_e(env.3, item);
    let count = list_len(env.4, fields);
    let mut field_tys: Vec<BasicTypeEnum<'ctx>> = Vec::new();
    let mut idx = 0i64;
    while idx < count {
        let field = list_get(env.4, fields, idx);
        let declared = ty_key_of(env.3, node_b(env.3, field));
        let concrete = subst_key(env.3, env.4, declared, &from, &to);
        let ty = llvm_type(env, concrete)?;
        field_tys.push(ty);
        idx += 1;
    }
    Ok(env.0.struct_type(&field_tys, false).into())
}

/// The enum type `{i64 tag, [N x i8] payload}` (or `{i64 tag}` when every
/// variant is a unit variant), derived from the declared variant payloads.
fn enum_llvm<'ctx, 'a>(env: &mut TyEnv<'ctx, 'a>, key: i64, item: i64, args: i64) -> Result<BasicTypeEnum<'ctx>, CodegenError> {
    let (size, align, count) = enum_payload_bounds(env, item, args)?;
    let padded = round_up(size, align);
    let ty = if padded == 0 {
        env.0.struct_type(&[env.0.i64_type().into()], false).into()
    } else {
        let payload = env.0.i8_type().array_type(padded as u32);
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

/// The largest payload size and alignment across the declared variants,
/// and the variant count.
fn enum_payload_bounds<'ctx, 'a>(env: &mut TyEnv<'ctx, 'a>, item: i64, args: i64) -> Result<(i64, i64, i64), CodegenError> {
    let from = declared_param_keys(env.3, env.4, item);
    let to = list_to_vec(env.4, args);
    let variants = node_e(env.3, item);
    let count = list_len(env.4, variants);
    let mut max_size = 0i64;
    let mut max_align = 1i64;
    let mut idx = 0i64;
    while idx < count {
        let variant = list_get(env.4, variants, idx);
        let payload_decl = node_b(env.3, variant);
        let pcount = list_len(env.4, payload_decl);
        if pcount > 0 {
            let mut field_tys: Vec<BasicTypeEnum<'ctx>> = Vec::new();
            let mut pidx = 0i64;
            while pidx < pcount {
                let declared = ty_key_of(env.3, list_get(env.4, payload_decl, pidx));
                let concrete = subst_key(env.3, env.4, declared, &from, &to);
                let ty = llvm_type(env, concrete)?;
                field_tys.push(ty);
                pidx += 1;
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
        }
        idx += 1;
    }
    Ok((max_size, max_align, count))
}

/// The payload struct type of `(enum key, variant index)` — a struct of
/// the variant's declared payload types with the enum's argument
/// substitution applied.  Cached per pair.
pub fn payload_struct_of<'ctx, 'a>(env: &mut TyEnv<'ctx, 'a>, enum_key: i64, variant_idx: i64) -> Result<BasicTypeEnum<'ctx>, CodegenError> {
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
    let row = row_of(env.3, enum_key)?;
    let sym = node_c(env.3, row);
    let args = node_d(env.3, row);
    let item = node_c(env.3, sym);
    let from = declared_param_keys(env.3, env.4, item);
    let to = list_to_vec(env.4, args);
    let variants = node_e(env.3, item);
    let variant = list_get(env.4, variants, variant_idx);
    let payload_decl = node_b(env.3, variant);
    let pcount = list_len(env.4, payload_decl);
    let mut field_tys: Vec<BasicTypeEnum<'ctx>> = Vec::new();
    let mut pidx = 0i64;
    while pidx < pcount {
        let declared = ty_key_of(env.3, list_get(env.4, payload_decl, pidx));
        let concrete = subst_key(env.3, env.4, declared, &from, &to);
        let ty = llvm_type(env, concrete)?;
        field_tys.push(ty);
        pidx += 1;
    }
    let ty = if pcount == 0 {
        env.0.struct_type(&[], false).into()
    } else {
        env.0.struct_type(&field_tys, false).into()
    };
    env.7.push((enum_key, variant_idx, ty));
    Ok(ty)
}

/// The declared-parameter keys of a type declaration, in order.
fn declared_param_keys(nodes: &[i64], lists: &[Vec<i64>], item: i64) -> Vec<i64> {
    let params = if node_a(nodes, item) == ITEM_NATIVE_TYPE {
        node_e(nodes, item)
    } else {
        node_f(nodes, item)
    };
    let mut keys: Vec<i64> = Vec::new();
    let count = list_len(lists, params);
    let mut idx = 0i64;
    while idx < count {
        let param = list_get(lists, params, idx);
        if node_tag(nodes, param) == NODE_TY && node_a(nodes, param) == TY_PARAM {
            keys.push(ty_key_of(nodes, param));
        }
        idx += 1;
    }
    keys
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
