// SPDX-License-Identifier: Apache-2.0

use super::assert_work_refusal;

const SINGLE_REFERENCE: [u32; 1] = [55];

fn write_marked_reference(bytes: &mut [u8], offset: usize, record_index: u32) {
    bytes[offset] = 1;
    bytes[offset + 1..offset + 5].copy_from_slice(&record_index.to_le_bytes());
}

fn current_extrude_bytes() -> Vec<u8> {
    let mut bytes = vec![0; 113];
    bytes[25] = 1;
    bytes[26..30].copy_from_slice(&SINGLE_REFERENCE[0].to_le_bytes());
    bytes[37..41].copy_from_slice(&1u32.to_le_bytes());
    bytes[41..45].copy_from_slice(&1u32.to_le_bytes());
    bytes[45..49].copy_from_slice(&1u32.to_le_bytes());
    bytes[49] = 0;
    bytes[50] = 1;
    bytes[51] = 0;
    bytes[55..63].copy_from_slice(&1.0f64.to_le_bytes());
    write_marked_reference(&mut bytes, 79, SINGLE_REFERENCE[0]);
    bytes[96..100].copy_from_slice(&1u32.to_le_bytes());
    bytes[109..113].copy_from_slice(&0u32.to_le_bytes());
    bytes
}

#[test]
fn current_extrude_candidate_membership_propagates_work_refusal() {
    let bytes = current_extrude_bytes();
    let context = cadmpeg_test_support::service_decode_context();
    assert!(matches!(
        super::super::exact_current_extrude_prologue(
            &context,
            &bytes,
            0,
            bytes.len(),
            &SINGLE_REFERENCE,
            false,
        ),
        Ok(Some(
            crate::records::feature::extrude::DesignExtrudePrologue::ReferenceAware {
                operation: crate::records::feature::extrude::DesignExtrudeOperation::Join,
                direction_face_extend_values: [1, 1],
                side_extent_discriminators: [1, 0],
                extent: crate::records::feature::extrude::DesignExtrudeExtent::OneSidedDistance,
                ..
            }
        ))
    ));
    assert_work_refusal(
        "search F3D current Extrude candidate reference member",
        |ctx| {
            super::super::exact_current_extrude_prologue(
                ctx,
                &bytes,
                0,
                bytes.len(),
                &SINGLE_REFERENCE,
                false,
            )
        },
    );
}

#[test]
fn current_extrude_slot_membership_propagates_work_refusal() {
    let bytes = current_extrude_bytes();
    let context = cadmpeg_test_support::service_decode_context();
    assert!(matches!(
        super::super::exact_current_extrude_prologue(
            &context,
            &bytes,
            0,
            bytes.len(),
            &SINGLE_REFERENCE,
            false,
        ),
        Ok(Some(
            crate::records::feature::extrude::DesignExtrudePrologue::ReferenceAware {
                operation: crate::records::feature::extrude::DesignExtrudeOperation::Join,
                direction_face_extend_values: [1, 1],
                side_extent_discriminators: [1, 0],
                extent: crate::records::feature::extrude::DesignExtrudeExtent::OneSidedDistance,
                ..
            }
        ))
    ));
    assert_work_refusal("search F3D current Extrude slot reference members", |ctx| {
        super::super::exact_current_extrude_prologue(
            ctx,
            &bytes,
            0,
            bytes.len(),
            &SINGLE_REFERENCE,
            false,
        )
    });
}

fn compact_shifted_extrude_bytes() -> Vec<u8> {
    use crate::layout::shifted_extrude_offset_283_two_sided_tail as tail;
    use crate::layout::shifted_extrude_prologue as prologue;

    let mut bytes = vec![0; 283];
    bytes[prologue::PREFIX_CONSTANT..prologue::PREFIX_CONSTANT + 4]
        .copy_from_slice(&1u32.to_le_bytes());
    bytes[prologue::OPERATION..prologue::OPERATION + 4].copy_from_slice(&2u32.to_le_bytes());
    bytes[31..35].copy_from_slice(&2u32.to_le_bytes());
    for offset in [
        tail::FIRST_PARAMETER_REFERENCE,
        tail::SECOND_PARAMETER_REFERENCE,
        tail::TRAILING_ENTITY_REFERENCE,
    ] {
        write_marked_reference(&mut bytes, offset, SINGLE_REFERENCE[0]);
    }
    bytes[tail::FIRST_SIDE_EXTENT..tail::FIRST_SIDE_EXTENT + 4]
        .copy_from_slice(&1u32.to_le_bytes());
    bytes[tail::SECOND_SIDE_EXTENT..tail::SECOND_SIDE_EXTENT + 4]
        .copy_from_slice(&1u32.to_le_bytes());
    bytes
}

#[test]
fn compact_shifted_extrude_parameter_membership_propagates_work_refusal() {
    let bytes = compact_shifted_extrude_bytes();
    let context = cadmpeg_test_support::service_decode_context();
    assert!(matches!(
        super::super::exact_legacy_shifted_extrude_prologue(
            &context,
            &bytes,
            0,
            283,
            &SINGLE_REFERENCE,
        ),
        Ok(Some(
            crate::records::feature::extrude::DesignExtrudePrologue::LegacyShifted {
                operation: crate::records::feature::extrude::DesignExtrudeOperation::Cut,
                direction_face_extend_values: [2, 0],
                side_extent_discriminators: [1, 1],
                extent: Some(
                    crate::records::feature::extrude::DesignExtrudeExtent::TwoSidedDistance
                ),
                ..
            }
        ))
    ));
    assert_work_refusal(
        "search F3D compact shifted Extrude parameter references",
        |ctx| {
            super::super::exact_legacy_shifted_extrude_prologue(
                ctx,
                &bytes,
                0,
                283,
                &SINGLE_REFERENCE,
            )
        },
    );
}

fn shifted_extrude_bytes() -> Vec<u8> {
    use crate::layout::shifted_extrude_prologue as prologue;

    let mut bytes = vec![0; 272];
    bytes[prologue::PREFIX_CONSTANT..prologue::PREFIX_CONSTANT + 4]
        .copy_from_slice(&1u32.to_le_bytes());
    bytes[prologue::OPERATION..prologue::OPERATION + 4].copy_from_slice(&2u32.to_le_bytes());
    bytes[31..35].copy_from_slice(&2u32.to_le_bytes());
    for offset in [139, 159, 182] {
        write_marked_reference(&mut bytes, offset, SINGLE_REFERENCE[0]);
    }
    bytes[155..159].copy_from_slice(&1u32.to_le_bytes());
    bytes[178..182].copy_from_slice(&1u32.to_le_bytes());
    bytes
}

#[test]
fn shifted_extrude_parameter_membership_propagates_work_refusal() {
    let bytes = shifted_extrude_bytes();
    let context = cadmpeg_test_support::service_decode_context();
    assert!(matches!(
        super::super::exact_legacy_shifted_extrude_prologue(
            &context,
            &bytes,
            0,
            272,
            &SINGLE_REFERENCE,
        ),
        Ok(Some(
            crate::records::feature::extrude::DesignExtrudePrologue::LegacyShifted {
                operation: crate::records::feature::extrude::DesignExtrudeOperation::Cut,
                direction_face_extend_values: [2, 0],
                side_extent_discriminators: [1, 1],
                extent: Some(
                    crate::records::feature::extrude::DesignExtrudeExtent::TwoSidedDistance
                ),
                ..
            }
        ))
    ));
    assert_work_refusal("search F3D shifted Extrude parameter references", |ctx| {
        super::super::exact_legacy_shifted_extrude_prologue(ctx, &bytes, 0, 272, &SINGLE_REFERENCE)
    });
}

#[test]
fn class_338_extrude_reads_its_fixed_references() {
    use crate::layout::legacy_class_338_two_sided_distance_extrude_frame as layout;
    use crate::test_support::lp_utf16;

    const REFERENCES: [u32; 10] = [11, 12, 13, 14, 15, 16, 17, 18, 19, 20];
    let mut bytes = vec![0; layout::LEN + 7];
    bytes[layout::PREFIX_CONSTANT..layout::PREFIX_CONSTANT + 4]
        .copy_from_slice(&layout::PREFIX_CONSTANT_VALUE.to_le_bytes());
    bytes[layout::OPERATION..layout::OPERATION + 4].copy_from_slice(&1u32.to_le_bytes());
    bytes[layout::DIRECTION..layout::DIRECTION + 4]
        .copy_from_slice(&layout::DIRECTION_VALUE.to_le_bytes());
    bytes[layout::FACE_EXTEND..layout::FACE_EXTEND + 4]
        .copy_from_slice(&layout::FACE_EXTEND_VALUE.to_le_bytes());
    bytes[layout::GEOMETRY_KIND] = 1;
    bytes[layout::PROFILE_NORMAL..layout::PROFILE_NORMAL + 8]
        .copy_from_slice(&1.0f64.to_le_bytes());
    bytes[layout::NULL_SCOPE_SCALAR_LANE] = 1;
    for (offset, record_index) in [
        (layout::FIRST_SIDE_PARAMETER_REFERENCE, REFERENCES[0]),
        (layout::PROFILE_GROUP_REFERENCE, REFERENCES[1]),
        (layout::BODY_GROUP_REFERENCE, REFERENCES[2]),
    ] {
        write_marked_reference(&mut bytes, offset, record_index);
    }
    bytes[layout::FIRST_SIDE_EXTENT..layout::FIRST_SIDE_EXTENT + 4]
        .copy_from_slice(&layout::FIRST_SIDE_EXTENT_VALUE.to_le_bytes());
    bytes[layout::SECOND_SIDE_EXTENT..layout::SECOND_SIDE_EXTENT + 4]
        .copy_from_slice(&layout::SECOND_SIDE_EXTENT_VALUE.to_le_bytes());
    let mut guid = Vec::new();
    lp_utf16(&mut guid, "00000000-0000-0000-0000-000000000000");
    bytes[layout::GUID..layout::GUID + guid.len()].copy_from_slice(&guid);
    bytes[layout::REFERENCE_COUNT..layout::REFERENCE_COUNT + 4]
        .copy_from_slice(&layout::REFERENCE_COUNT_VALUE.to_le_bytes());
    bytes[layout::LEN + 4..layout::LEN + 7].copy_from_slice(b"262");

    let context = cadmpeg_test_support::service_decode_context();
    assert!(matches!(
        super::super::exact_class_338_two_sided_distance_extrude_prologue(
            &context,
            &bytes,
            0,
            layout::LEN,
            "338",
            "262",
            layout::REFERENCE_COUNT,
            &REFERENCES,
        ),
        Ok(Some(
            crate::records::feature::extrude::DesignExtrudePrologue::LegacyShifted {
                operation: crate::records::feature::extrude::DesignExtrudeOperation::Join,
                direction_face_extend_values: [2, 0],
                side_extent_discriminators: [1, 1],
                extent: Some(
                    crate::records::feature::extrude::DesignExtrudeExtent::TwoSidedDistance
                ),
                ..
            }
        ))
    ));
}
