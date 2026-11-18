// SPDX-License-Identifier: Apache-2.0

use std::collections::HashMap;

use cadmpeg_core::decode::u64_from_index;

use super::direct_joint_origin_frame;
use crate::design::test_support::indexed_record_offsets_for_test;
use crate::layout::named_parameter_scope_tail as named_tail;
use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
use crate::records::identity::{Located, ReferenceRun};

#[test]
fn standalone_joint_origin_requires_the_typed_placement_and_scope_backlink() {
    const OWNER: u32 = 23;
    const CARRIER: u32 = 24;
    const CONSTRUCTION: u32 = 81;
    const EPS: f64 = 1.0e-12;
    let matrix: [[f64; 4]; 4] = [
        [0.0, -1.0, 0.0, 1.5],
        [1.0, 0.0, 0.0, -2.0],
        [0.0, 0.0, 1.0, 0.25],
        [0.0, 0.0, 0.0, 1.0],
    ];
    for (scope_length, carrier_length, construction_reference, suffix, version) in [
        (315, 369, 45, [0, 0, 0, 0], 6),
        (398, 391, 45, [0, 0, 0, 0], 6),
        (403, 412, 46, [0, 0, 1, 0], 8),
    ] {
        for (source_kind, label) in [
            ("JointOrigin", None),
            ("Ursprung der Verbindung", None),
            ("JointOrigin", Some("Pin frame")),
            ("Ursprung der Verbindung", Some("Pin frame")),
        ] {
            let kind_delta =
                2 * (source_kind.encode_utf16().count() - "JointOrigin".encode_utf16().count());
            let tail_delta = label.map_or(0, |text| {
                named_tail::LEN - 72 + 2 * text.encode_utf16().count()
            });
            let scope_length = scope_length + kind_delta + tail_delta;
            let mut scope = DesignParameterScope::empty(
                "f3d:Design/BulkStream.dat:design-parameter-scope#0",
                DesignFeatureKind::JointOrigin,
                OWNER,
            );
            scope
                .try_edit(|draft| {
                    draft.reference_members = ReferenceRun::unlocated(vec![CARRIER, CONSTRUCTION]);
                    draft.layout_fixture_references();
                    draft.frame_length = u64_from_index(scope_length);
                    draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
                    draft.layout_fixture_tail();
                    if let Some(value) = label {
                        draft.feature_ordinal_offset -= u64_from_index(tail_delta);
                        draft.label = Some(Located {
                            value: value.to_owned(),
                            offset: draft.feature_ordinal_offset
                                + u64_from_index(named_tail::LABEL_LENGTH + size_of::<u32>()),
                        });
                    }
                })
                .unwrap();
            scope.source_kind = (source_kind != "JointOrigin").then(|| source_kind.to_owned());
            let types = HashMap::from([
                (
                    u64::from(OWNER),
                    ("02E8AA49-08B8-439C-A8C7-221BE20143D8", 2),
                ),
                (
                    u64::from(CARRIER),
                    ("7B6C3C2D-4096-4E6D-B225-699E8C2D9355", version),
                ),
            ]);
            let start = scope_length + 30;
            let end = start + carrier_length;
            let backlink = end - 11;
            let suffix_offset = construction_reference + 11;
            let matrix_offset = suffix_offset + suffix.len();
            let mut bytes = vec![0; end + 22];
            for (at, class_tag, record_index) in [
                (start, b"153", CARRIER),
                (end, b"249", CARRIER),
                (end + 11, b"301", CONSTRUCTION),
            ] {
                bytes[at..at + 4].copy_from_slice(&3u32.to_le_bytes());
                bytes[at + 4..at + 7].copy_from_slice(class_tag);
                bytes[at + 7..at + 11].copy_from_slice(&record_index.to_le_bytes());
            }
            for (at, record_index) in [
                (start + construction_reference, CONSTRUCTION),
                (backlink, OWNER),
            ] {
                bytes[at] = 1;
                bytes[at + 1..at + 5].copy_from_slice(&record_index.to_le_bytes());
            }
            bytes[start + suffix_offset..start + matrix_offset].copy_from_slice(&suffix);
            for (index, value) in matrix.into_iter().flatten().enumerate() {
                let at = start + matrix_offset + index * size_of::<f64>();
                bytes[at..at + size_of::<f64>()].copy_from_slice(&value.to_le_bytes());
            }
            let records = indexed_record_offsets_for_test(&bytes);
            assert!(direct_joint_origin_frame(&bytes, &records, &scope, &HashMap::new()).is_none());
            let (frame, carrier) =
                direct_joint_origin_frame(&bytes, &records, &scope, &types).unwrap();
            assert_eq!(carrier.frame_length, u64_from_index(carrier_length));
            assert_eq!(
                frame.transform_offset,
                u64_from_index(start + matrix_offset)
            );
            assert_eq!(
                frame.reference,
                Some((
                    CONSTRUCTION,
                    u64_from_index(start + construction_reference + 1)
                ))
            );
            for (actual, expected) in frame
                .transform
                .rows()
                .into_iter()
                .flatten()
                .zip(matrix.into_iter().flatten())
            {
                assert!((actual - expected).abs() < EPS);
            }

            let mut malformed = bytes.clone();
            malformed[start + matrix_offset..][..size_of::<f64>()]
                .copy_from_slice(&f64::NAN.to_le_bytes());
            let malformed_records = indexed_record_offsets_for_test(&malformed);
            assert!(
                direct_joint_origin_frame(&malformed, &malformed_records, &scope, &types).is_none()
            );
            for invalid_byte in [
                start + 11,
                start + construction_reference,
                start + suffix_offset + 2,
                backlink + 1,
                end - 1,
                end + 7,
            ] {
                bytes[invalid_byte] ^= 1;
                let records = indexed_record_offsets_for_test(&bytes);
                assert!(direct_joint_origin_frame(&bytes, &records, &scope, &types).is_none());
                bytes[invalid_byte] ^= 1;
            }
        }
    }
}
