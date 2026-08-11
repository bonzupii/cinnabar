//! Fuzzes compiler generalization end-to-end.
//!
//! The positive corpus is built correct-by-construction: a generator-side
//! scope table tracks every declared variable's type and expected value, so
//! generated programs are valid without an interpreter and the executed
//! binaries are checked against the generator's own expected final value.
//! Structure is randomized recursively (tree depth 1..4, struct fields 1..6,
//! enum variants 1..5, simple and nested loops, enum and fixed-array
//! matches, slice rest patterns, fallible slice indexing, `&mut` field
//! assignment, trait-bounded generics, parenthesized arithmetic chains).  The negative corpus proves
//! linearity is tracked by type (TYD_NATIVE), never by name: randomized
//! custom native handles that are leaked or double-moved must be rejected
//! with the exact linear-consumption diagnostics.
//!
//! The PRNG seed is drawn from OS entropy on every run, so each gate
//! execution tests completely new trees.  Set CINNABAR_FUZZ_SEED to replay a
//! specific run.  Any failure saves the generated source to
//! tests/fixtures/repro/fuzz_fail_<seed>.cnb and prints the seed to stderr.
//! CINNABAR_TEST_PROFILE selects full (default), balanced, or smoke coverage;
//! the CINNABAR_FUZZ_* controls can override individual corpus budgets.

#[path = "support/test_controls.rs"]
mod test_controls;

use std::hash::{BuildHasher, Hash, Hasher};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use test_controls::{evenly_selected, profile_name, profile_usize, test_profile, usize_control};

const DEFAULT_POSITIVE_CASES: usize = 80;
const DEFAULT_NEGATIVE_CASES: usize = 80;
const BALANCED_POSITIVE_CASES: usize = 32;
const BALANCED_NEGATIVE_CASES: usize = 32;
const BALANCED_RUN_CASES: usize = 8;
const SMOKE_POSITIVE_CASES: usize = 8;
const SMOKE_NEGATIVE_CASES: usize = 8;
const SMOKE_RUN_CASES: usize = 2;
const DEFAULT_RUN_TIMEOUT_SECS: usize = 10;
const DEFAULT_COMPILE_TIMEOUT_SECS: usize = 30;
const TIMEOUT_CODE: i32 = 124;
const NEG_SEED_XOR: u64 = 0x9E37_79B9_7F4A_7C15;

struct FuzzConfig {
    profile: test_controls::TestProfile,
    positive_cases: usize,
    negative_cases: usize,
    run_cases: usize,
    run_timeout_secs: u64,
    compile_timeout_secs: u64,
}

fn fuzz_config() -> FuzzConfig {
    let profile = test_profile();
    let positive_default = profile_usize(
        profile,
        DEFAULT_POSITIVE_CASES,
        BALANCED_POSITIVE_CASES,
        SMOKE_POSITIVE_CASES,
    );
    let negative_default = profile_usize(
        profile,
        DEFAULT_NEGATIVE_CASES,
        BALANCED_NEGATIVE_CASES,
        SMOKE_NEGATIVE_CASES,
    );
    let positive_cases = usize_control("CINNABAR_FUZZ_POSITIVE_CASES", positive_default);
    let negative_cases = usize_control("CINNABAR_FUZZ_NEGATIVE_CASES", negative_default);
    let run_default = profile_usize(
        profile,
        positive_cases,
        positive_cases.min(BALANCED_RUN_CASES),
        positive_cases.min(SMOKE_RUN_CASES),
    );
    let run_cases = usize_control("CINNABAR_FUZZ_RUN_CASES", run_default);
    assert!(
        run_cases <= positive_cases,
        "CINNABAR_FUZZ_RUN_CASES ({}) cannot exceed CINNABAR_FUZZ_POSITIVE_CASES ({})",
        run_cases,
        positive_cases
    );
    let run_timeout = usize_control("CINNABAR_TEST_RUN_TIMEOUT_SECS", DEFAULT_RUN_TIMEOUT_SECS);
    let compile_timeout = usize_control(
        "CINNABAR_TEST_COMPILE_TIMEOUT_SECS",
        DEFAULT_COMPILE_TIMEOUT_SECS,
    );
    assert!(run_timeout > 0, "CINNABAR_TEST_RUN_TIMEOUT_SECS must be greater than zero");
    assert!(
        compile_timeout > 0,
        "CINNABAR_TEST_COMPILE_TIMEOUT_SECS must be greater than zero"
    );
    FuzzConfig {
        profile,
        positive_cases,
        negative_cases,
        run_cases,
        run_timeout_secs: run_timeout as u64,
        compile_timeout_secs: compile_timeout as u64,
    }
}

const KEYWORDS: &[&str] = &[
    "fun", "end", "val", "var", "const", "pub", "nat", "impure", "if", "elif", "else",
    "while", "break", "continue", "return", "match", "use", "as", "mod", "type", "trait",
    "impl", "try", "rest",
];

const BAIT_PREFIXES: &[&str] = &["Block", "Vec", "String", "HashMap", "Socket", "Fd"];

#[derive(Clone, Copy)]
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Rng {
        Rng(seed | 1)
    }

    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    fn below(&mut self, n: u64) -> u64 {
        if n == 0 {
            return 0;
        }
        self.next() % n
    }

    fn range(&mut self, lo: i64, hi: i64) -> i64 {
        lo + self.below((hi - lo) as u64) as i64
    }
}

fn char_pascal(rng: &mut Rng) -> char {
    let kind = rng.below(3);
    if kind == 0 {
        (b'a' + rng.below(26) as u8) as char
    } else if kind == 1 {
        (b'A' + rng.below(26) as u8) as char
    } else {
        (b'0' + rng.below(10) as u8) as char
    }
}

fn char_snake(rng: &mut Rng) -> char {
    let kind = rng.below(3);
    if kind == 0 {
        (b'a' + rng.below(26) as u8) as char
    } else if kind == 1 {
        (b'0' + rng.below(10) as u8) as char
    } else {
        '_'
    }
}

fn name_used(used: &[String], name: &str) -> bool {
    let mut idx = 0usize;
    while idx < used.len() {
        match used.get(idx) {
            Some(existing) => {
                if existing == name {
                    return true;
                }
            }
            None => {}
        }
        idx += 1;
    }
    false
}

fn keyword(name: &str) -> bool {
    let mut idx = 0usize;
    while idx < KEYWORDS.len() {
        match KEYWORDS.get(idx) {
            Some(kw) => {
                if *kw == name {
                    return true;
                }
            }
            None => {}
        }
        idx += 1;
    }
    false
}

// PascalCase: first character uppercase (or an uppercase bait prefix); the
// remainder is alphanumeric (underscores are banned by the casing rule).
fn unique_pascal(rng: &mut Rng, used: &mut Vec<String>, prefix: &str) -> String {
    let mut name = String::from(prefix);
    if name.is_empty() {
        name.push((b'A' + rng.below(26) as u8) as char);
    }
    let extra = 3 + rng.below(6) as usize;
    let mut n = 0usize;
    while n < extra {
        name.push(char_pascal(rng));
        n += 1;
    }
    while name_used(used, &name) {
        name.push(char_pascal(rng));
    }
    used.push(name.clone());
    name
}

// SCREAMING_SNAKE_CASE: first character uppercase; the remainder is
// uppercase, digits, or underscores.
fn unique_screaming(rng: &mut Rng, used: &mut Vec<String>) -> String {
    let mut name = String::new();
    name.push((b'A' + rng.below(26) as u8) as char);
    let extra = 3 + rng.below(6) as usize;
    let mut n = 0usize;
    while n < extra {
        let kind = rng.below(3);
        if kind == 0 {
            name.push((b'A' + rng.below(26) as u8) as char);
        } else if kind == 1 {
            name.push((b'0' + rng.below(10) as u8) as char);
        } else {
            name.push('_');
        }
        n += 1;
    }
    while name_used(used, &name) {
        name.push('_');
    }
    used.push(name.clone());
    name
}

// snake_case: first character lowercase; the remainder is lowercase, digits,
// or underscores.  Keywords are avoided so generated programs parse.
fn unique_snake(rng: &mut Rng, used: &mut Vec<String>) -> String {
    let mut name = String::new();
    name.push((b'a' + rng.below(26) as u8) as char);
    let extra = 3 + rng.below(6) as usize;
    let mut n = 0usize;
    while n < extra {
        name.push(char_snake(rng));
        n += 1;
    }
    while name_used(used, &name) || keyword(&name) {
        name.push(char_snake(rng));
    }
    used.push(name.clone());
    name
}

// Custom native handles sometimes wear bait prefixes matching the builtin
// native surfaces.  If linearity or lowering were keyed on those names, the
// suffixed names would behave differently from purely random ones and the
// negative corpus would catch it.
fn unique_handle(rng: &mut Rng, used: &mut Vec<String>) -> String {
    let idx = rng.below(BAIT_PREFIXES.len() as u64) as usize;
    let prefix = match BAIT_PREFIXES.get(idx) {
        Some(p) => *p,
        None => "",
    };
    unique_pascal(rng, used, prefix)
}

// ---- arithmetic semantics shared by helper functions and the generator ----

#[derive(Clone, Copy)]
enum UnarySem {
    Add(i64),
    Sub(i64),
    Mul(i64),
    Xor(i64),
    Shl(i64),
    AddMul(i64, i64),
}

#[derive(Clone, Copy)]
enum BinarySem {
    AddAdd(i64),
    SubAdd(i64),
    MulAdd(i64),
    XorAdd(i64),
}

#[derive(Clone, Copy)]
enum BoolSem {
    IsEven,
    Less(i64),
}

fn apply_unary(sem: &UnarySem, x: i64) -> i64 {
    match sem {
        UnarySem::Add(k) => x + *k,
        UnarySem::Sub(k) => x - *k,
        UnarySem::Mul(k) => x * *k,
        UnarySem::Xor(k) => x ^ *k,
        UnarySem::Shl(k) => x << *k,
        UnarySem::AddMul(k1, k2) => (x + *k1) * *k2,
    }
}

fn unary_body(sem: &UnarySem) -> String {
    match sem {
        UnarySem::Add(k) => format!("x + {}", k),
        UnarySem::Sub(k) => format!("x - {}", k),
        UnarySem::Mul(k) => format!("x * {}", k),
        UnarySem::Xor(k) => format!("x ^ {}", k),
        UnarySem::Shl(k) => format!("x << {}", k),
        UnarySem::AddMul(k1, k2) => format!("(x + {}) * {}", k1, k2),
    }
}

fn apply_binary(sem: &BinarySem, x: i64, y: i64) -> i64 {
    match sem {
        BinarySem::AddAdd(k) => (x + y) + *k,
        BinarySem::SubAdd(k) => (x + *k) - y,
        BinarySem::MulAdd(k) => (x * *k) + y,
        BinarySem::XorAdd(k) => (x ^ y) + *k,
    }
}

fn binary_body(sem: &BinarySem) -> String {
    match sem {
        BinarySem::AddAdd(k) => format!("(x + y) + {}", k),
        BinarySem::SubAdd(k) => format!("(x + {}) - y", k),
        BinarySem::MulAdd(k) => format!("(x * {}) + y", k),
        BinarySem::XorAdd(k) => format!("(x ^ y) + {}", k),
    }
}

fn apply_bool(sem: &BoolSem, x: i64) -> bool {
    match sem {
        BoolSem::IsEven => (x & 1) == 0,
        BoolSem::Less(k) => x < *k,
    }
}

fn bool_body(sem: &BoolSem) -> String {
    match sem {
        BoolSem::IsEven => "(x & 1) == 0".to_string(),
        BoolSem::Less(k) => format!("x < {}", k),
    }
}

fn pick_unary_sem(rng: &mut Rng) -> UnarySem {
    let kind = rng.below(6);
    if kind == 0 {
        UnarySem::Add(rng.range(1, 5))
    } else if kind == 1 {
        UnarySem::Sub(rng.range(1, 5))
    } else if kind == 2 {
        UnarySem::Mul(rng.range(2, 4))
    } else if kind == 3 {
        UnarySem::Xor(rng.range(1, 5))
    } else if kind == 4 {
        UnarySem::Shl(rng.range(1, 3))
    } else {
        UnarySem::AddMul(rng.range(1, 4), rng.range(2, 4))
    }
}

fn pick_binary_sem(rng: &mut Rng) -> BinarySem {
    let kind = rng.below(4);
    if kind == 0 {
        BinarySem::AddAdd(rng.range(1, 5))
    } else if kind == 1 {
        BinarySem::SubAdd(rng.range(1, 5))
    } else if kind == 2 {
        BinarySem::MulAdd(rng.range(1, 5))
    } else {
        BinarySem::XorAdd(rng.range(1, 5))
    }
}

fn pick_bool_sem(rng: &mut Rng) -> BoolSem {
    if rng.below(2) == 0 {
        BoolSem::IsEven
    } else {
        BoolSem::Less(rng.range(1, 8))
    }
}

// ---- program model ----

#[derive(Clone)]
struct StructDef {
    name: String,
    fields: Vec<String>,
}

#[derive(Clone)]
struct EnumDef {
    variants: Vec<(String, usize)>,
}

#[derive(Clone)]
struct OobInfo {
    k0: i64,
    k1: i64,
    a: i64,
    b: i64,
}

#[derive(Clone)]
struct IntVar {
    name: String,
    value: i64,
}

#[derive(Clone)]
struct BoolVar {
    name: String,
    value: bool,
}

#[derive(Clone)]
struct StructVar {
    name: String,
    struct_idx: usize,
    fields: Vec<i64>,
}

#[derive(Clone)]
struct State {
    ints: Vec<IntVar>,
    bools: Vec<BoolVar>,
    structs_v: Vec<StructVar>,
}

fn int_of(state: &State, name: &str) -> Option<i64> {
    let mut found: Option<i64> = None;
    let mut idx = 0usize;
    while idx < state.ints.len() {
        match state.ints.get(idx) {
            Some(var) => {
                if var.name == name {
                    found = Some(var.value);
                }
            }
            None => {}
        }
        idx += 1;
    }
    found
}

fn bool_of(state: &State, name: &str) -> Option<bool> {
    let mut found: Option<bool> = None;
    let mut idx = 0usize;
    while idx < state.bools.len() {
        match state.bools.get(idx) {
            Some(var) => {
                if var.name == name {
                    found = Some(var.value);
                }
            }
            None => {}
        }
        idx += 1;
    }
    found
}

fn struct_of(state: &State, name: &str) -> Option<Vec<i64>> {
    let mut found: Option<Vec<i64>> = None;
    let mut idx = 0usize;
    while idx < state.structs_v.len() {
        match state.structs_v.get(idx) {
            Some(var) => {
                if var.name == name {
                    found = Some(var.fields.clone());
                }
            }
            None => {}
        }
        idx += 1;
    }
    found
}

fn set_int(state: &mut State, name: &str, value: i64) {
    let mut idx = 0usize;
    while idx < state.ints.len() {
        match state.ints.get_mut(idx) {
            Some(var) => {
                if var.name == name {
                    var.value = value;
                }
            }
            None => {}
        }
        idx += 1;
    }
}

fn add_int(state: &mut State, name: &str, value: i64) {
    state.ints.push(IntVar {
        name: name.to_string(),
        value,
    });
}

fn add_bool(state: &mut State, name: &str, value: bool) {
    state.bools.push(BoolVar {
        name: name.to_string(),
        value,
    });
}

fn add_struct(state: &mut State, name: &str, struct_idx: usize, fields: Vec<i64>) {
    state.structs_v.push(StructVar {
        name: name.to_string(),
        struct_idx,
        fields,
    });
}

fn set_struct_field(state: &mut State, name: &str, field_idx: usize, value: i64) {
    let mut idx = 0usize;
    while idx < state.structs_v.len() {
        match state.structs_v.get_mut(idx) {
            Some(var) => {
                if var.name == name {
                    match var.fields.get_mut(field_idx) {
                        Some(slot) => {
                            *slot = value;
                        }
                        None => {}
                    }
                }
            }
            None => {}
        }
        idx += 1;
    }
}

fn restore(state: &mut State, snap: &State) {
    state.ints = snap.ints.clone();
    state.bools = snap.bools.clone();
    state.structs_v = snap.structs_v.clone();
}

fn in_state(state: &State, name: &str) -> bool {
    let mut idx = 0usize;
    while idx < state.ints.len() {
        match state.ints.get(idx) {
            Some(var) => {
                if var.name == name {
                    return true;
                }
            }
            None => {}
        }
        idx += 1;
    }
    let mut bidx = 0usize;
    while bidx < state.bools.len() {
        match state.bools.get(bidx) {
            Some(var) => {
                if var.name == name {
                    return true;
                }
            }
            None => {}
        }
        bidx += 1;
    }
    let mut sidx = 0usize;
    while sidx < state.structs_v.len() {
        match state.structs_v.get(sidx) {
            Some(var) => {
                if var.name == name {
                    return true;
                }
            }
            None => {}
        }
        sidx += 1;
    }
    false
}

// After an `if`/`elif`/`else` chain, drop every variable declared inside a
// branch: its locals go out of scope at the branch's `end`, so a later
// reference to them would name an unknown variable.  Pre-existing names keep
// their post-branch values (Cinnabar if bodies are scoped).
fn prune_to_snapshot(state: &mut State, snap: &State) {
    let mut kept_ints: Vec<IntVar> = Vec::new();
    let mut idx = 0usize;
    while idx < state.ints.len() {
        match state.ints.get(idx) {
            Some(var) => {
                if in_state(snap, &var.name) {
                    kept_ints.push(var.clone());
                }
            }
            None => {}
        }
        idx += 1;
    }
    state.ints = kept_ints;
    let mut kept_bools: Vec<BoolVar> = Vec::new();
    let mut bidx = 0usize;
    while bidx < state.bools.len() {
        match state.bools.get(bidx) {
            Some(var) => {
                if in_state(snap, &var.name) {
                    kept_bools.push(var.clone());
                }
            }
            None => {}
        }
        bidx += 1;
    }
    state.bools = kept_bools;
    let mut kept_structs: Vec<StructVar> = Vec::new();
    let mut sidx = 0usize;
    while sidx < state.structs_v.len() {
        match state.structs_v.get(sidx) {
            Some(var) => {
                if in_state(snap, &var.name) {
                    kept_structs.push(var.clone());
                }
            }
            None => {}
        }
        sidx += 1;
    }
    state.structs_v = kept_structs;
}

struct Gen {
    rng: Rng,
    used: Vec<String>,
    src: String,
    indent: String,
    structs: Vec<StructDef>,
    enums: Vec<EnumDef>,
    unary_helpers: Vec<(String, UnarySem)>,
    binary_helpers: Vec<(String, BinarySem)>,
    bool_helpers: Vec<(String, BoolSem)>,
    shared_helpers: Vec<(String, Vec<usize>)>,
    mut_helpers: Vec<(String, Vec<usize>)>,
    bounded: String,
    oob: Option<OobInfo>,
    finish: Option<String>,
    arr_len: usize,
    state: State,
    total: String,
}

impl Gen {
    fn push(&mut self, line: &str) {
        self.src.push_str(&self.indent);
        self.src.push_str(line);
        self.src.push('\n');
    }

    fn fresh_snake(&mut self) -> String {
        unique_snake(&mut self.rng, &mut self.used)
    }

    fn fresh_pascal(&mut self) -> String {
        unique_pascal(&mut self.rng, &mut self.used, "")
    }

    fn fresh_screaming(&mut self) -> String {
        unique_screaming(&mut self.rng, &mut self.used)
    }
}

fn field_name(g: &Gen, sidx: usize, fidx: usize) -> String {
    match g.structs.get(sidx) {
        Some(d) => match d.fields.get(fidx) {
            Some(n) => n.clone(),
            None => String::new(),
        },
        None => String::new(),
    }
}

fn struct_literal_text(g: &Gen, sidx: usize, vals: &[i64]) -> String {
    let sname = match g.structs.get(sidx) {
        Some(d) => d.name.clone(),
        None => String::new(),
    };
    let mut line = format!("{}(", sname);
    let mut f = 0usize;
    while f < vals.len() {
        let fname = field_name(g, sidx, f);
        let value = match vals.get(f) {
            Some(v) => *v,
            None => 0,
        };
        if f == 0 {
            line = format!("{}{}: {}", line, fname, value);
        } else {
            line = format!("{}, {}: {}", line, fname, value);
        }
        f += 1;
    }
    line.push(')');
    line
}

fn has_index(list: &[usize], value: usize) -> bool {
    let mut idx = 0usize;
    while idx < list.len() {
        match list.get(idx) {
            Some(v) => {
                if *v == value {
                    return true;
                }
            }
            None => {}
        }
        idx += 1;
    }
    false
}

fn field_sum_values(values: &[i64], indices: &[usize]) -> i64 {
    let mut sum = 0i64;
    let mut f = 0usize;
    while f < indices.len() {
        match indices.get(f) {
            Some(fidx) => match values.get(*fidx) {
                Some(v) => {
                    sum += *v;
                }
                None => {}
            },
            None => {}
        }
        f += 1;
    }
    sum
}

fn oob_value(oinfo: &OobInfo, index: i64, length: i64) -> i64 {
    if index == oinfo.k0 && length == oinfo.k1 {
        oinfo.a
    } else {
        oinfo.b
    }
}

fn pick_int_name(g: &mut Gen) -> String {
    if g.rng.below(2) == 0 {
        return g.total.clone();
    }
    let count = g.state.ints.len();
    if count == 0 {
        return g.total.clone();
    }
    let idx = g.rng.below(count as u64) as usize;
    let mut out = g.total.clone();
    let mut i = 0usize;
    while i < count {
        if i == idx {
            match g.state.ints.get(i) {
                Some(var) => {
                    out = var.name.clone();
                }
                None => {}
            }
        }
        i += 1;
    }
    out
}

fn pick_bool_name(g: &mut Gen) -> Option<String> {
    let count = g.state.bools.len();
    if count == 0 {
        return None;
    }
    let idx = g.rng.below(count as u64) as usize;
    match g.state.bools.get(idx) {
        Some(var) => Some(var.name.clone()),
        None => None,
    }
}

fn pick_struct_var(g: &mut Gen, want_idx: usize) -> Option<(String, Vec<i64>)> {
    let mut matches: Vec<(String, Vec<i64>)> = Vec::new();
    let mut idx = 0usize;
    while idx < g.state.structs_v.len() {
        match g.state.structs_v.get(idx) {
            Some(var) => {
                if var.struct_idx == want_idx {
                    matches.push((var.name.clone(), var.fields.clone()));
                }
            }
            None => {}
        }
        idx += 1;
    }
    if matches.is_empty() {
        return None;
    }
    let pick = g.rng.below(matches.len() as u64) as usize;
    match matches.get(pick) {
        Some(found) => Some(found.clone()),
        None => None,
    }
}

fn pick_field(g: &mut Gen) -> Option<(String, String, i64)> {
    let mut candidates: Vec<(String, String, i64)> = Vec::new();
    let mut idx = 0usize;
    while idx < g.state.structs_v.len() {
        match g.state.structs_v.get(idx) {
            Some(var) => {
                if !var.fields.is_empty() {
                    let fcount = var.fields.len();
                    let fidx = g.rng.below(fcount as u64) as usize;
                    match var.fields.get(fidx) {
                        Some(value) => {
                            let fname = field_name(g, var.struct_idx, fidx);
                            candidates.push((var.name.clone(), fname, *value));
                        }
                        None => {}
                    }
                }
            }
            None => {}
        }
        idx += 1;
    }
    if candidates.is_empty() {
        return None;
    }
    let pick = g.rng.below(candidates.len() as u64) as usize;
    match candidates.get(pick) {
        Some(found) => Some(found.clone()),
        None => None,
    }
}

fn pick_unary(g: &mut Gen) -> Option<(String, UnarySem)> {
    let count = g.unary_helpers.len();
    if count == 0 {
        return None;
    }
    let idx = g.rng.below(count as u64) as usize;
    match g.unary_helpers.get(idx) {
        Some((name, sem)) => Some((name.clone(), *sem)),
        None => None,
    }
}

fn pick_binary(g: &mut Gen) -> Option<(String, BinarySem)> {
    let count = g.binary_helpers.len();
    if count == 0 {
        return None;
    }
    let idx = g.rng.below(count as u64) as usize;
    match g.binary_helpers.get(idx) {
        Some((name, sem)) => Some((name.clone(), *sem)),
        None => None,
    }
}

fn pick_bool(g: &mut Gen) -> Option<(String, BoolSem)> {
    let count = g.bool_helpers.len();
    if count == 0 {
        return None;
    }
    let idx = g.rng.below(count as u64) as usize;
    match g.bool_helpers.get(idx) {
        Some((name, sem)) => Some((name.clone(), *sem)),
        None => None,
    }
}

fn pick_shared(g: &mut Gen) -> Option<(String, Vec<usize>)> {
    let count = g.shared_helpers.len();
    if count == 0 {
        return None;
    }
    let idx = g.rng.below(count as u64) as usize;
    match g.shared_helpers.get(idx) {
        Some((name, fields)) => Some((name.clone(), fields.clone())),
        None => None,
    }
}

fn pick_mut(g: &mut Gen) -> Option<(String, Vec<usize>)> {
    let count = g.mut_helpers.len();
    if count == 0 {
        return None;
    }
    let idx = g.rng.below(count as u64) as usize;
    match g.mut_helpers.get(idx) {
        Some((name, fields)) => Some((name.clone(), fields.clone())),
        None => None,
    }
}

fn struct_reader_parts(rng: &mut Rng, sdef: &StructDef) -> (Vec<usize>, String) {
    let fcount = sdef.fields.len();
    let pick_count = if fcount == 1 { 1 } else { 1 + rng.below(2) as usize };
    let mut idxs: Vec<usize> = Vec::new();
    let mut p = 0usize;
    while p < pick_count {
        let mut attempts = 0usize;
        let mut found: Option<usize> = None;
        while attempts < 8 && found.is_none() {
            let candidate = rng.below(fcount as u64) as usize;
            if !has_index(&idxs, candidate) {
                found = Some(candidate);
            }
            attempts += 1;
        }
        match found {
            Some(fidx) => {
                idxs.push(fidx);
            }
            None => {}
        }
        p += 1;
    }
    let mut body = String::new();
    let mut f = 0usize;
    while f < idxs.len() {
        match idxs.get(f) {
            Some(fidx) => {
                let fname = match sdef.fields.get(*fidx) {
                    Some(n) => n.clone(),
                    None => String::new(),
                };
                if f == 0 {
                    body = format!("value.{}", fname);
                } else {
                    body = format!("{} + value.{}", body, fname);
                }
            }
            None => {}
        }
        f += 1;
    }
    (idxs, body)
}

// ---- conditions ----

#[derive(Clone, Copy)]
enum CmpOp {
    Eq,
    Ne,
    Lt,
    Gt,
    Le,
    Ge,
}

fn cmp_op_text(op: &CmpOp) -> String {
    match op {
        CmpOp::Eq => "==".to_string(),
        CmpOp::Ne => "!=".to_string(),
        CmpOp::Lt => "<".to_string(),
        CmpOp::Gt => ">".to_string(),
        CmpOp::Le => "<=".to_string(),
        CmpOp::Ge => ">=".to_string(),
    }
}

fn cmp_eval(a: i64, op: &CmpOp, b: i64) -> bool {
    match op {
        CmpOp::Eq => a == b,
        CmpOp::Ne => a != b,
        CmpOp::Lt => a < b,
        CmpOp::Gt => a > b,
        CmpOp::Le => a <= b,
        CmpOp::Ge => a >= b,
    }
}

enum Cond {
    IntCmp {
        var: String,
        op: CmpOp,
        k: i64,
    },
    BoolVar {
        var: String,
    },
    Even {
        var: String,
    },
}

fn cond_text(cond: &Cond) -> String {
    match cond {
        Cond::IntCmp { var, op, k } => format!("{} {} {}", var, cmp_op_text(op), k),
        Cond::BoolVar { var } => var.clone(),
        Cond::Even { var } => format!("({} & 1) == 0", var),
    }
}

fn eval_cond(g: &Gen, cond: &Cond) -> bool {
    match cond {
        Cond::IntCmp { var, op, k } => {
            let x = match int_of(&g.state, var) {
                Some(v) => v,
                None => 0,
            };
            cmp_eval(x, op, *k)
        }
        Cond::BoolVar { var } => match bool_of(&g.state, var) {
            Some(b) => b,
            None => false,
        },
        Cond::Even { var } => {
            let x = match int_of(&g.state, var) {
                Some(v) => v,
                None => 0,
            };
            (x & 1) == 0
        }
    }
}

fn pick_cond(g: &mut Gen) -> Cond {
    let kind = g.rng.below(3);
    if kind == 0 {
        let var = pick_int_name(g);
        let op_idx = g.rng.below(6);
        let op = if op_idx == 0 {
            CmpOp::Eq
        } else if op_idx == 1 {
            CmpOp::Ne
        } else if op_idx == 2 {
            CmpOp::Lt
        } else if op_idx == 3 {
            CmpOp::Gt
        } else if op_idx == 4 {
            CmpOp::Le
        } else {
            CmpOp::Ge
        };
        Cond::IntCmp {
            var,
            op,
            k: g.rng.range(0, 6),
        }
    } else if kind == 1 {
        match pick_bool_name(g) {
            Some(var) => Cond::BoolVar { var },
            None => Cond::IntCmp {
                var: pick_int_name(g),
                op: CmpOp::Gt,
                k: 1,
            },
        }
    } else {
        Cond::Even {
            var: pick_int_name(g),
        }
    }
}

// ---- arm expressions for enum matches ----

fn arm_expr(g: &mut Gen, apcount: usize, binds: &[String], payloads: &[i64]) -> (String, i64) {
    if apcount == 0 {
        let k = g.rng.range(1, 7);
        (k.to_string(), k)
    } else if apcount == 1 {
        let a = match payloads.first() {
            Some(v) => *v,
            None => 0,
        };
        let an = match binds.first() {
            Some(n) => n.clone(),
            None => String::new(),
        };
        let kind = g.rng.below(4);
        if kind == 0 {
            (an, a)
        } else if kind == 1 {
            let k = g.rng.range(1, 5);
            (format!("{} + {}", an, k), a + k)
        } else if kind == 2 {
            let k = g.rng.range(1, 4);
            (format!("({} * 2) + {}", an, k), a * 2 + k)
        } else {
            let k = g.rng.range(a + 1, a + 5);
            (format!("{} - {}", k, an), k - a)
        }
    } else {
        let a = match payloads.first() {
            Some(v) => *v,
            None => 0,
        };
        let b = match payloads.get(1) {
            Some(v) => *v,
            None => 0,
        };
        let an = match binds.first() {
            Some(n) => n.clone(),
            None => String::new(),
        };
        let bn = match binds.get(1) {
            Some(n) => n.clone(),
            None => String::new(),
        };
        let kind = g.rng.below(3);
        if kind == 0 {
            let k = g.rng.range(1, 5);
            (format!("({} + {}) + {}", an, bn, k), a + b + k)
        } else if kind == 1 {
            (format!("({} * 2) + {}", an, bn), a * 2 + b)
        } else {
            (format!("({} + {})", an, bn), a + b)
        }
    }
}

// ---- statement generation ----

fn gen_arith_assign(g: &mut Gen) {
    let total = g.total.clone();
    let cur = match int_of(&g.state, &total) {
        Some(v) => v,
        None => 0,
    };
    let kind = g.rng.below(8);
    if kind == 0 {
        let k = g.rng.range(1, 5);
        g.push(&format!("{} = {} + {}", total, total, k));
        set_int(&mut g.state, &total, cur + k);
    } else if kind == 1 {
        let k = g.rng.range(1, 5);
        g.push(&format!("{} = {} - {}", total, total, k));
        set_int(&mut g.state, &total, cur - k);
    } else if kind == 2 {
        let k = g.rng.range(2, 4);
        g.push(&format!("{} = {} * {}", total, total, k));
        set_int(&mut g.state, &total, cur * k);
    } else if kind == 3 {
        let k = g.rng.range(1, 5);
        g.push(&format!("{} = {} ^ {}", total, total, k));
        set_int(&mut g.state, &total, cur ^ k);
    } else if kind == 4 {
        let k = g.rng.range(1, 5);
        g.push(&format!("{} = {} | {}", total, total, k));
        set_int(&mut g.state, &total, cur | k);
    } else if kind == 5 {
        let k = g.rng.range(1, 5);
        g.push(&format!("{} = {} & {}", total, total, k));
        set_int(&mut g.state, &total, cur & k);
    } else if kind == 6 {
        let k = g.rng.range(1, 3);
        g.push(&format!("{} = {} << {}", total, total, k));
        set_int(&mut g.state, &total, cur << k);
    } else {
        let other = pick_int_name(g);
        let other_val = match int_of(&g.state, &other) {
            Some(v) => v,
            None => 0,
        };
        g.push(&format!("{} = {} + {}", total, total, other));
        set_int(&mut g.state, &total, cur + other_val);
    }
}

fn gen_unary_helper(g: &mut Gen) {
    match pick_unary(g) {
        Some((name, sem)) => {
            let total = g.total.clone();
            let cur = match int_of(&g.state, &total) {
                Some(v) => v,
                None => 0,
            };
            let arg = pick_int_name(g);
            let arg_val = match int_of(&g.state, &arg) {
                Some(v) => v,
                None => 0,
            };
            let result = apply_unary(&sem, arg_val);
            let y = g.fresh_snake();
            g.push(&format!("val {} = {}({})", y, name, arg));
            g.push(&format!("{} = {} + {}", total, total, y));
            add_int(&mut g.state, &y, result);
            set_int(&mut g.state, &total, cur + result);
        }
        None => gen_arith_assign(g),
    }
}

fn gen_binary_helper(g: &mut Gen) {
    match pick_binary(g) {
        Some((name, sem)) => {
            let total = g.total.clone();
            let cur = match int_of(&g.state, &total) {
                Some(v) => v,
                None => 0,
            };
            let a = pick_int_name(g);
            let b = pick_int_name(g);
            let av = match int_of(&g.state, &a) {
                Some(v) => v,
                None => 0,
            };
            let bv = match int_of(&g.state, &b) {
                Some(v) => v,
                None => 0,
            };
            let result = apply_binary(&sem, av, bv);
            let y = g.fresh_snake();
            g.push(&format!("val {} = {}({}, {})", y, name, a, b));
            g.push(&format!("{} = {} + {}", total, total, y));
            add_int(&mut g.state, &y, result);
            set_int(&mut g.state, &total, cur + result);
        }
        None => gen_arith_assign(g),
    }
}

fn gen_bool_helper(g: &mut Gen) {
    match pick_bool(g) {
        Some((name, sem)) => {
            let total = g.total.clone();
            let arg = pick_int_name(g);
            let arg_val = match int_of(&g.state, &arg) {
                Some(v) => v,
                None => 0,
            };
            let result = apply_bool(&sem, arg_val);
            let b = g.fresh_snake();
            g.push(&format!("val {} = {}({})", b, name, arg));
            g.push(&format!("if {}", b));
            g.indent.push_str("  ");
            let k = g.rng.range(1, 4);
            g.push(&format!("{} = {} + {}", total, total, k));
            g.indent.truncate(g.indent.len() - 2);
            g.push("end");
            add_bool(&mut g.state, &b, result);
            if result {
                let cur = match int_of(&g.state, &total) {
                    Some(v) => v,
                    None => 0,
                };
                set_int(&mut g.state, &total, cur + k);
            }
        }
        None => gen_arith_assign(g),
    }
}

fn gen_field_read(g: &mut Gen) {
    match pick_field(g) {
        Some((pname, fname, value)) => {
            let total = g.total.clone();
            let cur = match int_of(&g.state, &total) {
                Some(v) => v,
                None => 0,
            };
            g.push(&format!("{} = {} + {}.{}", total, total, pname, fname));
            set_int(&mut g.state, &total, cur + value);
        }
        None => gen_arith_assign(g),
    }
}

fn gen_bounded_call(g: &mut Gen) {
    if g.bounded.is_empty() {
        gen_arith_assign(g);
        return;
    }
    match pick_struct_var(g, 0) {
        Some((pname, fields)) => {
            let total = g.total.clone();
            let cur = match int_of(&g.state, &total) {
                Some(v) => v,
                None => 0,
            };
            let add = match fields.first() {
                Some(v) => *v,
                None => 0,
            };
            let y = g.fresh_snake();
            g.push(&format!("val {} = {}(&{})", y, g.bounded, pname));
            g.push(&format!("{} = {} + {}", total, total, y));
            add_int(&mut g.state, &y, add);
            set_int(&mut g.state, &total, cur + add);
        }
        None => gen_arith_assign(g),
    }
}

fn gen_shared_call(g: &mut Gen) {
    match pick_shared(g) {
        Some((name, fields)) => match pick_struct_var(g, 0) {
            Some((pname, sfields)) => {
                let add = field_sum_values(&sfields, &fields);
                let total = g.total.clone();
                let cur = match int_of(&g.state, &total) {
                    Some(v) => v,
                    None => 0,
                };
                let y = g.fresh_snake();
                g.push(&format!("val {} = {}(&{})", y, name, pname));
                g.push(&format!("{} = {} + {}", total, total, y));
                add_int(&mut g.state, &y, add);
                set_int(&mut g.state, &total, cur + add);
            }
            None => gen_arith_assign(g),
        },
        None => gen_arith_assign(g),
    }
}

fn gen_mut_call(g: &mut Gen) {
    match pick_mut(g) {
        Some((name, fields)) => match pick_struct_var(g, 0) {
            Some((pname, sfields)) => {
                let add = field_sum_values(&sfields, &fields);
                let total = g.total.clone();
                let cur = match int_of(&g.state, &total) {
                    Some(v) => v,
                    None => 0,
                };
                let y = g.fresh_snake();
                g.push(&format!("val {} = {}(&mut {})", y, name, pname));
                g.push(&format!("{} = {} + {}", total, total, y));
                add_int(&mut g.state, &y, add);
                set_int(&mut g.state, &total, cur + add);
            }
            None => gen_arith_assign(g),
        },
        None => gen_arith_assign(g),
    }
}

fn gen_struct_block(g: &mut Gen, sidx: usize) {
    let sdef = match g.structs.get(sidx) {
        Some(d) => d.clone(),
        None => {
            gen_arith_assign(g);
            return;
        }
    };
    let fcount = sdef.fields.len();
    let pname = g.fresh_snake();
    let mut values: Vec<i64> = Vec::new();
    let mut f = 0usize;
    while f < fcount {
        values.push(g.rng.range(0, 5));
        f += 1;
    }
    g.push(&format!("var {} = {}", pname, struct_literal_text(g, sidx, &values)));
    add_struct(&mut g.state, &pname, sidx, values.clone());
    let rname = g.fresh_snake();
    g.push(&format!("val {} = &mut {}", rname, pname));
    let writes = 1 + g.rng.below(2) as usize;
    let mut w = 0usize;
    while w < writes {
        let fidx = g.rng.below(fcount as u64) as usize;
        let value = g.rng.range(0, 6);
        let fname = field_name(g, sidx, fidx);
        g.push(&format!("{}.{} = {}", rname, fname, value));
        set_struct_field(&mut g.state, &pname, fidx, value);
        w += 1;
    }
    let f0 = match struct_of(&g.state, &pname) {
        Some(fields) => match fields.first() {
            Some(v) => *v,
            None => 0,
        },
        None => 0,
    };
    let f0name = field_name(g, sidx, 0);
    let total = g.total.clone();
    let cur = match int_of(&g.state, &total) {
        Some(v) => v,
        None => 0,
    };
    g.push(&format!("{} = {} + {}.{}", total, total, pname, f0name));
    set_int(&mut g.state, &total, cur + f0);
}

// Emits one arm per variant of `edef`, binding payloads into fresh names and
// computing each arm's expression from the constructed payload values.  The
// arm whose index is `taken` is the one that executes at runtime; its value
// is returned so the generator can fold it into its expected total.
fn emit_enum_arms(g: &mut Gen, edef: &EnumDef, payloads: &[i64], taken: usize) -> i64 {
    let vcount = edef.variants.len();
    let mut arm_expected = 0i64;
    let mut arm_idx = 0usize;
    while arm_idx < vcount {
        match edef.variants.get(arm_idx) {
            Some((aname, apcount)) => {
                let mut binds: Vec<String> = Vec::new();
                let mut b = 0usize;
                while b < *apcount {
                    binds.push(g.fresh_snake());
                    b += 1;
                }
                let mut pattern = aname.clone();
                if *apcount > 0 {
                    pattern.push('(');
                    let mut b = 0usize;
                    while b < binds.len() {
                        match binds.get(b) {
                            Some(bn) => {
                                if b == 0 {
                                    pattern.push_str(bn);
                                } else {
                                    pattern = format!("{}, {}", pattern, bn);
                                }
                            }
                            None => {}
                        }
                        b += 1;
                    }
                    pattern.push(')');
                }
                let (expr_text, expr_value) = arm_expr(g, *apcount, &binds, payloads);
                g.push(&format!("    {} => {}", pattern, expr_text));
                if arm_idx == taken {
                    arm_expected = expr_value;
                }
            }
            None => {}
        }
        arm_idx += 1;
    }
    arm_expected
}

fn gen_enum_match(g: &mut Gen) {
    let ecount = g.enums.len();
    if ecount == 0 {
        gen_arith_assign(g);
        return;
    }
    let eidx = g.rng.below(ecount as u64) as usize;
    let edef = match g.enums.get(eidx) {
        Some(d) => d.clone(),
        None => {
            gen_arith_assign(g);
            return;
        }
    };
    let vcount = edef.variants.len();
    let vidx = g.rng.below(vcount as u64) as usize;
    let (vname, pcount) = match edef.variants.get(vidx) {
        Some(v) => (v.0.clone(), v.1),
        None => {
            gen_arith_assign(g);
            return;
        }
    };
    let mut payloads: Vec<i64> = Vec::new();
    let mut p = 0usize;
    while p < pcount {
        payloads.push(g.rng.range(0, 6));
        p += 1;
    }
    let ename = g.fresh_snake();
    let mut ctor = format!("val {} = {}", ename, vname);
    if pcount > 0 {
        ctor.push('(');
        let mut p = 0usize;
        while p < payloads.len() {
            match payloads.get(p) {
                Some(value) => {
                    ctor = format!("{}{}", ctor, value);
                    if p + 1 < payloads.len() {
                        ctor.push_str(", ");
                    }
                }
                None => {}
            }
            p += 1;
        }
        ctor.push(')');
    }
    g.push(&ctor);
    let mname = g.fresh_snake();
    g.push(&format!("val {} = match {}", mname, ename));
    let arm_expected = emit_enum_arms(g, &edef, &payloads, vidx);
    g.push("  end");
    let total = g.total.clone();
    let cur = match int_of(&g.state, &total) {
        Some(v) => v,
        None => 0,
    };
    g.push(&format!("{} = {} + {}", total, total, mname));
    set_int(&mut g.state, &total, cur + arm_expected);
}

fn gen_while(g: &mut Gen) {
    let total = g.total.clone();
    let cur = match int_of(&g.state, &total) {
        Some(v) => v,
        None => 0,
    };
    let n = 2 + g.rng.below(4) as i64;
    let k = 1 + g.rng.below(4) as i64;
    let i = g.fresh_snake();
    let form = g.rng.below(4);
    if form == 0 {            g.push(&format!("var {}: I64 = 0", i));
        g.push(&format!("while {} < {}", i, n));
        g.indent.push_str("  ");
        g.push(&format!("{} = {} + {}", total, total, k));
        g.push(&format!("{} = {} + 1", i, i));
        g.indent.truncate(g.indent.len() - 2);
        g.push("end");
        set_int(&mut g.state, &total, cur + n * k);
        add_int(&mut g.state, &i, n);
    } else if form == 1 {            g.push(&format!("var {}: I64 = 0", i));
        g.push(&format!("while {} < {}", i, n));
        g.indent.push_str("  ");
        g.push(&format!("{} = {} + {}", total, total, i));
        g.push(&format!("{} = {} + 1", i, i));
        g.indent.truncate(g.indent.len() - 2);
        g.push("end");
        set_int(&mut g.state, &total, cur + n * (n - 1) / 2);
        add_int(&mut g.state, &i, n);
    } else if form == 2 {            g.push(&format!("var {}: I64 = 0", i));
        g.push("while true");
        g.indent.push_str("  ");
        g.push(&format!("{} = {} + 1", i, i));
        g.push(&format!("if {} >= {}", i, n));
        g.indent.push_str("  ");
        g.push("break");
        g.indent.truncate(g.indent.len() - 2);
        g.push("end");
        g.push(&format!("{} = {} + {}", total, total, k));
        g.indent.truncate(g.indent.len() - 2);
        g.push("end");
        set_int(&mut g.state, &total, cur + (n - 1) * k);
        add_int(&mut g.state, &i, n);
    } else {            g.push(&format!("var {}: I64 = 0", i));
        g.push(&format!("while {} < {}", i, n));
        g.indent.push_str("  ");
        g.push(&format!("{} = {} + 1", i, i));
        g.push(&format!("if ({} & 1) == 0", i));
        g.indent.push_str("  ");
        g.push("continue");
        g.indent.truncate(g.indent.len() - 2);
        g.push("end");
        g.push(&format!("{} = {} + {}", total, total, k));
        g.indent.truncate(g.indent.len() - 2);
        g.push("end");
        set_int(&mut g.state, &total, cur + ((n + 1) / 2) * k);
        add_int(&mut g.state, &i, n);
    }
}

// Overwrites one field of a live struct variable through a fresh `&mut`
// reference (`r.field = value`), keeping the generator's copy in sync so
// later reads of that field fold the new value.
fn gen_struct_field_assign(g: &mut Gen) {
    let mut candidates: Vec<(String, usize, usize)> = Vec::new();
    let mut idx = 0usize;
    while idx < g.state.structs_v.len() {
        match g.state.structs_v.get(idx) {
            Some(var) => {
                let fcount = match g.structs.get(var.struct_idx) {
                    Some(d) => d.fields.len(),
                    None => 0,
                };
                let mut f = 0usize;
                while f < fcount {
                    candidates.push((var.name.clone(), var.struct_idx, f));
                    f += 1;
                }
            }
            None => {}
        }
        idx += 1;
    }
    if candidates.is_empty() {
        gen_arith_assign(g);
        return;
    }
    let pick = g.rng.below(candidates.len() as u64) as usize;
    let (pname, sidx, fidx) = match candidates.get(pick) {
        Some(c) => c.clone(),
        None => {
            gen_arith_assign(g);
            return;
        }
    };
    let value = g.rng.range(0, 6);
    let fname = field_name(g, sidx, fidx);
    let rname = g.fresh_snake();
    g.push(&format!("val {} = &mut {}", rname, pname));
    g.push(&format!("{}.{} = {}", rname, fname, value));
    set_struct_field(&mut g.state, &pname, fidx, value);
}

// A longer parenthesized arithmetic expression combining two live ints.
fn gen_arith_expr_chain(g: &mut Gen) {
    let total = g.total.clone();
    let cur = match int_of(&g.state, &total) {
        Some(v) => v,
        None => 0,
    };
    let a = pick_int_name(g);
    let b = pick_int_name(g);
    let av = match int_of(&g.state, &a) {
        Some(v) => v,
        None => 0,
    };
    let bv = match int_of(&g.state, &b) {
        Some(v) => v,
        None => 0,
    };
    let kind = g.rng.below(3);
    if kind == 0 {
        g.push(&format!("{} = {} + (({} + {}) * 2)", total, total, a, b));
        set_int(&mut g.state, &total, cur + (av + bv) * 2);
    } else if kind == 1 {
        g.push(&format!("{} = {} + (({} * 2) + {})", total, total, a, b));
        set_int(&mut g.state, &total, cur + av * 2 + bv);
    } else {
        g.push(&format!("{} = {} + (({} - {}) + 3)", total, total, a, b));
        set_int(&mut g.state, &total, cur + (av - bv) + 3);
    }
}

// A local fixed-size array matched with a full element pattern, folding the
// pattern bindings' fields into the total (`[s0, s1] => s0.f0 + s1.f0`).
fn gen_array_match_stmt(g: &mut Gen) {
    let sdef0 = match g.structs.first() {
        Some(d) => d.clone(),
        None => {
            gen_arith_assign(g);
            return;
        }
    };
    let fcount = sdef0.fields.len();
    let n = 2 + g.rng.below(2) as usize;
    let mut elems: Vec<Vec<i64>> = Vec::new();
    let mut e = 0usize;
    while e < n {
        let mut vals: Vec<i64> = Vec::new();
        let mut f = 0usize;
        while f < fcount {
            vals.push(g.rng.range(0, 5));
            f += 1;
        }
        elems.push(vals);
        e += 1;
    }
    let arr = g.fresh_snake();
    let mut line = format!("val {} = [", arr);
    let mut e = 0usize;
    while e < elems.len() {
        match elems.get(e) {
            Some(vals) => {
                if e == 0 {
                    line = format!("{}{}", line, struct_literal_text(g, 0, vals));
                } else {
                    line = format!("{}, {}", line, struct_literal_text(g, 0, vals));
                }
            }
            None => {}
        }
        e += 1;
    }
    line.push(']');
    g.push(&line);
    let f0name = field_name(g, 0, 0);
    let mut pattern = String::new();
    let mut expr = String::new();
    let mut value = 0i64;
    let mut i = 0usize;
    while i < n {
        let bname = g.fresh_snake();
        if i == 0 {
            pattern = bname.clone();
        } else {
            pattern = format!("{}, {}", pattern, bname);
        }
        match elems.get(i) {
            Some(vals) => match vals.first() {
                Some(v) => {
                    if i == 0 {
                        expr = format!("{}.{}", bname, f0name);
                    } else {
                        expr = format!("{} + {}.{}", expr, bname, f0name);
                    }
                    value += *v;
                }
                None => {}
            },
            None => {}
        }
        i += 1;
    }
    let mname = g.fresh_snake();
    g.push(&format!("val {} = match {}", mname, arr));
    g.push(&format!("  [{}] => {}", pattern, expr));
    g.push("  end");
    let total = g.total.clone();
    let cur = match int_of(&g.state, &total) {
        Some(v) => v,
        None => 0,
    };
    g.push(&format!("{} = {} + {}", total, total, mname));
    set_int(&mut g.state, &total, cur + value);
}

// A local array of enum values indexed at a constant and matched exhaustively.
fn gen_enum_array_match(g: &mut Gen) {
    let ecount = g.enums.len();
    if ecount == 0 {
        gen_arith_assign(g);
        return;
    }
    let eidx = g.rng.below(ecount as u64) as usize;
    let edef = match g.enums.get(eidx) {
        Some(d) => d.clone(),
        None => {
            gen_arith_assign(g);
            return;
        }
    };
    let vcount = edef.variants.len();
    let n = 2 + g.rng.below(2) as usize;
    let mut elems: Vec<(usize, Vec<i64>)> = Vec::new();
    let mut e = 0usize;
    while e < n {
        let vidx = g.rng.below(vcount as u64) as usize;
        let pcount = match edef.variants.get(vidx) {
            Some(v) => v.1,
            None => {
                gen_arith_assign(g);
                return;
            }
        };
        let mut payloads: Vec<i64> = Vec::new();
        let mut p = 0usize;
        while p < pcount {
            payloads.push(g.rng.range(0, 6));
            p += 1;
        }
        elems.push((vidx, payloads));
        e += 1;
    }
    let arr = g.fresh_snake();
    let mut line = format!("val {} = [", arr);
    let mut e = 0usize;
    while e < elems.len() {
        match elems.get(e) {
            Some((vidx, payloads)) => {
                let vname = match edef.variants.get(*vidx) {
                    Some(v) => v.0.clone(),
                    None => String::new(),
                };
                let mut ctor = vname;
                if !payloads.is_empty() {
                    ctor.push('(');
                    let mut p = 0usize;
                    while p < payloads.len() {
                        match payloads.get(p) {
                            Some(value) => {
                                ctor = format!("{}{}", ctor, value);
                                if p + 1 < payloads.len() {
                                    ctor.push_str(", ");
                                }
                            }
                            None => {}
                        }
                        p += 1;
                    }
                    ctor.push(')');
                }
                if e == 0 {
                    line = format!("{}{}", line, ctor);
                } else {
                    line = format!("{}, {}", line, ctor);
                }
            }
            None => {}
        }
        e += 1;
    }
    line.push(']');
    g.push(&line);
    let k = g.rng.below(n as u64) as usize;
    let (vidx, payloads) = match elems.get(k) {
        Some(pair) => pair.clone(),
        None => {
            gen_arith_assign(g);
            return;
        }
    };
    let mname = g.fresh_snake();
    g.push(&format!("val {} = match {}[{}]", mname, arr, k));
    let arm_expected = emit_enum_arms(g, &edef, &payloads, vidx);
    g.push("  end");
    let total = g.total.clone();
    let cur = match int_of(&g.state, &total) {
        Some(v) => v,
        None => 0,
    };
    g.push(&format!("{} = {} + {}", total, total, mname));
    set_int(&mut g.state, &total, cur + arm_expected);
}

// Two nested bounded loops, exercising loop-in-loop lowering; the inner
// counter is scoped to the outer body and never leaves the generator state.
fn gen_nested_while(g: &mut Gen) {
    let total = g.total.clone();
    let cur = match int_of(&g.state, &total) {
        Some(v) => v,
        None => 0,
    };
    let n1 = 2 + g.rng.below(3) as i64;
    let n2 = 2 + g.rng.below(3) as i64;
    let k = 1 + g.rng.below(4) as i64;
    let i = g.fresh_snake();
    g.push(&format!("var {}: I64 = 0", i));
    g.push(&format!("while {} < {}", i, n1));
    g.indent.push_str("  ");
    let j = g.fresh_snake();
    g.push(&format!("var {}: I64 = 0", j));
    g.push(&format!("while {} < {}", j, n2));
    g.indent.push_str("  ");
    g.push(&format!("{} = {} + {}", total, total, k));
    g.push(&format!("{} = {} + 1", j, j));
    g.indent.truncate(g.indent.len() - 2);
    g.push("end");
    g.push(&format!("{} = {} + 1", i, i));
    g.indent.truncate(g.indent.len() - 2);
    g.push("end");
    set_int(&mut g.state, &total, cur + n1 * n2 * k);
    add_int(&mut g.state, &i, n1);
}

fn gen_if(g: &mut Gen, depth: i32) {
    let branch_count = 1 + g.rng.below(3) as i32;
    let has_else = g.rng.below(2) == 1;
    let pre = g.state.clone();
    let mut conds: Vec<Cond> = Vec::new();
    let mut taken: Option<i32> = None;
    let mut n = 0i32;
    while n < branch_count {
        let c = pick_cond(g);
        if taken.is_none() && eval_cond(g, &c) {
            taken = Some(n);
        }
        conds.push(c);
        n += 1;
    }
    let mut merged: Option<State> = None;
    let mut k = 0i32;
    while k < branch_count {
        let cond = match conds.get(k as usize) {
            Some(c) => c,
            None => break,
        };
        if k == 0 {
            g.push(&format!("if {}", cond_text(cond)));
        } else {
            g.push(&format!("elif {}", cond_text(cond)));
        }
        let snap = g.state.clone();
        g.indent.push_str("  ");
        gen_body(g, depth + 1);
        g.indent.truncate(g.indent.len() - 2);
        if taken == Some(k) {
            merged = Some(g.state.clone());
        }
        restore(&mut g.state, &snap);
        k += 1;
    }
    if has_else {
        g.push("else");
        let snap = g.state.clone();
        g.indent.push_str("  ");
        gen_body(g, depth + 1);
        g.indent.truncate(g.indent.len() - 2);
        if taken.is_none() {
            merged = Some(g.state.clone());
        }
        restore(&mut g.state, &snap);
    }
    g.push("end");
    match merged {
        Some(mut state) => {
            prune_to_snapshot(&mut state, &pre);
            restore(&mut g.state, &state);
        }
        None => {}
    }
}

fn gen_body(g: &mut Gen, depth: i32) {
    let count = 1 + g.rng.below(3) as i32;
    let mut n = 0i32;
    while n < count {
        gen_stmt(g, depth);
        n += 1;
    }
}

fn gen_stmt(g: &mut Gen, depth: i32) {
    let pick = g.rng.below(18);
    if pick < 2 {
        gen_arith_assign(g);
    } else if pick < 4 {
        gen_unary_helper(g);
    } else if pick < 5 {
        gen_binary_helper(g);
    } else if pick < 6 {
        gen_bool_helper(g);
    } else if pick < 7 {
        gen_field_read(g);
    } else if pick < 8 {
        gen_bounded_call(g);
    } else if pick < 9 {
        gen_shared_call(g);
    } else if pick < 10 {
        gen_mut_call(g);
    } else if pick < 11 {
        gen_struct_field_assign(g);
    } else if pick < 12 {
        gen_arith_expr_chain(g);
    } else if pick < 13 {
        gen_array_match_stmt(g);
    } else if pick < 14 {
        gen_enum_array_match(g);
    } else if pick < 15 {
        gen_nested_while(g);
    } else if pick < 16 {
        if depth < 4 {
            gen_if(g, depth);
        } else {
            gen_arith_assign(g);
        }
    } else {
        if depth < 4 {
            gen_while(g);
        } else {
            gen_field_read(g);
        }
    }
}

// ---- endings ----

fn gen_simple_ending(g: &mut Gen) {
    let total = g.total.clone();
    let expected = match int_of(&g.state, &total) {
        Some(v) => v,
        None => 0,
    };
    g.push(&format!("if {} == {}", total, expected));
    g.indent.push_str("  ");
    g.push("return 0");
    g.indent.truncate(g.indent.len() - 2);
    g.push("end");
    g.push("return 1");
}

fn gen_array_block(g: &mut Gen) {
    let finish = match g.finish.clone() {
        Some(f) => f,
        None => {
            gen_simple_ending(g);
            return;
        }
    };
    let oinfo = match g.oob.clone() {
        Some(o) => o,
        None => {
            gen_simple_ending(g);
            return;
        }
    };
    let n = g.arr_len;
    if n < 2 {
        gen_simple_ending(g);
        return;
    }
    let arr = g.fresh_snake();
    let p0 = g.fresh_snake();
    let rest = g.fresh_snake();
    let mut elems: Vec<Vec<i64>> = Vec::new();
    let mut e = 0usize;
    while e < n {
        let fcount = match g.structs.first() {
            Some(d) => d.fields.len(),
            None => 0,
        };
        let mut vals: Vec<i64> = Vec::new();
        let mut f = 0usize;
        while f < fcount {
            vals.push(g.rng.range(0, 5));
            f += 1;
        }
        elems.push(vals);
        e += 1;
    }
    let sname = match g.structs.first() {
        Some(d) => d.name.clone(),
        None => String::new(),
    };
    let f0name = field_name(g, 0, 0);
    let mut line = format!("var {}: [{}; {}] = [", arr, sname, n);
    let mut e = 0usize;
    while e < elems.len() {
        match elems.get(e) {
            Some(vals) => {
                if e == 0 {
                    line = format!("{}{}", line, struct_literal_text(g, 0, vals));
                } else {
                    line = format!("{}, {}", line, struct_literal_text(g, 0, vals));
                }
            }
            None => {}
        }
        e += 1;
    }
    line.push(']');
    g.push(&line);
    let total = g.total.clone();
    let mut cur = match int_of(&g.state, &total) {
        Some(v) => v,
        None => 0,
    };
    let k0 = g.rng.below(n as u64) as usize;
    let v0 = match elems.get(k0) {
        Some(vals) => match vals.first() {
            Some(v) => *v,
            None => 0,
        },
        None => 0,
    };
    g.push(&format!("{} = {} + {}[{}].{}", total, total, arr, k0, f0name));
    cur += v0;
    match pick_shared(g) {
        Some((name, fields)) => {
            let k1 = g.rng.below(n as u64) as usize;
            let add = match elems.get(k1) {
                Some(vals) => field_sum_values(vals, &fields),
                None => 0,
            };
            let y = g.fresh_snake();
            g.push(&format!("val {} = {}(&{}[{}])", y, name, arr, k1));
            g.push(&format!("{} = {} + {}", total, total, y));
            cur += add;
        }
        None => {}
    }
    match pick_mut(g) {
        Some((name, fields)) => {
            let k2 = g.rng.below(n as u64) as usize;
            let add = match elems.get(k2) {
                Some(vals) => field_sum_values(vals, &fields),
                None => 0,
            };
            let y = g.fresh_snake();
            g.push(&format!("val {} = {}(&mut {}[{}])", y, name, arr, k2));
            g.push(&format!("{} = {} + {}", total, total, y));
            cur += add;
        }
        None => {}
    }
    set_int(&mut g.state, &total, cur);
    let d0e = match elems.get(1) {
        Some(vals) => match vals.first() {
            Some(v) => *v,
            None => 0,
        },
        None => 0,
    };
    let rest_len = (n - 1) as i64;
    let d1e = oob_value(&oinfo, rest_len, rest_len);
    let p0f = match elems.first() {
        Some(vals) => match vals.first() {
            Some(v) => *v,
            None => 0,
        },
        None => 0,
    };
    // The finish function's htail match re-reads hrest[0].f0, the same
    // element as the hd0 Ok arm, so its contribution is d0e again.
    let want = cur + d0e + d1e + p0f + d0e;
    g.push(&format!("match {}", arr));
    g.push(&format!(
        "  [{}, {} @ ..] => return {}({}, {}, {}, {})",
        p0, rest, finish, p0, rest, total, want
    ));
    g.push("end");
}

// ---- corpus generators ----

fn generate_positive(rng: &mut Rng, seed: u64, iteration: usize) -> String {
    let mut g = Gen {
        rng: *rng,
        used: vec!["main".to_string()],
        src: String::new(),
        indent: String::new(),
        structs: Vec::new(),
        enums: Vec::new(),
        unary_helpers: Vec::new(),
        binary_helpers: Vec::new(),
        bool_helpers: Vec::new(),
        shared_helpers: Vec::new(),
        mut_helpers: Vec::new(),
        bounded: String::new(),
        oob: None,
        finish: None,
        arr_len: 0,
        state: State {
            ints: Vec::new(),
            bools: Vec::new(),
            structs_v: Vec::new(),
        },
        total: String::new(),
    };
    g.push(&format!(
        "#!| fuzzer-generated positive program (seed {}, iteration {}) |#",
        seed, iteration
    ));
    g.push("");
    let use_array = g.rng.below(100) < 65;
    let struct_count = 1 + g.rng.below(2) as usize;
    let mut s = 0usize;
    while s < struct_count {
        let sname = g.fresh_pascal();
        let fcount = 1 + g.rng.below(6) as usize;
        let mut fields: Vec<String> = Vec::new();
        let mut f = 0usize;
        while f < fcount {
            fields.push(g.fresh_snake());
            f += 1;
        }
        g.push(&format!("pub type {}", sname));
        let mut f = 0usize;
        while f < fields.len() {
            match fields.get(f) {
                Some(fname) => g.push(&format!("  pub {}: I64", fname)),
                None => {}
            }
            f += 1;
        }
        g.push("end");
        g.push("");
        g.structs.push(StructDef { name: sname, fields });
        s += 1;
    }
    let enum_count = 1 + g.rng.below(2) as usize;
    let mut e = 0usize;
    while e < enum_count {
        let ename = g.fresh_pascal();
        let vcount = 1 + g.rng.below(5) as usize;
        let mut variants: Vec<(String, usize)> = Vec::new();
        let mut v = 0usize;
        while v < vcount {
            let vname = g.fresh_pascal();
            let pcount = g.rng.below(3) as usize;
            variants.push((vname, pcount));
            v += 1;
        }
        g.push(&format!("pub type {}", ename));
        let mut v = 0usize;
        while v < variants.len() {
            match variants.get(v) {
                Some((vname, pcount)) => {
                    if *pcount == 0 {
                        g.push(&format!("  pub {}", vname));
                    } else {
                        let mut line = format!("  pub {}", vname);
                        line.push('(');
                        let mut p = 0usize;
                        while p < *pcount {
                            if p == 0 {
                                line.push_str("I64");
                            } else {
                                line.push_str(", I64");
                            }
                            p += 1;
                        }
                        line.push(')');
                        g.push(&line);
                    }
                }
                None => {}
            }
            v += 1;
        }
        g.push("end");
        g.push("");
        g.enums.push(EnumDef { variants });
        e += 1;
    }
    let sdef0 = match g.structs.first() {
        Some(d) => d.clone(),
        None => StructDef {
            name: String::new(),
            fields: Vec::new(),
        },
    };
    let f0name = match sdef0.fields.first() {
        Some(n) => n.clone(),
        None => String::new(),
    };
    let trait_name = g.fresh_pascal();
    let trait_method = g.fresh_snake();
    g.push(&format!("pub trait {}", trait_name));
    g.push(&format!("  pub fun {}(value: &Self) I64", trait_method));
    g.push("end");
    g.push("");
    g.push(&format!("pub impl {} for {}", trait_name, sdef0.name));
    g.push(&format!("  pub fun {}(value: &{}) I64", trait_method, sdef0.name));
    g.push(&format!("    return value.{}", f0name));
    g.push("  end");
    g.push("end");
    g.push("");
    let bounded = g.fresh_snake();
    g.push(&format!("fun {}<T: {}>(value: &T) I64", bounded, trait_name));
    g.push(&format!("  return {}.{}(value)", trait_name, trait_method));
    g.push("end");
    g.push("");
    g.bounded = bounded;
    let uname = g.fresh_snake();
    let usem = pick_unary_sem(&mut g.rng);
    g.push(&format!("fun {}(x: I64) I64", uname));
    g.push(&format!("  return {}", unary_body(&usem)));
    g.push("end");
    g.push("");
    g.unary_helpers.push((uname, usem));
    let bname = g.fresh_snake();
    let bsem = pick_binary_sem(&mut g.rng);
    g.push(&format!("fun {}(x: I64, y: I64) I64", bname));
    g.push(&format!("  return {}", binary_body(&bsem)));
    g.push("end");
    g.push("");
    g.binary_helpers.push((bname, bsem));
    let flname = g.fresh_snake();
    let flsem = pick_bool_sem(&mut g.rng);
    g.push(&format!("fun {}(x: I64) Bool", flname));
    g.push(&format!("  return {}", bool_body(&flsem)));
    g.push("end");
    g.push("");
    g.bool_helpers.push((flname, flsem));
    let (shr_fields, shr_body) = struct_reader_parts(&mut g.rng, &sdef0);
    let shr_name = g.fresh_snake();
    g.push(&format!("fun {}(value: &{}) I64", shr_name, sdef0.name));
    g.push(&format!("  return {}", shr_body));
    g.push("end");
    g.push("");
    g.shared_helpers.push((shr_name, shr_fields));
    let (mut_fields, mut_body) = struct_reader_parts(&mut g.rng, &sdef0);
    let mut_name = g.fresh_snake();
    g.push(&format!("fun {}(value: &mut {}) I64", mut_name, sdef0.name));
    g.push(&format!("  return {}", mut_body));
    g.push("end");
    g.push("");
    g.mut_helpers.push((mut_name, mut_fields));
    if use_array {
        let arr_n = 2 + g.rng.below(3) as usize;
        let oob_name = g.fresh_snake();
        let c0 = g.fresh_screaming();
        let c1 = g.fresh_screaming();
        let k0 = g.rng.range(0, 7);
        let k1 = g.rng.range(0, 7);
        let a = g.rng.range(0, 11);
        let b = g.rng.range(0, 11);
        g.push(&format!("pub const {}: Usize = {}", c0, k0));
        g.push(&format!("pub const {}: Usize = {}", c1, k1));
        g.push("");
        g.push(&format!("fun {}(index: Usize, length: Usize) I64", oob_name));
        g.push(&format!("  if index == {} && length == {}", c0, c1));
        g.push(&format!("    return {}", a));
        g.push("  end");
        g.push(&format!("  return {}", b));
        g.push("end");
        g.push("");
        let finish_name = g.fresh_snake();
        let hp0 = g.fresh_snake();
        let hrest = g.fresh_snake();
        let htotal = g.fresh_snake();
        let hwant = g.fresh_snake();
        let hd0 = g.fresh_snake();
        let hd1 = g.fresh_snake();
        let htail = g.fresh_snake();
        let rest_len = (arr_n - 1) as i64;
        g.push(&format!(
            "fun {}({}: {}, {}: &[{}], {}: I64, {}: I64) I64",
            finish_name, hp0, sdef0.name, hrest, sdef0.name, htotal, hwant
        ));
        g.push(&format!("  val {} = match &{}[0]", hd0, hrest));
        g.push(&format!("    Ok(value) => value.{}", f0name));
        g.push(&format!(
            "    Err(IndexOutOfBounds(index, length)) => {}(index, length)",
            oob_name
        ));
        g.push("  end");
        g.push(&format!("  val {} = match &{}[{}]", hd1, hrest, rest_len));
        g.push(&format!("    Ok(value) => value.{}", f0name));
        g.push(&format!(
            "    Err(IndexOutOfBounds(index, length)) => {}(index, length)",
            oob_name
        ));
        g.push("  end");
        g.push(&format!("  val {} = match {}", htail, hrest));
        g.push("    [] => 0");
        g.push(&format!("    [h0, hrest2 @ ..] => h0.{}", f0name));
        g.push("  end");
        g.push(&format!(
            "  if (((({} + {}) + {}) + {}.{}) + {}) == {}",
            htotal, hd0, hd1, hp0, f0name, htail, hwant
        ));
        g.push("    return 0");
        g.push("  end");
        g.push("  return 1");
        g.push("end");
        g.push("");
        g.oob = Some(OobInfo { k0, k1, a, b });
        g.finish = Some(finish_name);
        g.arr_len = arr_n;
    }
    g.push("pub fun main() I64");
    g.indent = "  ".to_string();
    let total = g.fresh_snake();
    g.push(&format!("var {}: I64 = 0", total));
    g.total = total.clone();
    add_int(&mut g.state, &total, 0);
    gen_struct_block(&mut g, 0);
    if struct_count == 2 && g.rng.below(2) == 0 {
        gen_struct_block(&mut g, 1);
    }
    if g.rng.below(2) == 0 {
        gen_enum_match(&mut g);
    }
    if g.rng.below(2) == 0 {
        gen_bounded_call(&mut g);
    }
    let body_stmts = 3 + g.rng.below(5) as i32;
    let mut b = 0i32;
    while b < body_stmts {
        gen_stmt(&mut g, 1);
        b += 1;
    }
    if use_array {
        gen_array_block(&mut g);
    } else {
        gen_simple_ending(&mut g);
    }
    g.indent = String::new();
    g.push("end");
    *rng = g.rng;
    g.src
}

fn generate_negative(rng: &mut Rng, shape: usize) -> (String, &'static str) {
    let mut used: Vec<String> = vec!["main".to_string()];
    let m = unique_pascal(rng, &mut used, "");
    let err_ty = unique_pascal(rng, &mut used, "");
    let fault = unique_pascal(rng, &mut used, "");
    let handle = unique_handle(rng, &mut used);
    let make = unique_snake(rng, &mut used);
    let destroy = unique_snake(rng, &mut used);
    let h1 = unique_snake(rng, &mut used);
    let h2 = unique_snake(rng, &mut used);
    let mut src = String::new();
    let mut push = |line: &str| {
        src.push_str(line);
        src.push('\n');
    };
    push(&format!("#!| fuzzer-generated linearity probe (shape {}) |#", shape));
    push(&format!("pub mod {}", m));
    push(&format!("  pub type {}", err_ty));
    push(&format!("    pub {}", fault));
    push("  end");
    push("");
    push(&format!("  pub nat type {}", handle));
    push(&format!(
        "  pub nat fun {}() impure Result({}, {})",
        make, handle, err_ty
    ));
    if shape != 0 {
        push(&format!("  pub nat fun {}(h: {}) impure Unit", destroy, handle));
    }
    push("end");
    push("");
    push(&format!("use {}.{}", m, make));
    if shape != 0 {
        push(&format!("use {}.{}", m, destroy));
    }
    push("");
    push("pub fun main() impure I64");
    push(&format!("  val {} = match {}()", h1, make));
    push("    Ok(value) => value");
    push(&format!("    Err({}.{}) => return 0", m, fault));
    push("  end");
    let want: &'static str;
    if shape == 0 {
        want = "must be consumed";
    } else if shape == 1 {
        push(&format!("  {}({})", destroy, h1));
        push(&format!("  {}({})", destroy, h1));
        want = "use of moved value";
    } else if shape == 2 {
        push(&format!("  val {} = match {}()", h2, make));
        push("    Ok(value) => value");
        push(&format!("    Err({}.{}) => return 0", m, fault));
        push("  end");
        push(&format!("  {}({})", destroy, h1));
        want = "must be consumed";
    } else {
        push(&format!("  val {} = match {}()", h2, make));
        push("    Ok(value) => value");
        push(&format!("    Err({}.{}) => return 0", m, fault));
        push("  end");
        push(&format!("  {}({})", destroy, h1));
        push(&format!("  {}({})", destroy, h1));
        push(&format!("  {}({})", destroy, h2));
        want = "use of moved value";
    }
    push("  return 0");
    push("end");
    (src, want)
}

// ---- tool invocation ----

fn run_tool(cmd: &mut Command, secs: u64) -> (i32, String) {
    let mut child = match cmd.stdout(Stdio::piped()).stderr(Stdio::piped()).spawn() {
        Ok(c) => c,
        Err(err) => {
            eprintln!("spawn failed: {}", err);
            return (127, String::new());
        }
    };
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(secs);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let out = match child.wait_with_output() {
                    Ok(o) => o,
                    Err(err) => {
                        eprintln!("reap failed: {}", err);
                        let code = match status.code() {
                            Some(c) => c,
                            None => 139,
                        };
                        return (code, String::new());
                    }
                };
                let mut text = String::from_utf8_lossy(&out.stdout).to_string();
                text.push_str(&String::from_utf8_lossy(&out.stderr));
                let code = match status.code() {
                    Some(c) => c,
                    None => 139,
                };
                return (code, text);
            }
            Ok(None) => {
                if std::time::Instant::now() >= deadline {
                    match child.kill() {
                        Ok(()) => {}
                        Err(err) => {
                            if err.kind() != std::io::ErrorKind::InvalidInput {
                                eprintln!("kill after deadline failed: {}", err);
                            }
                        }
                    }
                    match child.wait() {
                        Ok(status) => {
                            let detail = match status.code() {
                                Some(c) => format!("timed out (exit {})", c),
                                None => "timed out (no exit code)".to_string(),
                            };
                            return (TIMEOUT_CODE, detail);
                        }
                        Err(err) => {
                            eprintln!("reap failed: {}", err);
                            return (TIMEOUT_CODE, "timed out".to_string());
                        }
                    }
                }
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            Err(err) => {
                eprintln!("wait failed: {}", err);
                return (139, String::new());
            }
        }
    }
}

fn compile_and_link(cinnabar: &str, src_path: &Path, bin: &Path, secs: u64) -> (i32, String) {
    let mut cmd = Command::new(cinnabar);
    cmd.arg(src_path).arg("-o").arg(bin);
    run_tool(&mut cmd, secs)
}

fn compile_to_llvm(cinnabar: &str, src_path: &Path, ir: &Path, secs: u64) -> (i32, String) {
    let mut cmd = Command::new(cinnabar);
    cmd.arg(src_path).arg("--emit-llvm").arg("-o").arg(ir);
    run_tool(&mut cmd, secs)
}

fn run_binary(bin: &Path, secs: u64) -> (i32, String) {
    let mut cmd = Command::new(bin);
    run_tool(&mut cmd, secs)
}

fn temp_dir() -> PathBuf {
    std::env::temp_dir().join(format!("cinnabar_fuzz_{}", std::process::id()))
}

// Removes the fuzzer's temp dir on every exit path, including a panic from
// a failure assertion.  Without this, a failed iteration leaks the compiled
// binaries (each carrying a ~4.5 MB embedded-libc.a link copy) and fills
// the tmpfs over repeated runs.
struct TempDirGuard(PathBuf);

impl Drop for TempDirGuard {
    fn drop(&mut self) {
        match std::fs::remove_dir_all(&self.0) {
            Ok(()) => {}
            Err(err) => eprintln!("fuzz temp cleanup failed: {}", err),
        }
    }
}

// A failed fixture write must fail the test, never silently pass an
// iteration that then has nothing to compile or run.
fn write_fixture(path: &Path, src: &str) {
    if let Err(err) = std::fs::write(path, src) {
        assert!(false, "cannot write fixture {}: {}", path.display(), err);
    }
}

// Removes failure artifacts left by earlier runs so a green run never leaves
// a stale `fuzz_fail_*.cnb` behind in the fixture tree.
fn clear_stale_failures() {
    let repro = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("repro");
    let entries = match std::fs::read_dir(&repro) {
        Ok(entries) => entries,
        Err(err) => {
            eprintln!("fuzz: cannot scan {}: {}", repro.display(), err);
            return;
        }
    };
    for entry in entries {
        if let Ok(entry) = entry {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.starts_with("fuzz_fail_") && name.ends_with(".cnb") {
                if let Err(err) = std::fs::remove_file(entry.path()) {
                    assert!(false, "cannot remove stale artifact {}: {}", entry.path().display(), err);
                }
            }
        }
    }
}

fn save_failure(seed: u64, src: &str) -> PathBuf {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("repro")
        .join(format!("fuzz_fail_{}.cnb", seed));
    match std::fs::write(&path, src) {
        Ok(()) => {}
        Err(err) => eprintln!("cannot save failure artifact {}: {}", path.display(), err),
    }
    path
}

fn run_seed() -> u64 {
    match std::env::var("CINNABAR_FUZZ_SEED") {
        Ok(text) => match text.parse::<u64>() {
            Ok(value) => return value,
            Err(err) => {
                eprintln!("ignoring invalid CINNABAR_FUZZ_SEED: {}", err);
            }
        },
        Err(err) => {
            match err {
                std::env::VarError::NotPresent => {}
                std::env::VarError::NotUnicode(value) => {
                    eprintln!("ignoring non-unicode CINNABAR_FUZZ_SEED: {:?}", value);
                }
            }
        }
    }
    let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
    match std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
        Ok(duration) => {
            duration.as_nanos().hash(&mut hasher);
        }
        Err(err) => {
            err.duration().as_nanos().hash(&mut hasher);
        }
    }
    std::process::id().hash(&mut hasher);
    hasher.finish()
}

#[test]
fn fuzz_generalization_corpus() {
    let seed = run_seed();
    let config = fuzz_config();
    eprintln!("fuzz seed: {}", seed);
    eprintln!(
        "fuzz profile: {} (positive={}, negative={}, link+run={}, remaining positive cases emit LLVM only)",
        profile_name(config.profile),
        config.positive_cases,
        config.negative_cases,
        config.run_cases
    );
    clear_stale_failures();
    let cinnabar = env!("CARGO_BIN_EXE_cinnabar");
    let dir = temp_dir();
    if let Err(err) = std::fs::create_dir_all(&dir) {
        assert!(false, "cannot create temp dir {}: {}", dir.display(), err);
    }
    let guard = TempDirGuard(dir.clone());

    let mut pos_rng = Rng::new(seed);
    let mut idx = 0usize;
    while idx < config.positive_cases {
        let src = generate_positive(&mut pos_rng, seed, idx);
        let src_path = dir.join(format!("pos_{}.cnb", idx));
        let bin = dir.join(format!("pos_{}_bin", idx));
        let ir = dir.join(format!("pos_{}.ll", idx));
        write_fixture(&src_path, &src);
        let execute = evenly_selected(idx, config.positive_cases, config.run_cases);
        let (code, out) = if execute {
            compile_and_link(
                &cinnabar,
                &src_path,
                &bin,
                config.compile_timeout_secs,
            )
        } else {
            compile_to_llvm(&cinnabar, &src_path, &ir, config.compile_timeout_secs)
        };
        if code != 0 {
            let path = save_failure(seed, &src);
            eprintln!("fuzz failure seed: {}", seed);
            assert_eq!(
                code,
                0,
                "positive iteration {} failed to compile (code {}):\n{}\n--- source ---\n{}\n--- saved to {} ---",
                idx,
                code,
                out,
                src,
                path.display()
            );
        }
        if out.contains("internal:") {
            let path = save_failure(seed, &src);
            eprintln!("fuzz failure seed: {}", seed);
            assert!(
                !out.contains("internal:"),
                "positive iteration {} hit an internal error:\n{}\n--- source ---\n{}\n--- saved to {} ---",
                idx,
                out,
                src,
                path.display()
            );
        }
        if execute {
            let (run_code, run_out) = run_binary(&bin, config.run_timeout_secs);
            if run_code != 0 {
                let path = save_failure(seed, &src);
                eprintln!("fuzz failure seed: {}", seed);
                assert_eq!(
                    run_code,
                    0,
                    "positive iteration {} ran with exit {} (want 0):\n{}\n--- source ---\n{}\n--- saved to {} ---",
                    idx,
                    run_code,
                    run_out,
                    src,
                    path.display()
                );
            }
        }
        idx += 1;
    }

    let neg_seed = seed ^ NEG_SEED_XOR;
    let mut neg_rng = Rng::new(neg_seed);
    let mut nidx = 0usize;
    while nidx < config.negative_cases {
        let shape = nidx % 4;
        let (src, want) = generate_negative(&mut neg_rng, shape);
        let src_path = dir.join(format!("lin_{}.cnb", nidx));
        let bin = dir.join(format!("lin_{}_bin", nidx));
        write_fixture(&src_path, &src);
        let (code, out) = compile_and_link(
            &cinnabar,
            &src_path,
            &bin,
            config.compile_timeout_secs,
        );
        if code == 0 {
            let path = save_failure(neg_seed, &src);
            eprintln!("fuzz failure seed: {}", neg_seed);
            assert_ne!(
                code,
                0,
                "linearity probe {} (shape {}) was accepted by the compiler:\n--- source ---\n{}\n--- saved to {} ---",
                nidx,
                shape,
                src,
                path.display()
            );
        }
        let has_want = out.contains(want);
        if !has_want {
            let path = save_failure(neg_seed, &src);
            eprintln!("fuzz failure seed: {}", neg_seed);
            assert!(
                has_want,
                "linearity probe {} (shape {}) was rejected without diagnostic '{}' (code {}):\n--- compiler output ---\n{}\n--- source ---\n{}\n--- saved to {} ---",
                nidx,
                shape,
                want,
                code,
                out,
                src,
                path.display()
            );
        }
        nidx += 1;
    }

    drop(guard);
}
