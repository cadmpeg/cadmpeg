// SPDX-License-Identifier: Apache-2.0

#[test]
fn sketch_constraint_mask_decodes_equal_length_bit() {
    let (kinds, unknown) =
        crate::records::sketch_relations::constraint_kinds_from_state(0x0000_0008);
    assert_eq!(
        kinds,
        [crate::records::sketch_relations::SketchConstraintKind::EqualLength]
    );
    assert_eq!(unknown, 0);
}

#[test]
fn zero_sketch_constraint_state_decodes_as_coincident() {
    let (kinds, unknown) = crate::records::sketch_relations::constraint_kinds_from_state(0);
    assert_eq!(
        kinds,
        [crate::records::sketch_relations::SketchConstraintKind::Coincident]
    );
    assert_eq!(unknown, 0);
}
