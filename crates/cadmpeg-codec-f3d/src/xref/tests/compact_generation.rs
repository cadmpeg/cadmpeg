// SPDX-License-Identifier: Apache-2.0

use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::test_support::{local_reference, lp_ascii, lp_utf16};

fn carrier(role: &str, attributes_present: bool, transform: Option<[[f64; 4]; 4]>) -> Vec<u8> {
    let target = || {
        let mut reference = vec![1];
        reference.extend_from_slice(&3_u64.to_le_bytes());
        reference.push(1);
        reference.extend_from_slice(&71_u64.to_le_bytes());
        reference.push(0);
        lp_ascii(&mut reference, "E03784ED-5E19-4E14-B9F2-3B07017018CD");
        reference.push(0);
        reference
    };
    let mut bytes = Vec::new();
    lp_ascii(&mut bytes, "380");
    bytes.extend_from_slice(&10_u64.to_le_bytes());
    lp_ascii(&mut bytes, "");
    bytes.push(1);
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    bytes.extend(target());
    bytes.extend_from_slice(&9_u32.to_le_bytes());
    bytes.push(u8::from(attributes_present));
    if attributes_present {
        bytes.extend_from_slice(&0_u32.to_le_bytes());
    }
    bytes.extend_from_slice(&2_u32.to_le_bytes());
    lp_utf16(&mut bytes, "11111111-2222-3333-4444-555555555555");
    lp_utf16(&mut bytes, "66666666-7777-8888-9999-aaaaaaaaaaaa");
    bytes.push(0);
    bytes.extend(target());
    bytes.push(u8::from(transform.is_none()));
    if let Some(matrix) = transform {
        for value in matrix.into_iter().flatten() {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
    }
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    lp_utf16(&mut bytes, role);
    bytes.push(0);
    bytes.extend(local_reference(7));
    bytes
}

#[test]
fn compact_legacy_placements_keep_owner_key_and_qualified_role() {
    let matrix = [
        [0.0, -1.0, 0.0, 2.0],
        [1.0, 0.0, 0.0, 3.0],
        [0.0, 0.0, 1.0, 4.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    for role in [
        "aaaabbbb-cccc-dddd-eeee-ffff00001111",
        "aaaabbbb-cccc-dddd-eeee-ffff00001111_urn:adsk.wipprod:fs.file:vf.example?version=1",
    ] {
        for attributes in [false, true] {
            for transform in [None, Some(matrix)] {
                let frame = carrier(role, attributes, transform);
                let expected_len = 399
                    + usize::from(attributes) * 4
                    + usize::from(transform.is_some()) * 128
                    + (role.encode_utf16().count() - 36) * 2;
                assert_eq!(frame.len(), expected_len);
                let details = super::super::compact_legacy_placement_details(&frame).unwrap();
                assert_eq!(details.owner, 7);
                assert_eq!(details.role.to_string(), role);
                assert_eq!(details.transform, transform);
                assert_eq!(
                    details.occurrence_guid.as_str(),
                    "66666666-7777-8888-9999-aaaaaaaaaaaa"
                );
                assert_eq!(super::super::legacy_placement_target(&frame), Some((3, 71)));
                let decoded = crate::F3dCodec
                    .decode(
                        &mut std::io::Cursor::new(super::document_with_placement(role, frame)),
                        &DecodeOptions::default(),
                    )
                    .unwrap();
                let native = crate::test_support::native_test::f3d_native(decoded.ir());
                let reference = &native.xref_references[0];
                assert!(super::super::placement_source_matches(reference).unwrap());
                assert_eq!(reference.neutron_role.as_str(), role);
                let source = reference.placement_source.as_ref().unwrap();
                assert_eq!(source.owner.as_ref().unwrap().target, 7);
                assert_eq!(
                    decoded
                        .report()
                        .losses
                        .iter()
                        .filter(|loss| {
                            loss.code
                                == crate::loss::F3dLossCode::XrefPlacementMetadataNative.kind()
                        })
                        .count(),
                    1
                );
                let occurrence = decoded
                    .ir()
                    .model
                    .occurrences
                    .iter()
                    .find(|occurrence| {
                        occurrence.native_ref.as_deref() == Some(reference.id.as_str())
                    })
                    .unwrap();
                let expected = if transform.is_some() {
                    [
                        [0.0, -1.0, 0.0, 20.0],
                        [1.0, 0.0, 0.0, 30.0],
                        [0.0, 0.0, 1.0, 40.0],
                    ]
                } else {
                    [
                        [1.0, 0.0, 0.0, 0.0],
                        [0.0, 1.0, 0.0, 0.0],
                        [0.0, 0.0, 1.0, 0.0],
                    ]
                };
                assert_eq!(occurrence.transform.unwrap().affine_rows(), expected);
                assert!(!decoded.report().losses.iter().any(|loss| {
                    loss.code == crate::loss::F3dLossCode::XrefPlacementUndecoded.kind()
                }));
            }
        }
    }
}

#[test]
fn compact_legacy_placements_require_complete_equal_targets_and_rigid_matrix() {
    let role = "aaaabbbb-cccc-dddd-eeee-ffff00001111";
    let matrix = cadmpeg_ir::transform::Transform::identity().rows();
    let valid = carrier(role, false, Some(matrix));
    for end in 0..valid.len() {
        assert!(super::super::compact_legacy_placement_details(&valid[..end]).is_none());
    }
    let details = super::super::compact_legacy_placement_details(&valid).unwrap();
    let mut refused = Vec::new();
    let mut zero_owner = valid.clone();
    let owner = zero_owner.len() - 10;
    zero_owner[owner..owner + 8].copy_from_slice(&0_u64.to_le_bytes());
    refused.push(zero_owner);
    let mut zero_ordinal = valid.clone();
    let ordinal = crate::layout::legacy_attributed_external_placement_prefix::LEN + 1;
    zero_ordinal[ordinal..ordinal + 4].copy_from_slice(&0_u32.to_le_bytes());
    refused.push(zero_ordinal);
    let mut unequal = valid.clone();
    let repeated = details.transform_offset.unwrap() - 60;
    unequal[repeated..repeated + 8].copy_from_slice(&4_u64.to_le_bytes());
    refused.push(unequal);
    let mut reflection = valid.clone();
    let matrix_at = details.transform_offset.unwrap();
    reflection[matrix_at..matrix_at + 8].copy_from_slice(&(-1.0_f64).to_le_bytes());
    refused.push(reflection);
    let mut extra = valid;
    extra.push(0);
    refused.push(extra);
    for frame in refused {
        let decoded = crate::F3dCodec
            .decode(
                &mut std::io::Cursor::new(super::document_with_placement(role, frame)),
                &DecodeOptions::default(),
            )
            .unwrap();
        assert_eq!(
            decoded
                .report()
                .losses
                .iter()
                .filter(|loss| {
                    loss.code == crate::loss::F3dLossCode::XrefPlacementUndecoded.kind()
                })
                .count(),
            1
        );
    }
}

#[test]
fn supported_legacy_identity_owner_frame_keeps_its_projection() {
    let role = "aaaabbbb-cccc-dddd-eeee-ffff00001111";
    let frame = super::legacy_occurrence_record(role, 10, None);
    let placement = super::super::occurrence_placement(&frame, None)
        .unwrap()
        .unwrap();
    assert!(placement.frame_bytes.is_empty());
    assert!(placement.owner.is_none());
    assert!(placement.occurrence_guid.is_none());
}

#[test]
fn compact_legacy_target_binds_its_current_world_pose() {
    let role = "aaaabbbb-cccc-dddd-eeee-ffff00001111";
    let frame = carrier(role, false, None);
    let placement = super::super::occurrence_placement(&frame, None)
        .unwrap()
        .unwrap();
    let current = [
        [0.0, -1.0, 0.0, 2.0],
        [1.0, 0.0, 0.0, 3.0],
        [0.0, 0.0, 1.0, 4.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    let path = super::legacy_path_pose::legacy_root_component_path_carrier(
        "E03784ED-5E19-4E14-B9F2-3B07017018CD",
        71,
        current,
    );
    let first = crate::layout::legacy_attributed_external_placement_prefix::TARGET_REFERENCE;
    let local = first + crate::layout::legacy_occurrence_target_reference::LEN + 4;
    let after = local + crate::test_support::local_reference(10).len() + 4;
    let mut reversed = path.clone();
    reversed.splice(
        first..after,
        path[local..after]
            .iter()
            .chain(&path[first..local])
            .copied(),
    );
    for path in [path, reversed] {
        let pose = super::super::component_path_pose(
            &path,
            Some(crate::metastream::MODERN_SERIALIZER_MAGIC),
        )
        .unwrap();
        let super::super::CurrentPose::One(selected) =
            super::super::current_pose(&[pose], &placement, role)
        else {
            panic!("one current world pose");
        };
        assert_eq!(selected.transform.unwrap().rows(), current);
    }
}
