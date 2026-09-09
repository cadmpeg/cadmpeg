// SPDX-License-Identifier: Apache-2.0
#![allow(
    clippy::cloned_ref_to_slice_refs,
    clippy::default_trait_access,
    clippy::trivially_copy_pass_by_ref,
    clippy::uninlined_format_args,
    clippy::wildcard_imports
)]
use super::prelude::*;
use crate::layout::work_plane_legacy_325_matrix_frame as work_plane_325;
use crate::layout::work_plane_legacy_class_290_matrix_frame as work_plane_class_290;

#[test]
fn legacy_work_plane_325_byte_frames_decode_their_matrix() {
    const EPS_WORK_PLANE_TEST_VALUE: f64 = 1.0e-12;

    type WorkPlaneFrameCase = (
        &'static [u8; 3],
        &'static [u8; 3],
        u32,
        [u8; 4],
        [[f64; 4]; 4],
    );
    let cases: [WorkPlaneFrameCase; 4] = [
        (
            b"290",
            b"262",
            73u32,
            [1, 1, 0, 0],
            [
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 2.5],
                [1.0, 0.0, 0.0, 0.0],
                [0.0, 0.0, 0.0, 1.0],
            ],
        ),
        (
            b"308",
            b"257",
            74u32,
            [0, 0, 0, 0],
            [
                [0.0, 0.0, 1.0, 1.25],
                [1.0, 0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0, -0.75],
                [0.0, 0.0, 0.0, 1.0],
            ],
        ),
        (
            b"320",
            b"258",
            75u32,
            [0, 0, 0, 0],
            [
                [1.0, 0.0, 0.0, 3.0],
                [0.0, 0.0, -1.0, 0.0],
                [0.0, 1.0, 0.0, 1.5],
                [0.0, 0.0, 0.0, 1.0],
            ],
        ),
        (
            b"364",
            b"263",
            76u32,
            [0, 0, 0, 0],
            crate::records::SketchPlacementMatrix::IDENTITY.rows(),
        ),
    ];

    for (class_tag, paired_class_tag, record_index, prefix_marker, transform) in cases {
        let mut bytes = vec![0; work_plane_325::LEN];
        bytes[0..4].copy_from_slice(&3u32.to_le_bytes());
        bytes[4..7].copy_from_slice(class_tag);
        bytes[7..11].copy_from_slice(&record_index.to_le_bytes());
        bytes[work_plane_class_290::PREFIX_MARKER..work_plane_325::MATRIX]
            .copy_from_slice(&prefix_marker);
        for (ordinal, value) in transform.into_iter().flatten().enumerate() {
            let at = work_plane_325::MATRIX + ordinal * 8;
            bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
        }
        bytes.extend_from_slice(&3u32.to_le_bytes());
        bytes.extend_from_slice(paired_class_tag);
        bytes.extend_from_slice(&record_index.to_le_bytes());

        let mut scope = DesignParameterScope::empty(
            "f3d:test:scope#1",
            crate::records::feature::DesignFeatureKind::WorkPlane,
            1,
        );
        scope
            .try_edit(|draft| {
                draft.reference_members =
                    crate::records::ReferenceRun::unlocated(vec![record_index]);
            })
            .unwrap();
        let decoded = exact_work_plane_frame(&bytes, &IndexedRecordOffsets::build(&bytes), &scope)
            .expect("325-byte WorkPlane frame");
        for (actual_row, expected_row) in decoded.transform.iter().zip(transform.iter()) {
            for (actual, expected) in actual_row.iter().zip(expected_row.iter()) {
                assert!((actual - expected).abs() < EPS_WORK_PLANE_TEST_VALUE);
            }
        }
        assert_eq!(decoded.transform_offset, work_plane_325::MATRIX as u64);
        assert_eq!(decoded.reference, None);
    }
}
