// SPDX-License-Identifier: Apache-2.0
//! Operation offsets and extent-slot spans of reference-aware Extrude scopes.

use super::super::{direct_extrude_operation_valid, extent_slot_expansion_valid};

#[test]
fn plain_scope_starts_its_operation_at_28() {
    assert!(direct_extrude_operation_valid(Some(100), false, Some(128)));
    for operation in [122, 126, 127, 129] {
        assert!(!direct_extrude_operation_valid(Some(100), false, Some(operation)));
    }
}

#[test]
fn typed_scope_starts_its_operation_at_22_or_26() {
    assert!(direct_extrude_operation_valid(Some(100), true, Some(122)));
    assert!(direct_extrude_operation_valid(Some(100), true, Some(126)));
    for operation in [121, 123, 125, 127, 128] {
        assert!(!direct_extrude_operation_valid(Some(100), true, Some(operation)));
    }
}

#[test]
fn operation_offset_overflow_is_refused() {
    assert!(!direct_extrude_operation_valid(Some(u64::MAX), false, Some(27)));
    assert!(!direct_extrude_operation_valid(Some(u64::MAX), true, Some(21)));
}

#[test]
fn plain_slots_widen_the_span_by_ten_bytes() {
    for slots in 0..=7 {
        assert!(extent_slot_expansion_valid(false, 10 * slots));
    }
    assert!(!extent_slot_expansion_valid(false, 80));
    assert!(!extent_slot_expansion_valid(false, 5));
    assert!(!extent_slot_expansion_valid(false, 50 + 1));
}

#[test]
fn typed_slots_widen_the_span_by_fifty_bytes() {
    for slots in 0..=7 {
        assert!(extent_slot_expansion_valid(true, 50 * slots));
    }
    assert!(!extent_slot_expansion_valid(true, 400));
    assert!(!extent_slot_expansion_valid(true, 10));
    assert!(!extent_slot_expansion_valid(true, 149));
}
