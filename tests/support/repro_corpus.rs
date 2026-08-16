//! The expected-success fixture corpus.
//!
//! One table, shared by every suite that runs it. `repro_harness` runs these
//! for their exit codes; `sanitizer_gate` runs them again under a memory
//! checker. A second copy would drift, and the two suites would quietly stop
//! covering the same programs — which is exactly the kind of hand-maintained
//! duplicate AGENTS.md calls a standing correctness bug.
//!
//! **Invariants:**
//! - This table is the single definition of the expected-success corpus.
//!   A suite that needs a subset samples from it; it does not keep its own
//!   list.
//! - Each entry carries the exit code its fixture must produce, so adding a
//!   fixture means stating what it does rather than only that it compiles.

/// Fixture stem under `tests/fixtures/repro/`, and the exit code it must
/// produce.
pub(crate) const EXPECT_OK: &[(&str, i32)] = &[
    ("hello", 0),
    ("net_primitives", 0),
    ("liveness_many_bindings", 100),
    ("mini", 0),
    ("array_test", 0),
    ("borrow_index", 0),
    ("enum_array_index", 0),
    ("result_array_index", 0),
    ("idx10d_mut_disjoint", 30),
    ("idx10e_same_expr_disjoint", 30),
    ("rec_test", 120),
    ("tail_rec", 64),
    ("mem_probe", 0),
    ("mem_byte_access", 0),
    ("hanoi", 255),
    ("head", 10),
    ("enum_test", 0),
    ("mem2", 0),
    ("vm2", 1),
    ("vm3", 1),
    ("vm4", 1),
    ("vm6", 1),
    ("vm7", 5),
    ("vm8", 1),
    ("vm9", 5),
    ("vm11", 4),
    ("vm", 120),
    ("continue_test", 9),
    ("jump_test", 3),
    ("jump2", 3),
    ("jump3", 3),
    ("jump4", 1),
    ("nested_continue_test", 109),
    ("elif_test", 1),
    ("elif_chain", 3),
    ("modulo", 42),
    ("div_runtime", 7),
    ("int_min_neg1", 0),
    ("shift_mask", 0),
    ("int_width_grid", 0),
    ("int_literal_context", 0),
    ("string_literal", 0),
    ("string_print", 0),
    ("string_static_borrow", 0),
    ("file_roundtrip", 0),
    ("runtime_io", 0),
    ("empty_block", 0),
    ("utf8_validation", 0),
    ("multiline_const", 30),
    ("fib", 155),
    ("linear_branch_consume", 0),
    ("linear_loop_consume", 0),
    ("linear_field_reinit", 0),
    ("linear_ref_swap", 0),
    ("linear_field_consume", 0),
    ("linear_ref_nonlinear_read", 14),
    ("ret_borrow_shared_twice", 0),
    ("ret_borrow_single_origin", 0),
    ("slice_test", 0),
    ("vec_pop_drain", 0),
    ("hash_map_remove_drain", 0),
    ("hash_map_struct_key", 0),
    ("hash_map_collision", 0),
    ("hash_map_resize", 0),
    ("hash_map_slice_key", 0),
    ("native_slice_view", 0),
    ("process_spawn_wait", 0),
    ("exit_diag_renamed", 42),
    ("mem_test", 0),
    ("rt1", 0),
    ("vec_test", 0),
    ("full_rt", 0),
];
