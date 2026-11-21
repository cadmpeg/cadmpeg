// SPDX-License-Identifier: Apache-2.0
//! A pattern payload whose direction one is not active.

fn owner(
    record_index: u32,
    local_ordinal: u32,
    evaluated_value: f64,
) -> crate::records::parameters::DesignParameterOwner {
    let evaluated_value_offset = 500 + u64::from(local_ordinal);
    crate::records::parameters::DesignParameterOwner::try_from(
        crate::records::parameters::DesignParameterOwnerWire {
            id: format!("f3d:Design/BulkStream.dat:design-parameter-owner#{record_index}"),
            byte_offset: evaluated_value_offset - 40,
            frame_length: 104,
            class_tag: crate::records::references::DesignClassTag::try_from("457".to_owned())
                .unwrap(),
            record_index,
            scope_record_index: 10,
            local_ordinal,
            evaluated_value,
            evaluated_value_offset,
            parameter_record_index: record_index + 1,
            owned_ordinal: local_ordinal,
            variant: Some(0),
            companion_record_index: record_index + 2,
        },
    )
    .unwrap()
}

#[test]
fn a_payload_v_direction_binds_when_only_direction_two_is_active() {
    let (scope, _, _) = super::distribution_inputs();
    let mut bytes = super::spacing_direction_frame();
    bytes[89..93].copy_from_slice(&10u32.to_le_bytes());
    let payload_start = bytes.len();
    bytes.extend(super::distribution_payload(1, None, 0));
    let owners = [
        owner(50, 0, 5.0),
        owner(51, 1, 3.0),
        owner(52, 2, 0.0),
        owner(53, 3, -0.9),
    ];
    let construction = super::tested_rectangular_pattern_construction(
        &bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
        &scope,
        &owners,
    )
    .expect("rectangular-pattern construction");
    assert_eq!(construction.active_direction_count(), 1);
    let [direction] = construction.directions() else {
        panic!("one U-direction record");
    };
    assert_eq!(direction.record_index, 20);
    let secondary = construction
        .secondary_direction()
        .expect("payload V direction");
    assert_eq!(
        secondary.value,
        crate::test_support::reals([0.0, 0.0, -3.0])
    );
    assert_eq!(secondary.offset, u64::try_from(payload_start + 63).unwrap());
    let distribution = construction.distribution().expect("distribution block");
    assert!(distribution.symmetric.value);
    assert!(!distribution.total_extent.value);
}
