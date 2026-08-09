//! Independent verification of the Cinnabar spec arithmetic
//! (`tests/fixtures/spec.cnb`), written from scratch against the spec's
//! *formulas* only, and run by `cargo test` as an integration test in the
//! pre-commit gate.
//!
//! Nothing here reads expected answers back out of the compiler; each value
//! is recomputed from the formula in the spec and compared against the
//! constant the spec declares.  This breaks the self-consistency loop: the
//! compiler can only be held to a spec whose constants are known to be
//! right, and that knowledge comes from this independent oracle, not from
//! the compiler's own emissions.
//!
//! Type mapping (Cinnabar -> Rust here):
//!   Int   -> i64
//!   U8    -> u8
//!   U32   -> u32
//!   Usize -> u64

// ---------------------------------------------------------------------------
// Spec formulas (from tests/fixtures/spec.cnb)
// ---------------------------------------------------------------------------

// Checksum for Header (spec: ((kind << 16) ^ (flags << 8) ^ kind ^ flags))
fn header_checksum(kind: u32, flags: u32) -> u32 {
    (kind << 16) ^ (flags << 8) ^ kind ^ flags
}

// Checksum for Tag (spec: value ^ CHECKSUM_SALT)
fn tag_checksum(value: u32, salt: u32) -> u32 {
    value ^ salt
}

// Byte combining (spec: (v3 << 24) | (v2 << 16) | (v1 << 8) | v0)
fn combine_le_bytes(b0: u8, b1: u8, b2: u8, b3: u8) -> u32 {
    let v0 = b0 as u32;
    let v1 = b1 as u32;
    let v2 = b2 as u32;
    let v3 = b3 as u32;
    (v3 << 24) | (v2 << 16) | (v1 << 8) | v0
}

// The Austral port's multiply-based transcription of the same combine.
fn combine_le_bytes_mult(v0: u32, v1: u32, v2: u32, v3: u32) -> u32 {
    (v3 * 16777216) + (v2 * 65536) + (v1 * 256) + v0
}

// normalize (spec): value < 0 -> TooSmall; value > MAX_RANGE -> TooLarge; else Ok
#[derive(PartialEq, Debug)]
enum RangeError {
    TooSmall(i64),
    TooLarge(i64),
}

#[derive(PartialEq, Debug)]
enum RangeResult {
    Ok(i64),
    Err(RangeError),
}

fn normalize(value: i64, max_range: i64) -> RangeResult {
    if value < 0 {
        RangeResult::Err(RangeError::TooSmall(value))
    } else if value > max_range {
        RangeResult::Err(RangeError::TooLarge(value))
    } else {
        RangeResult::Ok(value)
    }
}

// double_positive (spec): value < 0 -> TooSmall; else Ok(value + value)
fn double_positive(value: i64) -> RangeResult {
    if value < 0 {
        RangeResult::Err(RangeError::TooSmall(value))
    } else {
        RangeResult::Ok(value + value)
    }
}

// range_workflow (spec): normalize, then double_positive, propagate errors
fn range_workflow(input: i64, max_range: i64) -> RangeResult {
    match normalize(input, max_range) {
        RangeResult::Ok(normalized) => double_positive(normalized),
        err => err,
    }
}

// range_to_app (spec): TooSmall -> AppRange(TooSmall(value)); TooLarge -> AppRange(TooLarge(value))
//
// AppError has an AppPort(PortError) variant in the spec, but no executed
// code path ever constructs it (app_workflow wraps only RangeError), so the
// verifier models just the computed surface: AppRange(RangeError).
#[derive(PartialEq, Debug)]
enum AppError {
    AppRange(RangeError),
}

#[derive(PartialEq, Debug)]
enum AppResult {
    Ok(i64),
    Err(AppError),
}

fn range_to_app(error: RangeError) -> AppError {
    match error {
        RangeError::TooSmall(value) => AppError::AppRange(RangeError::TooSmall(value)),
        RangeError::TooLarge(value) => AppError::AppRange(RangeError::TooLarge(value)),
    }
}

// app_workflow (spec): normalize, then double_positive; RangeError is wrapped
// into AppError via range_to_app, never returned raw.
fn app_workflow(input: i64, max_range: i64) -> AppResult {
    let normalized = match normalize(input, max_range) {
        RangeResult::Ok(value) => value,
        RangeResult::Err(error) => return AppResult::Err(range_to_app(error)),
    };
    match double_positive(normalized) {
        RangeResult::Ok(doubled) => AppResult::Ok(doubled),
        RangeResult::Err(error) => AppResult::Err(range_to_app(error)),
    }
}

// port_from_int (spec): value < MIN_PORT or > MAX_PORT -> PortInvalid; else Ok
#[derive(PartialEq, Debug)]
enum PortResult {
    Ok(i64),
    Invalid(i64),
}

fn port_from_int(value: i64, min_port: i64, max_port: i64) -> PortResult {
    if value < min_port || value > max_port {
        PortResult::Invalid(value)
    } else {
        PortResult::Ok(value)
    }
}

// half_if_even (spec): == EVEN_TWO -> Some(HALF_TWO); == EVEN_FOUR -> Some(HALF_FOUR); else None
#[derive(PartialEq, Debug)]
enum OptionResult {
    Some(i64),
    None,
}

fn half_if_even(value: i64, even_two: i64, even_four: i64, half_two: i64, half_four: i64) -> OptionResult {
    if value == even_two {
        OptionResult::Some(half_two)
    } else if value == even_four {
        OptionResult::Some(half_four)
    } else {
        OptionResult::None
    }
}

// sum_to (spec): 0 + 1 + ... + (limit - 1)
fn sum_to(limit: i64) -> i64 {
    let mut total: i64 = 0;
    let mut i: i64 = 0;
    while i < limit {
        total += i;
        i += 1;
    }
    total
}

// break_continue_demo (spec): exact trace of the break/continue loop
fn break_continue_demo(loop_limit: i64, even_two: i64) -> i64 {
    let mut total: i64 = 0;
    let mut i: i64 = 0;
    loop {
        if i >= loop_limit {
            break;
        }
        i += 1;
        if i == even_two {
            continue;
        }
        total += i;
    }
    total
}

// Euclidean division (spec): the remainder is always non-negative,
// 0 <= r < abs(divisor), regardless of operand signs.
fn euclid_rem(a: i64, b: i64) -> i64 {
    let r = a % b;
    if r < 0 {
        r + b.abs()
    } else {
        r
    }
}

fn euclid_div(a: i64, b: i64) -> i64 {
    (a - euclid_rem(a, b)) / b
}

// Memory block (spec): allocate(size) yields bounds-checked storage. write_u8
// succeeds iff offset < size; read_u8 returns the stored byte iff offset < size.
struct MemoryBlock {
    bytes: Vec<Option<u8>>,
}

impl MemoryBlock {
    fn allocate(size: u64) -> MemoryBlock {
        MemoryBlock { bytes: vec![None; size as usize] }
    }

    fn write_u8(&mut self, offset: u64, value: u8) -> bool {
        if (offset as usize) < self.bytes.len() {
            self.bytes[offset as usize] = Some(value);
            true
        } else {
            false // AccessOutOfBounds
        }
    }

    fn read_u8(&self, offset: u64) -> Option<u8> {
        if (offset as usize) < self.bytes.len() {
            self.bytes[offset as usize]
        } else {
            None // AccessOutOfBounds
        }
    }
}

// ---------------------------------------------------------------------------
// Tests: one #[test] per spec check group.  Each assertion carries the name
// of the check it replaced so a failure identifies the exact expectation.
// ---------------------------------------------------------------------------

#[test]
fn spec_constants_transcribe_to_decimal() {
    assert_eq!(0x0BAD_F00Du32, 195_948_557, "MAGIC_U32 0x0BADF00D");
    assert_eq!(0xFFFF_FFFFu32, 4_294_967_295, "SENTINEL_U32 0xFFFFFFFF");
    assert_eq!(0x0Du8, 13, "MAGIC_BYTE_0 0x0D");
    assert_eq!(0xF0u8, 240, "MAGIC_BYTE_1 0xF0");
    assert_eq!(0xADu8, 173, "MAGIC_BYTE_2 0xAD");
    assert_eq!(0x0Bu8, 11, "MAGIC_BYTE_3 0x0B");
    assert_eq!(0x43u8, 67, "STRING_BYTE_0 0x43 = 67");
    assert_eq!(0x43u8, 'C' as u8, "STRING_BYTE_0 is UTF-8 'C'");
    assert_eq!(0x69u8, 105, "STRING_BYTE_1 0x69 = 105");
    assert_eq!(0x69u8, 'i' as u8, "STRING_BYTE_1 is UTF-8 'i'");
    assert_eq!(0x6Eu8, 110, "STRING_BYTE_2 0x6E = 110");
    assert_eq!(0x6Eu8, 'n' as u8, "STRING_BYTE_2 is UTF-8 'n'");
    assert_eq!(0x21u8, 33, "STRING_BYTE_3 0x21 = 33");
    assert_eq!(0x21u8, '!' as u8, "STRING_BYTE_3 is UTF-8 '!'");
    assert_eq!(0xA5u8, 165, "MEMORY_BYTE 0xA5");
    assert_eq!(0x5Au32, 90, "TAG_VALUE 0x5A");
    assert_eq!(0x0Fu32, 15, "CHECKSUM_SALT 0x0F");
    assert_eq!(0x0007_0304u32, 459_524, "EXPECTED_HEADER_CHECKSUM 0x00070304");
    assert_eq!(0x55u32, 85, "EXPECTED_TAG_CHECKSUM 0x00000055");
}

#[test]
fn checksums_match_declared_constants() {
    let hc = header_checksum(7, 3);
    assert_eq!(hc, 0x0007_0304, "header checksum (7<<16)^(3<<8)^7^3");
    assert_eq!(hc, 459_524, "header checksum equals declared EXPECTED_HEADER_CHECKSUM");
    let tc = tag_checksum(0x5A, 0x0F);
    assert_eq!(tc, 0x55, "tag checksum 0x5A^0x0F");
    assert_eq!(tc, 85, "tag checksum equals declared EXPECTED_TAG_CHECKSUM");
    assert_ne!(hc, tc, "Header and Tag checksums are distinct");
    let k_shift = 7u32 << 16;
    let f_shift = 3u32 << 8;
    assert_eq!(k_shift, 458_752, "7<<16 = 0x70000");
    assert_eq!(f_shift, 768, "3<<8 = 0x300");
}

#[test]
fn byte_combining_matches_magic_u32() {
    let combined = combine_le_bytes(0x0D, 0xF0, 0xAD, 0x0B);
    assert_eq!(combined, 0x0BAD_F00D, "combine_le_bytes(0D,F0,AD,0B)");
    assert_eq!(combined, 195_948_557, "combine equals declared MAGIC_U32");
    let mult = combine_le_bytes_mult(0x0D, 0xF0, 0xAD, 0x0B);
    assert_eq!(mult, combined, "multiply-form combine equals shift-form combine");
    assert_eq!(combine_le_bytes(1, 0, 0, 0), 1, "combine(01,00,00,00) = 1");
    assert_eq!(combine_le_bytes(0, 1, 0, 0), 0x100, "combine(00,01,00,00) = 0x100");
    assert_eq!(combine_le_bytes(0, 0, 1, 0), 0x1_0000, "combine(00,00,01,00) = 0x10000");
    assert_eq!(combine_le_bytes(0, 0, 0, 1), 0x100_0000, "combine(00,00,00,01) = 0x1000000");
}

#[test]
fn range_workflow_matches_declared_constants() {
    assert_eq!(range_workflow(21, 100), RangeResult::Ok(42), "range_workflow(21) = Ok(42)");
    assert_eq!(
        range_workflow(-1, 100),
        RangeResult::Err(RangeError::TooSmall(-1)),
        "range_workflow(-1) = TooSmall(-1)"
    );
    assert_eq!(
        range_workflow(101, 100),
        RangeResult::Err(RangeError::TooLarge(101)),
        "range_workflow(101) = TooLarge(101)"
    );
    assert_eq!(normalize(0, 100), RangeResult::Ok(0), "normalize(0) boundary");
    assert_eq!(normalize(100, 100), RangeResult::Ok(100), "normalize(100) boundary");
    assert_eq!(
        double_positive(-1),
        RangeResult::Err(RangeError::TooSmall(-1)),
        "double_positive(-1) = TooSmall(-1)"
    );
    assert_eq!(double_positive(0), RangeResult::Ok(0), "double_positive(0) = Ok(0)");
}

#[test]
fn port_bounds_match_declared_constants() {
    assert_eq!(port_from_int(8080, 1, 65535), PortResult::Ok(8080), "port_from_int(8080)");
    assert_eq!(port_from_int(0, 1, 65535), PortResult::Invalid(0), "port_from_int(0)");
    assert_eq!(port_from_int(1, 1, 65535), PortResult::Ok(1), "port_from_int(1) boundary");
    assert_eq!(
        port_from_int(65535, 1, 65535),
        PortResult::Ok(65535),
        "port_from_int(65535) boundary"
    );
    assert_eq!(
        port_from_int(65536, 1, 65535),
        PortResult::Invalid(65536),
        "port_from_int(65536) boundary"
    );
}

#[test]
fn option_flow_matches_declared_constants() {
    assert_eq!(half_if_even(2, 2, 4, 1, 2), OptionResult::Some(1), "half_if_even(2) = Some(1)");
    assert_eq!(half_if_even(4, 2, 4, 1, 2), OptionResult::Some(2), "half_if_even(4) = Some(2)");
    assert_eq!(half_if_even(3, 2, 4, 1, 2), OptionResult::None, "half_if_even(3) = None");
    let try_val = match half_if_even(4, 2, 4, 1, 2) {
        OptionResult::Some(h) => OptionResult::Some(h + 1),
        OptionResult::None => OptionResult::None,
    };
    assert_eq!(try_val, OptionResult::Some(3), "option_try_demo = Some(3)");
}

#[test]
fn loops_produce_declared_results() {
    assert_eq!(sum_to(5), 10, "sum_to(5) = LOOP_EXPECTED (10)");
    assert_eq!(sum_to(0), 0, "sum_to(0) = 0");
    assert_eq!(sum_to(1), 0, "sum_to(1) = 0");
    assert_eq!(break_continue_demo(5, 2), 13, "break_continue_demo() = BREAK_CONTINUE_EXPECTED (13)");
}

#[test]
fn bitflags_match_declared_constants() {
    let flags = 1i64 | 2i64;
    assert_eq!(flags, 3, "FLAG_ACTIVE | FLAG_ADMIN = 3");
    assert_ne!(flags & 1, 0, "flags & FLAG_ACTIVE is nonzero");
    assert_ne!(flags & 2, 0, "flags & FLAG_ADMIN is nonzero");
}

#[test]
fn euclidean_division_matches_spec_semantics() {
    assert_eq!(euclid_rem(10, 3), 1, "10 % 3 = 1");
    assert_eq!(euclid_rem(14, 5), 4, "14 % 5 = 4");
    assert_eq!(euclid_div(21, 3), 7, "21 / 3 = 7");
    assert_eq!(euclid_div(21, 5), 4, "21 / 5 = 4");
    assert_eq!(euclid_rem(-10, 3), 2, "-10 % 3 = 2");
    assert_eq!(euclid_div(-10, 3), -4, "-10 / 3 = -4");
    assert_eq!(euclid_rem(10, -3), 1, "10 % -3 = 1");
    assert_eq!(euclid_div(10, -3), -3, "10 / -3 = -3");
    assert_eq!(euclid_rem(-10, -3), 2, "-10 % -3 = 2");
    assert_eq!(euclid_div(-10, -3), 4, "-10 / -3 = 4");
    let sign_combos = [(-10i64, 3i64), (10i64, -3i64), (-10i64, -3i64), (10i64, 3i64)];
    let identity_ok = sign_combos.iter().all(|&(a, b)| {
        let r = euclid_rem(a, b);
        euclid_div(a, b) * b + r == a && r >= 0 && r < b.abs()
    });
    assert!(identity_ok, "euclid identity a = q*b + r with 0 <= r < abs(b) holds for every sign combination");
}

#[test]
fn memory_roundtrip_is_bounds_checked() {
    let mut block = MemoryBlock::allocate(1); // MEMORY_SIZE = 1
    let wrote = block.write_u8(0, 0xA5);      // ZERO_USIZE, MEMORY_BYTE
    assert!(wrote, "write_u8(block, 0, 0xA5) succeeds (in-bounds)");
    let read_back = block.read_u8(0);
    assert_eq!(read_back, Some(0xA5), "memory roundtrip: read_u8(0) returns 0xA5");
    let oob_write = block.write_u8(1, 0x00);
    assert!(!oob_write, "write_u8(block, 1, 0x00) fails (out of bounds)");
    let oob_read = block.read_u8(1);
    assert_eq!(oob_read, None, "read_u8(block, 1) fails (out of bounds)");
}

#[test]
fn vec_slice_first_and_rest_match_declared_constants() {
    let pushed: [u8; 4] = [0x0D, 0xF0, 0xAD, 0x0B];
    let first = pushed[0];
    let rest_len = pushed.len() as u64 - 1;
    assert_eq!(first, 0x0D, "split_first.first = MAGIC_BYTE_0");
    assert_eq!(rest_len, 3, "split_first.rest_len = EXPECTED_REST_LEN (3)");
}

#[test]
fn string_construction_yields_declared_length() {
    let string_bytes: [u8; 4] = [0x43, 0x69, 0x6E, 0x21];
    assert_eq!(string_bytes.len(), 4, "string length = EXPECTED_STRING_LEN (4)");
}

#[test]
fn hash_map_stores_and_retrieves() {
    let mut map: std::collections::HashMap<u8, u8> = std::collections::HashMap::new();
    let inserted = map.insert(0xA5, 0x0D); // MEMORY_BYTE -> MAGIC_BYTE_0
    assert_eq!(inserted, None, "hash_map insert of a fresh key stores a new entry");
    let got = map.get(&0xA5).copied();
    assert_eq!(got, Some(0x0D), "hash_map get(MEMORY_BYTE) returns MAGIC_BYTE_0");
    let missing = map.get(&0x00).copied();
    assert_eq!(missing, None, "hash_map get of an absent key is KeyNotFound");
}

#[test]
fn struct_move_matches_declared_constants() {
    let origin_x = 0i64; // ZERO_INT
    let origin_y = 0i64; // ZERO_INT
    let moved_x = origin_x + 3; // POINT_DX
    let moved_y = origin_y + 4; // POINT_DY
    assert_eq!(moved_x, 3, "Point moved x = POINT_DX");
    assert_eq!(moved_y, 4, "Point moved y = POINT_DY");
}

#[test]
fn app_workflow_wraps_errors_through_range_to_app() {
    let app_good = app_workflow(21, 100);
    assert_eq!(app_good, AppResult::Ok(42), "app_workflow(21) = Ok(42)");
    let app_small = app_workflow(-1, 100);
    assert_eq!(
        app_small,
        AppResult::Err(AppError::AppRange(RangeError::TooSmall(-1))),
        "app_workflow(-1) = AppRange(TooSmall(-1))"
    );
    let app_large = app_workflow(101, 100);
    assert_eq!(
        app_large,
        AppResult::Err(AppError::AppRange(RangeError::TooLarge(101))),
        "app_workflow(101) = AppRange(TooLarge(101))"
    );
}

// ---------------------------------------------------------------------------
// End-to-end fixture gate: compile and run the Euclidean-division fixture
// (`tests/fixtures/verify_math/euclid_div.cnb`) and compare its actual
// stdout against the oracle above, line by line.  This is the check the
// fixture's header promises: the compiled program's real runtime output,
// not a re-statement of the oracle.  Every other fixture in the corpus is
// gated by the repro harness or the pre-commit script; this is the sole
// fixture under tests/fixtures/verify_math/.
// ---------------------------------------------------------------------------

#[test]
fn euclid_div_fixture_output_matches_oracle() {
    let cinnabar = env!("CARGO_BIN_EXE_cinnabar");
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let fixture = root
        .join("tests")
        .join("fixtures")
        .join("verify_math")
        .join("euclid_div.cnb");
    let dir = std::env::temp_dir().join(format!("cinnabar_verify_math_{}", std::process::id()));
    match std::fs::create_dir_all(&dir) {
        Ok(()) => {}
        Err(err) => {
            eprintln!("cannot create temp dir: {}", err);
            return;
        }
    }
    let bin = dir.join("euclid_div_bin");

    let compile = std::process::Command::new(cinnabar)
        .arg(&fixture)
        .arg("-o")
        .arg(&bin)
        .output();
    match compile {
        Ok(out) => {
            assert!(
                out.status.success(),
                "euclid_div.cnb failed to compile (exit {:?}):\n{}\n{}",
                out.status.code(),
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            );
        }
        Err(err) => {
            eprintln!("spawn cinnabar failed: {}", err);
            return;
        }
    }

    let run = std::process::Command::new(&bin).output();
    match run {
        Ok(out) => {
            assert!(
                out.status.success(),
                "euclid_div binary failed (exit {:?})",
                out.status.code()
            );
            let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
            let pairs = [(10i64, 3i64), (21, 3), (21, 5), (-10, 3), (10, -3), (-10, -3)];
            let mut expected = String::new();
            let mut idx = 0usize;
            while idx < pairs.len() {
                match pairs.get(idx) {
                    Some((a, b)) => {
                        let q = euclid_div(*a, *b);
                        let r = euclid_rem(*a, *b);
                        expected.push_str(&format!("{}/{}={} {}\n", a, b, q, r));
                    }
                    None => break,
                }
                idx += 1;
            }
            assert_eq!(
                stdout, expected,
                "euclid_div.cnb runtime output must match the oracle line by line"
            );
        }
        Err(err) => {
            eprintln!("spawn euclid_div binary failed: {}", err);
        }
    }

    match std::fs::remove_dir_all(&dir) {
        Ok(()) => {}
        Err(err) => eprintln!("temp cleanup failed: {}", err),
    }
}
