// SPDX-License-Identifier: Apache-2.0
#![allow(
    unused_imports,
    clippy::cloned_ref_to_slice_refs,
    clippy::default_trait_access,
    clippy::trivially_copy_pass_by_ref,
    clippy::uninlined_format_args,
    clippy::wildcard_imports
)]
use super::prelude::*;

#[test]
fn sketch_constraint_mask_decodes_equal_length_bit() {
    let (kinds, unknown) = crate::records::constraint_kinds_from_state(0x0000_0008);
    assert_eq!(kinds, [crate::records::SketchConstraintKind::EqualLength]);
    assert_eq!(unknown, 0);
}

#[test]
fn zero_sketch_constraint_state_decodes_as_coincident() {
    let (kinds, unknown) = crate::records::constraint_kinds_from_state(0);
    assert_eq!(kinds, [crate::records::SketchConstraintKind::Coincident]);
    assert_eq!(unknown, 0);
}
