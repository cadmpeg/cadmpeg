// SPDX-License-Identifier: Apache-2.0
//! Current poses from component paths with a legacy target element.

/// A component-path carrier whose path is a legacy target and then a local
/// element that names placement record 10.
fn legacy_root_component_path_carrier(
    type_guid: &str,
    identity: u64,
    transform: [[f64; 4]; 4],
) -> Vec<u8> {
    use crate::test_support::local_reference;

    let mut bytes = Vec::new();
    bytes.extend_from_slice(&3_u32.to_le_bytes());
    bytes.extend_from_slice(b"257");
    bytes.extend_from_slice(&11_u64.to_le_bytes());
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    bytes.push(1);
    bytes.extend_from_slice(&2_u32.to_le_bytes());
    bytes.push(1);
    bytes.extend_from_slice(&3_u64.to_le_bytes());
    bytes.push(1);
    bytes.extend_from_slice(&identity.to_le_bytes());
    bytes.push(0);
    bytes.extend_from_slice(&36_u32.to_le_bytes());
    bytes.extend_from_slice(type_guid.as_bytes());
    bytes.push(0);
    bytes.extend_from_slice(&20_u32.to_le_bytes());
    bytes.extend(local_reference(10));
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    bytes.push(0);
    bytes.push(0);
    for value in transform.into_iter().flatten() {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    bytes.extend(local_reference(10));
    bytes.push(2);
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    bytes.extend_from_slice(&42_u32.to_le_bytes());
    bytes.extend(local_reference(8));
    bytes.extend(local_reference(3));
    bytes.extend(local_reference(6));
    bytes
}

#[test]
fn a_legacy_root_component_path_supplies_the_current_pose_of_its_placement() {
    use crate::test_support::legacy_attributed_external_placement;

    const ROOT_COMPONENT: &str = "E03784ED-5E19-4E14-B9F2-3B07017018CD";
    const IDENTITY: u64 = 0x0102_0304_0506_0708;
    let magic = Some(crate::metastream::MODERN_SERIALIZER_MAGIC);
    let role = "aaaabbbb-cccc-dddd-eeee-ffff00001111";
    let stored = [
        [1.0, 0.0, 0.0, 1.6],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    let current = [
        [1.0, 0.0, 0.0, 0.5],
        [0.0, 0.0, -1.0, 1.0],
        [0.0, 1.0, 0.0, 0.4],
        [0.0, 0.0, 0.0, 1.0],
    ];
    let frame = legacy_attributed_external_placement(role, false, Some(stored));
    let placements =
        super::super::occurrence_placements(&frame, &super::super::indexed_records(&frame), None);
    let [placement] = placements.as_slice() else {
        panic!("one placement");
    };

    let carrier = legacy_root_component_path_carrier(ROOT_COMPONENT, IDENTITY, current);
    let pose = super::super::component_path_pose(&carrier, magic).expect("legacy pose");
    assert_eq!(pose.carrier_record_index, 11);
    assert_eq!(pose.placement_record_index, 10);
    assert_eq!(pose.legacy_target, Some((3, IDENTITY)));
    assert_eq!(pose.transform, Some(current));
    let super::super::CurrentPose::One(selected) =
        super::super::current_pose(&[pose], placement, role)
    else {
        panic!("one current pose");
    };
    assert_eq!(selected.carrier_record_index, 11);
    assert_eq!(
        selected
            .transform
            .map(crate::records::xref::XrefPlacementTransform::rows),
        Some(current)
    );

    // A target with another source occurrence identity names another design.
    let other = legacy_root_component_path_carrier(ROOT_COMPONENT, IDENTITY + 1, current);
    let other = super::super::component_path_pose(&other, magic).expect("legacy pose");
    assert!(matches!(
        super::super::current_pose(&[other], placement, role),
        super::super::CurrentPose::None
    ));
    // A legacy target of another type is not a root component.
    let untyped = legacy_root_component_path_carrier(
        "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee",
        IDENTITY,
        current,
    );
    assert!(super::super::component_path_pose(&untyped, magic).is_none());
}
